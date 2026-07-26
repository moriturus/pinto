//! Temporary pre-operation snapshots for multi-record board mutations.

use crate::error::{Error, Result};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tempfile::TempDir;
use tokio::fs;

/// A complete board snapshot that can restore the state before a failed operation.
///
/// The live `.pinto/.lock` is deliberately excluded. The caller keeps its lock guard alive while
/// restoring, so copying the lock marker would replace the file that owns the OS lock.
#[derive(Debug)]
pub(crate) struct BoardRecoveryPoint {
    board_dir: PathBuf,
    snapshot: TempDir,
}

impl BoardRecoveryPoint {
    /// Capture every board entry except the live lock file before a mutation starts.
    pub(crate) async fn capture(board_dir: &Path) -> Result<Self> {
        let snapshot = tempfile::tempdir().map_err(|error| Error::io(board_dir, &error))?;
        if let Err(error) = copy_tree(board_dir, snapshot.path(), true).await {
            drop(snapshot);
            return Err(error);
        }
        Ok(Self {
            board_dir: board_dir.to_path_buf(),
            snapshot,
        })
    }

    /// Restore the captured state while preserving the current lock marker.
    pub(crate) async fn restore(&self) -> Result<()> {
        let mut entries = fs::read_dir(&self.board_dir)
            .await
            .map_err(|error| Error::io(&self.board_dir, &error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| Error::io(&self.board_dir, &error))?
        {
            if entry.file_name() == ".lock" {
                continue;
            }
            remove_entry(&entry.path()).await?;
        }
        copy_tree(self.snapshot.path(), &self.board_dir, false).await
    }

    /// Return the live board path for diagnostics when automatic recovery itself fails.
    pub(crate) fn board_dir(&self) -> &Path {
        &self.board_dir
    }

    /// Keep the temporary snapshot on disk for manual recovery when automatic restoration fails.
    pub(crate) fn preserve_snapshot(self) -> PathBuf {
        self.snapshot.keep()
    }
}

fn copy_tree<'a>(
    source: &'a Path,
    destination: &'a Path,
    skip_lock: bool,
) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        fs::create_dir_all(destination)
            .await
            .map_err(|error| Error::io(destination, &error))?;
        let mut entries = fs::read_dir(source)
            .await
            .map_err(|error| Error::io(source, &error))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|error| Error::io(source, &error))?
        {
            if skip_lock && entry.file_name() == ".lock" {
                continue;
            }
            let source_path = entry.path();
            let destination_path = destination.join(entry.file_name());
            let file_type = entry
                .file_type()
                .await
                .map_err(|error| Error::io(&source_path, &error))?;
            if file_type.is_dir() {
                copy_tree(&source_path, &destination_path, false).await?;
            } else {
                fs::copy(&source_path, &destination_path)
                    .await
                    .map_err(|error| Error::io(&destination_path, &error))?;
            }
        }
        Ok(())
    })
}

async fn remove_entry(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .await
        .map_err(|error| Error::io(path, &error))?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
            .await
            .map_err(|error| Error::io(path, &error))?;
    } else {
        fs::remove_file(path)
            .await
            .map_err(|error| Error::io(path, &error))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recovery_point_restores_files_and_keeps_lock_marker() {
        let board = tempfile::tempdir().expect("board");
        fs::create_dir_all(board.path().join("tasks"))
            .await
            .expect("tasks");
        fs::write(board.path().join("tasks/T-1.md"), "old")
            .await
            .expect("item");
        fs::write(board.path().join("config.toml"), "old config")
            .await
            .expect("config");
        fs::write(board.path().join(".lock"), "current owner")
            .await
            .expect("lock marker");

        let point = BoardRecoveryPoint::capture(board.path())
            .await
            .expect("capture");
        fs::remove_dir_all(board.path().join("tasks"))
            .await
            .expect("mutate");
        fs::write(board.path().join("config.toml"), "new config")
            .await
            .expect("mutate config");
        point.restore().await.expect("restore");

        assert_eq!(
            fs::read_to_string(board.path().join("tasks/T-1.md"))
                .await
                .expect("restored item"),
            "old"
        );
        assert_eq!(
            fs::read_to_string(board.path().join("config.toml"))
                .await
                .expect("restored config"),
            "old config"
        );
        assert_eq!(
            fs::read_to_string(board.path().join(".lock"))
                .await
                .expect("lock marker"),
            "current owner"
        );
    }

    #[tokio::test]
    async fn recovery_point_can_be_kept_for_manual_recovery() {
        let board = tempfile::tempdir().expect("board");
        fs::write(board.path().join("config.toml"), "old config")
            .await
            .expect("config");

        let point = BoardRecoveryPoint::capture(board.path())
            .await
            .expect("capture");
        let snapshot = point.preserve_snapshot();

        assert_eq!(
            fs::read_to_string(snapshot.join("config.toml"))
                .await
                .expect("kept config"),
            "old config"
        );
        fs::remove_dir_all(snapshot)
            .await
            .expect("cleanup snapshot");
    }
}
