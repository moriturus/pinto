//! CLI add and item-template flows.

use super::common::*;

#[test]
fn add_creates_task_file_with_frontmatter() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Write the parser"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("Write the parser"));

    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-1.md")).expect("task file");
    assert!(task.contains("title = \"Write the parser\""));
    assert!(task.contains("status = \"todo\""));
    assert!(task.contains("id = \"T-1\""));
    assert!(task.contains("rank = "));
}

#[test]
fn add_applies_options() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "add",
            "Configured task",
            "--points",
            "8",
            "--label",
            "backend",
            "--label",
            "urgent",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();

    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-1.md")).expect("task file");
    assert!(task.contains("points = 8"));
    assert!(task.contains("backend"));
    assert!(task.contains("urgent"));
    assert!(task.contains("sprint = \"S-1\""));
}

#[test]
fn add_rejects_invalid_or_missing_sprint_before_creating_an_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Malformed", "--sprint", "S 1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid sprint id"));
    pinto(dir.path())
        .args(["add", "Missing", "--sprint", "S-9"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("sprint not found"));

    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([])
    );
    pinto(dir.path())
        .args(["add", "After validation errors"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"));
}

#[test]
fn add_sets_parent_and_multiple_dependencies() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Parent"]).assert().success();
    pinto(dir.path())
        .args(["add", "Dependency"])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "add",
            "Story",
            "--parent",
            "T-1",
            "--depends-on",
            "T-1",
            "T-2",
        ])
        .assert()
        .success();

    let item = show_json(pinto(dir.path()).args(["show", "T-3", "--json"]));
    assert_eq!(item["parent"], "T-1");
    assert_eq!(item["depends_on"], serde_json::json!(["T-1", "T-2"]));
}

#[test]
fn add_rejects_missing_parent_and_does_not_create_an_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Story", "--parent", "T-404"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-404"))
        .stderr(predicate::str::contains("not found"));

    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([])
    );
}

#[test]
fn add_rejects_missing_dependency_and_self_parent() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Missing dependency", "--depends-on", "T-404"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-404"))
        .stderr(predicate::str::contains("not found"));

    pinto(dir.path())
        .args(["add", "Self parent", "--parent", "T-1"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("cycle"));

    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([])
    );
}

#[test]
fn add_dependency_cycle_is_a_warning_and_not_an_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Self dependent", "--depends-on", "T-1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("dependency introduces a cycle"));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["depends_on"], serde_json::json!(["T-1"]));
}

#[test]
fn add_help_lists_relationship_options() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-P, --parent"))
        .stdout(predicate::str::contains("-d, --depends-on"));
}

#[test]
fn add_uses_a_plain_text_item_template() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create template dir");
    std::fs::write(
        template_dir.join("story.md"),
        "## Acceptance Criteria\n\n- [ ] Complete",
    )
    .expect("write template");

    pinto(dir.path())
        .args(["add", "Template-backed item", "--template", "story"])
        .assert()
        .success();

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["body"], "## Acceptance Criteria\n\n- [ ] Complete");
}

#[test]
fn add_with_missing_template_explains_where_to_create_it() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let expected_path = dir
        .path()
        .join(".pinto")
        .join("templates")
        .join("item")
        .join("missing.md")
        .display()
        .to_string();

    pinto(dir.path())
        .args(["add", "Item", "--template", "missing"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("template not found: item/missing"))
        .stderr(predicate::str::contains(expected_path));
}

#[test]
fn add_with_unreadable_template_explains_which_file_to_fix() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let template_path = dir
        .path()
        .join(".pinto")
        .join("templates")
        .join("item")
        .join("broken.md");
    std::fs::create_dir_all(&template_path).expect("create directory in place of template");
    let expected_path = template_path.display().to_string();

    pinto(dir.path())
        .args(["add", "Item", "--template", "broken"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("template cannot be read"))
        .stderr(predicate::str::contains(expected_path));
}

