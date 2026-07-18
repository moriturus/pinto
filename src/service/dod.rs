//! Common DoD (Definition of Done) services.
//!
//! Manage the Definition of Done shared by all backlog items. It complements each item's
//! Acceptance Criteria and is stored as plain Markdown in `.pinto/dod.md` (without frontmatter).

use super::{open_board, open_board_locked};
use crate::error::{Error, Result};
use crate::storage::atomic_write;
use std::path::{Path, PathBuf};
use tokio::fs;

/// File name to save the common DoD (directly under `.pinto/`).
const DOD_FILE: &str = "dod.md";

/// Load the board's common DoD, returning `None` when the file is absent or empty.
///
/// Return the content trimmed of leading and trailing whitespace. Return [`Error::NotInitialized`]
/// when the board is uninitialized.
pub async fn common_dod(project_dir: &Path) -> Result<Option<String>> {
    let (board_dir, _repo, _config) = open_board(project_dir).await?;
    read_common_dod(&board_dir).await
}

/// Load the common DoD from an already opened board directory.
pub(crate) async fn read_common_dod(board_dir: &Path) -> Result<Option<String>> {
    let path = board_dir.join(DOD_FILE);
    match fs::read_to_string(&path).await {
        Ok(text) => {
            let trimmed = text.trim();
            Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(&path, &e)),
    }
}

/// Replace the board's common DoD and return the written file path.
///
/// Return [`Error::EmptyDod`] when `text` is blank or [`Error::NotInitialized`] when the board is
/// uninitialized.
pub async fn set_common_dod(project_dir: &Path, text: &str) -> Result<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::EmptyDod);
    }
    let (board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let path = board_dir.join(DOD_FILE);
    // Add a newline at the end to make it Git-friendly (same as existing task files).
    let body = format!("{trimmed}\n");
    atomic_write(&path, &body).await?;
    repo.commit("pinto: update common DoD").await?;
    Ok(path)
}

/// Delete a board's common DoD. `true` if it exists, `false` (idempotent) if it does not exist.
///
/// [`Error::NotInitialized`] if the board is uninitialized.
pub async fn clear_common_dod(project_dir: &Path) -> Result<bool> {
    let (board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let path = board_dir.join(DOD_FILE);
    match fs::remove_file(&path).await {
        Ok(()) => {
            repo.commit("pinto: remove common DoD").await?;
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Error::io(&path, &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::init_temp;

    #[tokio::test]
    async fn common_dod_is_none_when_unset() {
        let dir = init_temp().await;
        assert_eq!(common_dod(dir.path()).await.unwrap(), None);
    }

    #[tokio::test]
    async fn set_then_load_roundtrips() {
        let dir = init_temp().await;
        set_common_dod(dir.path(), "- [ ] tests pass\n- [ ] reviewed")
            .await
            .expect("set succeeds");
        assert_eq!(
            common_dod(dir.path()).await.unwrap().as_deref(),
            Some("- [ ] tests pass\n- [ ] reviewed"),
        );
    }

    #[tokio::test]
    async fn set_replaces_previous_value() {
        let dir = init_temp().await;
        set_common_dod(dir.path(), "old").await.unwrap();
        set_common_dod(dir.path(), "new").await.unwrap();
        assert_eq!(
            common_dod(dir.path()).await.unwrap().as_deref(),
            Some("new")
        );
    }

    #[tokio::test]
    async fn set_persists_as_plain_markdown_file() {
        let dir = init_temp().await;
        let path = set_common_dod(dir.path(), "- [ ] done").await.unwrap();
        assert_eq!(path, dir.path().join(".pinto").join("dod.md"));
        let raw = std::fs::read_to_string(&path).expect("file exists");
        // Plain Markdown with no front matter (one trailing newline).
        assert!(!raw.starts_with("+++"), "no frontmatter: {raw:?}");
        assert_eq!(raw, "- [ ] done\n");
    }

    #[tokio::test]
    async fn set_rejects_empty_text() {
        let dir = init_temp().await;
        let err = set_common_dod(dir.path(), "   \n\t").await.unwrap_err();
        assert!(matches!(err, Error::EmptyDod), "got {err:?}");
    }

    #[tokio::test]
    async fn clear_removes_and_is_idempotent() {
        let dir = init_temp().await;
        set_common_dod(dir.path(), "x").await.unwrap();
        assert!(clear_common_dod(dir.path()).await.unwrap(), "removed");
        assert_eq!(common_dod(dir.path()).await.unwrap(), None);
        assert!(
            !clear_common_dod(dir.path()).await.unwrap(),
            "already absent"
        );
    }

    #[tokio::test]
    async fn common_dod_without_init_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let err = common_dod(dir.path()).await.unwrap_err();
        assert!(matches!(err, Error::NotInitialized { .. }), "got {err:?}");
    }
}
