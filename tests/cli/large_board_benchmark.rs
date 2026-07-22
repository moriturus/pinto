//! CLI workflow used to prepare and measure generated large boards.

use super::common::*;
use pinto::rank::Rank;
use serde_json::{Value, json};
use std::fs;

const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00+00:00";

#[test]
fn generated_board_supports_list_show_add_and_move() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let snapshot = json!({
        "items": [
            {
                "id": "T-1",
                "title": "Generated one",
                "status": "todo",
                "rank": "i",
                "points": null,
                "labels": [],
                "assignee": null,
                "sprint": null,
                "parent": null,
                "depends_on": [],
                "start_at": null,
                "done_at": null,
                "commits": [],
                "created": "2026-01-01T00:00:00+00:00",
                "updated": "2026-01-01T00:00:00+00:00",
                "body": ""
            },
            {
                "id": "T-2",
                "title": "Generated two",
                "status": "todo",
                "rank": "j",
                "points": null,
                "labels": [],
                "assignee": null,
                "sprint": null,
                "parent": null,
                "depends_on": [],
                "start_at": null,
                "done_at": null,
                "commits": [],
                "created": "2026-01-01T00:00:00+00:00",
                "updated": "2026-01-01T00:00:00+00:00",
                "body": ""
            },
            {
                "id": "T-3",
                "title": "Generated three",
                "status": "todo",
                "rank": "k",
                "points": null,
                "labels": [],
                "assignee": null,
                "sprint": null,
                "parent": null,
                "depends_on": [],
                "start_at": null,
                "done_at": null,
                "commits": [],
                "created": "2026-01-01T00:00:00+00:00",
                "updated": "2026-01-01T00:00:00+00:00",
                "body": ""
            }
        ],
        "sprints": [],
        "config": {
            "columns": ["todo", "in-progress", "review", "done"],
            "display": {"markdown": true, "timezone": "local"},
            "done_column": "done",
            "points": {"aggregate_children": false},
            "project": {"key": "T", "name": "generated-board"},
            "storage": {"backend": "file"},
            "tui": {"confirm_quit": true},
            "wip": {"enabled": true}
        },
        "dod": null
    });
    let snapshot_path = dir.path().join("generated-snapshot.json");
    fs::write(
        &snapshot_path,
        serde_json::to_vec(&snapshot).expect("serialize generated snapshot"),
    )
    .expect("write generated snapshot");

    pinto(dir.path())
        .args(["import", snapshot_path.to_str().expect("snapshot path")])
        .assert()
        .success();

    let listed = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(listed.as_array().map(Vec::len), Some(3));
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-2", "--json"]))["title"],
        "Generated two"
    );

    pinto(dir.path())
        .args(["add", "Measured benchmark item"])
        .assert()
        .success();
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-4", "--json"]))["title"],
        "Measured benchmark item"
    );

    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["status"],
        "in-progress"
    );
}

#[test]
fn generated_thousand_item_board_can_be_imported() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let snapshot_path = dir.path().join("thousand-item-snapshot.json");
    fs::write(
        &snapshot_path,
        serde_json::to_vec(&generated_snapshot(1_000)).expect("serialize generated snapshot"),
    )
    .expect("write generated snapshot");

    pinto(dir.path())
        .args(["import", snapshot_path.to_str().expect("snapshot path")])
        .assert()
        .success();

    let listed = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(listed.as_array().map(Vec::len), Some(1_000));
}

fn generated_snapshot(size: usize) -> Value {
    let mut previous = None;
    let items = (1..=size)
        .map(|number| {
            let rank = Rank::after(previous.as_ref());
            previous = Some(rank.clone());
            json!({
                "id": format!("T-{number}"),
                "title": format!("Generated benchmark item {number}"),
                "status": "todo",
                "rank": rank.as_str(),
                "points": null,
                "labels": [],
                "assignee": null,
                "sprint": null,
                "parent": null,
                "depends_on": [],
                "start_at": null,
                "done_at": null,
                "commits": [],
                "created": FIXED_TIMESTAMP,
                "updated": FIXED_TIMESTAMP,
                "body": ""
            })
        })
        .collect::<Vec<_>>();

    json!({
        "items": items,
        "sprints": [],
        "config": {
            "columns": ["todo", "in-progress", "review", "done"],
            "display": {"markdown": true, "timezone": "local"},
            "done_column": "done",
            "points": {"aggregate_children": false},
            "project": {"key": "T", "name": "generated-board"},
            "storage": {"backend": "file"},
            "tui": {"confirm_quit": true},
            "wip": {"enabled": true}
        },
        "dod": null
    })
}
