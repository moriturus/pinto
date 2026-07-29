//! Sprint Retro record commands.

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
    assert_eq!(retros[0]["body"], "Planned notes");
    assert_eq!(retros[1]["id"], "S-2");
    assert_eq!(retros[2]["id"], "S-3");
    assert!(retros[0]["created"].is_string());
    assert!(retros[0]["updated"].is_string());

    let shown = show_json(pinto(dir.path()).args(["sprint", "retro", "show", "S-2", "--json"]));
    assert_eq!(shown["id"], "S-2");
    assert_eq!(shown["body"], "Active notes");
    pinto(dir.path())
        .args(["sprint", "retro", "show", "S-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S-1"))
        .stdout(predicate::str::contains("Planned notes"));

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
