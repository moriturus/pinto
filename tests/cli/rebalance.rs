//! `rebalance` CLI behavior.

use super::common::*;

#[test]
fn rebalance_cli_shortens_a_narrow_rank_interval() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for title in ["A", "B"] {
        pinto(dir.path()).args(["add", title]).assert().success();
    }

    for number in 3..=40 {
        let title = format!("Item {number}");
        pinto(dir.path())
            .args(["add", title.as_str()])
            .assert()
            .success();
        let id = format!("T-{number}");
        pinto(dir.path())
            .args(["reorder", id.as_str(), "--before", "T-2"])
            .assert()
            .success();
    }

    let before = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let before_items = before.as_array().expect("list --json is an array");
    assert!(
        before_items
            .iter()
            .any(|item| item["rank"].as_str().unwrap().len() > 2),
        "repeated insertion must create a narrow, long rank interval"
    );
    let before_ids: Vec<_> = before_items
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();

    pinto(dir.path())
        .args(["rebalance", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would rebalance"));
    pinto(dir.path())
        .arg("rebalance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Rebalanced"));

    let after = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let after_items = after.as_array().expect("list --json is an array");
    let after_ids: Vec<_> = after_items
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(after_ids, before_ids, "rebalance preserves backlog order");
    assert!(
        after_items
            .iter()
            .all(|item| item["rank"].as_str().unwrap().len() == 2),
        "a forty-item sibling scope uses two-digit ranks"
    );
}

#[test]
fn rebalance_reports_when_an_empty_board_is_already_balanced() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("rebalance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Ranks already balanced"));
}
