//! Associate Git commits with backlog items.
//!
//! Record which changes correspond to which items by storing commit SHAs. The service provides
//! three operations:
//!
//! - [`link_commits`] — explicitly associate SHAs; Git is not required because they are stored as plain strings.
//! - [`unlink_commits`] — remove recorded commits, optionally using a unique SHA prefix.
//! - [`scan_commits`] — scan `git log` and link commits whose messages contain an item ID. This is
//!   the only operation that requires Git and returns [`Error::Git`] when it is unavailable.
//!
//! **Policy**: To avoid another crate, scanning invokes the `git` CLI through
//! [`tokio::process`]. Waiting for the subprocess is asynchronous; see `docs/DESIGN.md` §3.4.
//! The selected backend persists all updates to [`BacklogItem::commits`].

use super::open_board_locked;
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::storage::BacklogItemRepository;
use chrono::Utc;
use std::path::Path;
use std::process::Output;
use tokio::process::Command;

/// Result of [`link_commits`] or [`unlink_commits`], including the updated PBI and changed SHAs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOutcome {
    /// PBI with its updated commit list.
    pub item: BacklogItem,
    /// SHAs that were actually added or removed; unchanged entries are omitted.
    pub changed: Vec<String>,
}

/// Add commit SHAs in `shas` to PBI `id` and return [`LinkOutcome`].
///
/// Ignore blank or already-recorded SHAs, making the operation idempotent. Update and save the PBI
/// only when at least one SHA is new. Return [`Error::NotInitialized`] for an uninitialized board
/// or [`Error::NotFound`] when `id` does not exist. Git is not required because SHAs are plain text.
pub async fn link_commits(project_dir: &Path, id: &ItemId, shas: &[String]) -> Result<LinkOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut item = repo.load(id).await?;
    let mut changed = Vec::new();
    for sha in shas {
        if item.link_commit(sha.clone()) {
            changed.push(sha.trim().to_string());
        }
    }
    if !changed.is_empty() {
        item.updated = Utc::now();
        repo.save(&item).await?;
        repo.commit(&format!("pinto: update {}", item.id)).await?;
    }
    Ok(LinkOutcome { item, changed })
}

/// Remove commits matching the SHA prefixes in `shas` from PBI `id` and return [`LinkOutcome`].
///
/// Match each argument by prefix, so the shortened SHA shown by `show` can be used. Save only when
/// something was removed. Return [`Error::NotInitialized`] or [`Error::NotFound`] as appropriate.
pub async fn unlink_commits(
    project_dir: &Path,
    id: &ItemId,
    shas: &[String],
) -> Result<LinkOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut item = repo.load(id).await?;
    let mut changed = Vec::new();
    for arg in shas {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        // Enumerate prefix matches and then remove them (separate borrowing and deletion during scanning).
        let matched: Vec<String> = item
            .commits
            .iter()
            .filter(|c| c.starts_with(arg))
            .cloned()
            .collect();
        for sha in matched {
            if item.unlink_commit(&sha) {
                changed.push(sha);
            }
        }
    }
    if !changed.is_empty() {
        item.updated = Utc::now();
        repo.save(&item).await?;
        repo.commit(&format!("pinto: update {}", item.id)).await?;
    }
    Ok(LinkOutcome { item, changed })
}

/// Result of [`scan_commits`], listing new `(PBI ID, SHA)` links.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScanOutcome {
    /// New links in oldest-commit order.
    pub links: Vec<(ItemId, String)>,
}

/// Scan `git log` and associate commits whose messages contain a PBI ID.
///
/// If `since` is set, scan `<since>..HEAD`; otherwise scan the full history. Match IDs as tokens,
/// so `T-53` does not match `T-531`. Skip pinto bookkeeping commits whose subject starts with
/// `pinto:` and do not duplicate existing links.
///
/// Return [`Error::Git`] when Git is unavailable or the project is not a repository.
pub async fn scan_commits(project_dir: &Path, since: Option<&str>) -> Result<ScanOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut items = repo.list().await?;
    let commits = git_log(project_dir, since).await?;

    let mut changed = vec![false; items.len()];
    let mut links = Vec::new();
    for (sha, message) in &commits {
        // Skip pinto's bookkeeping commits; scan only commits representing user work.
        let subject = message.lines().next().unwrap_or("");
        if subject.trim_start().starts_with("pinto:") {
            continue;
        }
        for (i, item) in items.iter_mut().enumerate() {
            let id = item.id.to_string();
            if message_mentions(message, &id) && item.link_commit(sha.clone()) {
                changed[i] = true;
                links.push((item.id.clone(), sha.clone()));
            }
        }
    }

    let now = Utc::now();
    for (i, item) in items.iter_mut().enumerate() {
        if changed[i] {
            item.updated = now;
            repo.save(item).await?;
        }
    }
    if !links.is_empty() {
        repo.commit("pinto: link commits").await?;
    }
    Ok(ScanOutcome { links })
}

