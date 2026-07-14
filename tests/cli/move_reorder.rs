//! Move, reorder, and WIP transition flows.

use super::common::*;

#[test]
fn move_transitions_status_verified_before_and_after() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // 遷移前は todo。
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

    // 遷移後は in-progress。
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

    // 追加直後は未着手・未完了。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Started:     -"))
        .stdout(predicate::str::contains("Completed:   -"));

    // in-progress へ入ると start_at が記録される（done はまだ）。
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

    // done へ入ると done_at が記録される。
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    let done = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert!(done["done_at"].is_string(), "done_at recorded: {done}");
    // start_at は最初の着手時刻を保つ。
    assert_eq!(done["start_at"], started["start_at"]);

    // done を離れると done_at は消える（start_at は残る）。
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    let reopened = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(reopened["done_at"], serde_json::Value::Null);
    assert_eq!(reopened["start_at"], started["start_at"]);
}

#[test]
fn reorder_before_reference_updates_list_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for t in ["A", "B", "C"] {
        pinto(dir.path()).args(["add", t]).assert().success();
    }

    // C を A の前へ → 表示順 C, A, B。
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

    // rank だけ変わり status は in-progress のまま。
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

    // 移動先（--before/--after/--top/--bottom）未指定は clap の排他必須グループで弾く。
    pinto(dir.path()).args(["reorder", "T-1"]).assert().code(1);
}

#[test]
fn reorder_with_conflicting_targets_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();

    // --top と --before の同時指定は排他グループ違反。
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

    // 状態は変わっていない。
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

    // `mv` と同様、末尾を移動先として T-1 と T-3 をまとめて in-progress へ。
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
    // 指定していない T-2 は todo のまま。
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

    // 存在しない T-99 は黙って無視せずエラーにするが、T-1・T-2 は移動する。
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

    // 移動先ステータスを伴わない単独指定は clap が弾く。
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

    // 1 件目は上限内 → 警告なし。
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());

    // 2 件目で上限超過 → 移動は成功（コード 0）だが stderr に警告。
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

    // 一括移動で上限超過 → 両方成功しつつ、超過警告は宛先について一度だけ出す。
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
    // enabled=false ならプロジェクト全体で無効。
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
