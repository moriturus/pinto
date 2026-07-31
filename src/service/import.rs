//! Restore a complete board from an `export --json` snapshot.
//!
//! Import is the inverse of [`crate::service::export_snapshot`]. It rebuilds a board's active and
//! archived PBIs, Sprints, Sprint child records, configuration, and common Definition of Done from a
//! [`BoardSnapshot`], the same structure the export produces. The CLI parses the JSON contract
//! back into that structure, so this service is agnostic to the wire format.
//!
//! **Fail fast on a populated board**: importing into a board that already holds active PBIs,
//! archived PBIs, or Sprints returns [`ImportOutcome::Refused`] unless the caller opts into
//! replacement. Replacement mirrors the snapshot: existing active PBIs, archived PBIs, Sprints, and
//! child records are removed before the snapshot is written, so the resulting board reflects the
//! snapshot exactly.
//!
//! **Serialized like migration**: the whole operation runs under the board write lock, and the
//! configuration is switched only after all record writes succeed.

use super::export::BoardSnapshot;
use super::{open_board_locked, restore_board_after_failure};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::sprint_record::SprintRecordKind;
use crate::storage::{
    Backend, BacklogItemRepository, BoardRecoveryPoint, SprintRepository, atomic_write,
};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use tokio::fs;

/// File name of the common DoD, stored directly under `.pinto/` (mirrors [`crate::service::dod`]).
const DOD_FILE: &str = "dod.md";

/// Synthetic path label for referential-integrity errors surfaced from an import snapshot.
const SNAPSHOT_LABEL: &str = "<import snapshot>";

