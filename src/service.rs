//! Use case (application service).
//!
//! High-level operations called from CLI/TUI. Combine the domain layer and persistence layer.

mod board;
mod burndown;
mod commits;
mod cycletime;
mod dependency;
mod dod;
mod item;
mod lifecycle;
mod migrate;
mod order;
mod points;
mod relations;
mod search;
mod settings;
mod sprint;
mod template;
#[cfg(test)]
mod test_support;
mod velocity;
mod wip;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::storage::{Backend, BoardLock};
pub use board::{Board, BoardColumn, BoardQuery, SortKey, board};
pub use burndown::{Burndown, BurndownDay, BurndownMetric, burndown};
pub use commits::{LinkOutcome, SyncOutcome, link_commits, sync_commits, unlink_commits};
pub use cycletime::{CycleTimeFilter, CycleTimeReport, DurationSummary, cycle_time};
pub use dependency::{
    DependencyOutcome, ItemDetail, add_dependency, item_detail, remove_dependency,
};
pub use dod::{clear_common_dod, common_dod, set_common_dod};
pub use item::{
    AddItemOutcome, EditOutcome, ItemEdit, ListFilter, NewItem, RebalanceOutcome, RemoveOutcome,
    ReorderTarget, add_item, add_item_with_outcome, apply_item_edit, edit_item, item_edit_template,
    list_items, move_item, rebalance, remove_item, reorder_item, show_item,
};
pub use lifecycle::{InitOutcome, init_board};
pub use migrate::{MigrateOutcome, migrate_storage};
pub use order::{Forest, build_forest, hierarchical, hierarchical_order};
pub(crate) use points::apply_effective_points;
pub use search::{SearchFilter, SearchMode};
pub use settings::{DisplaySettings, TuiSettings, display_settings, tui_settings};
pub(crate) use sprint::validate_sprint_assignment;
pub use sprint::{
    assign_sprint, assign_sprint_by_status, assign_sprint_raw, close_sprint, create_sprint,
    delete_sprint, edit_sprint, list_sprints, set_sprint_capacity, sprint_capacity, start_sprint,
    unassign_sprint,
};
use std::path::{Path, PathBuf};
pub use template::template_body;
use tokio::fs;
pub use velocity::{VelocityReport, VelocitySprint, velocity};
pub use wip::{WipViolation, check_wip, wip_violations};

/// Matching mode for a multi-label filter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LabelMatch {
    /// Match an item carrying at least one requested label (OR).
    #[default]
    Any,
    /// Match an item carrying every requested label (AND).
    All,
}

impl LabelMatch {
    /// Return whether `item_labels` satisfy the requested labels in this mode.
    #[must_use]
    pub fn matches(self, item_labels: &[String], requested: &[String]) -> bool {
        match self {
            Self::Any => requested
                .iter()
                .any(|requested| item_labels.iter().any(|label| label == requested)),
            Self::All => requested
                .iter()
                .all(|requested| item_labels.iter().any(|label| label == requested)),
        }
    }
}

/// Acquire the board write lock for a caller that needs a consistent board snapshot.
///
/// The returned guard releases `.pinto/.lock` when dropped. Read-only commands normally do not
/// need this helper; snapshot-style workflows use it to keep their copy operation coherent with
/// ordinary writers.
pub async fn lock_board(project_dir: &Path) -> Result<BoardLock> {
    let (board_dir, _) = initialized_board_paths(project_dir).await?;
    BoardLock::acquire(&board_dir).await
}

/// Open an initialized board, returning its directory (`.pinto/`), the [`Backend`] selected by
/// configuration, and the loaded [`Config`].
///
/// Returns [`Error::NotInitialized`] when `.pinto/config.toml` is missing. Centralizing this check
/// (and backend selection) here keeps every service function from repeating it and keeps the service
/// layer independent of the persistence implementation (file/git/sqlite).
///
/// `config.toml` is read exactly once per command and the [`Config`] is returned as-is. Not calling
/// `Config::load` again in callers halves the I/O and avoids inconsistencies from settings changing
/// between loads.
async fn open_board(project_dir: &Path) -> Result<(PathBuf, Backend, Config)> {
    let (board_dir, config_path) = initialized_board_paths(project_dir).await?;
    let config = Config::load(&config_path).await?;
    let repo = Backend::open(&board_dir, config.storage.backend).await?;
    Ok((board_dir, repo, config))
}

/// Return the board and configuration paths after the minimal initialization check.
async fn initialized_board_paths(project_dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let board_dir = project_dir.join(".pinto");
    let config_path = board_dir.join("config.toml");
    if !fs::try_exists(&config_path)
        .await
        .map_err(|e| Error::io(&config_path, &e))?
    {
        return Err(Error::NotInitialized { path: board_dir });
    }
    Ok((board_dir, config_path))
}

/// Open an initialized board for writing. Returns everything [`open_board`] does, plus the advisory
/// [`BoardLock`] that serializes writes.
///
/// Other processes are excluded only while the returned lock guard is alive, so the caller must keep
/// it bound until `save` completes (binding it for the rest of the function is enough). This stops
/// concurrent `pinto` processes (CLI/TUI) from interleaving read-modify-write and losing updates
/// (last-writer-wins). Read-only operations take no lock and use [`open_board`] instead.
async fn open_board_locked(project_dir: &Path) -> Result<(PathBuf, Backend, Config, BoardLock)> {
    open_board_locked_with_hook(project_dir, || {}).await
}

