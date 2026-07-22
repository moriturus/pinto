//! Persistent history of item IDs issued by a board.
//!
//! The history is deliberately a small plain-text file shared by every storage backend. This
//! keeps IDs from being reused when an item is physically deleted or when a board changes backend.

use super::atomic_write;
use crate::backlog::ItemId;
use crate::error::{Error, Result};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use tokio::fs;

const ISSUED_IDS_FILE: &str = "issued_ids";

/// Return the board-local path containing one issued item ID per line.
pub(crate) fn path(root: &Path) -> PathBuf {
    root.join(ISSUED_IDS_FILE)
}

/// Record `id` once in the board's plain-text ID history.
pub(crate) async fn record(root: &Path, id: &ItemId) -> Result<()> {
    fs::create_dir_all(root)
        .await
        .map_err(|error| Error::io(root, &error))?;

    let path = path(root);
    let contents = match fs::read_to_string(&path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(Error::io(&path, &error)),
    };
    let id = id.to_string();
    if contents.lines().any(|line| line.trim() == id) {
        return Ok(());
    }

    let mut updated = contents;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&id);
    updated.push('\n');
    atomic_write(&path, &updated).await
}

/// Record a batch of item IDs with one read and one atomic replacement.
pub(crate) async fn record_many(root: &Path, ids: &[ItemId]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    fs::create_dir_all(root)
        .await
        .map_err(|error| Error::io(root, &error))?;

    let path = path(root);
    let contents = match fs::read_to_string(&path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(Error::io(&path, &error)),
    };
    let mut known = contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<HashSet<_>>();
    let mut updated = contents;
    for id in ids {
        let id = id.to_string();
        if !known.insert(id.clone()) {
            continue;
        }
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push_str(&id);
        updated.push('\n');
    }

    atomic_write(&path, &updated).await
}

/// Read the largest issued number for `prefix`.
pub(crate) async fn max_number(root: &Path, prefix: &str) -> Result<u32> {
    let path = path(root);
    let contents = match fs::read_to_string(&path).await {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(Error::io(&path, &error)),
    };

    let mut max = 0;
    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id = line.parse::<ItemId>().map_err(|error| {
            Error::parse(
                &path,
                format!(
                    "invalid issued item ID on line {}: {error}",
                    line_number + 1
                ),
            )
        })?;
        if id.prefix() == prefix {
            max = max.max(id.number());
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn records_ids_once_and_reads_the_largest_number() {
        let dir = tempdir().expect("tempdir");
        record(dir.path(), &ItemId::new("T", 3))
            .await
            .expect("record");
        record(dir.path(), &ItemId::new("T", 1))
            .await
            .expect("record");
        record(dir.path(), &ItemId::new("T", 3))
            .await
            .expect("duplicate record is a no-op");

        assert_eq!(max_number(dir.path(), "T").await.expect("max"), 3);
        assert_eq!(
            fs::read_to_string(path(dir.path())).await.expect("history"),
            "T-3\nT-1\n"
        );
    }

    #[tokio::test]
    async fn records_a_batch_once_without_duplicate_history() {
        let dir = tempdir().expect("tempdir");
        record_many(
            dir.path(),
            &[
                ItemId::new("T", 3),
                ItemId::new("T", 1),
                ItemId::new("T", 3),
            ],
        )
        .await
        .expect("record batch");

        assert_eq!(
            fs::read_to_string(path(dir.path())).await.expect("history"),
            "T-3\nT-1\n"
        );
    }

    #[tokio::test]
    async fn rejects_corrupted_history() {
        let dir = tempdir().expect("tempdir");
        fs::write(path(dir.path()), "T-1\nnot-an-id\n")
            .await
            .expect("write history");

        let error = max_number(dir.path(), "T")
            .await
            .expect_err("corrupted history must fail");
        assert!(matches!(error, Error::Parse { .. }));
    }
}