/// Verify the snapshot is a self-consistent board before any backend write.
///
/// The wire format only guarantees that each child record's own ID matches its parent field; it
/// carries no cross-record guarantees. This check enforces the board model that the export always
/// satisfies, considering the active and archived PBIs together as one board:
///
/// - unique Sprint IDs and at most one Retro and one Review per Sprint;
/// - every Sprint satisfying the domain invariants ([`crate::sprint::Sprint::validate`]): a
///   non-blank title, a two-sided non-inverted period, a Goal on every active Sprint, and no Goal
///   outcome without a Goal;
/// - every child record and every action-PBI `source` naming a Sprint and child record present in
///   the snapshot;
/// - unique PBI IDs across the active and archived collections (no duplicate and no collision
///   between the two stores);
/// - every `parent` and `depends_on` reference resolving to a PBI present in the snapshot, and
///   every `sprint` assignment resolving to a Sprint present in the snapshot;
/// - a non-empty title and a `status` naming one of `config`'s workflow columns on every PBI;
/// - acyclic `parent` and `depends_on` graphs;
/// - a rank unique within each active PBI's `(status, parent)` scope.
///
/// The reference set, status, and graph checks span the active and archived PBIs together because
/// `doctor` inspects both stores as one board. A normal-form [`Rank`] string is validated at parse
/// time, but rank *uniqueness* within a scope is a separate board condition `doctor` enforces on the
/// active PBIs, so it is checked here too.
///
/// It returns [`Error::Parse`] describing the first violation so `import` refuses a snapshot that
/// would produce a board `doctor` flags, instead of writing it and reporting success. The checks
/// mirror `doctor`'s health boundary, so a healthy `export` always round-trips while a snapshot that
/// would import to an unhealthy board is rejected before any write.
fn validate_snapshot(snapshot: &BoardSnapshot, config: &Config) -> Result<()> {
    let mut sprint_ids = HashSet::with_capacity(snapshot.sprints.len());
    for sprint in &snapshot.sprints {
        if !sprint_ids.insert(&sprint.id) {
            return Err(snapshot_error(format!("duplicate Sprint `{}`", sprint.id)));
        }
        // The wire format rebuilds each Sprint field-by-field, bypassing the domain mutators, so a
        // snapshot can carry a Sprint state the tool could never produce (a blank title, a one-sided
        // or inverted period, an active Sprint without a Goal, or a Goal outcome without a Goal).
        // Reject it here so `import` never writes a Sprint that normal commands cannot read or that
        // the persistence layer would silently normalize.
        sprint
            .validate()
            .map_err(|error| snapshot_error(error.to_string()))?;
    }

    for (kind, records) in [
        (SprintRecordKind::Retro, &snapshot.retros),
        (SprintRecordKind::Review, &snapshot.reviews),
    ] {
        let mut seen = HashSet::with_capacity(records.len());
        for record in records {
            if !seen.insert(&record.id) {
                return Err(snapshot_error(format!(
                    "duplicate {} `{}`",
                    kind.display_name(),
                    record.id
                )));
            }
            if !sprint_ids.contains(&record.id) {
                return Err(snapshot_error(format!(
                    "{} `{}` has no parent Sprint in the snapshot",
                    kind.display_name(),
                    record.id
                )));
            }
        }
    }

    // The active and archived PBIs form one board: their IDs must be unique across both stores, and
    // every reference must resolve within their union. An active PBI legitimately parents to an
    // archived PBI, so both collections contribute to the resolvable ID set.
    let mut item_ids = HashSet::with_capacity(snapshot.items.len() + snapshot.archived_items.len());
    for item in snapshot.items.iter().chain(&snapshot.archived_items) {
        if !item_ids.insert(&item.id) {
            return Err(snapshot_error(format!("duplicate PBI `{}`", item.id)));
        }
    }
    for item in snapshot.items.iter().chain(&snapshot.archived_items) {
        if let Some(parent) = &item.parent
            && !item_ids.contains(parent)
        {
            return Err(snapshot_error(format!(
                "PBI `{}` refers to missing parent `{parent}`",
                item.id
            )));
        }
        for dependency in &item.depends_on {
            if !item_ids.contains(dependency) {
                return Err(snapshot_error(format!(
                    "PBI `{}` depends on missing PBI `{dependency}`",
                    item.id
                )));
            }
        }
        if let Some(sprint) = &item.sprint
            && !sprint_ids.iter().any(|id| id.as_str() == sprint)
        {
            return Err(snapshot_error(format!(
                "PBI `{}` refers to missing Sprint `{sprint}`",
                item.id
            )));
        }
    }

    let retro_ids: HashSet<_> = snapshot.retros.iter().map(|record| &record.id).collect();
    let review_ids: HashSet<_> = snapshot.reviews.iter().map(|record| &record.id).collect();
    for item in snapshot.items.iter().chain(&snapshot.archived_items) {
        let Some(source) = &item.source else {
            continue;
        };
        if !sprint_ids.contains(&source.sprint_id) {
            return Err(snapshot_error(format!(
                "action PBI `{}` sources Sprint `{}`, which is absent from the snapshot",
                item.id, source.sprint_id
            )));
        }
        let record_present = match source.kind {
            SprintRecordKind::Retro => retro_ids.contains(&source.sprint_id),
            SprintRecordKind::Review => review_ids.contains(&source.sprint_id),
        };
        if !record_present {
            return Err(snapshot_error(format!(
                "action PBI `{}` sources {} `{}`, which is absent from the snapshot",
                item.id,
                source.kind.display_name(),
                source.sprint_id
            )));
        }
    }

    // Every PBI must carry a non-empty title and a status naming a configured workflow column, and
    // the parent and dependency graphs must be acyclic. These are the remaining conditions `doctor`
    // enforces that the wire format does not, so importing a snapshot that satisfies them lands on a
    // board `doctor` reports as healthy.
    let columns: HashSet<&str> = config.columns.iter().map(String::as_str).collect();
    for item in snapshot.items.iter().chain(&snapshot.archived_items) {
        if item.title.trim().is_empty() {
            return Err(snapshot_error(format!(
                "PBI `{}` has an empty title",
                item.id
            )));
        }
        if !columns.contains(item.status.as_str()) {
            return Err(snapshot_error(format!(
                "PBI `{}` has status `{}`, which is not a configured workflow column",
                item.id,
                item.status.as_str()
            )));
        }
    }

    let mut parent_edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut dependency_edges: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in snapshot.items.iter().chain(&snapshot.archived_items) {
        if let Some(parent) = &item.parent {
            parent_edges
                .entry(item.id.to_string())
                .or_default()
                .push(parent.to_string());
        }
        if !item.depends_on.is_empty() {
            dependency_edges
                .entry(item.id.to_string())
                .or_default()
                .extend(item.depends_on.iter().map(ToString::to_string));
        }
    }
    if let Some(cycle) = first_cycle(&parent_edges) {
        return Err(snapshot_error(format!(
            "PBI parent references form a cycle: {}",
            cycle.join(" -> ")
        )));
    }
    if let Some(cycle) = first_cycle(&dependency_edges) {
        return Err(snapshot_error(format!(
            "PBI dependencies form a cycle: {}",
            cycle.join(" -> ")
        )));
    }

    // `doctor` requires each active PBI's rank to be unique within its `(status, parent)` scope, so
    // reject a snapshot that reuses a rank in one scope before it can import to a board `doctor`
    // flags. A normal-form rank string is not enough: two PBIs can each hold a valid rank yet share
    // it in the same scope. Only the active PBIs are checked because `doctor` does not require rank
    // uniqueness among archived PBIs. `parent` defaults to the empty string, matching `doctor`.
    let mut rank_scopes: HashSet<(&str, String, String)> =
        HashSet::with_capacity(snapshot.items.len());
    for item in &snapshot.items {
        let parent = item
            .parent
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default();
        let scope = (item.status.as_str(), parent, item.rank.to_string());
        if !rank_scopes.insert(scope) {
            return Err(snapshot_error(format!(
                "PBI `{}` reuses rank `{}` in status `{}` and parent scope `{}`",
                item.id,
                item.rank,
                item.status.as_str(),
                item.parent
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            )));
        }
    }

    Ok(())
}

