//! Git backend.
//!
//! Add Git commits to backlog-item and sprint mutations without changing the plain-text storage
//! provided by [`FileRepository`]. Git supplies the history and undo operations.
//!
//! **Policy**: invoke the `git` CLI through [`tokio::process`] to avoid another dependency and keep
//! the backend lightweight. Waiting for the subprocess is asynchronous (see `docs/DESIGN.md` §3.4).
//!
//! **Behavior when Git is uninitialized or absent**:
//! - If the directory containing `.pinto` is not a Git repository, `git init` runs automatically.
//! - If `git` is not on `PATH`, return [`Error::Git`] and suggest installing Git or setting
//!   `[storage] backend = "file"`.
//!
//! **Commit message convention**: `pinto: <verb> <id>` (`add` / `update` / `remove` / `archive`).

use super::file_repository::FileRepository;
use super::repository::{BacklogItemRepository, SprintRepository};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::sprint::{Sprint, SprintId};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

/// File persistence with a thin Git-commit layer.
#[derive(Debug, Clone)]
pub struct GitRepository {
    /// File implementation responsible for actual reading and writing.
    file: FileRepository,
    /// The root of the board (`.pinto/`).
    root: PathBuf,
    /// Working directory to run git (parent of `.pinto` = project root).
    workdir: PathBuf,
    /// Paths that were already dirty when a write operation opened the backend.
    ///
    /// They remain in the user's worktree/index and are deliberately excluded from the pinto
    /// commit. `GitRepository::new` leaves this empty for the low-level repository API; the service
    /// layer uses [`Self::prepare`] before it starts a board mutation.
    baseline_paths: Vec<String>,
}

