//! Complete board export and JSON contract.

use super::common::*;
use std::collections::BTreeMap;
use std::fs;

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
