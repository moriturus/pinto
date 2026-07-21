//! Git storage backend behavior observed through the CLI.

use super::common::*;

#[test]
fn git_backend_commits_each_change_through_cli() {
    // In an isolated temporary Git repository, verify end to end that each CLI write creates a
    // commit. Also cover automatic initialization and the fallback identity used on a bare CI
    // runner with no ambient Git identity.
    let dir = TempDir::new().expect("temp dir");
    pinto_isolated_git(dir.path())
        .arg("init")
        .assert()
        .success();

    // Select the Git backend in config.toml, reproducing a user hand-edit.
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("write config");

    // The directory is not a Git repository beforehand.
    assert!(!dir.path().join(".git").exists());

    pinto_isolated_git(dir.path())
        .args(["add", "First task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--points", "3"])
        .assert()
        .success();

    // Git is initialized automatically and each write operation creates a commit.
    assert!(
        dir.path().join(".git").exists(),
        "auto-initialized git repo"
    );
    let subjects = git_log_field(dir.path(), "%s");
    assert_eq!(subjects, ["pinto: update T-1", "pinto: add T-1"]);
    // With no ambient identity, the commit uses pinto's default author identity.
    let authors = git_log_field(dir.path(), "%ae");
    assert_eq!(authors, ["pinto@localhost", "pinto@localhost"]);
}

fn assert_git_commit_and_clean(dir: &Path, subject: &str) {
    assert_eq!(
        git_log_field(dir, "%s").first().map(String::as_str),
        Some(subject),
        "the operation must create the expected latest commit"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .expect("git status");
    assert!(
        status.status.success() && status.stdout.is_empty(),
        "operation must leave a clean worktree: {}",
        String::from_utf8_lossy(&status.stdout)
    );
    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("git tree");
    assert!(
        !String::from_utf8_lossy(&tree.stdout)
            .lines()
            .any(|path| path == ".pinto/.lock")
    );
}

#[test]
fn undo_reverts_the_most_recent_git_mutation() {
    // On the Git backend `undo` creates a revert commit for the last mutation and reports the
    // reverted subject, while leaving the worktree clean and earlier items intact.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Keep task"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Undo task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    // Mutate once more so the mutation under test is a plain add, then undo it.
    pinto_isolated_git(dir.path())
        .args(["add", "Latest task"])
        .assert()
        .success();
    assert_eq!(
        git_log_field(dir.path(), "%s").first().map(String::as_str),
        Some("pinto: add T-3")
    );

    pinto_isolated_git(dir.path())
        .arg("undo")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Reverted the most recent board mutation: pinto: add T-3",
        ));

    // A revert commit tops the history; the reverted item is gone, the others remain.
    assert_git_commit_and_clean(dir.path(), "Revert \"pinto: add T-3\"");
    pinto_isolated_git(dir.path())
        .args(["show", "T-3"])
        .assert()
        .failure();
    pinto_isolated_git(dir.path())
        .args(["list", "--long"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keep task"))
        .stdout(predicate::str::contains("Undo task"))
        .stdout(predicate::str::contains("Latest task").not());
}

#[test]
fn undo_refuses_when_head_is_not_a_pinto_commit() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Tracked task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    // A user commit lands on top of the pinto mutation.
    std::fs::write(dir.path().join("NOTES.md"), "notes\n").expect("write note");
    run_git(dir.path(), &["add", "NOTES.md"]);
    run_git(dir.path(), &["config", "user.name", "Fixture"]);
    run_git(
        dir.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    run_git(dir.path(), &["commit", "-m", "chore: notes"]);

    pinto_isolated_git(dir.path())
        .arg("undo")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("is not a pinto board mutation"));
}

#[test]
fn undo_on_file_backend_reports_no_history() {
    // The default file backend keeps no history, so `undo` fails fast with recovery guidance.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Only task"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("undo")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("undo requires the git backend"))
        .stderr(predicate::str::contains("file backend keeps no history"));
}

