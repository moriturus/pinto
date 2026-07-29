//! Persistence backend selected in configuration.
//!
//! Build the concrete implementation selected by [`crate::config::StorageBackend`] and dispatch
//! [`BacklogItemRepository`] and [`SprintRepository`] calls through one enum. The service layer
//! therefore remains independent of the concrete backend.
//!
//! The traits use RPITIT (`impl Future`) and cannot be used as `dyn` traits here, so enum dispatch
//! keeps resolution static and lightweight.

use super::file_repository::FileRepository;
use super::git_repository::GitRepository;
use super::repository::{BacklogItemRepository, SprintRecordRepository, SprintRepository};
#[cfg(feature = "sqlite")]
use super::sqlite_repository::SqliteRepository;
use crate::backlog::{BacklogItem, ItemId};
use crate::config::StorageBackend;
use crate::error::{Error, Result};
use crate::sprint::{Sprint, SprintId};
use crate::sprint_record::{SprintRecord, SprintRecordKind};
use std::path::PathBuf;

/// Persistence backend selected in configuration.
#[derive(Debug, Clone)]
pub enum Backend {
    /// Local file backend (the default).
    File(FileRepository),
    /// Git backend, which commits every change operation.
    Git(GitRepository),
    /// SQLite backend (optional feature `sqlite`). PBIs and Sprints live in one database file;
    /// Sprint Retro and Review records stay in the shared `.pinto/retro/` and `.pinto/review/`
    /// Markdown directories so they survive backend migration without a per-backend copy.
    #[cfg(feature = "sqlite")]
    Sqlite(SqliteRepository),
}

impl Backend {
    /// Save a batch of items efficiently on the file backend while preserving regular per-item
    /// repository semantics for the other backends.
    pub(crate) async fn save_item_batch(&self, items: &[BacklogItem]) -> Result<()> {
        match self {
            Backend::File(repository) => repository.save_batch(items).await,
            Backend::Git(repository) => repository.save_item_batch(items).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => repository.save_item_batch(items).await,
        }
    }

    /// Replace all active board records in one operation-specific persistence boundary.
    pub(crate) async fn replace_board(
        &self,
        items: &[BacklogItem],
        sprints: &[Sprint],
        records: &[SprintRecord],
    ) -> Result<()> {
        match self {
            Backend::File(repository) => {
                clear_active_board(repository).await?;
                repository.save_batch(items).await?;
                save_sprints_and_records(repository, sprints, records).await
            }
            Backend::Git(repository) => {
                clear_active_board(repository).await?;
                repository.save_item_batch(items).await?;
                save_sprints_and_records(repository, sprints, records).await
            }
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => repository.replace_board(items, sprints, records).await,
        }
    }

    /// Build from the board root (`.pinto/`) and the selected backend type.
    ///
    /// No I/O is performed during construction; Git repository preparation is delayed until the
    /// first commit.
    ///
    /// # Errors
    ///
    /// This constructor currently performs no I/O and returns an error only if a future backend
    /// implementation adds fallible construction. It makes no durable changes, so retrying is
    /// safe.
    pub async fn open(root: impl Into<PathBuf>, backend: StorageBackend) -> Result<Self> {
        let root = root.into();
        match backend {
            StorageBackend::File => Ok(Backend::File(FileRepository::new(root))),
            StorageBackend::Git => Ok(Backend::Git(GitRepository::new(root))),
            #[cfg(feature = "sqlite")]
            StorageBackend::Sqlite => Ok(Backend::Sqlite(SqliteRepository::new(root))),
        }
    }

    /// Build a backend for a write operation and snapshot pre-existing Git changes.
    pub(crate) async fn open_for_write(
        root: impl Into<PathBuf>,
        backend: StorageBackend,
    ) -> Result<Self> {
        let root = root.into();
        match backend {
            StorageBackend::File => Ok(Backend::File(FileRepository::new(root))),
            StorageBackend::Git => Ok(Backend::Git(GitRepository::new(root).prepare().await?)),
            #[cfg(feature = "sqlite")]
            StorageBackend::Sqlite => Ok(Backend::Sqlite(SqliteRepository::new(root))),
        }
    }

    /// Commit board-level files for a Git-backed mutation. Other backends already persist their
    /// changes transactionally and therefore have nothing to do here.
    pub(crate) async fn commit(&self, message: &str) -> Result<()> {
        match self {
            Backend::File(_) => Ok(()),
            Backend::Git(repository) => repository.commit(message).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(_) => Ok(()),
        }
    }

    /// Undo the most recent completed board mutation and return its subject.
    ///
    /// Only the Git backend records history, so it delegates to [`GitRepository::undo_last`]. The
    /// historyless backends fail with [`Error::UndoUnsupported`], which names the backend and the
    /// recovery options.
    pub(crate) async fn undo(&self) -> Result<String> {
        match self {
            Backend::File(_) => Err(Error::UndoUnsupported {
                backend: "file".to_string(),
            }),
            Backend::Git(repository) => repository.undo_last().await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(_) => Err(Error::UndoUnsupported {
                backend: "sqlite".to_string(),
            }),
        }
    }
}

