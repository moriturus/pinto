//! Sprint Retro record commands.

use super::common::*;

#[test]
fn retro_and_review_show_generated_sprint_context_separately_from_markdown() {
    for record in ["retro", "review"] {
        let dir = TempDir::new().expect("temp dir");
        pinto(dir.path()).arg("init").assert().success();
        pinto(dir.path())
            .args([
                "sprint",
                "new",
                "S-1",
                "Sprint One",
                "--goal",
                "Ship it",
                "--start",
                "2026-07-06",
                "--end",
                "2026-07-08",
            ])
            .assert()
            .success();
        pinto(dir.path())
            .args(["add", "Estimated task", "--points", "3"])
            .assert()
            .success();
        pinto(dir.path())
            .args(["sprint", "add", "S-1", "T-1"])
            .assert()
            .success();
        pinto(dir.path())
            .args([
                "sprint",
                "capacity",
                "S-1",
                "--daily-hours",
                "4",
                "--holidays",
                "0",
                "--deduction-factor",
                "1",
            ])
            .assert()
            .success();
        pinto(dir.path())
            .args(["sprint", record, "new", "S-1", "--body", "Authored notes"])
            .assert()
            .success();

        let shown = show_json(pinto(dir.path()).args(["sprint", record, "show", "S-1", "--json"]));
        assert_eq!(shown["body"], "Authored notes");
        assert_eq!(shown["context"]["sprint"]["id"], "S-1");
        assert_eq!(shown["context"]["sprint"]["goal"], "Ship it");
        assert_eq!(shown["context"]["sprint"]["state"], "planned");
        assert_eq!(
            shown["context"]["sprint"]["start"],
            "2026-07-06T00:00:00+00:00"
        );
        assert_eq!(
            shown["context"]["sprint"]["end"],
            "2026-07-08T00:00:00+00:00"
        );
        assert_eq!(shown["context"]["capacity"]["working_days"], 3);
        assert_eq!(shown["context"]["capacity"]["hours"], 12.0);
        assert_eq!(shown["context"]["velocity"], serde_json::Value::Null);
        assert!(shown["context"]["burndown"].is_object());
        assert_eq!(shown["context"]["cycle_time"], serde_json::Value::Null);
        assert_eq!(shown["context"]["spillover"], serde_json::Value::Null);

        pinto(dir.path())
            .args(["sprint", record, "show", "S-1"])
            .assert()
            .success()
            .stdout(predicate::str::contains("Sprint Context (generated)"))
            .stdout(predicate::str::contains("Goal: Ship it"))
            .stdout(predicate::str::contains("State: planned"))
            .stdout(predicate::str::contains("Velocity: unavailable"))
            .stdout(predicate::str::contains("Authored notes"));
        pinto(dir.path())
            .args(["sprint", record, "show", "S-1", "--plain"])
            .assert()
            .success()
            .stdout(predicate::eq("Authored notes\n"));
    }
}

#[test]
fn retro_plain_emits_nothing_for_an_empty_body() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();
    // A record created without a body has empty authored Markdown; `--plain` must emit exactly
    // that, so a body-only pipe stays empty rather than leaking the Sprint ID heading.
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "retro", "show", "S-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::eq(""));
}

