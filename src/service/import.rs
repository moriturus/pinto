//! Restore a complete board from an `export --json` snapshot.
//!
//! Import is the inverse of [`crate::service::export_snapshot`]. It rebuilds a board's PBIs,
//! Sprints, configuration, and common Definition of Done from a [`BoardSnapshot`], the same
//! structure the export produces. The CLI parses the JSON contract back into that structure, so
//! this service is agnostic to the wire format.
//!
//! **Fail fast on a populated board**: importing into a board that already holds active PBIs or
//! Sprints returns [`ImportOutcome::Refused`] unless the caller opts into replacement. Replacement
//! mirrors the snapshot: existing active PBIs and Sprints are removed before the snapshot is
//! written, so the resulting board reflects the snapshot exactly.
//!
//! **Serialized like migration**: the whole operation runs under the board write lock, and the
//! configuration is switched only after the item and sprint writes succeed.

use super::export::BoardSnapshot;
use super::open_board_locked;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::storage::{Backend, BacklogItemRepository, SprintRepository, atomic_write};
use std::path::Path;
use tokio::fs;

/// File name of the common DoD, stored directly under `.pinto/` (mirrors [`crate::service::dod`]).
const DOD_FILE: &str = "dod.md";

/// Result of [`import_board`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// The snapshot was written. Reports how many PBIs and Sprints were restored.
    Imported {
        /// Number of PBIs written from the snapshot.
        items: usize,
        /// Number of Sprints written from the snapshot.
        sprints: usize,
    },
    /// The board already held data and no replacement was requested. Reports the existing counts
    /// that blocked the import so the caller can explain what would be overwritten.
    Refused {
        /// Number of active PBIs already on the board.
        items: usize,
        /// Number of Sprints already on the board.
        sprints: usize,
    },
}

