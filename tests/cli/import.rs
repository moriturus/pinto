//! `import` restores a board from the `export --json` contract.

use super::common::*;
use std::fs;

/// Capture the stdout of `export --json` for `dir` as a UTF-8 string.
fn export_snapshot(dir: &Path) -> String {
    let output = pinto(dir)
        .args(["export", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(output).expect("export JSON is UTF-8")
}

/// Build a populated source board and return its export snapshot document.
fn populated_snapshot(dir: &Path) -> String {
    pinto(dir).arg("init").assert().success();
    pinto(dir)
        .args(["add", "Alpha", "--label", "x", "--body", "Body A"])
        .assert()
        .success();
    pinto(dir).args(["add", "Beta"]).assert().success();
    pinto(dir)
        .args(["sprint", "new", "S-1", "First sprint"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir)
        .args(["dod", "set", "- [ ] reviewed"])
        .assert()
        .success();
    export_snapshot(dir)
}

#[test]
fn import_restores_items_sprints_and_dod_into_an_empty_board() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    let snapshot_path = dest.path().join("snapshot.json");
    fs::write(&snapshot_path, &snapshot).expect("write snapshot");

    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", snapshot_path.to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Imported 2 item(s) and 1 sprint(s).",
        ));

    let items = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    let titles: Vec<&str> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Alpha", "Beta"]);

    let sprints = json_stdout(pinto(dest.path()).args(["sprint", "list", "--json"]));
    assert_eq!(sprints.as_array().unwrap().len(), 1);
    assert_eq!(sprints[0]["id"], "S-1");

    pinto(dest.path())
        .arg("dod")
        .assert()
        .success()
        .stdout(predicate::str::contains("- [ ] reviewed"));
}

#[test]
fn import_round_trip_reproduces_an_identical_export() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    let snapshot_path = dest.path().join("snapshot.json");
    fs::write(&snapshot_path, &snapshot).expect("write snapshot");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", snapshot_path.to_str().unwrap()])
        .assert()
        .success();

    let round_tripped = export_snapshot(dest.path());
    assert_eq!(
        round_tripped, snapshot,
        "export -> import -> export must reproduce the same JSON contract"
    );
}

#[test]
fn import_into_a_non_empty_board_fails_without_force() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    let snapshot_path = dest.path().join("snapshot.json");
    fs::write(&snapshot_path, &snapshot).expect("write snapshot");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["add", "Existing"])
        .assert()
        .success();

    pinto(dest.path())
        .args(["import", snapshot_path.to_str().unwrap()])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));

    // A refused import leaves the board untouched.
    let items = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    let titles: Vec<&str> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Existing"]);
}

#[test]
fn import_force_replaces_existing_board_data() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    let snapshot_path = dest.path().join("snapshot.json");
    fs::write(&snapshot_path, &snapshot).expect("write snapshot");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["add", "Existing"])
        .assert()
        .success();

    pinto(dest.path())
        .args(["import", "--force", snapshot_path.to_str().unwrap()])
        .assert()
        .success();

    let items = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    let titles: Vec<&str> = items
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Alpha", "Beta"], "snapshot replaces prior data");
}

#[test]
fn import_reads_the_snapshot_from_standard_input() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", "-"])
        .write_stdin(snapshot)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Imported 2 item(s) and 1 sprint(s).",
        ));

    let items = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    assert_eq!(items.as_array().unwrap().len(), 2);
}

#[test]
fn import_rejects_a_malformed_snapshot() {
    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", "-"])
        .write_stdin("{ not valid json ")
        .assert()
        .code(1);
}
