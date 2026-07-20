//! File and SQLite storage backends and `migrate` transitions.

use super::common::*;

#[test]
fn unknown_storage_backend_is_a_user_error_not_panic() {
    // An unknown backend is reported as an actionable parse error (user error, exit code 1), not
    // ignored. Since `sqlite` is valid with the feature enabled, use a value that is always unknown.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"postgres\"\n\n[wip]\nenabled = true\n",
    )
    .expect("write config");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown variant"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn file_backend_roundtrips_add_and_list() {
    // Verify that the default file backend preserves existing behavior through the backend abstraction.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Via backend"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Via backend"));
}

#[test]
fn migrate_on_uninitialized_dir_errors() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["migrate", "--to", "file"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn migrate_to_current_backend_is_noop() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "file"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already using file"));
}

#[test]
fn migrate_to_git_switches_the_backend() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Git task"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success()
        .stdout(predicate::str::contains("now git"));
}

#[cfg(feature = "sqlite")]
#[test]
fn migrate_to_sqlite_switches_backend_and_persists() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Persisted task"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrated 1 item(s)"))
        .stdout(predicate::str::contains("now sqlite"));

    // The configuration switches and the database file is created.
    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("backend = \"sqlite\""), "backend switched");
    assert!(
        dir.path().join(".pinto/board.sqlite3").is_file(),
        "db file created"
    );

    // Subsequent `list` commands read from SQLite, providing an end-to-end backend check.
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Persisted task"));

    // Add operations also update the SQLite backend.
    pinto(dir.path())
        .args(["add", "Second on sqlite"])
        .assert()
        .success();
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Second on sqlite"));
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_normalized_schema_roundtrips_via_cli() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success();

    // Add a PBI with multiple labels and points (a typed optional field).
    pinto(dir.path())
        .args([
            "add", "First", "--label", "backend", "--label", "urgent", "--points", "5",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    // Add a dependency stored in the related table.
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    // list restores labels and points from the typed columns.
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("(5)"))
        .stdout(predicate::str::contains("[backend, urgent]"));

    // show reflects the dependency.
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Depends on:").and(predicate::str::contains("T-1")));

    // Archiving removes the item from list, while ID allocation does not reuse the archived ID.
    pinto(dir.path()).args(["rm", "T-1"]).assert().success();
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("First").not())
        .stdout(predicate::str::contains("Second"));
    pinto(dir.path())
        .args(["add", "Third"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-3"));
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_corruption_is_rejected_by_list_and_show() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Visible task"])
        .assert()
        .success();

    let db_path = dir.path().join(".pinto/board.sqlite3");
    let conn = rusqlite::Connection::open(db_path).expect("open sqlite database");
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE items SET title = ' ' WHERE id = 'T-1'", [])
        .expect("corrupt item title");

    for args in [["list"].as_slice(), ["show", "T-1"].as_slice()] {
        pinto(dir.path())
            .args(args)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("empty item title"));
    }
}

#[cfg(not(feature = "sqlite"))]
#[test]
fn migrate_to_sqlite_without_feature_errors() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--features sqlite"));
}