#[test]
fn retro_plain_preserves_a_body_that_already_ends_with_a_newline() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint"])
        .assert()
        .success();

    // A body authored through the template or editor keeps its own trailing newline. Write such a
    // record directly so its stored body is exactly `One\nTwo\n`.
    let retro = dir.path().join(".pinto/retro/S-1.md");
    std::fs::create_dir_all(retro.parent().unwrap()).expect("retro directory");
    std::fs::write(
        &retro,
        "+++\nid = \"S-1\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"\n+++\n\nOne\nTwo\n\n",
    )
    .expect("write retro record");

    // The stored body keeps its trailing newline, and `--plain` must reproduce it byte for byte
    // rather than padding it with a spurious blank line.
    let shown = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]));
    assert_eq!(shown["body"], "One\nTwo\n");
    pinto(dir.path())
        .args(["sprint", "retro", "show", "S-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::eq("One\nTwo\n"));
}

fn create_sprints(dir: &Path) {
    pinto(dir)
        .args(["sprint", "new", "S-1", "Planned", "--goal", "Plan"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "new", "S-2", "Active", "--goal", "Build"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "start", "S-2"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "new", "S-3", "Closed", "--goal", "Ship"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "start", "S-3"])
        .assert()
        .success();
    pinto(dir)
        .args(["sprint", "close", "S-3"])
        .assert()
        .success();
}

#[test]
fn retro_can_be_created_for_each_sprint_state_and_is_listed_as_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    create_sprints(dir.path());

    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "Planned notes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created retro S-1"));
    pinto(dir.path())
        .args(["sprint", "retro", "S-2", "--body", "Active notes"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-3", "--body", "Closed notes"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["sprint", "retro", "list", "--json"]));
    let retros = value.as_array().expect("retro list --json is an array");
    assert_eq!(retros.len(), 3);
    assert_eq!(retros[0]["id"], "S-1");
    assert_eq!(retros[0]["sprint_id"], "S-1");
    assert_eq!(retros[0]["body"], "Planned notes");
    assert_eq!(retros[1]["id"], "S-2");
    assert_eq!(retros[2]["id"], "S-3");
    assert!(retros[0]["created"].is_string());
    assert!(retros[0]["updated"].is_string());

    let shown = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-2", "--json"]));
    assert_eq!(shown["id"], "S-2");
    assert_eq!(shown["sprint_id"], "S-2");
    assert_eq!(shown["body"], "Active notes");
    // `--plain` is body-only: exactly the authored Markdown plus a trailing newline, with no
    // Sprint ID heading that would corrupt a pipe, diff, or reuse of the record body.
    pinto(dir.path())
        .args(["sprint", "retro", "show", "S-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::eq("Planned notes\n"));

    let path = dir.path().join(".pinto/retro/S-1.md");
    let markdown = std::fs::read_to_string(path).expect("retro file exists");
    assert!(markdown.contains("id = \"S-1\""));
}

#[test]
fn retro_requires_an_existing_sprint_and_rejects_duplicates() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-9", "--body", "Missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sprint not found"));
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "First"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "Second"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("retro already exists"));

    pinto(dir.path())
        .args(["retro", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("external command `retro list`"));
    pinto(dir.path())
        .args(["sprint", "retro"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "sprint retro requires a Sprint ID",
        ));
}

#[test]
fn retro_template_and_editor_use_the_template_as_initial_content() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();
    let template_dir = dir.path().join(".pinto/templates/retro");
    std::fs::create_dir_all(&template_dir).expect("create retro template dir");
    std::fs::write(template_dir.join("review.md"), "## What went well")
        .expect("write retro template");

    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--template", "review"])
        .assert()
        .success();
    assert_eq!(
        show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]))["body"],
        "## What went well"
    );

    pinto(dir.path())
        .args(["sprint", "new", "S-2", "Sprint 2", "--goal", "Ship"])
        .assert()
        .success();
    #[cfg(unix)]
    {
        let editor = editor_script(
            dir.path(),
            "retro-editor.sh",
            "printf '\\n## Action item' >> \"$1\"",
        );
        pinto(dir.path())
            .env("EDITOR", &editor)
            .args([
                "sprint",
                "retro",
                "new",
                "S-2",
                "--template",
                "review",
                "--edit",
            ])
            .assert()
            .success();
        assert_eq!(
            show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-2", "--json"]))["body"],
            "## What went well\n## Action item"
        );

        pinto(dir.path())
            .env("EDITOR", &editor)
            .args(["sprint", "retro", "edit", "S-1"])
            .assert()
            .success();
        assert_eq!(
            show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]))["body"],
            "## What went well\n## Action item"
        );
    }
}

