//! Undo the most recent completed board mutation.
//!
//! Recovery is backend-specific: only the Git backend records each mutation as a commit, so it can
//! revert the last one. The historyless backends refuse with an actionable [`crate::error::Error::UndoUnsupported`]
//! from the backend layer. See `docs/book/src/undo.md` for the feature decision record.

use super::open_board_locked;
use crate::error::Result;
use std::path::Path;

/// The result of a successful undo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoOutcome {
    /// The subject of the board mutation that was reverted (e.g. `pinto: add T-1`).
    pub reverted: String,
}

/// Revert the most recent completed board mutation.
///
/// The operation runs under the board write lock, serializing it against other writers just like an
/// ordinary mutation. On the Git backend it creates a revert commit and returns the reverted
/// subject; on backends without history it returns [`crate::error::Error::UndoUnsupported`].
pub async fn undo_last_mutation(project_dir: &Path) -> Result<UndoOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let reverted = repo.undo().await?;
    Ok(UndoOutcome { reverted })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, StorageBackend};
    use crate::error::Error;
    use crate::service::{ListFilter, NewItem, add_item, init_board, list_items};
    use tempfile::TempDir;

    async fn init_with_backend(dir: &Path, backend: StorageBackend) {
        init_board(dir).await.expect("init");
        let config_path = dir.join(".pinto/config.toml");
        let mut config = Config::load(&config_path).await.expect("config");
        config.storage.backend = backend;
        config.save(&config_path).await.expect("save config");
    }

    #[tokio::test]
    async fn undo_reverts_the_last_git_mutation() {
        let dir = TempDir::new().expect("temp dir");
        init_with_backend(dir.path(), StorageBackend::Git).await;
        let first = add_item(dir.path(), "First", NewItem::default())
            .await
            .expect("add");
        let second = add_item(dir.path(), "Second", NewItem::default())
            .await
            .expect("add");

        let outcome = undo_last_mutation(dir.path()).await.expect("undo");
        assert_eq!(outcome.reverted, format!("pinto: add {}", second.id));

        // The reverted item is gone; the earlier one remains listed.
        let ids: Vec<_> = list_items(dir.path(), &ListFilter::default())
            .await
            .expect("list")
            .into_iter()
            .map(|item| item.id)
            .collect();
        assert!(ids.contains(&first.id), "first item remains");
        assert!(!ids.contains(&second.id), "second item reverted");
    }

    #[tokio::test]
    async fn undo_on_file_backend_is_unsupported() {
        let dir = TempDir::new().expect("temp dir");
        init_with_backend(dir.path(), StorageBackend::File).await;
        add_item(dir.path(), "First", NewItem::default())
            .await
            .expect("add");

        let err = undo_last_mutation(dir.path())
            .await
            .expect_err("file backend keeps no history");
        assert!(
            matches!(err, Error::UndoUnsupported { backend } if backend == "file"),
            "expected UndoUnsupported(file)"
        );
    }
}
