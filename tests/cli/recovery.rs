//! Failure recovery for multi-record mutations.

use super::common::*;

fn set_backend(dir: &Path, backend: &str) {
    if backend == "git" {
        pinto_isolated_git(dir)
            .args(["migrate", "--to", backend])
            .assert()
            .success();
    } else if backend == "sqlite" {
        pinto(dir)
            .args(["migrate", "--to", backend])
            .assert()
            .success();
    }
}

fn run_backend_command(dir: &Path, backend: &str, args: &[&str]) -> Command {
    let mut command = if backend == "git" {
        pinto_isolated_git(dir)
    } else {
        pinto(dir)
    };
    command.args(args);
    command
}

fn seed_split_board(dir: &Path, backend: &str) {
    run_backend_command(dir, backend, &["init"])
        .assert()
        .success();
    set_backend(dir, backend);
    run_backend_command(dir, backend, &["add", "Source", "--body", "body"])
        .assert()
        .success();
}

fn source_snapshot(dir: &Path, backend: &str) -> String {
    run_backend_command(dir, backend, &["init"])
        .assert()
        .success();
    set_backend(dir, backend);
    run_backend_command(dir, backend, &["add", "Imported A"])
        .assert()
        .success();
    run_backend_command(dir, backend, &["add", "Imported B"])
        .assert()
        .success();
    run_backend_command(dir, backend, &["sprint", "new", "S-1", "Imported sprint"])
        .assert()
        .success();
    let output = run_backend_command(dir, backend, &["export", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("export is UTF-8")
}

fn seed_import_destination(dir: &Path, backend: &str) {
    run_backend_command(dir, backend, &["init"])
        .assert()
        .success();
    set_backend(dir, backend);
    run_backend_command(dir, backend, &["add", "Existing"])
        .assert()
        .success();
    run_backend_command(dir, backend, &["sprint", "new", "S-old", "Existing sprint"])
        .assert()
        .success();
}

fn split_failure_restores_the_pre_operation_board(backend: &str) {
    let dir = TempDir::new().expect("temp dir");
    seed_split_board(dir.path(), backend);
    let before = json_stdout(&mut run_backend_command(
        dir.path(),
        backend,
        &["list", "--json"],
    ));

    run_backend_command(dir.path(), backend, &["split", "T-1", "Slice A", "Slice B"])
        .env("PINTO_TEST_FAIL_AFTER_RECORD_WRITES", "1")
        .assert()
        .failure()
        .stderr(predicate::str::contains("pre-operation state"));

    let after = json_stdout(&mut run_backend_command(
        dir.path(),
        backend,
        &["list", "--json"],
    ));
    assert_eq!(after, before, "split failure must restore the board");
    run_backend_command(dir.path(), backend, &["show", "T-2"])
        .assert()
        .failure();
}

fn import_failure_restores_the_pre_operation_board(backend: &str) {
    let source = TempDir::new().expect("source temp dir");
    let snapshot = source_snapshot(source.path(), backend);

    let destination = TempDir::new().expect("destination temp dir");
    seed_import_destination(destination.path(), backend);
    let snapshot_path = destination.path().join("snapshot.json");
    std::fs::write(&snapshot_path, snapshot).expect("write snapshot");
    let before = json_stdout(&mut run_backend_command(
        destination.path(),
        backend,
        &["export", "--json"],
    ));

    run_backend_command(
        destination.path(),
        backend,
        &[
            "import",
            "--force",
            snapshot_path.to_str().expect("snapshot path"),
        ],
    )
    .env("PINTO_TEST_FAIL_AFTER_RECORD_WRITES", "1")
    .assert()
    .failure()
    .stderr(predicate::str::contains("pre-operation state"));

    let after = json_stdout(&mut run_backend_command(
        destination.path(),
        backend,
        &["export", "--json"],
    ));
    assert_eq!(after, before, "import failure must restore the full board");
    run_backend_command(destination.path(), backend, &["show", "T-2"])
        .assert()
        .failure();
}

#[test]
fn multi_record_commands_expose_recovery_in_help() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["split", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("recoverable operation"));
    pinto(dir.path())
        .args(["import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("recoverable operation"));
}

#[test]
fn file_split_failure_restores_the_pre_operation_board() {
    split_failure_restores_the_pre_operation_board("file");
}

#[test]
fn file_import_failure_restores_the_pre_operation_board() {
    import_failure_restores_the_pre_operation_board("file");
}

#[test]
fn git_split_failure_restores_the_pre_operation_board() {
    split_failure_restores_the_pre_operation_board("git");
}

#[test]
fn git_import_failure_restores_the_pre_operation_board() {
    import_failure_restores_the_pre_operation_board("git");
}

#[test]
fn git_import_switch_preserves_preexisting_board_changes() {
    let source = TempDir::new().expect("source temp dir");
    let snapshot = source_snapshot(source.path(), "file");

    let destination = TempDir::new().expect("destination temp dir");
    seed_import_destination(destination.path(), "file");
    pinto_isolated_git(destination.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let staged_note = destination.path().join(".pinto/preexisting-note.md");
    std::fs::write(&staged_note, "keep staged\n").expect("write staged note");
    run_git(destination.path(), &["add", ".pinto/preexisting-note.md"]);

    let snapshot_path = destination.path().join("snapshot.json");
    std::fs::write(&snapshot_path, snapshot).expect("write snapshot");
    pinto_isolated_git(destination.path())
        .args([
            "import",
            "--force",
            snapshot_path.to_str().expect("snapshot path"),
        ])
        .assert()
        .success();

    assert_eq!(
        git_log_field(destination.path(), "%s")
            .first()
            .map(String::as_str),
        Some("pinto: import board (2 items, 1 sprints)")
    );
    let committed_note = std::process::Command::new("git")
        .args(["show", "HEAD:.pinto/preexisting-note.md"])
        .current_dir(destination.path())
        .output()
        .expect("inspect committed note");
    assert!(
        !committed_note.status.success(),
        "pre-existing board changes must not enter the import commit"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(destination.path())
        .output()
        .expect("inspect worktree");
    assert!(
        String::from_utf8_lossy(&status.stdout).contains("A  .pinto/preexisting-note.md"),
        "pre-existing staged board change must remain staged: {}",
        String::from_utf8_lossy(&status.stdout)
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_split_failure_restores_the_pre_operation_board() {
    split_failure_restores_the_pre_operation_board("sqlite");
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_import_failure_restores_the_pre_operation_board() {
    import_failure_restores_the_pre_operation_board("sqlite");
}
