//! Board initialization, discovery, and directory overrides.

use super::common::*;

#[test]
fn init_creates_board_files() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(dir.path().join(".pinto/config.toml").is_file());
    assert!(dir.path().join(".pinto/tasks").is_dir());

    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("todo"), "default columns present");
    assert!(config.contains("[project]"), "project section present");
    assert!(
        config.contains("[display]") && config.contains("timezone = \"local\""),
        "display timezone defaults are discoverable"
    );
    assert!(
        !config.contains("key_bindings"),
        "personal Kanban keys are not shared board configuration"
    );
}

#[test]
fn init_twice_is_idempotent() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already initialized"));
}

#[test]
fn commands_discover_the_nearest_ancestor_board() {
    let outer = TempDir::new().expect("outer temp dir");
    pinto(outer.path()).arg("init").assert().success();
    pinto(outer.path())
        .args(["add", "Outer item"])
        .assert()
        .success();

    let inner = outer.path().join("workspace");
    std::fs::create_dir_all(&inner).expect("create inner directory");
    pinto(&inner).arg("init").assert().success();
    pinto(&inner).args(["add", "Inner item"]).assert().success();

    pinto(&inner)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Inner item"));

    let nested = inner.join("src").join("module");
    std::fs::create_dir_all(&nested).expect("create nested directory");

    pinto(&nested)
        .args(["list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inner item"))
        .stdout(predicate::str::contains("Outer item").not());
}

#[test]
fn board_directory_can_be_overridden_by_flag_or_environment() {
    let board = TempDir::new().expect("board temp dir");
    pinto(board.path()).arg("init").assert().success();
    pinto(board.path())
        .args(["add", "Explicit board item"])
        .assert()
        .success();

    let environment_board = TempDir::new().expect("environment board temp dir");
    pinto(environment_board.path())
        .arg("init")
        .assert()
        .success();
    pinto(environment_board.path())
        .args(["add", "Environment board item"])
        .assert()
        .success();

    let worktree = TempDir::new().expect("worktree temp dir");
    pinto(worktree.path())
        .env("PINTO_DIR", environment_board.path())
        .args(["--dir", board.path().to_str().expect("board path"), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Explicit board item"))
        .stdout(predicate::str::contains("Environment board item").not());

    pinto(worktree.path())
        .env("PINTO_DIR", board.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Explicit board item"));
}

#[test]
fn board_directory_override_accepts_the_pinto_directory() {
    let board = TempDir::new().expect("board temp dir");
    pinto(board.path()).arg("init").assert().success();
    pinto(board.path())
        .args(["add", "Pinto path item"])
        .assert()
        .success();

    let worktree = TempDir::new().expect("worktree temp dir");
    pinto(worktree.path())
        .args([
            "list",
            "--dir",
            board.path().join(".pinto").to_str().expect("board path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pinto path item"));
}

#[test]
fn no_board_error_explains_discovery_and_overrides() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("ancestor"))
        .stderr(predicate::str::contains("--dir"))
        .stderr(predicate::str::contains("PINTO_DIR"));
}

#[test]
fn discovery_does_not_cross_a_repository_boundary() {
    let parent = TempDir::new().expect("parent temp dir");
    pinto(parent.path()).arg("init").assert().success();

    let repository = parent.path().join("repository");
    let nested = repository.join("src");
    std::fs::create_dir_all(&nested).expect("create repository directories");
    std::fs::create_dir(repository.join(".git")).expect("mark repository root");

    pinto(&nested)
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("ancestor"));
}

#[test]
fn write_command_recovers_a_stale_lock_file() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(dir.path().join(".pinto/.lock"), "999999999\n").expect("stale lock");

    pinto(dir.path())
        .args(["add", "Recovered write"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"));
}

#[test]
fn init_writes_default_file_storage_backend() {
    // `init` records the selected storage backend (file by default) in config for discoverability.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("[storage]"), "storage section present");
    assert!(
        config.contains("backend = \"file\""),
        "default backend is file"
    );
}