#[test]
fn retro_action_creates_a_normal_linked_pbi_and_reflects_its_status() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Retro Sprint", "--goal", "Ship"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "sprint",
            "new",
            "S-2",
            "Follow-up Sprint",
            "--goal",
            "Follow up",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Prerequisite"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "Follow up"])
        .assert()
        .success();

    pinto(dir.path())
        .args([
            "sprint",
            "retro",
            "action",
            "S-1",
            "Improve deployment checks",
            "--points",
            "3",
            "--label",
            "follow-up",
            "--assignee",
            "alice",
            "--sprint",
            "S-2",
            "--parent",
            "T-1",
            "--depends-on",
            "T-1",
            "--body",
            "Action details",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-2"));

    let item = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(item["title"], "Improve deployment checks");
    assert_eq!(item["points"], 3);
    assert_eq!(item["labels"], serde_json::json!(["follow-up"]));
    assert_eq!(item["assignee"], "alice");
    assert_eq!(item["sprint"], "S-2");
    assert_eq!(item["parent"], "T-1");
    assert_eq!(item["depends_on"], serde_json::json!(["T-1"]));
    assert_eq!(item["source"]["kind"], "retro");
    assert_eq!(item["source"]["sprint_id"], "S-1");
    let markdown =
        std::fs::read_to_string(dir.path().join(".pinto/tasks/T-2.md")).expect("action PBI file");
    assert!(markdown.contains("[source]") || markdown.contains("source ="));
    assert!(markdown.contains("kind = \"retro\""));
    assert!(markdown.contains("sprint_id = \"S-1\""));

    let shown = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]));
    assert_eq!(shown["actions"][0]["id"], "T-2");
    assert_eq!(shown["actions"][0]["status"], "todo");
    pinto(dir.path())
        .args(["sprint", "retro", "show", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Linked action PBIs"))
        .stdout(predicate::str::contains("T-2"))
        .stdout(predicate::str::contains("todo"));

    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["edit", "T-2", "--title", "Improved deployment checks"])
        .assert()
        .success();
    let updated = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]));
    assert_eq!(updated["actions"][0]["title"], "Improved deployment checks");
    assert_eq!(updated["actions"][0]["status"], "in-progress");

    pinto(dir.path()).args(["rm", "T-2"]).assert().success();
    let after_remove =
        show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]));
    assert_eq!(after_remove["actions"], serde_json::json!([]));
}

#[test]
fn retro_action_uses_the_item_template_and_links_the_retro() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Retro Sprint", "--goal", "Ship"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "## Follow-up"])
        .assert()
        .success();
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create item template dir");
    std::fs::write(template_dir.join("follow-up.md"), "## Details\n").expect("write item template");

    pinto(dir.path())
        .args([
            "sprint",
            "retro",
            "action",
            "S-1",
            "Document the release",
            "--template",
            "follow-up",
            "--body",
            "Add examples",
        ])
        .assert()
        .success();

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["body"], "## Details\n\nAdd examples");
    assert_eq!(item["source"]["kind"], "retro");
    assert_eq!(item["source"]["sprint_id"], "S-1");

    let retro = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-1", "--json"]));
    assert_eq!(
        retro["actions"],
        serde_json::json!([
            {"id": "T-1", "title": "Document the release", "status": "todo"}
        ])
    );
}

#[test]
fn retro_action_requires_the_source_record() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "retro", "action", "S-1", "Missing retro"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("retro not found"));
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"]))
            .as_array()
            .expect("list JSON array")
            .len(),
        0
    );
}

#[test]
fn retro_mutations_use_one_git_commit_boundary() {
    let dir = TempDir::new().expect("temp dir");
    pinto_isolated_git(dir.path())
        .arg("init")
        .assert()
        .success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");

    pinto_isolated_git(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["sprint", "retro", "new", "S-1", "--body", "First"])
        .assert()
        .success();
    assert_eq!(
        git_log_field(dir.path(), "%s").first().map(String::as_str),
        Some("pinto: add retro S-1")
    );

    pinto_isolated_git(dir.path())
        .args(["sprint", "retro", "edit", "S-1", "--body", "Second"])
        .assert()
        .success();
    assert_eq!(
        git_log_field(dir.path(), "%s").first().map(String::as_str),
        Some("pinto: update retro S-1")
    );
    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    assert!(
        String::from_utf8_lossy(&tree.stdout)
            .lines()
            .any(|path| path == ".pinto/retro/S-1.md")
    );
}

#[test]
fn retro_messages_follow_the_selected_locale() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();

    pinto(dir.path())
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "ja_JP.UTF-8")
        .args(["sprint", "retro", "new", "S-1", "--body", "本文"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Retro S-1 を作成しました"));
    pinto(dir.path())
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "ja_JP.UTF-8")
        .args(["sprint", "retro", "new", "S-1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Retro はすでに存在します"));
}