/// Restore the board in `project_dir` from `snapshot`.
///
/// Return [`Error::NotInitialized`] when the board is uninitialized. When the board already holds
/// active PBIs or Sprints and `force` is false, return [`ImportOutcome::Refused`] without changing
/// anything. Otherwise mirror the snapshot: remove existing active PBIs and Sprints, write the
/// snapshot's items and Sprints, overwrite `config.toml`, and set or clear the common DoD.
pub async fn import_board(
    project_dir: &Path,
    snapshot: BoardSnapshot,
    force: bool,
) -> Result<ImportOutcome> {
    let (board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let config_path = board_dir.join("config.toml");

    // Reject unknown fields and structurally invalid configuration before touching the board, so a
    // malformed snapshot never leaves a half-written restore behind.
    let config: Config = serde_json::from_value(snapshot.config.clone())
        .map_err(|error| Error::parse(&config_path, error.to_string()))?;
    config.validate(&config_path)?;

    // Emptiness is measured against the board as it is configured now. Refuse to clobber a
    // populated board unless replacement was explicitly requested.
    let existing_items = BacklogItemRepository::list(&repo).await?;
    let existing_sprints = SprintRepository::list(&repo).await?;
    if !force && (!existing_items.is_empty() || !existing_sprints.is_empty()) {
        return Ok(ImportOutcome::Refused {
            items: existing_items.len(),
            sprints: existing_sprints.len(),
        });
    }

    // Write to the backend the snapshot's configuration selects. In the common same-backend case
    // this is the current backend; otherwise the configuration switch below points future reads at
    // the restored data (like `migrate`).
    let target = Backend::open_for_write(&board_dir, config.storage.backend).await?;

    // Mirror the snapshot: drop the target's existing active PBIs and Sprints first so items absent
    // from the snapshot do not survive the restore.
    for item in BacklogItemRepository::list(&target).await? {
        BacklogItemRepository::delete(&target, &item.id).await?;
    }
    for sprint in SprintRepository::list(&target).await? {
        SprintRepository::delete(&target, &sprint.id).await?;
    }

    // Saving an item records its issued ID, so a later `add` never reuses a restored ID.
    for item in &snapshot.items {
        BacklogItemRepository::save(&target, item).await?;
    }
    for sprint in &snapshot.sprints {
        SprintRepository::save(&target, sprint).await?;
    }

    // Switch the save destination only after the writes succeed, keeping the pre-import backend
    // usable if a write failed.
    config.save(&config_path).await?;

    let dod_path = board_dir.join(DOD_FILE);
    match &snapshot.dod {
        Some(dod) => {
            let trimmed = dod.trim();
            if trimmed.is_empty() {
                remove_if_present(&dod_path).await?;
            } else {
                atomic_write(&dod_path, &format!("{trimmed}\n")).await?;
            }
        }
        None => remove_if_present(&dod_path).await?,
    }

    target
        .commit(&format!(
            "pinto: import board ({} items, {} sprints)",
            snapshot.items.len(),
            snapshot.sprints.len()
        ))
        .await?;

    Ok(ImportOutcome::Imported {
        items: snapshot.items.len(),
        sprints: snapshot.sprints.len(),
    })
}

/// Remove a file, treating an already-absent file as success.
async fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::io(path, &error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::{
        ListFilter, NewItem, add_item, create_sprint, export_snapshot, init_board, list_items,
        list_sprints, set_common_dod,
    };
    use crate::sprint::SprintId;
    use tempfile::TempDir;

    /// Build a populated source board and return its export snapshot.
    async fn populated_snapshot() -> (TempDir, BoardSnapshot) {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init source");
        add_item(dir.path(), "First", NewItem::default())
            .await
            .expect("add first");
        add_item(dir.path(), "Second", NewItem::default())
            .await
            .expect("add second");
        create_sprint(
            dir.path(),
            &SprintId::new("S-1").unwrap(),
            "Sprint 1",
            None,
            None,
        )
        .await
        .expect("sprint");
        set_common_dod(dir.path(), "- [ ] tests pass")
            .await
            .expect("dod");
        let snapshot = export_snapshot(dir.path()).await.expect("export");
        (dir, snapshot)
    }

    #[tokio::test]
    async fn import_into_empty_board_restores_items_sprints_and_dod() {
        let (_source, snapshot) = populated_snapshot().await;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let outcome = import_board(dest.path(), snapshot, false)
            .await
            .expect("import");
        assert_eq!(
            outcome,
            ImportOutcome::Imported {
                items: 2,
                sprints: 1
            }
        );

        let items = list_items(dest.path(), &ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);

        let sprints = list_sprints(dest.path()).await.expect("sprints");
        assert_eq!(sprints.len(), 1);
        assert_eq!(sprints[0].id.as_str(), "S-1");

        let dod = crate::service::common_dod(dest.path()).await.expect("dod");
        assert_eq!(dod.as_deref(), Some("- [ ] tests pass"));
    }

    #[tokio::test]
    async fn import_into_non_empty_board_is_refused_without_force() {
        let (_source, snapshot) = populated_snapshot().await;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        add_item(dest.path(), "Existing", NewItem::default())
            .await
            .expect("add existing");

        let outcome = import_board(dest.path(), snapshot, false)
            .await
            .expect("import call succeeds");
        assert_eq!(
            outcome,
            ImportOutcome::Refused {
                items: 1,
                sprints: 0
            }
        );

        // The board is untouched by a refused import.
        let items = list_items(dest.path(), &ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["Existing"]);
    }

    #[tokio::test]
    async fn force_replaces_existing_board_data() {
        let (_source, snapshot) = populated_snapshot().await;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        add_item(dest.path(), "Existing", NewItem::default())
            .await
            .expect("add existing");

        let outcome = import_board(dest.path(), snapshot, true)
            .await
            .expect("forced import");
        assert_eq!(
            outcome,
            ImportOutcome::Imported {
                items: 2,
                sprints: 1
            }
        );

        let items = list_items(dest.path(), &ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"], "snapshot replaces prior data");
    }

    #[tokio::test]
    async fn import_uninitialized_board_errors() {
        let (_source, snapshot) = populated_snapshot().await;
        let dir = TempDir::new().expect("temp dir");
        let error = import_board(dir.path(), snapshot, false)
            .await
            .expect_err("uninitialized");
        assert!(
            matches!(error, Error::NotInitialized { .. }),
            "got {error:?}"
        );
    }
}