/// Return the sorted node IDs on the first cycle found in `edges`, or `None` when the graph is
/// acyclic. `edges` maps each node to its out-neighbors. The traversal walks an explicit heap stack
/// with three-state coloring so a chain thousands of levels deep cannot overflow the native stack.
fn first_cycle(edges: &BTreeMap<String, Vec<String>>) -> Option<Vec<String>> {
    const UNVISITED: u8 = 0;
    const ON_PATH: u8 = 1;
    const DONE: u8 = 2;

    struct Frame<'a> {
        node: &'a str,
        targets: &'a [String],
        cursor: usize,
    }

    let mut state: BTreeMap<&str, u8> = BTreeMap::new();
    for root in edges.keys() {
        if state.get(root.as_str()).copied().unwrap_or(UNVISITED) != UNVISITED {
            continue;
        }
        let mut path: Vec<&str> = vec![root.as_str()];
        state.insert(root.as_str(), ON_PATH);
        let mut frames = vec![Frame {
            node: root.as_str(),
            targets: edges.get(root).map(Vec::as_slice).unwrap_or(&[]),
            cursor: 0,
        }];
        while let Some(frame) = frames.last_mut() {
            if frame.cursor < frame.targets.len() {
                let target = frame.targets[frame.cursor].as_str();
                frame.cursor += 1;
                match state.get(target).copied().unwrap_or(UNVISITED) {
                    UNVISITED => {
                        state.insert(target, ON_PATH);
                        path.push(target);
                        let targets = edges.get(target).map(Vec::as_slice).unwrap_or(&[]);
                        frames.push(Frame {
                            node: target,
                            targets,
                            cursor: 0,
                        });
                    }
                    ON_PATH => {
                        if let Some(start) = path.iter().position(|value| *value == target) {
                            let mut cycle: Vec<String> =
                                path[start..].iter().map(ToString::to_string).collect();
                            cycle.sort();
                            return Some(cycle);
                        }
                    }
                    _ => {}
                }
            } else {
                state.insert(frame.node, DONE);
                frames.pop();
                path.pop();
            }
        }
    }
    None
}

