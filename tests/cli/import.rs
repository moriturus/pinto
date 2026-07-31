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

/// Build a snapshot containing a Sprint, its Retro, and one action PBI promoted from that Retro.
fn snapshot_with_retro_action(dir: &Path) -> serde_json::Value {
    pinto(dir).arg("init").assert().success();
    pinto(dir)
        .args(["sprint", "new", "S-1", "First sprint"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "retro", "new", "S-1", "--body", "Notes"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "retro", "action", "S-1", "Follow up"])
        .assert()
        .success();
    serde_json::from_str(&export_snapshot(dir)).expect("export JSON parses")
}

/// Import `snapshot` into a fresh empty board and assert it is rejected without any durable write.
fn assert_snapshot_rejected(snapshot: &serde_json::Value, expected: &str) {
    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", "-"])
        .write_stdin(serde_json::to_string(snapshot).expect("serialize snapshot"))
        .assert()
        .code(1)
        .stderr(predicate::str::contains(expected));

    // A rejected import must leave the board empty: neither the orphaned record nor any Sprint is
    // written before the referential-integrity check fails.
    let sprints = json_stdout(pinto(dest.path()).args(["sprint", "list", "--json"]));
    assert!(
        sprints.as_array().is_some_and(Vec::is_empty),
        "no Sprint is written by a rejected import"
    );
    let retros = json_stdout(pinto(dest.path()).args(["sprint", "retro", "list", "--json"]));
    assert!(
        retros.as_array().is_some_and(Vec::is_empty),
        "no Retro is written by a rejected import"
    );
    let items = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    assert!(
        items.as_array().is_some_and(Vec::is_empty),
        "no PBI is written by a rejected import"
    );
}

#[test]
fn import_rejects_an_orphaned_child_record() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot = snapshot_with_retro_action(source.path());
    // Drop the Sprint but keep its Retro, leaving the Retro without a parent Sprint.
    snapshot["sprints"] = serde_json::json!([]);
    assert_snapshot_rejected(&snapshot, "no parent Sprint");
}

#[test]
fn import_rejects_a_duplicate_child_record() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot = snapshot_with_retro_action(source.path());
    // Append a second Retro for the same Sprint with a different body; a later "last wins" write
    // would silently discard one, so import must reject the duplicate up front.
    let mut duplicate = snapshot["retros"][0].clone();
    duplicate["body"] = serde_json::json!("Second");
    snapshot["retros"]
        .as_array_mut()
        .expect("retros is an array")
        .push(duplicate);
    assert_snapshot_rejected(&snapshot, "duplicate Retro");
}

#[test]
fn import_rejects_an_action_pbi_with_a_dangling_source() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot = snapshot_with_retro_action(source.path());
    // Remove the Retro while keeping the action PBI that sources it, leaving a dangling source.
    snapshot["retros"] = serde_json::json!([]);
    assert_snapshot_rejected(&snapshot, "is absent from the snapshot");
}

/// Assert `doctor` reports a healthy board for `dir`.
fn assert_board_healthy(dir: &Path) {
    pinto(dir)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("healthy"));
}

/// Build a board where an active child PBI references an archived parent, then round-trip it
/// through `export`/`import` on the given backend and assert the reference survives intact.
///
/// The source board is always the file backend; the destination backend is selected by rewriting
/// the snapshot's `config.storage.backend`, matching how the other cross-backend import tests target
/// each backend.
fn roundtrip_preserves_archived_parent(backend: &str) {
    let source = TempDir::new().expect("temp dir");
    pinto(source.path()).arg("init").assert().success();
    pinto(source.path())
        .args(["add", "Root"])
        .assert()
        .success();
    pinto(source.path())
        .args(["add", "Child", "--parent", "T-1"])
        .assert()
        .success();
    // Archive the parent. An active child legitimately keeps a reference to an archived parent, so
    // the source board stays healthy.
    pinto(source.path()).args(["rm", "T-1"]).assert().success();
    assert_board_healthy(source.path());

    let mut snapshot: serde_json::Value =
        serde_json::from_str(&export_snapshot(source.path())).expect("snapshot parses");
    // The archived parent must appear in the snapshot; excluding it would reimport as a dangling
    // reference.
    assert_eq!(snapshot["archived_items"][0]["id"], "T-1");
    snapshot["config"]["storage"]["backend"] = serde_json::Value::String(backend.to_string());

    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    let mut import = if backend == "git" {
        pinto_isolated_git(dest.path())
    } else {
        pinto(dest.path())
    };
    import
        .args(["import", "-"])
        .write_stdin(snapshot.to_string())
        .assert()
        .success();

    // The reimported board is healthy: the archived parent came along, so the child's reference
    // resolves instead of dangling.
    assert_board_healthy(dest.path());
    let active = json_stdout(pinto(dest.path()).args(["list", "--json"]));
    let active_ids: Vec<&str> = active
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(active_ids, ["T-2"]);
    let archived = json_stdout(pinto(dest.path()).args(["list", "--archived", "--json"]));
    let archived_ids: Vec<&str> = archived
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(archived_ids, ["T-1"]);
}