impl GitRepository {
    /// Build a repository for `.pinto/` without performing I/O. Git preparation is delayed until
    /// the first commit; read-only operations do not invoke Git.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        // Git runs from the parent of `.pinto`; use the root itself when no parent exists.
        let workdir = root
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let file = FileRepository::new(root.clone());
        Self {
            file,
            root,
            workdir,
            baseline_paths: Vec::new(),
        }
    }

    /// Capture the worktree state before a board mutation starts.
    ///
    /// Read-only backend construction still uses [`Self::new`], so it does not invoke Git. The
    /// service write path calls this method after acquiring the board lock. A repository that has
    /// not been initialized yet has no baseline; its first migration/write is treated as the
    /// initial board commit.
    pub(crate) async fn prepare(mut self) -> Result<Self> {
        self.baseline_paths = match self.status_paths().await {
            Ok(paths) => paths,
            // `git status` is expected to fail before the Git backend's lazy `git init`. Keep the
            // error deferred until the actual commit, where it can explain how to fix Git.
            Err(Error::Git(_)) => Vec::new(),
            Err(error) => return Err(error),
        };
        Ok(self)
    }

    /// A pathspec (relative to `workdir`) that limits git commits to the `.pinto` subtree.
    ///
    /// This avoids including unrelated changes that the user has staged outside of `.pinto`.
    fn board_pathspec(&self) -> &str {
        self.root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(".pinto")
    }

    /// Pathspec for the transient lock marker.
    fn lock_pathspec(&self) -> String {
        format!("{}/.lock", self.board_pathspec())
    }

    /// Run `git <args>` in `workdir` and return its raw output. Map a missing executable to
    /// [`Error::Git`].
    async fn run_git(&self, args: &[&str]) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.workdir)
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::Git(
                        "`git` command not found; install git or set `[storage] backend = \"file\"`"
                            .to_string(),
                    )
                } else {
                    Error::Git(format!("failed to run git {}: {e}", args.join(" ")))
                }
            })
    }

    /// Run `git <args>` and map a non-zero exit status to [`Error::Git`], including stderr.
    async fn git_checked(&self, args: &[&str]) -> Result<Output> {
        let out = self.run_git(args).await?;
        if out.status.success() {
            Ok(out)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(Error::Git(format!(
                "git {} failed: {}",
                args.join(" "),
                stderr.trim()
            )))
        }
    }

    /// Run `git <args>` with an alternate index file.
    async fn run_git_with_index(&self, args: &[&str], index: &Path) -> Result<Output> {
        Command::new("git")
            .args(args)
            .current_dir(&self.workdir)
            .env("GIT_INDEX_FILE", index)
            .output()
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::Git(
                        "`git` command not found; install git or set `[storage] backend = \"file\"`"
                            .to_string(),
                    )
                } else {
                    Error::Git(format!("failed to run git {}: {e}", args.join(" ")))
                }
            })
    }

    /// Run a Git command with an alternate index and include stderr on failure.
    async fn git_checked_with_index(&self, args: &[&str], index: &Path) -> Result<Output> {
        let out = self.run_git_with_index(args, index).await?;
        if out.status.success() {
            Ok(out)
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            Err(Error::Git(format!(
                "git {} failed: {}",
                args.join(" "),
                stderr.trim()
            )))
        }
    }

    /// Ensure `workdir` is a Git repository, initializing it when necessary.
    async fn ensure_repo(&self) -> Result<()> {
        let inside = self
            .run_git(&["rev-parse", "--is-inside-work-tree"])
            .await?;
        if !inside.status.success() {
            eprintln!(
                "warning: initializing a Git repository for the selected git backend in {}",
                self.workdir.display()
            );
            self.git_checked(&["init"]).await?;
        }
        Ok(())
    }

    /// Record one board mutation as one commit; do nothing when there are no changes.
    ///
    /// Build and commit an isolated temporary index rooted at `HEAD`. Only paths that became dirty
    /// after [`Self::prepare`] are copied into it, so pre-existing staged, unstaged, or untracked
    /// changes remain outside the pinto commit. The real index is rebuilt from the new `HEAD` and
    /// the caller's pre-existing staged entries are restored afterwards.
    ///
    /// The transient `.pinto/.lock` is never copied into the temporary index. If an old checkout
    /// tracked that path, it is removed from the new tree while the live lock file remains open;
    /// this avoids racing another writer by unlinking the lock before the guard is dropped.
    ///
    /// If Git fails, durable board files remain in the worktree and the user's real index is not
    /// changed. The caller can inspect/fix Git and retry the same operation or commit the durable
    /// files manually.
    pub(crate) async fn commit(&self, message: &str) -> Result<()> {
        self.ensure_repo().await?;
        let current = self.status_paths().await?;
        let baseline: HashSet<&str> = self.baseline_paths.iter().map(String::as_str).collect();
        let mut operation_paths: Vec<String> = current
            .into_iter()
            .filter(|path| !baseline.contains(path.as_str()))
            .collect();
        operation_paths.sort();
        operation_paths.dedup();

        let lock_path = self.lock_pathspec();
        let lock_tracked = self.index_has_path(&lock_path).await?;
        if operation_paths.is_empty() && !lock_tracked {
            return Ok(());
        }

        let staged = self.capture_staged_index().await?;
        if staged.entries.iter().any(|entry| entry.stage != 0) {
            return Err(Error::Git(
                "cannot commit while the Git index has unresolved conflicts; resolve them and retry"
                    .to_string(),
            ));
        }

        self.commit_with_isolated_index(message, &operation_paths, &lock_path, &staged)
            .await
    }

    /// Return changed paths relative to the current `HEAD`, excluding the transient lock marker.
    async fn status_paths(&self) -> Result<Vec<String>> {
        let spec = self.board_pathspec();
        let lock_spec = format!(":(exclude){spec}/.lock");
        let args = [
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--",
            spec,
            lock_spec.as_str(),
        ];
        let output = self.git_checked(&args).await?;
        Ok(parse_status_paths(&output.stdout))
    }

    /// Check whether `path` is present in the current real index.
    async fn index_has_path(&self, path: &str) -> Result<bool> {
        let output = self
            .run_git(&["ls-files", "--error-unmatch", "--", path])
            .await?;
        Ok(output.status.success())
    }

    /// Capture the user's staged index entries before the isolated commit changes `HEAD`.
    async fn capture_staged_index(&self) -> Result<StagedIndex> {
        let staged_output = self
            .run_git(&["diff", "--cached", "--name-only", "-z"])
            .await?;
        let staged_paths = if staged_output.status.success() {
            parse_nul_paths(&staged_output.stdout)
                .into_iter()
                .collect::<HashSet<_>>()
        } else {
            HashSet::new()
        };

        let index_output = self.git_checked(&["ls-files", "--stage", "-z"]).await?;
        let entries = parse_index_entries(&index_output.stdout)
            .into_iter()
            .filter(|entry| staged_paths.contains(&entry.path))
            .collect();
        Ok(StagedIndex {
            paths: staged_paths,
            entries,
        })
    }

    /// Commit an operation through a temporary index, leaving the user's index isolated.
    async fn commit_with_isolated_index(
        &self,
        message: &str,
        operation_paths: &[String],
        lock_path: &str,
        staged: &StagedIndex,
    ) -> Result<()> {
        let index = temporary_index_path();
        let result = async {
            let head = self.head().await?;
            if let Some(head) = head.as_deref() {
                self.git_checked_with_index(&["read-tree", head], &index)
                    .await?;
            } else {
                self.git_checked_with_index(&["read-tree", "--empty"], &index)
                    .await?;
            }

            if !operation_paths.is_empty() {
                let mut add_args = vec!["add", "-A", "--"];
                add_args.extend(operation_paths.iter().map(String::as_str));
                self.git_checked_with_index(&add_args, &index).await?;
            }

            // Do not stage the live lock contents. Remove the path from the temporary index while
            // the OS lock stays held; BoardLock removes the live marker when its guard is dropped.
            if self
                .run_git_with_index(&["ls-files", "--error-unmatch", "--", lock_path], &index)
                .await?
                .status
                .success()
            {
                self.git_checked_with_index(
                    &["update-index", "--force-remove", "--", lock_path],
                    &index,
                )
                .await?;
            }

            // Let users commit with their configured identity; use pinto's fallback only when Git
            // cannot resolve one.
            let identity = self.fallback_identity_args().await?;
            let mut commit_args: Vec<String> = identity;
            commit_args.extend(["commit".to_string(), "-m".to_string(), message.to_string()]);
            let commit_args: Vec<&str> = commit_args.iter().map(String::as_str).collect();
            self.git_checked_with_index(&commit_args, &index).await?;

            // HEAD now includes only the operation tree. Refresh the real index, then put back
            // exactly the entries that were staged before pinto started.
            self.restore_real_index(staged).await
        }
        .await;

        let cleanup = tokio::fs::remove_file(&index).await;
        match (result, cleanup) {
            (Err(error), _) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            (Ok(()), Err(error)) => Err(Error::io(&index, &error)),
        }
    }

    /// Return the current commit ID, or `None` for an initialized repository without a `HEAD`.
    async fn head(&self) -> Result<Option<String>> {
        let output = self.run_git(&["rev-parse", "--verify", "HEAD"]).await?;
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_string(),
        ))
    }

    /// Refresh the real index from `HEAD` and restore the user's staged paths.
    async fn restore_real_index(&self, staged: &StagedIndex) -> Result<()> {
        self.git_checked(&["read-tree", "HEAD"]).await?;
        for path in &staged.paths {
            let entries: Vec<&IndexEntry> = staged
                .entries
                .iter()
                .filter(|entry| &entry.path == path)
                .collect();
            if entries.is_empty() {
                // A staged deletion has no index entry; force-remove the HEAD entry.
                let output = self
                    .run_git(&["update-index", "--force-remove", "--", path])
                    .await?;
                if !output.status.success()
                    && !String::from_utf8_lossy(&output.stderr).contains("does not exist")
                {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(Error::Git(format!(
                        "git update-index --force-remove -- {path} failed: {}",
                        stderr.trim()
                    )));
                }
                continue;
            }
            for entry in entries {
                let cacheinfo = format!("{},{},{}", entry.mode, entry.object, entry.path);
                self.git_checked(&["update-index", "--add", "--cacheinfo", &cacheinfo])
                    .await?;
            }
        }
        Ok(())
    }

    /// Return pinto's default identity arguments when `user.email` is not configured; otherwise
    /// return an empty list. The supplied `-c` values override Git configuration, so only add
    /// them when no user identity is available.
    async fn fallback_identity_args(&self) -> Result<Vec<String>> {
        let email = self.run_git(&["config", "user.email"]).await?;
        let has_identity =
            email.status.success() && !String::from_utf8_lossy(&email.stdout).trim().is_empty();
        if has_identity {
            Ok(Vec::new())
        } else {
            Ok(vec![
                "-c".to_string(),
                "user.name=pinto".to_string(),
                "-c".to_string(),
                "user.email=pinto@localhost".to_string(),
            ])
        }
    }
}