/// Build a snapshot referential-integrity [`Error::Parse`] with the synthetic snapshot label.
fn snapshot_error(message: String) -> Error {
    Error::parse(Path::new(SNAPSHOT_LABEL), message)
}

/// Result of [`import_board`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportOutcome {
    /// The snapshot was written. Reports how many PBIs and Sprints were restored.
    Imported {
        /// Number of PBIs written from the snapshot, counting the active and archived collections
        /// together so the reported total matches everything actually restored.
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
/// Return [`Error::NotInitialized`] when the board is uninitialized. Reject a snapshot that would
/// produce a board `doctor` flags — duplicate Sprints, a Sprint that violates its domain invariants
/// (blank title, one-sided or inverted period, active Sprint without a Goal, or a Goal outcome
/// without a Goal), duplicate or orphaned Retro/Review records, duplicate PBI IDs across the active
/// and archived collections, a `parent`, `depends_on`, `sprint`, or action `source` reference
/// missing from the snapshot, an empty title, a status that is not a configured workflow column, a
/// `parent` or `depends_on` cycle, or a rank reused within an active PBI's `(status, parent)` scope
/// — with [`Error::Parse`] before any write. When the board already holds active PBIs, archived PBIs, or Sprints and `force`
/// is
/// false, return [`ImportOutcome::Refused`] without changing anything. Otherwise mirror the
/// snapshot: remove existing active PBIs, archived PBIs, Sprints, and child records, write all
/// snapshot records, overwrite `config.toml`, and set or clear the common DoD.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], snapshot configuration validation, referential-integrity, or
/// parsing errors, and
/// backend, file, or Git commit errors. Validation and the refusal path make no durable changes.
/// A forced replacement restores the pre-operation board when a write fails; if restoration or a
/// later commit fails, durable partial changes may remain. After inspecting the board and fixing
/// the cause, retrying is safe because the replacement operation mirrors the snapshot.
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

    // Reject a snapshot that would import to a board `doctor` flags — an inconsistent Sprint
    // hierarchy, a dangling or duplicated reference, an out-of-column status, or a relationship cycle
    // — before any write, so an unhealthy board can never be committed and then reported as a
    // success. The parsed `config` supplies the workflow columns the status check validates against.
    validate_snapshot(&snapshot, &config)?;

    // Emptiness is measured against the board as it is configured now, including the archive:
    // replacement clears archived PBIs too, so a board that holds only archived PBIs must not be
    // silently overwritten. Refuse to clobber a populated board unless replacement was explicitly
    // requested. The reported `items` count sums active and archived PBIs so the caller can explain
    // everything that would be removed.
    let existing_items = BacklogItemRepository::list(&repo).await?;
    let existing_archived = BacklogItemRepository::list_archived(&repo).await?;
    let existing_sprints = SprintRepository::list(&repo).await?;
    if !force
        && (!existing_items.is_empty()
            || !existing_archived.is_empty()
            || !existing_sprints.is_empty())
    {
        return Ok(ImportOutcome::Refused {
            items: existing_items.len() + existing_archived.len(),
            sprints: existing_sprints.len(),
        });
    }

    // Write to the backend the snapshot's configuration selects. In the common same-backend case
    // this is the current backend; otherwise the configuration switch below points future reads at
    // the restored data (like `migrate`).
    let target = Backend::open_for_write(&board_dir, config.storage.backend).await?;
    let records = snapshot
        .retros
        .iter()
        .chain(snapshot.reviews.iter())
        .cloned()
        .collect::<Vec<_>>();

    let recovery = BoardRecoveryPoint::capture(&board_dir).await?;
    if let Err(error) = target
        .replace_board(
            &snapshot.items,
            &snapshot.archived_items,
            &snapshot.sprints,
            &records,
        )
        .await
    {
        return restore_board_after_failure(recovery, "forced board import", error).await;
    }

    // Switch the save destination only after the writes succeed, keeping the pre-import backend
    // usable if a write failed.
    if let Err(error) = config.save(&config_path).await {
        return restore_board_after_failure(recovery, "forced board import", error).await;
    }

    let dod_path = board_dir.join(DOD_FILE);
    let dod_result = match &snapshot.dod {
        Some(dod) => {
            let trimmed = dod.trim();
            if trimmed.is_empty() {
                remove_if_present(&dod_path).await
            } else {
                atomic_write(&dod_path, &format!("{trimmed}\n")).await
            }
        }
        None => remove_if_present(&dod_path).await,
    };
    if let Err(error) = dod_result {
        return restore_board_after_failure(recovery, "forced board import", error).await;
    }

    // When the current backend is Git, it owns the shared board tree even if the snapshot switches
    // the selected backend to file or SQLite. Commit through that prepared source repository in
    // that case; otherwise the target owns the final commit boundary.
    let commit_backend = if matches!(&repo, Backend::Git(_)) {
        &repo
    } else {
        &target
    };
    // Count active and archived PBIs together: `replace_board` wrote both stores, so the reported
    // total and the commit subject must reflect every restored PBI, not just the active ones.
    let item_count = snapshot.items.len() + snapshot.archived_items.len();
    commit_backend
        .commit(&format!(
            "pinto: import board ({item_count} items, {} sprints)",
            snapshot.sprints.len()
        ))
        .await?;

    Ok(ImportOutcome::Imported {
        items: item_count,
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
    use crate::backlog::Status;
    use crate::service::{
        ListFilter, NewItem, add_item, create_sprint, export_snapshot, init_board, list_items,
        list_sprints, remove_item, set_common_dod,
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
    async fn import_removes_an_existing_dod_when_snapshot_has_none() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.dod = None;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        set_common_dod(dest.path(), "- [x] old DoD")
            .await
            .expect("old dod");

        import_board(dest.path(), snapshot, true)
            .await
            .expect("forced import");
        assert_eq!(crate::service::common_dod(dest.path()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn import_treats_blank_snapshot_dod_as_absent() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.dod = Some(" \n\t ".to_string());

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        import_board(dest.path(), snapshot, false)
            .await
            .expect("import");
        assert_eq!(crate::service::common_dod(dest.path()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn import_rejects_self_parent_cycle() {
        let (_source, mut snapshot) = populated_snapshot().await;
        let id = snapshot.items[0].id.clone();
        snapshot.items[0].parent = Some(id);

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("self parent cycle rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");

        // The rejected snapshot leaves the board empty.
        let items = list_items(dest.path(), &ListFilter::default())
            .await
            .expect("list");
        assert!(items.is_empty(), "no write on a rejected snapshot");
    }

    #[tokio::test]
    async fn import_rejects_dependency_cycle() {
        let (_source, mut snapshot) = populated_snapshot().await;
        let first = snapshot.items[0].id.clone();
        let second = snapshot.items[1].id.clone();
        snapshot.items[0].depends_on = vec![second];
        snapshot.items[1].depends_on = vec![first];

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("dependency cycle rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_status_outside_configured_columns() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.items[0].status = Status::new("not-a-column");

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("out-of-column status rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_empty_title() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.items[0].title = "   ".to_string();

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("empty title rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_sprint_with_empty_title() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.sprints[0].title = "   ".to_string();

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("empty Sprint title rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");

        let sprints = list_sprints(dest.path()).await.expect("sprints");
        assert!(sprints.is_empty(), "no write on a rejected snapshot");
    }

    #[tokio::test]
    async fn import_rejects_sprint_with_inverted_period() {
        let (_source, mut snapshot) = populated_snapshot().await;
        let base = snapshot.sprints[0].created;
        snapshot.sprints[0].start = Some(base + chrono::Duration::days(5));
        snapshot.sprints[0].end = Some(base + chrono::Duration::days(1));

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("inverted Sprint period rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_sprint_with_one_sided_period() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.sprints[0].start = Some(snapshot.sprints[0].created);
        snapshot.sprints[0].end = None;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("one-sided Sprint period rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_active_sprint_without_goal() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.sprints[0].state = crate::sprint::SprintState::Active;
        snapshot.sprints[0].goal = String::new();

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("active Sprint without a Goal rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_goal_outcome_without_goal() {
        let (_source, mut snapshot) = populated_snapshot().await;
        snapshot.sprints[0].goal = String::new();
        snapshot.sprints[0].goal_achieved = Some(true);

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("Goal outcome without a Goal rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn import_rejects_duplicate_active_rank_in_one_scope() {
        let (_source, mut snapshot) = populated_snapshot().await;
        // Two active PBIs share status "todo" and parent scope, so reusing the rank collides.
        let shared = snapshot.items[0].rank.clone();
        snapshot.items[1].rank = shared;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let error = import_board(dest.path(), snapshot, false)
            .await
            .expect_err("duplicate active rank rejected");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");

        let items = list_items(dest.path(), &ListFilter::default())
            .await
            .expect("list");
        assert!(items.is_empty(), "no write on a rejected snapshot");
    }

    #[tokio::test]
    async fn import_allows_duplicate_rank_across_different_scopes() {
        let (_source, mut snapshot) = populated_snapshot().await;
        // Re-parent the second PBI so the shared rank lands in a different parent scope, which
        // `doctor` treats as unique.
        let parent = snapshot.items[0].id.clone();
        let shared = snapshot.items[0].rank.clone();
        snapshot.items[1].parent = Some(parent);
        snapshot.items[1].rank = shared;

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let outcome = import_board(dest.path(), snapshot, false)
            .await
            .expect("distinct scopes accepted");
        assert_eq!(
            outcome,
            ImportOutcome::Imported {
                items: 2,
                sprints: 1
            }
        );
    }

    #[tokio::test]
    async fn import_counts_active_and_archived_items_together() {
        let source = TempDir::new().expect("temp dir");
        init_board(source.path()).await.expect("init source");
        add_item(source.path(), "Active", NewItem::default())
            .await
            .expect("add active");
        add_item(source.path(), "To archive", NewItem::default())
            .await
            .expect("add archived");
        let items = list_items(source.path(), &ListFilter::default())
            .await
            .expect("list source");
        let archived_id = items
            .iter()
            .find(|item| item.title == "To archive")
            .map(|item| item.id.clone())
            .expect("archived id");
        remove_item(source.path(), &archived_id, false)
            .await
            .expect("archive");
        let snapshot = export_snapshot(source.path()).await.expect("export");
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(snapshot.archived_items.len(), 1);

        let dest = TempDir::new().expect("temp dir");
        init_board(dest.path()).await.expect("init dest");
        let outcome = import_board(dest.path(), snapshot, false)
            .await
            .expect("import");
        assert_eq!(
            outcome,
            ImportOutcome::Imported {
                items: 2,
                sprints: 0
            },
            "reported count sums active and archived PBIs"
        );

        // Re-export the destination: it must hold exactly the one active and one archived PBI that
        // the reported count of 2 promised.
        let restored = export_snapshot(dest.path()).await.expect("re-export");
        assert_eq!(restored.items.len(), 1, "one active PBI restored");
        assert_eq!(
            restored.archived_items.len(),
            1,
            "one archived PBI restored"
        );
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
