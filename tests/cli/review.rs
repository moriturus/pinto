//! Sprint Review record commands.

use super::common::*;

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
fn review_can_be_created_for_each_parent_sprint_state_and_is_listed_as_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    create_sprints(dir.path());

    pinto(dir.path())
        .args(["sprint", "review", "new", "S-1", "--body", "Planned notes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created review S-1"));
    pinto(dir.path())
        .args(["sprint", "review", "S-2", "--body", "Active notes"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "review", "new", "S-3", "--body", "Closed notes"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["sprint", "review", "list", "--json"]));
    let reviews = value.as_array().expect("review list --json is an array");
    assert_eq!(reviews.len(), 3);
    assert_eq!(reviews[0]["id"], "S-1");
    assert_eq!(reviews[0]["body"], "Planned notes");
    assert_eq!(reviews[1]["id"], "S-2");
    assert_eq!(reviews[2]["id"], "S-3");
    assert!(reviews[0]["created"].is_string());
    assert!(reviews[0]["updated"].is_string());

    let shown = show_json(pinto(dir.path()).args(["sprint", "review", "show", "S-2", "--json"]));
    assert_eq!(shown["id"], "S-2");
    assert_eq!(shown["body"], "Active notes");
    pinto(dir.path())
        .args(["sprint", "review", "show", "S-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1"))
        .stdout(predicate::str::contains("Planned notes"));

    let path = dir.path().join(".pinto/review/S-1.md");
    let markdown = std::fs::read_to_string(path).expect("review file exists");
    assert!(markdown.contains("id = \"S-1\""));
}

#[test]
fn review_requires_an_existing_sprint_and_rejects_duplicates() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["sprint", "review", "new", "S-9", "--body", "Missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("sprint not found"));
    pinto(dir.path())
        .args(["sprint", "review", "new", "S-1", "--body", "First"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "review", "new", "S-1", "--body", "Second"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("review already exists"));

    pinto(dir.path())
        .args(["review", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("external command `review list`"));
    pinto(dir.path())
        .args(["sprint", "review"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "sprint review requires a Sprint ID",
        ));
}

#[test]
fn review_template_and_editor_use_the_template_as_initial_content() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();
    let template_dir = dir.path().join(".pinto/templates/review");
    std::fs::create_dir_all(&template_dir).expect("create review template dir");
    std::fs::write(template_dir.join("demo.md"), "## Delivered").expect("write review template");

    pinto(dir.path())
        .args(["sprint", "review", "new", "S-1", "--template", "demo"])
        .assert()
        .success();
    assert_eq!(
        show_json(pinto(dir.path()).args(["sprint", "review", "show", "S-1", "--json"]))["body"],
        "## Delivered"
    );

    pinto(dir.path())
        .args(["sprint", "new", "S-2", "Sprint 2", "--goal", "Ship"])
        .assert()
        .success();
    #[cfg(unix)]
    {
        let editor = editor_script(
            dir.path(),
            "review-editor.sh",
            "printf '\\n## Follow-up' >> \"$1\"",
        );
        pinto(dir.path())
            .env("EDITOR", &editor)
            .args([
                "sprint",
                "review",
                "new",
                "S-2",
                "--template",
                "demo",
                "--edit",
            ])
            .assert()
            .success();
        assert_eq!(
            show_json(pinto(dir.path()).args(["sprint", "review", "show", "S-2", "--json"]))["body"],
            "## Delivered\n## Follow-up"
        );

        pinto(dir.path())
            .env("EDITOR", &editor)
            .args(["sprint", "review", "edit", "S-1"])
            .assert()
            .success();
        assert_eq!(
            show_json(pinto(dir.path()).args(["sprint", "review", "show", "S-1", "--json"]))["body"],
            "## Delivered\n## Follow-up"
        );
    }
}

#[test]
fn review_action_uses_the_item_template_and_links_the_review() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Review Sprint", "--goal", "Ship"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "review", "new", "S-1", "--body", "## Follow-up"])
        .assert()
        .success();
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create item template dir");
    std::fs::write(template_dir.join("follow-up.md"), "## Details\n").expect("write item template");

    pinto(dir.path())
        .args([
            "sprint",
            "review",
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
    assert_eq!(item["source"]["kind"], "review");
    assert_eq!(item["source"]["sprint_id"], "S-1");

    let review = show_json(pinto(dir.path()).args(["sprint", "review", "show", "S-1", "--json"]));
    assert_eq!(
        review["actions"],
        serde_json::json!([
            {"id": "T-1", "title": "Document the release", "status": "todo"}
        ])
    );
}

#[test]
fn review_edit_and_mutations_use_one_git_commit_boundary() {
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
        .args(["sprint", "review", "new", "S-1", "--body", "First"])
        .assert()
        .success();
    assert_eq!(
        git_log_field(dir.path(), "%s").first().map(String::as_str),
        Some("pinto: add review S-1")
    );

    pinto_isolated_git(dir.path())
        .args(["sprint", "review", "edit", "S-1", "--body", "Second"])
        .assert()
        .success();
    assert_eq!(
        git_log_field(dir.path(), "%s").first().map(String::as_str),
        Some("pinto: update review S-1")
    );
    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    assert!(
        String::from_utf8_lossy(&tree.stdout)
            .lines()
            .any(|path| path == ".pinto/review/S-1.md")
    );
}

#[test]
fn review_messages_follow_the_selected_locale() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint", "--goal", "Ship"])
        .assert()
        .success();

    pinto(dir.path())
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "ja_JP.UTF-8")
        .args(["sprint", "review", "new", "S-1", "--body", "本文"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Review S-1 を作成しました"));
    pinto(dir.path())
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "ja_JP.UTF-8")
        .args(["sprint", "review", "new", "S-1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Review はすでに存在します"));
}
