//! Robustness against corrupt configuration, task files, and invalid IDs.

use super::common::*;

#[test]
fn invalid_config_values_are_cli_user_errors_with_actionable_paths() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let valid_config = r#"columns = ["todo", "in-progress", "review", "done"]
done_column = "done"

[project]
name = "demo"
key = "T"

[tui]
confirm_quit = true

[storage]
backend = "file"

[wip]
enabled = true

[display]
markdown = true
timezone = "local"
"#;

    let cases = [
        (
            "unknown field",
            valid_config.replace("timezone = \"local\"", "timezome = \"local\""),
            ["[display]", "timezome"],
        ),
        (
            "blank column",
            valid_config.replace(
                "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]",
                "columns = [\"todo\", \" \", \"review\", \"done\"]",
            ),
            ["[columns]", "blank"],
        ),
        (
            "duplicate column",
            valid_config
                .replace(
                    "columns = [\"todo\", \"in-progress\", \"review\", \"done\"]",
                    "columns = [\"todo\", \"todo\"]",
                )
                .replace("done_column = \"done\"", "done_column = \"todo\""),
            ["[columns]", "duplicate"],
        ),
        (
            "unknown WIP column",
            format!("{valid_config}\n[wip.limits]\nmissing = 1\n"),
            ["[wip.limits]", "missing"],
        ),
        (
            "blank project name",
            valid_config.replace("name = \"demo\"", "name = \"  \""),
            ["[project].name", "empty"],
        ),
    ];

    for (case, config, expected) in cases {
        std::fs::write(dir.path().join(".pinto/config.toml"), config).expect("write config");
        let output = pinto(dir.path()).arg("board").output().expect("run board");
        assert_eq!(output.status.code(), Some(1), "{case}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected[0]), "{case}: {stderr}");
        assert!(stderr.contains(expected[1]), "{case}: {stderr}");
        assert!(!stderr.contains("panicked"), "{case}: {stderr}");
    }
}

#[test]
fn invalid_item_id_cannot_escape_the_board_root() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let victim = dir.path().join("victim-1.md");
    std::fs::write(&victim, "must survive\n").expect("victim");

    let malicious_id = format!("{}-1", dir.path().join("victim").display());
    pinto(dir.path())
        .args(["rm", &malicious_id, "--force"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid item id"));
    assert!(
        victim.is_file(),
        "a board-external file must not be deleted"
    );
}

#[test]
fn invalid_item_ids_are_rejected_before_cli_side_effects() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Keep me"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint"])
        .assert()
        .success();

    let invalid = "../outside-1";
    let commands = vec![
        vec!["show", invalid],
        vec!["edit", invalid, "--title", "Changed"],
        vec!["rm", invalid, "--force"],
        vec!["dep", "add", invalid, "T-1"],
        vec!["dep", "rm", invalid, "T-1"],
        vec!["link", "add", invalid, "deadbeef"],
        vec!["link", "rm", invalid, "deadbeef"],
        vec!["sprint", "add", "S-1", invalid],
        vec!["sprint", "unassign", "S-1", invalid],
    ];
    for args in commands {
        let output = pinto(dir.path())
            .args(args)
            .assert()
            .failure()
            .code(1)
            .get_output()
            .clone();
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("invalid item id"),
            "invalid ID error missing for stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let plan = format!(r#"{{"commands":[["edit","{invalid}","--title","Changed"]]}}"#);
    pinto(dir.path())
        .args(["automate", "--plan", &plan, "--json"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("\"status\": \"invalid\""));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["title"], "Keep me");
    assert_eq!(item["depends_on"], serde_json::json!([]));
    assert_eq!(item["commits"], serde_json::json!([]));
    assert!(
        item["sprint"].is_null(),
        "invalid assignment must not persist"
    );
}

#[test]
fn corrupt_config_is_user_error_code_1_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // Corrupt the configuration as if it were damaged by hand; the parse error is user error 1.
    // `board` parses config to read column definitions, whereas `list` does not.
    std::fs::write(dir.path().join(".pinto/config.toml"), "columns = = broken")
        .expect("corrupt config");

    pinto(dir.path())
        .arg("board")
        .assert()
        .failure()
        .code(1)
        // Exit with a formatted, actionable message rather than panicking.
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn config_missing_required_section_is_user_error_code_1_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // Simulate a hand-edited configuration with the required [tui] section removed.
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
    )
    .expect("missing config section");

    pinto(dir.path())
        .arg("board")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn corrupt_task_file_is_user_error_code_1_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // Create a corrupt task file with its frontmatter delimiter missing.
    std::fs::write(
        dir.path().join(".pinto/tasks/T-1.md"),
        "no frontmatter here",
    )
    .expect("corrupt task");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn corrupt_frontmatter_is_user_error_code_1_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // Simulate a hand edit that corrupts the TOML in the frontmatter.
    std::fs::write(dir.path().join(".pinto/tasks/T-1.md"), "+++\nid = [\n+++\n")
        .expect("corrupt frontmatter");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-1.md"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn task_read_io_failure_remains_internal_error_code_2_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();
    std::fs::remove_file(dir.path().join(".pinto/tasks/T-1.md")).expect("remove task");
    std::fs::create_dir(dir.path().join(".pinto/tasks/T-1.md")).expect("replace with directory");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("T-1.md"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn filename_frontmatter_id_mismatch_is_user_error_before_cli_mutation() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();
    std::fs::rename(
        dir.path().join(".pinto/tasks/T-1.md"),
        dir.path().join(".pinto/tasks/T-2.md"),
    )
    .expect("rename corrupt task");

    for args in [vec!["list"], vec!["show", "T-1"]] {
        pinto(dir.path())
            .args(args)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("filename"))
            .stderr(predicate::str::contains("panicked").not());
    }

    pinto(dir.path())
        .args(["add", "Must not allocate"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("filename"));
    assert!(dir.path().join(".pinto/tasks/T-2.md").is_file());
    assert!(!dir.path().join(".pinto/tasks/T-3.md").exists());
}