#[test]
fn import_preserves_an_archived_parent_in_file_and_git_backends() {
    for backend in ["file", "git"] {
        roundtrip_preserves_archived_parent(backend);
    }
}

#[cfg(feature = "sqlite")]
#[test]
fn import_preserves_an_archived_parent_in_the_sqlite_backend() {
    roundtrip_preserves_archived_parent("sqlite");
}

#[test]
fn import_force_clears_a_destination_archive() {
    // A forced import mirrors the snapshot, so archived PBIs the snapshot omits must be removed —
    // otherwise a stale archived PBI could keep a dangling reference to a Sprint the import deleted.
    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["add", "Stale archived"])
        .assert()
        .success();
    pinto(dest.path()).args(["rm", "T-1"]).assert().success();
    let archived = json_stdout(pinto(dest.path()).args(["list", "--archived", "--json"]));
    assert_eq!(archived.as_array().unwrap().len(), 1);

    // Import an empty board with --force.
    let empty = TempDir::new().expect("temp dir");
    pinto(empty.path()).arg("init").assert().success();
    let snapshot = export_snapshot(empty.path());
    pinto(dest.path())
        .args(["import", "--force", "-"])
        .write_stdin(snapshot)
        .assert()
        .success();

    let archived_after = json_stdout(pinto(dest.path()).args(["list", "--archived", "--json"]));
    assert!(
        archived_after.as_array().unwrap().is_empty(),
        "a forced import must clear the destination archive"
    );
    assert_board_healthy(dest.path());
}

#[test]
fn import_into_a_board_with_only_archived_pbis_is_refused_without_force() {
    let source = TempDir::new().expect("temp dir");
    let snapshot = populated_snapshot(source.path());

    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["add", "Archived only"])
        .assert()
        .success();
    pinto(dest.path()).args(["rm", "T-1"]).assert().success();

    // The board holds no active PBI, but its archive must not be silently overwritten.
    pinto(dest.path())
        .args(["import", "-"])
        .write_stdin(snapshot)
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--force"));

    let archived = json_stdout(pinto(dest.path()).args(["list", "--archived", "--json"]));
    let titles: Vec<&str> = archived
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(
        titles,
        ["Archived only"],
        "a refused import leaves the archive intact"
    );
}

#[test]
fn import_rejects_a_snapshot_with_duplicate_pbi_ids() {
    let source = TempDir::new().expect("temp dir");
    let snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    let mut snapshot = snapshot;
    // Duplicate the first PBI so the same ID appears twice; a later "last wins" write would silently
    // drop one, so import must reject the collision up front.
    let mut duplicate = snapshot["items"][0].clone();
    duplicate["title"] = serde_json::json!("Second copy");
    snapshot["items"]
        .as_array_mut()
        .expect("items is an array")
        .push(duplicate);
    assert_snapshot_rejected(&snapshot, "duplicate PBI");
}

#[test]
fn import_rejects_a_snapshot_whose_pbi_references_a_missing_parent() {
    let source = TempDir::new().expect("temp dir");
    let snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    let mut snapshot = snapshot;
    // Point a PBI's parent at an ID absent from both the active and archived collections.
    snapshot["items"][1]["parent"] = serde_json::json!("T-99");
    assert_snapshot_rejected(&snapshot, "missing parent");
}

#[test]
fn import_rejects_a_snapshot_with_a_self_parent_cycle() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // A PBI parenting itself resolves to an existing ID, so only a graph check catches it. doctor
    // reports the resulting board's parent cycle, so import must reject the snapshot up front.
    let id = snapshot["items"][0]["id"].clone();
    snapshot["items"][0]["parent"] = id;
    assert_snapshot_rejected(&snapshot, "cycle");
}

