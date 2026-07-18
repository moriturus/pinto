//! Move, reorder, and WIP transition flows.

use super::common::*;

#[test]
fn move_transitions_status_verified_before_and_after() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // Before the transition, the item is todo.
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:      todo"));

    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success()
        .stdout(predicate::str::contains("in-progress"));

    // After the transition, the item is in-progress.
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:      in-progress"));
}

#[test]
fn move_records_start_and_done_timestamps() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // Immediately after creation, the item is neither started nor complete.
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started:     -"))
        .stdout(predicate::str::contains("Completed:   -"));

    // Entering in-progress records start_at; done_at is still unset.
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    let started = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert!(
        started["start_at"].is_string(),
        "start_at recorded: {started}"
    );
    assert_eq!(started["done_at"], serde_json::Value::Null);

    // Entering done records done_at.
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    let done = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert!(done["done_at"].is_string(), "done_at recorded: {done}");
    // start_at retains the first time work began.
    assert_eq!(done["start_at"], started["start_at"]);

    // Leaving done clears done_at while retaining start_at.
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    let reopened = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(reopened["done_at"], serde_json::Value::Null);
    assert_eq!(reopened["start_at"], started["start_at"]);
}

#[test]
fn move_to_done_warns_about_unchecked_acceptance_criteria_without_changing_body() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "add",
            "Incomplete task",
            "--body=- [x] shipped\n- [ ] documented",
        ])
        .assert()
        .success();
    let before = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));

    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success()
        .stdout(predicate::str::contains("done"))
        .stderr(predicate::str::contains("Acceptance Criteria"))
        .stderr(predicate::str::contains("1/2"));

    let after = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(after["status"], "done");
    assert_eq!(after["body"], before["body"]);
}

#[test]
fn move_to_done_with_completed_acceptance_criteria_is_silent() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "add",
            "Complete task",
            "--body=- [x] shipped\n- [X] documented",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

#[test]
fn reorder_before_reference_updates_list_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for t in ["A", "B", "C"] {
        pinto(dir.path()).args(["add", t]).assert().success();
    }

    // Move C before A; the display order becomes C, A, B.
    pinto(dir.path())
        .args(["reorder", "T-3", "--before", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Reordered T-3"));

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let ids: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|it| it["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["T-3", "T-1", "T-2"]);
}

#[test]
fn reorder_keeps_status_unchanged() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["reorder", "T-1", "--top"])
        .assert()
        .success();

    // Only rank changes; status remains in-progress.
    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["status"], "in-progress");
}

#[test]
fn reorder_relative_to_self_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();

    pinto(dir.path())
        .args(["reorder", "T-1", "--after", "T-1"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("itself"));
}

#[test]
fn reorder_without_target_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();

    // Omitting a destination (--before/--after/--top/--bottom) is rejected by clap's required
    // exclusive group.
    pinto(dir.path()).args(["reorder", "T-1"]).assert().code(1);
}

#[test]
fn reorder_with_conflicting_targets_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();

    // Supplying --top and --before together violates the exclusive group.
    pinto(dir.path())
        .args(["reorder", "T-1", "--top", "--before", "T-2"])
        .assert()
        .code(1);
}

#[test]
fn move_to_unknown_column_errors() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["move", "T-1", "archived"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("archived"));

    // The state remains unchanged.
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:      todo"));
}

#[test]
fn move_missing_id_exits_with_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["move", "T-99", "done"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-99"));
}

#[test]
fn move_invalid_id_is_reported_after_argument_parsing() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["move", "not-an-item-id", "done"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not-an-item-id"));
}

#[test]
fn move_transitions_multiple_items_to_status() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for t in ["A", "B", "C"] {
        pinto(dir.path()).args(["add", t]).assert().success();
    }

    // Like `mv`, use the final operand as the destination and move T-1 and T-3 together to
    // in-progress.
    pinto(dir.path())
        .args(["move", "T-1", "T-3", "in-progress"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Moved T-1"))
        .stdout(predicate::str::contains("Moved T-3"));

    for id in ["T-1", "T-3"] {
        pinto(dir.path())
            .args(["show", id])
            .assert()
            .success()
            .stdout(predicate::str::contains("Status:      in-progress"));
    }
    // Unspecified T-2 remains todo.
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Status:      todo"));
}

#[test]
fn move_multiple_continues_after_missing_id_and_exits_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();

    // Missing T-99 is reported instead of ignored, while T-1 and T-2 are moved.
    pinto(dir.path())
        .args(["move", "T-99", "T-1", "T-2", "in-progress"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("Moved T-1"))
        .stdout(predicate::str::contains("Moved T-2"))
        .stderr(predicate::str::contains("T-99"));

    for id in ["T-1", "T-2"] {
        pinto(dir.path())
            .args(["show", id])
            .assert()
            .success()
            .stdout(predicate::str::contains("Status:      in-progress"));
    }
}

#[test]
fn move_requires_id_and_status() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();

    // A lone ID without a destination status is rejected by clap.
    pinto(dir.path())
        .args(["move", "T-1"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn move_over_wip_limit_warns_on_stderr_but_succeeds() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);

    // The first item stays within the limit, so no warning is emitted.
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());

    // The second item exceeds the limit; the move still succeeds (exit code 0) but warns on stderr.
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Moved T-2"))
        .stderr(predicate::str::contains(
            "WIP limit exceeded in 'in-progress'",
        ))
        .stderr(predicate::str::contains("--no-wip-check"));
}

#[test]
fn move_with_no_wip_check_suppresses_warning() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["move", "T-2", "in-progress", "--no-wip-check"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn move_multiple_over_wip_limit_warns_once() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);

    // A bulk move can exceed the limit while both moves succeed; the destination warning appears
    // only once.
    let assert = pinto(dir.path())
        .args(["move", "T-1", "T-2", "in-progress"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Moved T-1"))
        .stdout(predicate::str::contains("Moved T-2"))
        .stderr(predicate::str::contains(
            "WIP limit exceeded in 'in-progress'",
        ));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    assert_eq!(
        stderr.matches("WIP limit exceeded").count(),
        1,
        "batch move warns once for the destination column: {stderr}"
    );
}

#[test]
fn move_multiple_with_no_wip_check_suppresses_warning() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);

    pinto(dir.path())
        .args(["move", "T-1", "T-2", "in-progress", "--no-wip-check"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn move_over_wip_limit_disabled_in_config_is_silent() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    // enabled=false disables the check for the entire project.
    set_wip_limit(dir.path(), "in-progress", 1, false);
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn reorder_within_siblings_updates_hierarchical_order() {
    // Reorder operates inside a sibling group: C3 before C1 puts it first among
    // P's children, while the subtree stays under P.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "P"]).assert().success(); // T-1
    for t in ["C1", "C2", "C3"] {
        pinto(dir.path())
            .args(["add", t, "--parent", "T-1"])
            .assert()
            .success();
    }

    pinto(dir.path())
        .args(["reorder", "T-4", "--before", "T-2"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let ids: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|it| it["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["T-1", "T-4", "T-2", "T-3"]);
}

#[test]
fn reorder_before_a_non_sibling_is_user_error_code_1() {
    // A child cannot be reordered relative to an unrelated top-level PBI; that
    // would only change its rank without moving it in the tree.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "P"]).assert().success(); // T-1
    pinto(dir.path())
        .args(["add", "CHILD", "--parent", "T-1"])
        .assert()
        .success(); // T-2
    pinto(dir.path()).args(["add", "ROOT"]).assert().success(); // T-3

    pinto(dir.path())
        .args(["reorder", "T-2", "--before", "T-3"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("not siblings"));
}
