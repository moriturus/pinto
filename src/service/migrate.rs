//! Migrate board data between the file, Git, and SQLite backends.
//!
//! Copy active backlog items and sprints from the configured source backend to the target backend,
//! then switch the backend setting. Shared serialization preserves the data without loss.
//!
//! **The source is non-destructive**: migration never deletes source data; it only changes which
//! backend the configuration points to. To switch back, migrate in the opposite direction.
//!
//! **Recoverable by re-execution**: migration is not transactional, so a mid-run failure can leave
//! a partial destination. The configuration changes only after all writes succeed, leaving the
//! current backend usable. Rerunning migration replaces the destination with a fresh mirror. The
//! whole operation is serialized by `.pinto/.lock`.
//!
//! **The destination is replaced as a mirror**: items absent from the source are removed from the
//! destination. This prevents stale destination data (for example, leftover `tasks/` files after a
//! file-to-SQLite migration) from reappearing later.
//!
//! **Scope**: migrate only active items and sprints. Archived items remain in the source backend;
//! after switching, pinto cannot reference them through the target backend.

use crate::config::{Config, StorageBackend};
use crate::error::{Error, Result};
use crate::storage::{Backend, BacklogItemRepository, BoardLock, SprintRepository};
use std::path::Path;
use tokio::fs;

/// Result of [`migrate_storage`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrateOutcome {
    /// Actually migrated (migration source, migration destination, and number of items copied).
    Migrated {
        /// Backend to migrate from.
        from: StorageBackend,
        /// Backend to migrate to.
        to: StorageBackend,
        /// Number of PBIs copied.
        items: usize,
        /// Number of sprints copied.
        sprints: usize,
    },
    /// No migration was needed because the target backend is already active.
    AlreadyUsing(StorageBackend),
}