#[test]
fn git_backend_commits_item_sprint_dod_and_removal_mutations() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "First task"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: migrate file to git");

    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--title", "Edited task"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["reorder", "T-2", "--top"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");

    pinto_isolated_git(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");
    pinto_isolated_git(dir.path())
        .args(["dep", "rm", "T-2", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");

    pinto_isolated_git(dir.path())
        .args(["link", "add", "T-1", "deadbeef"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");
    pinto_isolated_git(dir.path())
        .args(["link", "rm", "T-1", "deadbeef"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Release sprint",
            "--goal",
            "Ship",
            "--start",
            "2025-01-01",
            "--end",
            "2025-01-05",
        ])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: add S-1");

    pinto_isolated_git(dir.path())
        .args([
            "sprint",
            "capacity",
            "S-1",
            "--daily-hours",
            "8",
            "--holidays",
            "0",
            "--deduction-factor",
            "0.1",
        ])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");

    pinto_isolated_git(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");
    pinto_isolated_git(dir.path())
        .args(["sprint", "unassign", "S-1", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");
    pinto_isolated_git(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");

    pinto_isolated_git(dir.path())
        .args(["dod", "set", "- [ ] review"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update common DoD");
    pinto_isolated_git(dir.path())
        .args(["dod", "clear"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: remove common DoD");

    pinto_isolated_git(dir.path())
        .args(["rm", "T-2"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: archive T-2");
    pinto_isolated_git(dir.path())
        .args(["rm", "T-1", "--force"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: remove T-1");
}

#[test]
fn git_backend_warns_before_auto_initializing_a_repository() {
    // The Git backend runs `git init` on the first write, so the CLI emits an explicit warning to
    // make an otherwise unexpected initialization visible.
    let dir = TempDir::new().expect("temp dir");
    pinto_isolated_git(dir.path())
        .arg("init")
        .assert()
        .success();

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("write config");

    pinto_isolated_git(dir.path())
        .args(["add", "First task"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: initializing a Git repository for the selected git backend",
        ));
}

#[test]
fn git_backend_leaves_a_clean_tree_without_transient_lock_in_history() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");

    pinto_isolated_git(dir.path())
        .args(["add", "Tracked task"])
        .assert()
        .success();

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(
        status.status.success() && status.stdout.is_empty(),
        "Git backend must leave a clean worktree: {}",
        String::from_utf8_lossy(&status.stdout)
    );

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));
}

#[test]
fn git_backend_migration_commits_the_backend_switch() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Migrated task"])
        .assert()
        .success();

    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let head_config = std::process::Command::new("git")
        .args(["show", "HEAD:.pinto/config.toml"])
        .current_dir(dir.path())
        .output()
        .expect("read committed config");
    assert!(
        String::from_utf8_lossy(&head_config.stdout).contains("backend = \"git\""),
        "migration backend switch must be in HEAD"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[test]
fn git_backend_migration_is_one_commit_boundary() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "First migrated task"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second migrated task"])
        .assert()
        .success();

    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    assert_eq!(
        git_log_field(dir.path(), "%s"),
        ["pinto: migrate file to git"],
        "one user migration must produce one coherent commit"
    );
}

#[test]
fn git_backend_does_not_commit_preexisting_board_or_outside_changes() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Existing task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let inside = dir.path().join(".pinto/user-note.md");
    std::fs::write(&inside, "keep staged\n").expect("write board note");
    let untracked_inside = dir.path().join(".pinto/untracked-note.md");
    std::fs::write(&untracked_inside, "keep untracked\n").expect("write untracked board note");
    let outside = dir.path().join("user-note.md");
    std::fs::write(&outside, "keep staged outside\n").expect("write outside note");
    run_git(dir.path(), &["add", ".pinto/user-note.md", "user-note.md"]);

    pinto_isolated_git(dir.path())
        .args(["add", "New task"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/user-note.md"));
    assert!(!tree.lines().any(|path| path == ".pinto/untracked-note.md"));
    assert!(!tree.lines().any(|path| path == "user-note.md"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("A  .pinto/user-note.md"))
    );
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("?? .pinto/untracked-note.md"))
    );
    assert!(status.lines().any(|line| line.ends_with("user-note.md")));
}

#[test]
fn git_backend_writes_after_checking_out_a_tracked_lock_file() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");
    std::fs::write(dir.path().join(".pinto/.lock"), "old owner\n").expect("tracked lock fixture");

    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.name", "Fixture"]);
    run_git(
        dir.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    run_git(dir.path(), &["add", ".pinto"]);
    run_git(dir.path(), &["commit", "-m", "fixture with tracked lock"]);

    pinto_isolated_git(dir.path())
        .args(["add", "Write after checkout"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn git_commit_failure_preserves_worktree_change_and_real_index() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Durable task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let hooks = TempDir::new().expect("hooks dir");
    editor_script(hooks.path(), "pre-commit", "exit 1");
    run_git(
        dir.path(),
        &[
            "config",
            "core.hooksPath",
            hooks.path().to_str().expect("hooks path"),
        ],
    );

    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--title", "Durable but uncommitted"])
        .assert()
        .failure();

    pinto_isolated_git(dir.path())
        .args(["show", "T-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Durable but uncommitted"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.lines().any(|line| line == " M .pinto/tasks/T-1.md"),
        "the failed commit must leave the durable file change in the worktree: {status}"
    );
    assert!(
        std::process::Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(dir.path())
            .status()
            .expect("git diff --cached")
            .success(),
        "the failed commit must not modify the real index"
    );
}

#[test]
fn git_backend_commits_common_dod_and_leaves_no_lock_artifact() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");
    pinto_isolated_git(dir.path())
        .args(["dod", "set", "- [ ] review"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(tree.lines().any(|path| path == ".pinto/dod.md"));
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));
}