impl BacklogItemRepository for Backend {
    async fn save(&self, item: &BacklogItem) -> Result<()> {
        match self {
            Backend::File(r) => BacklogItemRepository::save(r, item).await,
            Backend::Git(r) => BacklogItemRepository::save(r, item).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::save(r, item).await,
        }
    }

    async fn load(&self, id: &ItemId) -> Result<BacklogItem> {
        match self {
            Backend::File(r) => BacklogItemRepository::load(r, id).await,
            Backend::Git(r) => BacklogItemRepository::load(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::load(r, id).await,
        }
    }

    async fn list(&self) -> Result<Vec<BacklogItem>> {
        match self {
            Backend::File(r) => BacklogItemRepository::list(r).await,
            Backend::Git(r) => BacklogItemRepository::list(r).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::list(r).await,
        }
    }

    async fn list_archived(&self) -> Result<Vec<BacklogItem>> {
        match self {
            Backend::File(r) => BacklogItemRepository::list_archived(r).await,
            Backend::Git(r) => BacklogItemRepository::list_archived(r).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::list_archived(r).await,
        }
    }

    async fn load_archived(&self, id: &ItemId) -> Result<BacklogItem> {
        match self {
            Backend::File(r) => BacklogItemRepository::load_archived(r, id).await,
            Backend::Git(r) => BacklogItemRepository::load_archived(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::load_archived(r, id).await,
        }
    }

    async fn delete(&self, id: &ItemId) -> Result<()> {
        match self {
            Backend::File(r) => BacklogItemRepository::delete(r, id).await,
            Backend::Git(r) => BacklogItemRepository::delete(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::delete(r, id).await,
        }
    }

    async fn archive(&self, id: &ItemId) -> Result<PathBuf> {
        match self {
            Backend::File(r) => r.archive(id).await,
            Backend::Git(r) => r.archive(id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => r.archive(id).await,
        }
    }

    async fn restore(&self, id: &ItemId) -> Result<()> {
        match self {
            Backend::File(r) => BacklogItemRepository::restore(r, id).await,
            Backend::Git(r) => BacklogItemRepository::restore(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => BacklogItemRepository::restore(r, id).await,
        }
    }

    async fn next_id(&self, prefix: &str) -> Result<ItemId> {
        match self {
            Backend::File(r) => r.next_id(prefix).await,
            Backend::Git(r) => r.next_id(prefix).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => r.next_id(prefix).await,
        }
    }
}

impl SprintRepository for Backend {
    async fn save(&self, sprint: &Sprint) -> Result<()> {
        match self {
            Backend::File(r) => SprintRepository::save(r, sprint).await,
            Backend::Git(r) => SprintRepository::save(r, sprint).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => SprintRepository::save(r, sprint).await,
        }
    }

    async fn load(&self, id: &SprintId) -> Result<Sprint> {
        match self {
            Backend::File(r) => SprintRepository::load(r, id).await,
            Backend::Git(r) => SprintRepository::load(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => SprintRepository::load(r, id).await,
        }
    }

    async fn list(&self) -> Result<Vec<Sprint>> {
        match self {
            Backend::File(r) => SprintRepository::list(r).await,
            Backend::Git(r) => SprintRepository::list(r).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => SprintRepository::list(r).await,
        }
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        match self {
            Backend::File(r) => SprintRepository::delete(r, id).await,
            Backend::Git(r) => SprintRepository::delete(r, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(r) => SprintRepository::delete(r, id).await,
        }
    }
}

impl SprintRecordRepository for Backend {
    async fn save(&self, record: &SprintRecord) -> Result<()> {
        match self {
            Backend::File(repository) => SprintRecordRepository::save(repository, record).await,
            Backend::Git(repository) => SprintRecordRepository::save(repository, record).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => SprintRecordRepository::save(repository, record).await,
        }
    }

    async fn load(&self, kind: SprintRecordKind, id: &SprintId) -> Result<SprintRecord> {
        match self {
            Backend::File(repository) => SprintRecordRepository::load(repository, kind, id).await,
            Backend::Git(repository) => SprintRecordRepository::load(repository, kind, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => SprintRecordRepository::load(repository, kind, id).await,
        }
    }

    async fn list(&self, kind: SprintRecordKind) -> Result<Vec<SprintRecord>> {
        match self {
            Backend::File(repository) => SprintRecordRepository::list(repository, kind).await,
            Backend::Git(repository) => SprintRecordRepository::list(repository, kind).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => SprintRecordRepository::list(repository, kind).await,
        }
    }

    async fn delete(&self, kind: SprintRecordKind, id: &SprintId) -> Result<()> {
        match self {
            Backend::File(repository) => SprintRecordRepository::delete(repository, kind, id).await,
            Backend::Git(repository) => SprintRecordRepository::delete(repository, kind, id).await,
            #[cfg(feature = "sqlite")]
            Backend::Sqlite(repository) => {
                SprintRecordRepository::delete(repository, kind, id).await
            }
        }
    }
}

/// Delete every active PBI, Sprint, and Sprint child record from `repository`.
///
/// Shared by the file and Git arms of [`Backend::replace_board`]; the two backends differ only in
/// how they batch the subsequent writes.
async fn clear_active_board<R>(repository: &R) -> Result<()>
where
    R: BacklogItemRepository + SprintRepository + SprintRecordRepository,
{
    for kind in SprintRecordKind::ALL {
        for record in SprintRecordRepository::list(repository, kind).await? {
            SprintRecordRepository::delete(repository, kind, &record.id).await?;
        }
    }
    for item in BacklogItemRepository::list(repository).await? {
        BacklogItemRepository::delete(repository, &item.id).await?;
    }
    for sprint in SprintRepository::list(repository).await? {
        SprintRepository::delete(repository, &sprint.id).await?;
    }
    Ok(())
}

/// Save every Sprint and child record after the PBIs have been written by a backend-specific batch.
async fn save_sprints_and_records<R>(
    repository: &R,
    sprints: &[Sprint],
    records: &[SprintRecord],
) -> Result<()>
where
    R: SprintRepository + SprintRecordRepository,
{
    for sprint in sprints {
        SprintRepository::save(repository, sprint).await?;
    }
    for record in records {
        SprintRecordRepository::save(repository, record).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    /// You can build a file backend and save/retrieve back and forth across traits.
    #[tokio::test]
    async fn file_backend_dispatches_save_and_load() {
        let dir = TempDir::new().expect("temp dir");
        let backend = Backend::open(dir.path().join(".pinto"), StorageBackend::File)
            .await
            .expect("open file backend");

        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "Dispatch",
            Status::new("todo"),
            Rank::after(None),
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .expect("valid item");

        BacklogItemRepository::save(&backend, &item)
            .await
            .expect("save via backend");
        let loaded = BacklogItemRepository::load(&backend, &item.id)
            .await
            .expect("load via backend");
        assert_eq!(loaded, item);
    }

    /// You can build a sqlite backend, and you can save and retrieve back and forth over traits.
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn sqlite_backend_dispatches_save_and_load() {
        let dir = TempDir::new().expect("temp dir");
        let backend = Backend::open(dir.path().join(".pinto"), StorageBackend::Sqlite)
            .await
            .expect("open sqlite backend");

        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "Dispatch",
            Status::new("todo"),
            Rank::after(None),
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .expect("valid item");

        BacklogItemRepository::save(&backend, &item)
            .await
            .expect("save via backend");
        let loaded = BacklogItemRepository::load(&backend, &item.id)
            .await
            .expect("load via backend");
        assert_eq!(loaded, item);
    }

    #[tokio::test]
    async fn save_item_batch_dispatches_to_git_and_sqlite_backends() {
        let item = BacklogItem::new(
            ItemId::new("T", 1),
            "Batch",
            Status::new("todo"),
            Rank::after(None),
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .expect("valid item");

        let git_dir = TempDir::new().expect("git temp dir");
        let git = Backend::Git(GitRepository::new(git_dir.path().join(".pinto")));
        git.save_item_batch(std::slice::from_ref(&item))
            .await
            .expect("git batch save");
        assert_eq!(
            BacklogItemRepository::list(&git).await.unwrap(),
            vec![item.clone()]
        );

        #[cfg(feature = "sqlite")]
        {
            let sqlite_dir = TempDir::new().expect("sqlite temp dir");
            let sqlite = Backend::Sqlite(SqliteRepository::new(sqlite_dir.path().join(".pinto")));
            sqlite
                .save_item_batch(std::slice::from_ref(&item))
                .await
                .expect("sqlite batch save");
            assert_eq!(
                BacklogItemRepository::list(&sqlite).await.unwrap(),
                vec![item]
            );
        }
    }

    #[tokio::test]
    async fn historyless_backends_report_undo_unsupported() {
        let file_dir = TempDir::new().expect("file temp dir");
        assert!(matches!(
            Backend::File(FileRepository::new(file_dir.path().join(".pinto")))
                .undo()
                .await,
            Err(Error::UndoUnsupported { backend }) if backend == "file"
        ));

        #[cfg(feature = "sqlite")]
        {
            let sqlite_dir = TempDir::new().expect("sqlite temp dir");
            assert!(matches!(
                Backend::Sqlite(SqliteRepository::new(sqlite_dir.path().join(".pinto")))
                    .undo()
                    .await,
                Err(Error::UndoUnsupported { backend }) if backend == "sqlite"
            ));
        }
    }
}