/// One staged entry captured from `git ls-files --stage`.
#[derive(Debug)]
struct IndexEntry {
    path: String,
    mode: String,
    object: String,
    stage: u8,
}

/// The user's staged state that must survive a pinto commit.
#[derive(Debug, Default)]
struct StagedIndex {
    paths: HashSet<String>,
    entries: Vec<IndexEntry>,
}

/// Parse NUL-delimited porcelain status records into relative paths.
fn parse_status_paths(bytes: &[u8]) -> Vec<String> {
    let mut paths = Vec::new();
    let mut records = bytes.split(|byte| *byte == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 {
            continue;
        }
        // Porcelain v1 uses two status bytes followed by a space. Rename/copy records have a
        // second NUL-delimited path; retain both so the next `git add -A` sees the full operation.
        paths.push(String::from_utf8_lossy(&record[3..]).into_owned());
        if (record[0] == b'R' || record[0] == b'C' || record[1] == b'R' || record[1] == b'C')
            && let Some(new_path) = records.next()
            && !new_path.is_empty()
        {
            paths.push(String::from_utf8_lossy(new_path).into_owned());
        }
    }
    paths
}

/// Parse NUL-delimited path records.
fn parse_nul_paths(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| String::from_utf8_lossy(record).into_owned())
        .collect()
}