#[test]
fn import_rejects_a_snapshot_with_a_dependency_cycle() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // Two PBIs depending on each other resolve individually but close a dependency cycle that doctor
    // flags, so import must reject them before any write.
    let first = snapshot["items"][0]["id"].clone();
    let second = snapshot["items"][1]["id"].clone();
    snapshot["items"][0]["depends_on"] = serde_json::json!([second]);
    snapshot["items"][1]["depends_on"] = serde_json::json!([first]);
    assert_snapshot_rejected(&snapshot, "cycle");
}

#[test]
fn import_rejects_a_snapshot_with_an_empty_sprint_title() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // A blank Sprint title imports to a board whose normal Sprint reads fail, yet doctor once
    // reported it healthy, so import must reject it before any write.
    snapshot["sprints"][0]["title"] = serde_json::json!("   ");
    assert_snapshot_rejected(&snapshot, "sprint title must not be empty");
}

#[test]
fn import_rejects_a_snapshot_with_an_inverted_sprint_period() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    snapshot["sprints"][0]["start"] = serde_json::json!("2026-08-05T00:00:00Z");
    snapshot["sprints"][0]["end"] = serde_json::json!("2026-08-01T00:00:00Z");
    assert_snapshot_rejected(&snapshot, "invalid sprint period");
}

#[test]
fn import_rejects_a_snapshot_with_a_one_sided_sprint_period() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // A period is set as a pair through normal edits, so only one side is inconsistent.
    snapshot["sprints"][0]["start"] = serde_json::json!("2026-08-01T00:00:00Z");
    snapshot["sprints"][0]["end"] = serde_json::Value::Null;
    assert_snapshot_rejected(&snapshot, "has only one of start/end set");
}

#[test]
fn import_rejects_a_snapshot_with_an_active_sprint_missing_a_goal() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // An active Sprint always had a Goal when it started, so a blank-Goal active Sprint is a state
    // normal commands never produce.
    snapshot["sprints"][0]["state"] = serde_json::json!("active");
    snapshot["sprints"][0]["goal"] = serde_json::json!("");
    assert_snapshot_rejected(&snapshot, "sprint goal must be set");
}

#[test]
fn import_rejects_a_snapshot_with_a_goal_outcome_but_no_goal() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // The persistence layer clears an outcome whose Goal is blank, so accepting this pair would
    // silently drop the outcome on the next write. Reject it instead of losing the value.
    snapshot["sprints"][0]["goal"] = serde_json::json!("");
    snapshot["sprints"][0]["goal_achieved"] = serde_json::json!(true);
    assert_snapshot_rejected(&snapshot, "records a goal outcome but has no goal");
}

#[test]
fn import_rejects_a_snapshot_that_reuses_a_rank_in_one_scope() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // Two active PBIs share status "todo" and parent scope, so a normal-form rank that is reused
    // imports to a board doctor flags with duplicate ranks. Reject it up front.
    let rank = snapshot["items"][0]["rank"].clone();
    snapshot["items"][1]["rank"] = rank;
    assert_snapshot_rejected(&snapshot, "reuses rank");
}

#[test]
fn import_allows_a_snapshot_that_reuses_a_rank_across_scopes() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // Re-parent the second PBI so the shared rank lands in a different parent scope, which doctor
    // treats as unique, so the import is accepted and stays healthy.
    let rank = snapshot["items"][0]["rank"].clone();
    let parent = snapshot["items"][0]["id"].clone();
    snapshot["items"][1]["parent"] = parent;
    snapshot["items"][1]["rank"] = rank;

    let dest = TempDir::new().expect("temp dir");
    pinto(dest.path()).arg("init").assert().success();
    pinto(dest.path())
        .args(["import", "-"])
        .write_stdin(snapshot.to_string())
        .assert()
        .success();
    assert_board_healthy(dest.path());
}

#[test]
fn import_rejects_a_snapshot_with_a_status_outside_configured_columns() {
    let source = TempDir::new().expect("temp dir");
    let mut snapshot: serde_json::Value =
        serde_json::from_str(&populated_snapshot(source.path())).expect("snapshot parses");
    // A status that names no workflow column imports to a board doctor flags, so import must reject
    // it using the snapshot's own configured columns.
    snapshot["items"][0]["status"] = serde_json::json!("not-a-column");
    assert_snapshot_rejected(&snapshot, "workflow column");
}
