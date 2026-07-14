//! Editing and removing backlog items.

use super::common::*;

#[test]
fn edit_updates_fields_and_persists() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Old"]).assert().success();

    pinto(dir.path())
        .args([
            "edit",
            "T-1",
            "--title",
            "New title",
            "--points",
            "8",
            "--label",
            "backend",
            "--label",
            "urgent",
            "--assignee",
            "alice",
            "--body",
            "Acceptance criteria",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated T-1"))
        .stdout(predicate::str::contains("New title"));

    // 変更が永続化され、show で確認できる。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("New title"))
        .stdout(predicate::str::contains("8"))
        .stdout(predicate::str::contains("alice"))
        .stdout(predicate::str::contains("backend, urgent"))
        .stdout(predicate::str::contains("Acceptance criteria"));
}

#[test]
fn edit_rejects_invalid_or_missing_sprint_without_mutating_the_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Task", "--sprint", "S-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["edit", "T-1", "--sprint", "S 2"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid sprint id"));
    pinto(dir.path())
        .args(["edit", "T-1", "--sprint", "S-9"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("sprint not found"));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["title"], "Task");
    assert_eq!(item["sprint"], "S-1");
}

#[test]
fn edit_with_no_fields_and_no_editor_guides_user() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // フィールド指定が無く `$EDITOR` も未設定なら、設定方法とフィールド編集を案内する。
    pinto(dir.path())
        .args(["edit", "T-1"])
        .env_remove("EDITOR")
        .env_remove("VISUAL")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no editor configured"))
        .stderr(predicate::str::contains("pinto edit <id> --title ..."));
}

#[cfg(unix)]
#[test]
fn edit_without_fields_opens_editor_and_applies_changes() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // `$EDITOR` がタイトル行を書き換える。保存後に検証され PBI へ反映される。
    let ed = editor_script(
        dir.path(),
        "ed.sh",
        "sed -i.bak 's/^title = .*/title = \"Edited in editor\"/' \"$1\"",
    );
    pinto(dir.path())
        .args(["edit", "T-1"])
        .env("EDITOR", &ed)
        .env_remove("VISUAL")
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated T-1"))
        .stdout(predicate::str::contains("Edited in editor"));

    // 永続化を確認する（.pinto は pinto コマンド経由でのみ参照する）。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edited in editor"));
}

#[cfg(unix)]
#[test]
fn edit_editor_rejects_missing_sprint_without_mutating_the_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Task", "--sprint", "S-1"])
        .assert()
        .success();

    let ed = editor_script(
        dir.path(),
        "bad-sprint.sh",
        "sed -i.bak 's/^sprint = .*/sprint = \"S-9\"/' \"$1\"",
    );
    pinto(dir.path())
        .args(["edit", "T-1"])
        .env("EDITOR", &ed)
        .env_remove("VISUAL")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("sprint not found"));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["sprint"], "S-1");
}

#[cfg(unix)]
#[test]
fn edit_without_fields_no_change_reports_unchanged() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // `true` は引数を無視し一時ファイルを変更しない＝キャンセル相当。
    pinto(dir.path())
        .args(["edit", "T-1"])
        .env("EDITOR", "true")
        .env_remove("VISUAL")
        .assert()
        .success()
        .stdout(predicate::str::contains("No changes to T-1"));
}

#[cfg(unix)]
#[test]
fn edit_without_fields_rejects_invalid_content_and_preserves_data() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // frontmatter を壊す編集は反映せず、元データを保つ。
    let marker = dir.path().join("editor-buffer-path");
    let ed = editor_script(
        dir.path(),
        "break.sh",
        "printf '%s' \"$1\" > \"$PINTO_BUFFER_PATH\"\nprintf 'garbage' > \"$1\"",
    );
    pinto(dir.path())
        .args(["edit", "T-1"])
        .env("EDITOR", &ed)
        .env("PINTO_BUFFER_PATH", &marker)
        .env_remove("VISUAL")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("edited content is invalid"));

    let buffer_path = std::fs::read_to_string(marker).expect("editor recorded buffer path");
    assert!(!Path::new(buffer_path.trim()).exists());

    // 元のタイトルが保たれている。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Task"));
}

#[test]
fn edit_missing_id_is_a_user_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["edit", "T-99", "--title", "x"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn rm_invalid_id_is_reported_after_argument_parsing() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["rm", "not-an-item-id"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not-an-item-id"));
}

#[test]
fn rm_rejects_invalid_ids_before_removing_any_valid_target() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Keep me"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["rm", "T-1", "../outside-1", "--force"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid item id"));

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Keep me"));
}

#[test]
fn rm_archives_by_default() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["rm", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived T-1"));

    // タスクは tasks から消え、アーカイブへ退避される。
    assert!(
        dir.path().join(".pinto/archive/T-1.md").is_file(),
        "archived file should exist"
    );
    assert!(
        !dir.path().join(".pinto/tasks/T-1.md").exists(),
        "task file should be moved out of tasks/"
    );
    // show では見つからない。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn rm_force_deletes_permanently() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["rm", "T-1", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted T-1"));

    // 物理削除はアーカイブに残さない。
    assert!(
        !dir.path().join(".pinto/archive/T-1.md").exists(),
        "force delete must not archive"
    );
    assert!(!dir.path().join(".pinto/tasks/T-1.md").exists());
}

#[test]
fn rm_force_does_not_reuse_id_or_rebind_commit_links() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Original"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["link", "add", "T-1", "deadbeef"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["rm", "T-1", "--force"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["add", "Replacement"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-2"));
    let replacement = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(replacement["commits"], serde_json::json!([]));
}

#[test]
fn rm_force_rejects_parent_and_dependency_references() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Target"]).assert().success();
    pinto(dir.path())
        .args(["add", "Child", "--parent", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dep", "add", "T-3", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["rm", "T-1", "--force"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("referenced by"))
        .stderr(predicate::str::contains("T-2"))
        .stderr(predicate::str::contains("T-3"));

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Target"));
}

#[test]
fn rm_missing_id_is_a_user_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["rm", "T-99"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn rm_archives_multiple_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();
    pinto(dir.path()).args(["add", "Third"]).assert().success();

    pinto(dir.path())
        .args(["rm", "T-1", "T-3"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Archived T-1"))
        .stdout(predicate::str::contains("Archived T-3"));

    for id in ["T-1", "T-3"] {
        pinto(dir.path())
            .args(["show", id])
            .assert()
            .failure()
            .stderr(predicate::str::contains("not found"));
    }
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Second"));
}

#[test]
fn rm_force_applies_to_multiple_items_and_continues_after_missing_id() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    pinto(dir.path())
        .args(["rm", "T-99", "T-1", "T-2", "--force"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("Deleted T-1"))
        .stdout(predicate::str::contains("Deleted T-2"))
        .stderr(predicate::str::contains("T-99"))
        .stderr(predicate::str::contains("not found"));

    for id in ["T-1", "T-2"] {
        pinto(dir.path())
            .args(["show", id])
            .assert()
            .failure()
            .stderr(predicate::str::contains("not found"));
    }
}

#[test]
fn rm_requires_at_least_one_id() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("rm")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("required"));
}