/// Parse the `mode object stage<TAB>path` records emitted by `git ls-files --stage -z`.
fn parse_index_entries(bytes: &[u8]) -> Vec<IndexEntry> {
    bytes
        .split(|byte| *byte == 0)
        .filter_map(|record| {
            let tab = record.iter().position(|byte| *byte == b'\t')?;
            let metadata = String::from_utf8_lossy(&record[..tab]);
            let mut fields = metadata.split_whitespace();
            let mode = fields.next()?.to_string();
            let object = fields.next()?.to_string();
            let stage = fields.next()?.parse().ok()?;
            let path = String::from_utf8_lossy(&record[tab + 1..]).into_owned();
            Some(IndexEntry {
                path,
                mode,
                object,
                stage,
            })
        })
        .collect()
}

/// Pick a process-local temporary index path without touching the user's index.
fn temporary_index_path() -> PathBuf {
    static NEXT_INDEX: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = NEXT_INDEX.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "pinto-index-{}-{nanos}-{sequence}",
        std::process::id()
    ))
}

impl BacklogItemRepository for GitRepository {
    async fn save(&self, item: &BacklogItem) -> Result<()> {
        // Git commit boundaries belong to the service operation, not to this single-file
        // persistence primitive. A user operation may update the item plus issued-ID history (or
        // several items), and the service commits that complete durable set once.
        BacklogItemRepository::save(&self.file, item).await
    }