/// Migrate from the current backend to `target` and switch the save location of `config.toml`.
///
/// Return [`Error::NotInitialized`] when `.pinto/config.toml` does not exist. If `target` is
/// already active, return [`MigrateOutcome::AlreadyUsing`] without changing anything.
pub async fn migrate_storage(project_dir: &Path, target: StorageBackend) -> Result<MigrateOutcome> {
    let board_dir = project_dir.join(".pinto");
    let config_path = board_dir.join("config.toml");
    if !fs::try_exists(&config_path)
        .await
        .map_err(|e| Error::io(&config_path, &e))?
    {
        return Err(Error::NotInitialized { path: board_dir });
    }

    // Migration rewrites both backends and switches where settings are saved. Serialize the
    // entire operation so it cannot overlap with another write.
    let _lock = BoardLock::acquire(&board_dir).await?;

    let mut config = Config::load(&config_path).await?;
    let from = config.storage.backend;
    if from == target {
        return Ok(MigrateOutcome::AlreadyUsing(from));
    }

    let source = Backend::open_for_write(&board_dir, from).await?;
    let dest = Backend::open_for_write(&board_dir, target).await?;

    let items = BacklogItemRepository::list(&source).await?;
    let sprints = SprintRepository::list(&source).await?;

    // Mirror the source's active set. A simple upsert would leave stale destination entries that
    // could reappear during a later reverse migration, so remove IDs absent from the source first.
    let keep_items: std::collections::HashSet<_> = items.iter().map(|i| i.id.clone()).collect();
    for existing in BacklogItemRepository::list(&dest).await? {
        if !keep_items.contains(&existing.id) {
            BacklogItemRepository::delete(&dest, &existing.id).await?;
        }
    }
    let keep_sprints: std::collections::HashSet<_> = sprints.iter().map(|s| s.id.clone()).collect();
    for existing in SprintRepository::list(&dest).await? {
        if !keep_sprints.contains(&existing.id) {
            SprintRepository::delete(&dest, &existing.id).await?;
        }
    }

    // Copy active PBIs and sprints to the destination. Saving the same IDs makes this idempotent.
    for item in &items {
        BacklogItemRepository::save(&dest, item).await?;
    }
    for sprint in &sprints {
        SprintRepository::save(&dest, sprint).await?;
    }

    // Switch the save destination. Subsequent commands open the target backend.
    config.storage.backend = target;
    config.save(&config_path).await?;
    // The Git repository that owns the board tree must commit the complete migration. When Git is
    // the source (Git → file/SQLite), committing through `dest` would be a no-op and leave the
    // backend switch uncommitted; when Git is the target, the destination owns the new tree.
    let commit_backend = if matches!(&source, Backend::Git(_)) {
        &source
    } else {
        &dest
    };
    commit_backend
        .commit(&format!("pinto: migrate {from} to {target}"))
        .await?;

    Ok(MigrateOutcome::Migrated {
        from,
        to: target,
        items: items.len(),
        sprints: sprints.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::init_board;
    #[cfg(feature = "sqlite")]
    use crate::{
        backlog::ItemId,
        service::{NewItem, add_item, create_sprint, remove_item},
        sprint::SprintId,
    };
    use tempfile::TempDir;

    #[tokio::test]
    async fn migrate_uninitialized_board_errors() {
        let dir = TempDir::new().expect("temp dir");
        let err = migrate_storage(dir.path(), StorageBackend::Git)
            .await
            .expect_err("uninitialized");
        assert!(matches!(err, Error::NotInitialized { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn migrate_to_current_backend_is_noop() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        // Default is file. Transitioning to file does nothing.
        let outcome = migrate_storage(dir.path(), StorageBackend::File)
            .await
            .expect("noop");
        assert_eq!(outcome, MigrateOutcome::AlreadyUsing(StorageBackend::File));
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn migrate_file_to_sqlite_copies_items_and_sprints_and_flips_config() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        add_item(dir.path(), "First", NewItem::default())
            .await
            .expect("add");
        add_item(dir.path(), "Second", NewItem::default())
            .await
            .expect("add");
        create_sprint(
            dir.path(),
            &SprintId::new("S-1").unwrap(),
            "Sprint 1",
            None,
            None,
        )
        .await
        .expect("sprint");

        let outcome = migrate_storage(dir.path(), StorageBackend::Sqlite)
            .await
            .expect("migrate");
        assert_eq!(
            outcome,
            MigrateOutcome::Migrated {
                from: StorageBackend::File,
                to: StorageBackend::Sqlite,
                items: 2,
                sprints: 1,
            }
        );

        // The configuration now selects SQLite.
        let config = Config::load(&dir.path().join(".pinto").join("config.toml"))
            .await
            .expect("load config");
        assert_eq!(config.storage.backend, StorageBackend::Sqlite);

        // Subsequent commands open SQLite and can read the migrated data.
        let items = crate::service::list_items(dir.path(), &crate::service::ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["First", "Second"]);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn migrate_back_does_not_resurrect_archived_items() {
        // During a file→SQLite→file round trip, an archived item is absent from the active SQLite
        // set, so the stale file-side task is removed instead of being revived.
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        add_item(dir.path(), "Keep", NewItem::default())
            .await
            .expect("add"); // T-1
        add_item(dir.path(), "Archive me", NewItem::default())
            .await
            .expect("add"); // T-2

        migrate_storage(dir.path(), StorageBackend::Sqlite)
            .await
            .expect("to sqlite");
        // Archive T-2 in SQLite; the file-side tasks/T-2.md remains for now.
        remove_item(dir.path(), &ItemId::new("T", 2), false)
            .await
            .expect("archive");

        migrate_storage(dir.path(), StorageBackend::File)
            .await
            .expect("back to file");

        let items = crate::service::list_items(dir.path(), &crate::service::ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Keep"], "退避済み T-2 は file 側で復活しない");
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn migrate_sqlite_back_to_file_roundtrips() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        add_item(dir.path(), "Only", NewItem::default())
            .await
            .expect("add");

        migrate_storage(dir.path(), StorageBackend::Sqlite)
            .await
            .expect("to sqlite");
        // Add one more item in SQLite, then migrate back to the file backend.
        add_item(dir.path(), "Added on sqlite", NewItem::default())
            .await
            .expect("add on sqlite");
        let outcome = migrate_storage(dir.path(), StorageBackend::File)
            .await
            .expect("back to file");
        assert_eq!(
            outcome,
            MigrateOutcome::Migrated {
                from: StorageBackend::Sqlite,
                to: StorageBackend::File,
                items: 2,
                sprints: 0,
            }
        );

        let items = crate::service::list_items(dir.path(), &crate::service::ListFilter::default())
            .await
            .expect("list");
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["Only", "Added on sqlite"]);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn migrate_rejects_corrupt_sqlite_item_before_writing_to_file() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        add_item(dir.path(), "Only", NewItem::default())
            .await
            .expect("add");
        migrate_storage(dir.path(), StorageBackend::Sqlite)
            .await
            .expect("to sqlite");

        let outside = dir.path().join("outside-1.md");
        std::fs::write(&outside, "must survive\n").expect("write sentinel");
        let db_path = dir.path().join(".pinto/board.sqlite3");
        let conn = rusqlite::Connection::open(&db_path).expect("open sqlite database");
        conn.execute("PRAGMA ignore_check_constraints = ON", [])
            .expect("disable checks for corruption fixture");
        conn.execute(
            "UPDATE items SET id = ?1, prefix = ?2 WHERE id = 'T-1'",
            rusqlite::params!["../outside-1", "../outside"],
        )
        .expect("corrupt sqlite row");

        let err = migrate_storage(dir.path(), StorageBackend::File)
            .await
            .expect_err("corrupt SQLite item must stop file migration");
        assert!(
            err.to_string().contains("invalid item id prefix"),
            "got {err}"
        );
        assert_eq!(
            std::fs::read_to_string(&outside).expect("sentinel remains readable"),
            "must survive\n"
        );
        assert!(
            dir.path().join(".pinto/tasks/T-1.md").is_file(),
            "source file data must remain untouched after rejected migration"
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn migrate_rejects_file_destination_archive_collision_before_overwriting() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        add_item(dir.path(), "Only", NewItem::default())
            .await
            .expect("add");
        migrate_storage(dir.path(), StorageBackend::Sqlite)
            .await
            .expect("to sqlite");

        let board = dir.path().join(".pinto");
        std::fs::create_dir_all(board.join("archive")).expect("create archive");
        std::fs::copy(board.join("tasks/T-1.md"), board.join("archive/T-1.md"))
            .expect("create archive collision");

        let err = migrate_storage(dir.path(), StorageBackend::File)
            .await
            .expect_err("destination archive collision must stop migration");
        assert!(err.to_string().contains("duplicate item ID"), "got {err}");

        let config = Config::load(&board.join("config.toml"))
            .await
            .expect("load config");
        assert_eq!(config.storage.backend, StorageBackend::Sqlite);
        assert!(board.join("tasks/T-1.md").is_file());
        assert!(board.join("archive/T-1.md").is_file());
    }
}
