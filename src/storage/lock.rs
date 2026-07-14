//! Exclusive control of board writing (advisory file lock).

use crate::error::{Error, Result};
use fs4::tokio::AsyncFileExt;
use std::io::{ErrorKind, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

/// Lock file name (directly under `.pinto/`).
const LOCK_FILE: &str = ".lock";
/// Delay between lock-acquisition attempts.
const RETRY_INTERVAL: Duration = Duration::from_millis(50);
/// Default maximum time to wait for another process to finish.
const DEFAULT_MAX_WAIT: Duration = Duration::from_secs(5);
/// Environment variable used to extend the wait for slow filesystems or Git hooks.
const MAX_WAIT_ENV: &str = "PINTO_LOCK_TIMEOUT_SECS";

/// Advisory RAII guard that serializes board writes.
///
/// The lock is acquired by opening `.pinto/.lock` and taking an OS-level exclusive lock. Unix and
/// macOS use `flock`, while Windows uses `LockFileEx`; both locks are released by the kernel when
/// the owner process terminates. If the file is already locked, acquisition retries until the wait
/// limit is reached, then returns [`Error::Locked`].
///
/// Serializing read-modify-write sequences (`list`/`load` → change → `save`) prevents concurrent
/// CLI/TUI processes from losing updates. Read-only operations do not acquire this lock.
///
/// The PID written to the file is diagnostic text only and is never used to decide ownership. This
/// avoids incorrectly reclaiming a lock when the recorded PID has been reused by another process.
#[derive(Debug)]
pub struct BoardLock {
    path: PathBuf,
    file: Option<tokio::fs::File>,
    identity: FileIdentity,
}

/// Stable identity of the opened lock file, used to avoid deleting a replacement path on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial_number: u32,
        file_index: u64,
    },
}

#[cfg(unix)]
fn identity_from_metadata(metadata: &std::fs::Metadata) -> std::io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;

    Ok(FileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn identity_from_metadata(metadata: &std::fs::Metadata) -> std::io::Result<FileIdentity> {
    use std::os::windows::fs::MetadataExt;

    let (Some(volume_serial_number), Some(file_index)) =
        (metadata.volume_serial_number(), metadata.file_index())
    else {
        return Err(std::io::Error::other("lock file identity is unavailable"));
    };
    Ok(FileIdentity::Windows {
        volume_serial_number,
        file_index,
    })
}

#[cfg(not(any(unix, windows)))]
fn identity_from_metadata(_metadata: &std::fs::Metadata) -> std::io::Result<FileIdentity> {
    Err(std::io::Error::other(
        "lock file identity is unsupported on this platform",
    ))
}

async fn file_identity(file: &tokio::fs::File) -> std::io::Result<FileIdentity> {
    let metadata = file.metadata().await?;
    identity_from_metadata(&metadata)
}

async fn path_identity(path: &Path) -> std::io::Result<FileIdentity> {
    let metadata = tokio::fs::metadata(path).await?;
    identity_from_metadata(&metadata)
}

fn locked_error(path: &Path) -> Error {
    Error::Locked {
        path: path.to_path_buf(),
    }
}

/// Resolve the lock wait limit without loading board configuration.
///
/// Write operations acquire the lock before opening `config.toml`, so a board-level setting cannot
/// safely control this value. An optional process environment override keeps the normal path
/// simple while allowing slow filesystems and Git hooks to use a longer timeout.
fn max_wait_from_env(value: Option<&str>) -> Duration {
    value
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_MAX_WAIT)
}

async fn write_owner_marker(file: &mut tokio::fs::File) -> std::io::Result<()> {
    file.set_len(0).await?;
    file.seek(SeekFrom::Start(0)).await?;
    file.write_all(format!("{}\n", std::process::id()).as_bytes())
        .await?;
    file.flush().await
}

impl BoardLock {
    /// Acquire the lock for `board_dir` (`.pinto/`), retrying briefly if another process holds it.
    ///
    /// Waits up to the default timeout, or `PINTO_LOCK_TIMEOUT_SECS` when set, for the other
    /// process to release it, then returns [`Error::Locked`].
    pub(crate) async fn acquire(board_dir: &Path) -> Result<Self> {
        let configured = std::env::var(MAX_WAIT_ENV).ok();
        Self::acquire_within(board_dir, max_wait_from_env(configured.as_deref())).await
    }

    /// Acquire the lock, waiting no longer than `max_wait`.
    async fn acquire_within(board_dir: &Path, max_wait: Duration) -> Result<Self> {
        let path = board_dir.join(LOCK_FILE);
        let start = Instant::now();
        loop {
            let file = match tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .await
            {
                Ok(file) => file,
                Err(error) => return Err(Error::io(&path, &error)),
            };

            match file.try_lock() {
                Ok(()) => {
                    let identity = match file_identity(&file).await {
                        Ok(identity) => identity,
                        Err(_) => {
                            let _ = file.unlock();
                            return Err(locked_error(&path));
                        }
                    };

                    let path_matches = match path_identity(&path).await {
                        Ok(path_identity) => path_identity == identity,
                        Err(error) if error.kind() == ErrorKind::NotFound => false,
                        Err(_) => {
                            let _ = file.unlock();
                            return Err(locked_error(&path));
                        }
                    };
                    if !path_matches {
                        let _ = file.unlock();
                        if start.elapsed() >= max_wait {
                            return Err(locked_error(&path));
                        }
                        tokio::time::sleep(RETRY_INTERVAL).await;
                        continue;
                    }

                    let mut file = file;
                    if let Err(error) = write_owner_marker(&mut file).await {
                        let _ = file.unlock();
                        return Err(Error::io(&path, &error));
                    }
                    return Ok(Self {
                        path,
                        file: Some(file),
                        identity,
                    });
                }
                Err(fs4::TryLockError::WouldBlock) => {
                    if start.elapsed() >= max_wait {
                        return Err(locked_error(&path));
                    }
                    tokio::time::sleep(RETRY_INTERVAL).await;
                }
                Err(fs4::TryLockError::Error(error)) => return Err(Error::io(&path, &error)),
            }
        }
    }
}

