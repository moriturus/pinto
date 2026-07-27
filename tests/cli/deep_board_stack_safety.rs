//! Deep parent, dependency, and cyclic boards must traverse without overflowing
//! the stack, exercising the ordering, aggregation, and cycle-inspection CLI
//! paths end to end. See P-50.

use super::common::*;
use pinto::rank::Rank;
use serde_json::{Value, json};
use std::fs;

const FIXED_TIMESTAMP: &str = "2026-01-01T00:00:00+00:00";

/// "Several thousand levels deep" per the acceptance criteria: large enough to
/// exercise the deep-traversal paths, small enough to import quickly.
const DEPTH: usize = 4_000;

/// Wrap generated `items` in a minimal board snapshot, toggling parent point
/// aggregation so the deep-chain aggregation path is exercised on demand.
fn snapshot(items: Vec<Value>, aggregate_children: bool) -> Value {
    json!({
        "items": items,
        "sprints": [],
        "config": {
            "columns": ["todo", "in-progress", "review", "done"],
            "display": {"markdown": true, "timezone": "local"},
            "done_column": "done",
            "points": {"aggregate_children": aggregate_children},
            "project": {"key": "T", "name": "deep-board"},
            "storage": {"backend": "file"},
            "tui": {"confirm_quit": true},
            "wip": {"enabled": true}
        },
        "dod": null
    })
}

/// One PBI with explicit `parent`/`depends_on`/`points` links.
fn item(
    number: usize,
    rank: &Rank,
    parent: Option<usize>,
    depends_on: &[usize],
    points: Value,
) -> Value {
    json!({
        "id": format!("T-{number}"),
        "title": format!("Deep item {number}"),
        "status": "todo",
        "rank": rank.as_str(),
        "points": points,
        "labels": [],
        "assignee": null,
        "sprint": null,
        "parent": parent.map(|p| format!("T-{p}")),
        "depends_on": depends_on.iter().map(|d| format!("T-{d}")).collect::<Vec<_>>(),
        "start_at": null,
        "done_at": null,
        "commits": [],
        "created": FIXED_TIMESTAMP,
        "updated": FIXED_TIMESTAMP,
        "body": ""
    })
}

/// Ascending, unique, short ranks so the imported board is already in canonical
/// order. Fixed-width base-36 keeps lexicographic order aligned with numeric
/// order and stays short at any depth, unlike chained `Rank::after`, which
/// saturates to multi-thousand-character strings.
fn ranks(count: usize) -> Vec<Rank> {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    const WIDTH: usize = 4; // 36^4 = 1_679_616 distinct ranks.
    (1..=count)
        .map(|n| {
            let mut value = n;
            let mut digits = vec![b'0'; WIDTH];
            for slot in digits.iter_mut().rev() {
                *slot = DIGITS[value % 36];
                value /= 36;
            }
            let mut rank = String::from_utf8(digits).expect("ascii digits");
            rank.push('1'); // Never end in '0', which is not a valid rank.
            Rank::parse(&rank).expect("valid rank")
        })
        .collect()
}

fn import(dir: &Path, snapshot: &Value) {
    let path = dir.join("snapshot.json");
    fs::write(
        &path,
        serde_json::to_vec(snapshot).expect("serialize snapshot"),
    )
    .expect("write snapshot");
    pinto(dir)
        .args(["import", path.to_str().expect("snapshot path")])
        .assert()
        .success();
}

#[test]
fn deep_parent_chain_lists_aggregates_and_passes_doctor() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // T-1 <- T-2 <- ... <- T-DEPTH; only the deepest leaf carries an estimate.
    let ranks = ranks(DEPTH);
    let items: Vec<Value> = (1..=DEPTH)
        .map(|n| {
            let points = if n == DEPTH { json!(3) } else { Value::Null };
            item(n, &ranks[n - 1], (n > 1).then(|| n - 1), &[], points)
        })
        .collect();
    import(dir.path(), &snapshot(items, true));

    // Ordering path: every item is listed, the root first down the single chain.
    let listed = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let listed = listed.as_array().expect("list array");
    assert_eq!(listed.len(), DEPTH);
    assert_eq!(listed[0]["id"], "T-1");

    // Aggregation path: the single leaf estimate propagates up the whole chain.
    let root = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(
        root["points"],
        json!(3),
        "leaf estimate rolls up to the root"
    );

    // Cycle-inspection path: an acyclic chain is healthy.
    pinto(dir.path()).arg("doctor").assert().success();
}

#[test]
fn deep_dependency_chain_passes_doctor_and_warns_on_a_back_edge() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // T-2 -> T-1, T-3 -> T-2, ... a dependency chain DEPTH links long.
    let ranks = ranks(DEPTH);
    let items = (1..=DEPTH)
        .map(|n| {
            let deps: Vec<usize> = if n > 1 { vec![n - 1] } else { Vec::new() };
            item(n, &ranks[n - 1], None, &deps, Value::Null)
        })
        .collect();
    import(dir.path(), &snapshot(items, false));

    // A straight dependency chain has no cycle.
    pinto(dir.path()).arg("doctor").assert().success();

    // Closing the chain back to the head is a warning-only cycle, and the deep
    // transitive walk that discovers it must not overflow the stack.
    pinto(dir.path())
        .args(["dep", "add", "T-1", &format!("T-{DEPTH}")])
        .assert()
        .success()
        .stderr(predicate::str::contains("cycle"));
}

#[test]
fn deep_dependency_cycle_is_reported_by_doctor() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // T-1 -> T-2 -> ... -> T-DEPTH -> T-1: a dependency cycle spanning every node.
    let ranks = ranks(DEPTH);
    let items = (1..=DEPTH)
        .map(|n| {
            let next = if n == DEPTH { 1 } else { n + 1 };
            item(n, &ranks[n - 1], None, &[next], Value::Null)
        })
        .collect();
    import(dir.path(), &snapshot(items, false));

    // Cycle inspection must terminate and flag the dependency cycle (exit 1).
    pinto(dir.path())
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicate::str::contains("dependency"));
}