/// Open a board for writing after an optional pre-lock hook.
///
/// The hook is a synchronous test seam placed after the minimal initialization check and before
/// lock acquisition. It lets concurrency tests prove that a writer is waiting at the lock while
/// another operation changes the selected backend, without adding timing-based sleeps to the
/// production path.
async fn open_board_locked_with_hook<F>(
    project_dir: &Path,
    before_lock: F,
) -> Result<(PathBuf, Backend, Config, BoardLock)>
where
    F: FnOnce(),
{
    let (board_dir, config_path) = initialized_board_paths(project_dir).await?;
    before_lock();
    let lock = BoardLock::acquire(&board_dir).await?;
    // Backend selection is intentionally inside the lock. A migration may switch config while a
    // writer is waiting; opening the repository before acquiring the lock would cache the old
    // backend and write a successful update where the new configuration no longer reads it.
    let config = Config::load(&config_path).await?;
    let repo = Backend::open_for_write(&board_dir, config.storage.backend).await?;
    Ok((board_dir, repo, config, lock))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "sqlite")]
    use crate::backlog::{BacklogItem, ItemId, Status};
    #[cfg(feature = "sqlite")]
    use crate::rank::Rank;
    use crate::service::init_board;
    #[cfg(feature = "sqlite")]
    use crate::storage::BacklogItemRepository;
    use crate::storage::StorageBackend;
    #[cfg(feature = "sqlite")]
    use chrono::Utc;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use tokio::time::{Duration, timeout};

    #[tokio::test]
    async fn writer_selects_backend_after_waiting_for_the_board_lock() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let board_dir = dir.path().join(".pinto");
        let held = BoardLock::acquire(&board_dir).await.expect("hold lock");

        let project_dir = dir.path().to_path_buf();
        let ready = Arc::new(Notify::new());
        let ready_for_writer = Arc::clone(&ready);
        let writer = tokio::spawn(async move {
            open_board_locked_with_hook(&project_dir, move || ready_for_writer.notify_one()).await
        });
        ready.notified().await;

        let config_path = board_dir.join("config.toml");
        let mut config = Config::load(&config_path).await.expect("config");
        config.storage.backend = StorageBackend::Git;
        config.save(&config_path).await.expect("switch backend");
        drop(held);

        let (_, backend, loaded, _) = writer
            .await
            .expect("writer task")
            .expect("writer opens board");
        assert!(matches!(backend, Backend::Git(_)));
        assert_eq!(loaded.storage.backend, StorageBackend::Git);
    }

    #[tokio::test]
    async fn read_only_board_open_does_not_wait_for_write_lock() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let board_dir = dir.path().join(".pinto");
        let held = BoardLock::acquire(&board_dir).await.expect("hold lock");

        let opened = timeout(Duration::from_secs(1), open_board(dir.path()))
            .await
            .expect("read-only open should not wait for the write lock")
            .expect("open board");
        assert!(matches!(opened.1, Backend::File(_)));
        drop(held);
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn waiting_writer_saves_to_backend_selected_after_config_switch() {
        let dir = TempDir::new().expect("temp dir");
        init_board(dir.path()).await.expect("init");
        let board_dir = dir.path().join(".pinto");
        let held = BoardLock::acquire(&board_dir).await.expect("hold lock");

        let ready = Arc::new(Notify::new());
        let ready_for_writer = Arc::clone(&ready);
        let project_dir = dir.path().to_path_buf();
        let writer = tokio::spawn(async move {
            let (_board_dir, repo, config, _lock) =
                open_board_locked_with_hook(&project_dir, move || ready_for_writer.notify_one())
                    .await?;
            assert_eq!(config.storage.backend, StorageBackend::Sqlite);
            let item = BacklogItem::new(
                ItemId::new("T", 1),
                "written after migration",
                Status::new("todo"),
                Rank::after(None),
                Utc::now(),
            )?;
            BacklogItemRepository::save(&repo, &item).await?;
            repo.commit("pinto: add T-1").await?;
            Ok::<_, Error>(item)
        });
        ready.notified().await;

        let config_path = board_dir.join("config.toml");
        let mut config = Config::load(&config_path).await.expect("config");
        // This is the migration boundary: its copy phase is covered by migrate.rs tests, while
        // this test holds the lock at the exact point where the waiting writer is blocked.
        config.storage.backend = StorageBackend::Sqlite;
        config.save(&config_path).await.expect("switch backend");
        drop(held);

        let item = writer
            .await
            .expect("writer task")
            .expect("writer saves item");
        let (_, repo, loaded) = open_board(dir.path()).await.expect("open board");
        assert_eq!(loaded.storage.backend, StorageBackend::Sqlite);
        let persisted = BacklogItemRepository::load(&repo, &item.id)
            .await
            .expect("item should be visible in the selected backend");
        assert_eq!(persisted, item);
    }
}