    async fn load(&self, id: &ItemId) -> Result<BacklogItem> {
        BacklogItemRepository::load(&self.file, id).await
    }

    async fn list(&self) -> Result<Vec<BacklogItem>> {
        BacklogItemRepository::list(&self.file).await
    }

    async fn delete(&self, id: &ItemId) -> Result<()> {
        BacklogItemRepository::delete(&self.file, id).await
    }

    async fn archive(&self, id: &ItemId) -> Result<PathBuf> {
        self.file.archive(id).await
    }

    async fn next_id(&self, prefix: &str) -> Result<ItemId> {
        self.file.next_id(prefix).await
    }
}

impl SprintRepository for GitRepository {
    async fn save(&self, sprint: &Sprint) -> Result<()> {
        SprintRepository::save(&self.file, sprint).await
    }

    async fn load(&self, id: &SprintId) -> Result<Sprint> {
        SprintRepository::load(&self.file, id).await
    }

    async fn list(&self) -> Result<Vec<Sprint>> {
        SprintRepository::list(&self.file).await
    }

    async fn delete(&self, id: &SprintId) -> Result<()> {
        SprintRepository::delete(&self.file, id).await
    }
}

#[cfg(test)]
mod tests {
    //! Integration testing in a temporary git repository (tempfile quarantine).
    //!
    //! Since it uses the actual `git` CLI, git is required in the execution environment (prerequisite for CI/development environment).

    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;
    use crate::sprint::SprintId;
    use chrono::{TimeZone, Utc};
    use tempfile::TempDir;

    /// Returns the GitRepository for `.pinto` and its parent (working directory).
    fn repo() -> (TempDir, GitRepository) {
        let dir = TempDir::new().expect("temp dir");
        let git = GitRepository::new(dir.path().join(".pinto"));
        (dir, git)
    }

