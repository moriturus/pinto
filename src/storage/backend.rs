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
use super::repository::{BacklogItemRepository, SprintRepository};
#[cfg(feature = "sqlite")]
use super::sqlite_repository::SqliteRepository;
use crate::backlog::{BacklogItem, ItemId};
use crate::config::StorageBackend;
use crate::error::Result;
use crate::sprint::{Sprint, SprintId};
use std::path::PathBuf;

/// Persistence backend selected in configuration.
#[derive(Debug, Clone)]
pub enum Backend {
    /// Local file backend (the default).
    File(FileRepository),
    /// Git backend, which commits every change operation.
    Git(GitRepository),
    /// SQLite backend (optional feature `sqlite`), stored in one database file.
    #[cfg(feature = "sqlite")]
    Sqlite(SqliteRepository),
}

impl Backend {
    /// Build from the board root (`.pinto/`) and the selected backend type.
    ///
    /// No I/O is performed during construction; Git repository preparation is delayed until the
    /// first commit.
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
}