/// Determine whether `message` contains `id` as a separate token.
///
/// Since `id` is ASCII (`<PREFIX>-<NUMBER>`), match it only when the preceding and following bytes
/// are not ASCII alphanumeric characters. This prevents `T-5` from matching part of `T-53`.
/// UTF-8 bytes for non-ASCII characters are outside the ASCII alphanumeric range and therefore
/// act as valid boundaries.
fn message_mentions(message: &str, id: &str) -> bool {
    let bytes = message.as_bytes();
    let mut start = 0;
    while let Some(pos) = message[start..].find(id) {
        let at = start + pos;
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        let after = at + id.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

/// Run `git log` and return (SHA, full message) in chronological order.
///
/// Use the machine-readable format `%H\0%B\x1e` (NUL separates the hash and body; RS separates
/// records), so message bodies may safely contain line breaks.
///
/// Return a clear [`Error::Git`] when the project is not a repository. A repository with no `HEAD`
/// is treated as empty history without exposing raw Git diagnostics. Both cases are detected using
/// locale-independent exit codes.
async fn git_log(project_dir: &Path, since: Option<&str>) -> Result<Vec<(String, String)>> {
    let inside = run_git(project_dir, &["rev-parse", "--is-inside-work-tree"]).await?;
    if !inside.status.success() {
        return Err(Error::Git(
            "not a git repository; run `git init` first, \
             or link SHAs manually with `pinto link add`"
                .to_string(),
        ));
    }
    // A repository with no HEAD has no commits to scan.
    let head = run_git(project_dir, &["rev-parse", "--verify", "--quiet", "HEAD"]).await?;
    if !head.status.success() {
        return Ok(Vec::new());
    }

    let range = since.map(|s| format!("{s}..HEAD"));
    let mut args = vec!["log", "--reverse", "--format=%H%x00%B%x1e"];
    if let Some(range) = range.as_deref() {
        args.push(range);
    }
    let out = run_git(project_dir, &args).await?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(Error::Git(format!("git log failed: {}", stderr.trim())));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut records = Vec::new();
    for record in text.split('\u{1e}') {
        let record = record.trim_matches(['\n', '\r']);
        if record.is_empty() {
            continue;
        }
        if let Some((sha, message)) = record.split_once('\u{0}') {
            records.push((sha.trim().to_string(), message.to_string()));
        }
    }
    Ok(records)
}

/// Run `git <args>` in `project_dir`, mapping a missing executable to a clear [`Error::Git`].
async fn run_git(project_dir: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .args(args)
        .current_dir(project_dir)
        .output()
        .await
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Git(
                    "`git` command not found; install git to scan commits, \
                     or link SHAs manually with `pinto link add`"
                        .to_string(),
                )
            } else {
                Error::Git(format!("failed to run git {}: {e}", args.join(" ")))
            }
        })
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{add_with, init_temp};
    use super::*;

    /// Reload PBI for testing (verify that the save was made permanent).
    async fn reload(dir: &Path, id: &ItemId) -> BacklogItem {
        let (_board_dir, repo, _config) = super::super::open_board(dir).await.expect("open");
        repo.load(id).await.expect("load")
    }

    #[tokio::test]
    async fn link_commits_adds_and_persists() {
        let dir = init_temp().await;
        let item = add_with(dir.path(), "Task", &[], None).await;

        let out = link_commits(
            dir.path(),
            &item.id,
            &["abc123".to_string(), "def456".to_string()],
        )
        .await
        .expect("link");
        assert_eq!(out.changed, ["abc123", "def456"]);

        let reloaded = reload(dir.path(), &item.id).await;
        assert_eq!(reloaded.commits, ["abc123", "def456"]);
    }

    #[tokio::test]
    async fn link_commits_is_idempotent() {
        let dir = init_temp().await;
        let item = add_with(dir.path(), "Task", &[], None).await;
        link_commits(dir.path(), &item.id, &["abc123".to_string()])
            .await
            .expect("link once");

        let out = link_commits(dir.path(), &item.id, &["abc123".to_string()])
            .await
            .expect("link twice");
        assert!(out.changed.is_empty(), "duplicate adds nothing");
        assert_eq!(reload(dir.path(), &item.id).await.commits, ["abc123"]);
    }

    #[tokio::test]
    async fn unlink_commits_matches_by_prefix() {
        let dir = init_temp().await;
        let item = add_with(dir.path(), "Task", &[], None).await;
        link_commits(
            dir.path(),
            &item.id,
            &["abc12345".to_string(), "def67890".to_string()],
        )
        .await
        .expect("link");

        // It can also be removed with abbreviated SHA (prefix match).
        let out = unlink_commits(dir.path(), &item.id, &["abc12".to_string()])
            .await
            .expect("unlink");
        assert_eq!(out.changed, ["abc12345"]);
        assert_eq!(reload(dir.path(), &item.id).await.commits, ["def67890"]);
    }

    #[tokio::test]
    async fn link_missing_item_is_not_found() {
        let dir = init_temp().await;
        let err = link_commits(dir.path(), &ItemId::new("T", 99), &["abc".to_string()])
            .await
            .expect_err("missing id");
        assert_eq!(err, Error::NotFound(ItemId::new("T", 99)));
    }

    #[test]
    fn message_mentions_respects_token_boundaries() {
        assert!(message_mentions("fix bug (T-53)", "T-53"));
        assert!(message_mentions("T-53 at start", "T-53"));
        assert!(message_mentions("close T-53", "T-53"));
        assert!(
            message_mentions("完了する T-53 を", "T-53"),
            "multibyte neighbors"
        );
        // Partial matches will not be falsely detected.
        assert!(!message_mentions("touch T-531 area", "T-53"));
        assert!(!message_mentions("xT-53", "T-53"));
        assert!(!message_mentions("no id here", "T-53"));
    }

    // --- scan (integration test using real `git` CLI. Requires git in execution environment)---

    async fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .await
            .expect("run git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Accumulate only messages in the history with an empty commit (does not touch the work tree).
    async fn commit(dir: &Path, message: &str) {
        git(dir, &["commit", "--allow-empty", "-m", message]).await;
    }

    #[tokio::test]
    async fn scan_links_commits_by_item_id_in_message() {
        let dir = init_temp().await;
        let a = add_with(dir.path(), "A", &[], None).await; // T-1
        let b = add_with(dir.path(), "B", &[], None).await; // T-2

        git(dir.path(), &["init"]).await;
        git(dir.path(), &["config", "user.email", "t@example.com"]).await;
        git(dir.path(), &["config", "user.name", "Tester"]).await;
        commit(dir.path(), "feat: implement A (T-1)").await; // → T-1
        commit(dir.path(), "chore: touch T-12 boundary only").await; // Must not be a false positive.
        commit(dir.path(), "pinto: update T-2").await; // Exclude bookkeeping commits.
        commit(dir.path(), "fix: finish B T-2").await; // → T-2

        let outcome = scan_commits(dir.path(), None).await.expect("scan");
        assert_eq!(outcome.links.len(), 2, "two new links");

        let a2 = reload(dir.path(), &a.id).await;
        let b2 = reload(dir.path(), &b.id).await;
        assert_eq!(a2.commits.len(), 1, "T-1 linked once (not the T-12 commit)");
        assert_eq!(
            b2.commits.len(),
            1,
            "T-2 linked once (pinto: bookkeeping skipped)"
        );
    }

    #[tokio::test]
    async fn scan_without_git_repo_errors_clearly() {
        let dir = init_temp().await;
        add_with(dir.path(), "A", &[], None).await;
        // There is a `.pinto`, but it is not a Git repository.
        let err = scan_commits(dir.path(), None).await.expect_err("no repo");
        assert!(
            matches!(&err, Error::Git(m) if m.contains("not a git repository")),
            "clear guidance, got {err:?}"
        );
    }

    #[tokio::test]
    async fn scan_on_repo_without_commits_is_empty_not_error() {
        let dir = init_temp().await;
        add_with(dir.path(), "A", &[], None).await;
        // A repository with no commits (no HEAD). Returns empty without revealing any raw git errors.
        git(dir.path(), &["init"]).await;
        let outcome = scan_commits(dir.path(), None).await.expect("empty history");
        assert!(outcome.links.is_empty());
    }

    #[tokio::test]
    async fn scan_is_idempotent() {
        let dir = init_temp().await;
        let a = add_with(dir.path(), "A", &[], None).await; // T-1

        git(dir.path(), &["init"]).await;
        git(dir.path(), &["config", "user.email", "t@example.com"]).await;
        git(dir.path(), &["config", "user.name", "Tester"]).await;
        commit(dir.path(), "feat: A T-1").await;

        scan_commits(dir.path(), None).await.expect("scan once");
        let second = scan_commits(dir.path(), None).await.expect("scan twice");
        assert!(second.links.is_empty(), "no new links on re-scan");
        assert_eq!(reload(dir.path(), &a.id).await.commits.len(), 1);
    }
}