    fn item(n: u32, title: &str) -> BacklogItem {
        BacklogItem::new(
            ItemId::new("T", n),
            title,
            Status::new("todo"),
            Rank::after(None),
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .expect("valid item")
    }

    /// Returns the most recent commit subject in chronological order.
    async fn commit_subjects(workdir: &Path) -> Vec<String> {
        let out = Command::new("git")
            .args(["log", "--format=%s"])
            .current_dir(workdir)
            .output()
            .await
            .expect("git log");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[tokio::test]
    async fn commit_after_save_auto_inits_repo_and_commits_add() {
        let (dir, git) = repo();
        // Not a git repository beforehand.
        assert!(!dir.path().join(".git").exists());

        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("save");
        git.commit("pinto: add T-1").await.expect("commit");

        // Automatically initialized.
        assert!(
            dir.path().join(".git").exists(),
            "auto-initialized git repo"
        );
        assert_eq!(commit_subjects(dir.path()).await, ["pinto: add T-1"]);
        // Files are actually tracked and stored.
        let loaded = BacklogItemRepository::load(&git, &ItemId::new("T", 1))
            .await
            .expect("load");
        assert_eq!(loaded.title, "First");
    }

    #[tokio::test]
    async fn separate_service_boundaries_commit_add_then_update() {
        let (dir, git) = repo();
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("add");
        git.commit("pinto: add T-1").await.expect("add commit");
        let mut edited = item(1, "First edited");
        edited.updated = Utc.timestamp_opt(2_000, 0).single().unwrap();
        BacklogItemRepository::save(&git, &edited)
            .await
            .expect("update");
        git.commit("pinto: update T-1")
            .await
            .expect("update commit");

        assert_eq!(
            commit_subjects(dir.path()).await,
            ["pinto: update T-1", "pinto: add T-1"]
        );
    }

    #[tokio::test]
    async fn delete_commits_remove() {
        let (dir, git) = repo();
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("add");
        git.commit("pinto: add T-1").await.expect("add commit");
        BacklogItemRepository::delete(&git, &ItemId::new("T", 1))
            .await
            .expect("delete");
        git.commit("pinto: remove T-1")
            .await
            .expect("remove commit");

        assert_eq!(
            commit_subjects(dir.path()).await,
            ["pinto: remove T-1", "pinto: add T-1"]
        );
    }

    #[tokio::test]
    async fn archive_commits_archive() {
        let (dir, git) = repo();
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("add");
        git.commit("pinto: add T-1").await.expect("add commit");
        let dest = git.archive(&ItemId::new("T", 1)).await.expect("archive");
        assert!(dest.ends_with("archive/T-1.md"));
        git.commit("pinto: archive T-1")
            .await
            .expect("archive commit");

        assert_eq!(
            commit_subjects(dir.path()).await,
            ["pinto: archive T-1", "pinto: add T-1"]
        );
    }

    #[tokio::test]
    async fn delete_missing_item_does_not_commit() {
        let (dir, git) = repo();
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("add");
        git.commit("pinto: add T-1").await.expect("add commit");
        let err = BacklogItemRepository::delete(&git, &ItemId::new("T", 99))
            .await
            .expect_err("missing delete errors");
        assert_eq!(err, Error::NotFound(ItemId::new("T", 99)));
        // The number of commits has not increased (only one add).
        assert_eq!(commit_subjects(dir.path()).await, ["pinto: add T-1"]);
    }

    #[tokio::test]
    async fn sprint_save_commits_add_then_update() {
        let (dir, git) = repo();
        let sprint = Sprint::new(
            SprintId::new("S-1").unwrap(),
            "Sprint 1",
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .unwrap();
        SprintRepository::save(&git, &sprint).await.expect("add");
        git.commit("pinto: add S-1").await.expect("add commit");

        let mut updated = sprint.clone();
        updated.goal = "goal".to_string();
        SprintRepository::save(&git, &updated)
            .await
            .expect("update");
        git.commit("pinto: update S-1")
            .await
            .expect("update commit");

        assert_eq!(
            commit_subjects(dir.path()).await,
            ["pinto: update S-1", "pinto: add S-1"]
        );
    }

    #[tokio::test]
    async fn sprint_delete_commits_remove() {
        let (dir, git) = repo();
        let sprint = Sprint::new(
            SprintId::new("S-1").unwrap(),
            "Sprint 1",
            Utc.timestamp_opt(1_000, 0).single().unwrap(),
        )
        .unwrap();
        SprintRepository::save(&git, &sprint).await.expect("add");
        git.commit("pinto: add S-1").await.expect("add commit");
        SprintRepository::delete(&git, &sprint.id)
            .await
            .expect("delete");
        git.commit("pinto: remove S-1")
            .await
            .expect("remove commit");
        assert_eq!(
            commit_subjects(dir.path()).await,
            ["pinto: remove S-1", "pinto: add S-1"]
        );
    }

    #[tokio::test]
    async fn sprint_delete_missing_does_not_commit() {
        let (dir, git) = repo();
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("add");
        git.commit("pinto: add T-1").await.expect("add commit");
        let err = SprintRepository::delete(&git, &SprintId::new("S-9").unwrap())
            .await
            .expect_err("missing sprint delete errors");
        assert_eq!(err, Error::SprintNotFound(SprintId::new("S-9").unwrap()));
        assert_eq!(commit_subjects(dir.path()).await, ["pinto: add T-1"]);
    }

    #[tokio::test]
    async fn commits_land_in_existing_ancestor_repo_without_reinit() {
        // If it is already a git repository, add commits to its history without re-init.
        let dir = TempDir::new().expect("temp dir");
        let out = Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .expect("git init");
        assert!(out.status.success());

        let git = GitRepository::new(dir.path().join(".pinto"));
        BacklogItemRepository::save(&git, &item(1, "First"))
            .await
            .expect("save");
        git.commit("pinto: add T-1").await.expect("commit");
        assert_eq!(commit_subjects(dir.path()).await, ["pinto: add T-1"]);
    }
}
