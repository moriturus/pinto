//! Corruption-tolerant file writing.

use crate::error::{Error, Result};
use std::path::Path;
use tokio::fs;

/// Write `contents` to `path` safely by writing a temporary file in the same directory and then
/// renaming it into place.
///
/// A `rename` within one filesystem is atomic, so if the process dies mid-write, `path` keeps its
/// old contents and is never left half-written. This keeps plain-text persistence robust even when
/// files are edited by hand. The caller guarantees the parent directory of `path` exists.
///
/// `fsync` is not performed, so a power loss after the `rename` but before the data reaches disk can
/// still lose the write. This guarantees consistency across a process crash but not durability
/// across power loss, an acceptable trade-off for plain-text use. If `rename` fails, remove the
/// temporary file on a best-effort basis so no stray files are left behind.
pub(crate) async fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};

    // Unique temporary file name within the same directory (process ID + sequential counter).
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_name().and_then(|s| s.to_str()).unwrap_or("pinto");
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp = dir.join(format!(".{stem}.{}.{seq}.tmp", std::process::id()));

    fs::write(&tmp, contents)
        .await
        .map_err(|e| Error::io(&tmp, &e))?;
    if let Err(e) = fs::rename(&tmp, path).await {
        // A failed rename leaves the temporary file behind; remove it best-effort while preserving
        // the original error if cleanup also fails.
        let _ = fs::remove_file(&tmp).await;
        return Err(Error::io(path, &e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn writes_contents_atomically() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("note.md");
        atomic_write(&path, "hello").await.expect("write");
        let read = fs::read_to_string(&path).await.expect("read");
        assert_eq!(read, "hello");
    }

    #[tokio::test]
    async fn removes_temp_file_when_rename_fails() {
        // Renaming onto an existing directory fails; verify that cleanup leaves no temporary file.
        let dir = tempdir().expect("tempdir");
        let target = dir.path().join("subdir");
        fs::create_dir(&target).await.expect("create dir");

        let err = atomic_write(&target, "data").await;
        assert!(err.is_err(), "rename onto a directory should fail");

        let mut entries = fs::read_dir(dir.path()).await.expect("read_dir");
        while let Some(entry) = entries.next_entry().await.expect("next entry") {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            assert!(
                !name.ends_with(".tmp"),
                "leftover temp file should be cleaned up: {name}"
            );
        }
    }
}