impl Drop for BoardLock {
    fn drop(&mut self) {
        let Some(file) = self.file.take() else {
            return;
        };

        // Remove the path while the OS lock is still held. Releasing first would let another
        // process acquire the path before this cleanup, and then delete that process's lock.
        // Compare the file identity so a manually replaced path is never removed accidentally.
        let same_file = std::fs::metadata(&self.path)
            .ok()
            .and_then(|metadata| identity_from_metadata(&metadata).ok())
            == Some(self.identity);
        if same_file {
            let _ = std::fs::remove_file(&self.path);
        }
        let _ = file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn acquire_then_release_allows_reacquire() {
        let dir = tempdir().expect("tempdir");
        let lock = BoardLock::acquire(dir.path()).await.expect("first acquire");
        // The lock file exists while the guard is held.
        assert!(dir.path().join(LOCK_FILE).exists());
        drop(lock);
        // Releasing the guard removes the file, so the lock can be acquired again.
        assert!(!dir.path().join(LOCK_FILE).exists());
        BoardLock::acquire(dir.path()).await.expect("reacquire");
    }

    #[test]
    fn lock_timeout_uses_the_default_and_accepts_a_valid_environment_value() {
        assert_eq!(max_wait_from_env(None), DEFAULT_MAX_WAIT);
        assert_eq!(
            max_wait_from_env(Some("30")),
            Duration::from_secs(30),
            "a slow Git hook can extend the wait through the environment"
        );
        assert_eq!(
            max_wait_from_env(Some("not-a-duration")),
            DEFAULT_MAX_WAIT,
            "invalid values fall back to the safe default"
        );
    }

    #[tokio::test]
    async fn second_acquire_while_held_times_out_with_locked() {
        let dir = tempdir().expect("tempdir");
        let _held = BoardLock::acquire(dir.path()).await.expect("hold");
        // A second acquisition returns `Locked` after the configured wait limit; use a short limit
        // here so the test remains fast.
        let err = BoardLock::acquire_within(dir.path(), Duration::from_millis(120))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Locked { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn stale_lock_owned_by_a_dead_process_is_recovered() {
        let dir = tempdir().expect("tempdir");
        tokio::fs::write(dir.path().join(LOCK_FILE), "999999999\n")
            .await
            .expect("write stale lock");

        let lock = BoardLock::acquire_within(dir.path(), Duration::from_millis(120))
            .await
            .expect("recover stale lock");

        drop(lock);
        assert!(!dir.path().join(LOCK_FILE).exists());
    }

    #[tokio::test]
    async fn pid_reuse_like_lock_file_is_recovered_without_trusting_pid_text() {
        let dir = tempdir().expect("tempdir");
        tokio::fs::write(
            dir.path().join(LOCK_FILE),
            format!("{}\n", std::process::id()),
        )
        .await
        .expect("write PID-reuse-like lock");

        let lock = BoardLock::acquire_within(dir.path(), Duration::from_millis(120))
            .await
            .expect("recover lock whose PID text belongs to another owner");

        drop(lock);
    }

    #[test]
    fn lock_test_child() {
        let Some(board_dir) = std::env::var_os("PINTO_LOCK_TEST_BOARD") else {
            return;
        };
        let ready = std::path::PathBuf::from(
            std::env::var_os("PINTO_LOCK_TEST_READY").expect("ready path"),
        );
        let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
        let _lock = runtime
            .block_on(BoardLock::acquire(Path::new(&board_dir)))
            .expect("child lock");
        std::fs::write(ready, b"ready").expect("ready marker");
        std::thread::park();
    }

    #[tokio::test]
    async fn lock_left_by_a_terminated_process_is_recovered() {
        struct ChildGuard(std::process::Child);

        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let dir = tempdir().expect("tempdir");
        let ready = dir.path().join("lock-child.ready");
        let mut child = ChildGuard(
            std::process::Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", "storage::lock::tests::lock_test_child"])
                .env("PINTO_LOCK_TEST_BOARD", dir.path())
                .env("PINTO_LOCK_TEST_READY", &ready)
                .spawn()
                .expect("spawn lock child"),
        );

        for _ in 0..200 {
            if ready.is_file() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ready.is_file(), "child did not acquire the lock");

        child.0.kill().expect("terminate lock child");
        child.0.wait().expect("wait for lock child");

        let lock = BoardLock::acquire_within(dir.path(), Duration::from_millis(120))
            .await
            .expect("recover lock after owner termination");
        drop(lock);
    }
}
