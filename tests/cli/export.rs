//! Complete board export and JSON contract.

use super::common::*;
use std::collections::BTreeMap;
use std::fs;
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;

fn board_file_bytes(board: &Path) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(current: &Path, root: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).expect("read board directory") {
            let path = entry.expect("read board entry").path();
            if path.is_dir() {
                visit(&path, root, files);
            } else {
                let relative = path.strip_prefix(root).expect("board path is nested");
                files.insert(
                    relative.to_path_buf(),
                    fs::read(&path).expect("read board file"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(board, board, &mut files);
    files
}

#[test]
fn export_json_contains_the_complete_board_snapshot_without_mutating_it() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Exported PBI", "--body", "item acceptance"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Export Sprint",
            "--goal",
            "Ship the export",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dod", "set", "- [ ] tests pass\n- [ ] docs updated"])
        .assert()
        .success();

    let before = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let before_sprints = json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"]));
    let before_files = board_file_bytes(&dir.path().join(".pinto"));
    let exported = json_stdout(pinto(dir.path()).args(["export", "--json"]));

    assert_eq!(exported["items"], before);
    assert_eq!(exported["sprints"], before_sprints);
    assert_eq!(
        exported["config"]["columns"],
        serde_json::json!(["todo", "in-progress", "review", "done"])
    );
    assert_eq!(exported["config"]["done_column"], "done");
    assert_eq!(exported["config"]["project"]["key"], "T");
    assert_eq!(exported["config"]["storage"]["backend"], "file");
    assert_eq!(exported["dod"], "- [ ] tests pass\n- [ ] docs updated");
    assert!(
        exported["items"][0]["created"]
            .as_str()
            .expect("created timestamp")
            .ends_with("+00:00")
    );
    assert!(
        exported["sprints"][0]["created"]
            .as_str()
            .expect("sprint created timestamp")
            .ends_with("+00:00")
    );

    let repeated = json_stdout(pinto(dir.path()).args(["export", "--json"]));
    assert_eq!(repeated, exported, "export is read-only and deterministic");
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        before
    );
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["sprint", "list", "--json"])),
        before_sprints
    );
    assert_eq!(
        board_file_bytes(&dir.path().join(".pinto")),
        before_files,
        "export does not change board files"
    );
}

#[test]
fn export_json_uses_empty_arrays_and_null_for_optional_dod() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let exported = json_stdout(pinto(dir.path()).args(["export", "--json"]));

    assert_eq!(exported["items"], serde_json::json!([]));
    assert_eq!(exported["sprints"], serde_json::json!([]));
    assert!(exported["dod"].is_null());
    assert!(exported["config"].is_object());
}

#[test]
fn export_json_waits_for_a_board_writer_before_snapshotting() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let held = runtime
        .block_on(pinto::service::lock_board(dir.path()))
        .expect("hold board lock");

    let binary = pinto(dir.path()).get_program().to_owned();
    let mut export = ProcessCommand::new(binary)
        .args(["export", "--json"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn export");

    thread::sleep(Duration::from_millis(150));
    assert!(
        export.try_wait().expect("poll export").is_none(),
        "export must wait while a writer owns the board lock"
    );

    drop(held);
    drop(runtime);

    let output = export.wait_with_output().expect("wait export");
    assert!(
        output.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot: serde_json::Value = serde_json::from_slice(&output.stdout).expect("export JSON");
    assert_eq!(snapshot["items"], serde_json::json!([]));
    assert_eq!(snapshot["sprints"], serde_json::json!([]));
}

#[test]
fn export_json_never_mixes_records_with_a_concurrent_sprint_close() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(
        dir.path(),
        &[
            "Item 1", "Item 2", "Item 3", "Item 4", "Item 5", "Item 6", "Item 7", "Item 8",
        ],
    );
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Release sprint", "--goal", "Ship"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "--status", "todo"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();

    let binary = pinto(dir.path()).get_program().to_owned();
    let close = ProcessCommand::new(&binary)
        .args(["sprint", "close", "S-1", "--release"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sprint close");
    thread::sleep(Duration::from_millis(20));
    let export = ProcessCommand::new(&binary)
        .args(["export", "--json"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn export");

    let close_output = close.wait_with_output().expect("wait sprint close");
    let export_output = export.wait_with_output().expect("wait export");
    assert!(
        close_output.status.success(),
        "sprint close failed: {}",
        String::from_utf8_lossy(&close_output.stderr)
    );
    assert!(
        export_output.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&export_output.stderr)
    );

    let snapshot: serde_json::Value =
        serde_json::from_slice(&export_output.stdout).expect("export JSON");
    let state = snapshot["sprints"][0]["state"]
        .as_str()
        .expect("sprint state");
    let expected_sprint = match state {
        "active" => Some("S-1"),
        "closed" => None,
        other => panic!("unexpected sprint state {other}"),
    };
    for item in snapshot["items"].as_array().expect("export items") {
        assert_eq!(
            item["sprint"].as_str(),
            expected_sprint,
            "export mixed a Sprint record with an item record"
        );
    }
}

#[test]
fn export_json_never_mixes_ranks_with_a_concurrent_rebalance() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for number in 1..=40 {
        let title = format!("Item {number}");
        pinto(dir.path())
            .args(["add", title.as_str()])
            .assert()
            .success();
        if number >= 3 {
            let id = format!("T-{number}");
            pinto(dir.path())
                .args(["reorder", id.as_str(), "--before", "T-2"])
                .assert()
                .success();
        }
    }

    let before = json_stdout(pinto(dir.path()).args(["export", "--json"]));
    let binary = pinto(dir.path()).get_program().to_owned();
    let rebalance = ProcessCommand::new(&binary)
        .args(["rebalance"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rebalance");
    thread::sleep(Duration::from_millis(20));
    let export = ProcessCommand::new(&binary)
        .args(["export", "--json"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn export");

    let rebalance_output = rebalance.wait_with_output().expect("wait rebalance");
    let export_output = export.wait_with_output().expect("wait export");
    assert!(
        rebalance_output.status.success(),
        "rebalance failed: {}",
        String::from_utf8_lossy(&rebalance_output.stderr)
    );
    assert!(
        export_output.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&export_output.stderr)
    );

    let after = json_stdout(pinto(dir.path()).args(["export", "--json"]));
    let exported: serde_json::Value =
        serde_json::from_slice(&export_output.stdout).expect("export JSON");
    let ranks = |snapshot: &serde_json::Value| {
        snapshot["items"]
            .as_array()
            .expect("export items")
            .iter()
            .map(|item| {
                (
                    item["id"].as_str().expect("item ID").to_string(),
                    item["rank"].as_str().expect("item rank").to_string(),
                )
            })
            .collect::<BTreeMap<_, _>>()
    };
    let exported_ranks = ranks(&exported);
    assert!(
        exported_ranks == ranks(&before) || exported_ranks == ranks(&after),
        "export combined old and new ranks"
    );
}