#[cfg(unix)]
#[test]
fn add_edit_uses_visual_first_and_saves_editor_content_as_body() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let visual = editor_script(
        dir.path(),
        "visual.sh",
        "printf 'body saved from VISUAL' > \"$1\"",
    );
    let editor = editor_script(
        dir.path(),
        "editor.sh",
        "printf 'body saved from EDITOR' > \"$1\"",
    );

    pinto(dir.path())
        .args(["add", "Edited body", "-E"])
        .env("VISUAL", visual)
        .env("EDITOR", editor)
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-1"));

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["body"], "body saved from VISUAL");
}

#[cfg(unix)]
#[test]
fn add_edit_removes_the_buffer_after_editor_success() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let marker = dir.path().join("editor-buffer-path");
    let editor = editor_script(
        dir.path(),
        "record-path.sh",
        "printf '%s' \"$1\" > \"$PINTO_BUFFER_PATH\"\nprintf 'saved body' > \"$1\"",
    );

    pinto(dir.path())
        .args(["add", "Cleanup", "--edit"])
        .env("EDITOR", &editor)
        .env("PINTO_BUFFER_PATH", &marker)
        .assert()
        .success();

    let buffer_path = std::fs::read_to_string(marker).expect("editor recorded buffer path");
    assert!(!Path::new(buffer_path.trim()).exists());
}

#[cfg(unix)]
#[test]
fn add_edit_removes_the_buffer_after_editor_failure() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let marker = dir.path().join("editor-buffer-path");
    let editor = editor_script(
        dir.path(),
        "fail-after-recording-path.sh",
        "printf '%s' \"$1\" > \"$PINTO_BUFFER_PATH\"\nexit 1",
    );

    pinto(dir.path())
        .args(["add", "Cleanup failure", "--edit"])
        .env("EDITOR", &editor)
        .env("PINTO_BUFFER_PATH", &marker)
        .assert()
        .failure()
        .stderr(predicate::str::contains("failed to launch editor"));

    let buffer_path = std::fs::read_to_string(marker).expect("editor recorded buffer path");
    assert!(!Path::new(buffer_path.trim()).exists());
}

#[cfg(unix)]
#[test]
fn add_template_and_edit_opens_template_as_initial_editor_content() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create template dir");
    std::fs::write(template_dir.join("story.md"), "Template body").expect("write template");
    let editor = editor_script(dir.path(), "append.sh", "printf '\\nEdited' >> \"$1\"");

    pinto(dir.path())
        .args(["add", "Edited template", "--template", "story", "--edit"])
        .env("EDITOR", editor)
        .env_remove("VISUAL")
        .assert()
        .success();

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["body"], "Template body\nEdited");
}

#[cfg(unix)]
#[test]
fn add_edit_accepts_an_unchanged_empty_body() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Empty body", "--edit"])
        .env("VISUAL", "true")
        .env_remove("EDITOR")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-1"));

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["body"], "");
}

#[test]
fn add_edit_without_configured_editor_guides_user_and_does_not_create_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "No editor", "--edit"])
        .env_remove("VISUAL")
        .env_remove("EDITOR")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no editor configured"));

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(value, serde_json::json!([]));
}

#[test]
fn add_edit_reports_editor_launch_failure_and_does_not_create_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Broken editor", "--edit"])
        .env("VISUAL", "pinto-editor-that-does-not-exist")
        .env_remove("EDITOR")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("failed to launch editor"))
        .stderr(predicate::str::contains("pinto-editor-that-does-not-exist"));

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(value, serde_json::json!([]));
}

#[test]
fn add_rejects_body_with_edit() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["add", "Conflicting body", "--body", "text", "--edit"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--body"))
        .stderr(predicate::str::contains("--edit"));
}

#[test]
fn add_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["add", "No board here"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[cfg(unix)]
#[test]
fn add_edit_reports_non_zero_editor_exit_as_user_error_and_does_not_create_item() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["add", "Failed editor", "--edit"])
        .env("VISUAL", "false")
        .env_remove("EDITOR")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("failed to launch editor"))
        .stderr(predicate::str::contains("non-zero status"));

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(value, serde_json::json!([]));
}
