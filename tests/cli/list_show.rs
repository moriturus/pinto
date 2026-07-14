//! List and show commands, including JSON output.

use super::common::*;

#[test]
fn display_timezone_changes_human_output_but_keeps_json_in_utc() {
    use chrono::{DateTime, FixedOffset};

    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Timezone boundary"])
        .assert()
        .success();
    let before = json_stdout(pinto(dir.path()).args(["show", "T-1", "--json"]));

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("timezone = \"local\"", "timezone = \"+09:00\""),
    )
    .expect("configure timezone");

    let json = json_stdout(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(
        json, before,
        "changing display settings does not rewrite data"
    );
    let created = json[0]["created"].as_str().expect("created timestamp");
    assert!(created.ends_with("+00:00"), "JSON remains UTC: {created}");
    let expected = DateTime::parse_from_rfc3339(created)
        .expect("RFC3339")
        .with_timezone(&FixedOffset::east_opt(9 * 3_600).expect("offset"))
        .format("%Y-%m-%dT%H:%M:%S+09:00")
        .to_string();

    pinto(dir.path())
        .args(["show", "T-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains(expected));
}

#[test]
fn list_shows_items_in_rank_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("First"))
        .stdout(predicate::str::contains("T-2"))
        .stdout(predicate::str::contains("Second"))
        // 追加順（rank 昇順）で T-1 が T-2 より前に出る。
        .stdout(predicate::str::is_match("(?s)T-1.*T-2").unwrap());
}

#[test]
fn list_filters_by_label() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Backend", "--label", "backend"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Frontend", "--label", "frontend"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["list", "--label", "backend"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Backend"))
        .stdout(predicate::str::contains("Frontend").not());
}

#[test]
fn list_label_filter_supports_multiple_labels_with_or_and_and_matching() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "new", "S-2", "Sprint Two"])
        .assert()
        .success();

    for (title, labels, sprint) in [
        ("Backend only", &["backend"][..], Some("S-1")),
        ("Frontend only", &["frontend"][..], Some("S-1")),
        ("Both labels", &["backend", "frontend"][..], Some("S-1")),
        ("Backend other sprint", &["backend"][..], Some("S-2")),
        ("Backend progress", &["backend", "ops"][..], Some("S-1")),
        ("No labels", &[][..], None),
    ] {
        let mut args = vec!["add", title];
        for label in labels {
            args.extend(["--label", label]);
        }
        if let Some(sprint) = sprint {
            args.extend(["--sprint", sprint]);
        }
        pinto(dir.path()).args(args).assert().success();
    }
    pinto(dir.path())
        .args(["move", "T-5", "in-progress"])
        .assert()
        .success();

    let or =
        json_stdout(pinto(dir.path()).args(["list", "--label", "backend", "frontend", "--json"]));
    let or_titles: Vec<_> = or
        .as_array()
        .expect("OR result is an array")
        .iter()
        .map(|item| item["title"].as_str().expect("title"))
        .collect();
    assert_eq!(
        or_titles,
        [
            "Backend only",
            "Frontend only",
            "Both labels",
            "Backend other sprint",
            "Backend progress",
        ]
    );

    let repeated_or = json_stdout(pinto(dir.path()).args([
        "list", "--label", "backend", "--label", "frontend", "--json",
    ]));
    assert_eq!(repeated_or, or, "repeated --label uses the same OR filter");

    let and = json_stdout(pinto(dir.path()).args([
        "list",
        "--label",
        "backend",
        "frontend",
        "--all-labels",
        "--json",
    ]));
    assert_eq!(
        and.as_array()
            .expect("AND result is an array")
            .iter()
            .map(|item| item["title"].as_str().expect("title"))
            .collect::<Vec<_>>(),
        ["Both labels"]
    );

    let composed = json_stdout(pinto(dir.path()).args([
        "list",
        "--status",
        "in-progress",
        "--sprint",
        "S-1",
        "--label",
        "backend",
        "frontend",
        "--json",
    ]));
    assert_eq!(
        composed.as_array().expect("composed result is an array")[0]["title"],
        "Backend progress"
    );

    let composed_and = json_stdout(pinto(dir.path()).args([
        "list",
        "--status",
        "todo",
        "--sprint",
        "S-1",
        "--label",
        "backend",
        "frontend",
        "--all-labels",
        "--json",
    ]));
    assert_eq!(
        composed_and
            .as_array()
            .expect("composed AND result is an array")[0]["title"],
        "Both labels"
    );

    let all = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(
        all.as_array().expect("unfiltered result is an array").len(),
        6
    );
}

#[test]
fn list_search_filters_item_fields_and_sprint_goal() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Release", "--goal", "Ship parser"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Generic title",
            "--body",
            "acceptance parser",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Unrelated"])
        .assert()
        .success();

    let by_body = json_stdout(pinto(dir.path()).args(["list", "--search", "acceptance", "--json"]));
    assert_eq!(by_body.as_array().unwrap().len(), 1);
    assert_eq!(by_body[0]["title"], "Generic title");

    let by_goal =
        json_stdout(pinto(dir.path()).args(["list", "--search", "Ship parser", "--json"]));
    assert_eq!(by_goal.as_array().unwrap().len(), 1);
    assert_eq!(by_goal[0]["title"], "Generic title");
}

#[test]
fn list_regex_search_and_invalid_pattern_are_reported() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Parser 42"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Parser x"])
        .assert()
        .success();

    let matched = json_stdout(pinto(dir.path()).args([
        "list",
        "--search",
        r"Parser \d+",
        "--regex",
        "--json",
    ]));
    assert_eq!(matched.as_array().unwrap().len(), 1);
    assert_eq!(matched[0]["title"], "Parser 42");

    pinto(dir.path())
        .args(["list", "--search", "[", "--regex"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid search pattern"));
}

#[test]
fn list_filters_by_status_shows_empty_message() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Todo item"])
        .assert()
        .success();

    // 既定は todo 列。存在しない done で絞ると 0 件のメッセージ。
    pinto(dir.path())
        .args(["list", "--status", "done"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No backlog items"));
}

#[test]
fn list_status_filter_supports_single_multiple_and_unspecified_values() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Todo in sprint",
            "--label",
            "backend",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "In progress in sprint",
            "--label",
            "backend",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Wrong label",
            "--label",
            "frontend",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "No sprint", "--label", "backend"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();

    let single = json_stdout(pinto(dir.path()).args(["list", "--status", "in-progress", "--json"]));
    assert_eq!(single.as_array().unwrap().len(), 1);
    assert_eq!(single[0]["title"], "In progress in sprint");

    let multiple = json_stdout(pinto(dir.path()).args([
        "list",
        "--status",
        "todo",
        "--status",
        "in-progress",
        "--label",
        "backend",
        "--sprint",
        "S-1",
        "--json",
    ]));
    let titles: Vec<_> = multiple
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Todo in sprint", "In progress in sprint"]);

    let unspecified = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(unspecified.as_array().unwrap().len(), 4);
}

#[test]
fn list_status_filter_rejects_unknown_status() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["list", "--status", "nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown status"))
        .stderr(predicate::str::contains("nope"));
}

#[test]
fn list_and_board_status_filter_accepts_space_separated_values() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Todo"]).assert().success();
    pinto(dir.path())
        .args(["add", "In progress"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();

    let list =
        json_stdout(pinto(dir.path()).args(["list", "--status", "todo", "in-progress", "--json"]));
    assert_eq!(list.as_array().unwrap().len(), 2);

    let board =
        json_stdout(pinto(dir.path()).args(["board", "--status", "todo", "in-progress", "--json"]));
    assert_eq!(board["columns"].as_array().unwrap().len(), 2);

    pinto(dir.path())
        .args(["list", "--status", "todo", "in-progress", "--long"])
        .assert()
        .success()
        .stdout(predicate::str::contains("STATUS"));
    pinto(dir.path())
        .args(["board", "--status", "todo", "in-progress", "--long"])
        .assert()
        .success()
        .stdout(predicate::str::contains("STATUS"));
}

#[test]
fn list_empty_board_reports_no_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No backlog items"));
}

#[test]
fn list_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn list_long_shows_metadata_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add", "Task", "--points", "5", "--label", "backend", "--sprint", "S-1",
        ])
        .assert()
        .success();

    for flag in ["-l", "--long"] {
        pinto(dir.path())
            .args(["list", flag, "--label", "--sprint"])
            .assert()
            .success()
            .stdout(predicate::str::contains("T-1"))
            .stdout(predicate::str::contains("Task"))
            .stdout(predicate::str::contains("todo"))
            .stdout(predicate::str::contains('5'))
            .stdout(predicate::str::contains("backend"))
            .stdout(predicate::str::contains("S-1"))
            // ヘッダ行に列名が出る。
            .stdout(predicate::str::contains("STATUS"))
            .stdout(predicate::str::contains("CREATED"))
            .stdout(predicate::str::contains("UPDATED"));
    }
}

#[test]
fn list_long_uses_scrum_columns_in_a_stable_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Stable columns", "--points", "5"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["edit", "T-1", "--assignee", "alice"])
        .assert()
        .success();

    let output = pinto(dir.path())
        .args(["ls", "-l"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header: Vec<_> = stdout
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();

    assert_eq!(
        header,
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "CREATED", "UPDATED"
        ]
    );
    assert!(stdout.contains("alice"), "assignee is shown: {stdout}");
}

#[test]
fn list_long_allows_bare_label_and_sprint_options_to_select_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Backend", "--label", "backend", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Frontend", "--label", "frontend"])
        .assert()
        .success();

    let output = pinto(dir.path())
        .args(["list", "--long", "--label", "--sprint"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header: Vec<_> = stdout
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();

    assert_eq!(
        header,
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "LABELS", "SPRINT", "CREATED", "UPDATED"
        ]
    );
    assert!(
        stdout.contains("Backend"),
        "bare options do not filter: {stdout}"
    );
    assert!(
        stdout.contains("Frontend"),
        "bare options do not filter: {stdout}"
    );
}

#[test]
fn list_filter_value_keeps_filtering_with_or_without_long() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Backend", "--label", "backend", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Frontend", "--label", "frontend"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["list", "--label", "backend"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Backend"))
        .stdout(predicate::str::contains("Frontend").not())
        .stdout(predicate::str::contains("STATUS").not());

    let output = pinto(dir.path())
        .args(["list", "--long", "--label", "backend"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header: Vec<_> = stdout
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    assert_eq!(
        header,
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "LABELS", "CREATED", "UPDATED"
        ]
    );
    assert!(
        stdout.contains("Backend"),
        "long filter keeps match: {stdout}"
    );
    assert!(
        !stdout.contains("Frontend"),
        "long filter excludes non-match: {stdout}"
    );

    let output = pinto(dir.path())
        .args(["list", "--long", "--sprint", "S-1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header: Vec<_> = stdout
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .collect();
    assert_eq!(
        header,
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "SPRINT", "CREATED", "UPDATED"
        ]
    );
    assert!(
        stdout.contains("Backend"),
        "long Sprint filter keeps match: {stdout}"
    );
    assert!(
        !stdout.contains("Frontend"),
        "long Sprint filter excludes non-match: {stdout}"
    );
}

#[test]
fn list_bare_filter_options_require_long_mode() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    for flag in ["--label", "--sprint"] {
        pinto(dir.path())
            .args(["list", flag])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("requires a value"));
    }
}

#[test]
fn list_all_labels_requires_a_label_value() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["list", "--long", "--label", "--all-labels"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--all-labels requires"));
}

#[test]
fn list_without_long_flag_keeps_concise_output() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Concise task", "--points", "3"])
        .assert()
        .success();

    // 既定表示（フラグ無し）ではヘッダ行や CREATED/UPDATED 列は出ない（従来どおり）。
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("STATUS").not())
        .stdout(predicate::str::contains("CREATED").not());
}

#[test]
fn list_long_combines_with_filters() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Backend task", "--label", "backend"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Frontend task", "--label", "frontend"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["list", "-l", "--label", "backend"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Backend task"))
        .stdout(predicate::str::contains("Frontend task").not());
}

#[test]
fn list_long_with_json_ignores_long_and_keeps_json_schema() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Task", "--points", "2"])
        .assert()
        .success();

    // `--json` は元々全メタ情報を含むため、`-l` を併用しても JSON スキーマは変わらない
    // （`-l` は無視される）。
    let with_long = json_stdout(pinto(dir.path()).args(["list", "--json", "-l"]));
    let without_long = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(with_long, without_long);
}

#[test]
fn list_long_empty_board_reports_no_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["list", "-l"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No backlog items"));
}

#[test]
fn show_displays_all_fields_and_body() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Detailed task",
            "--points",
            "5",
            "--label",
            "backend",
            "--sprint",
            "S-1",
            "--body",
            "Acceptance criteria here",
        ])
        .assert()
        .success();

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("Detailed task"))
        .stdout(predicate::str::contains("todo"))
        .stdout(predicate::str::contains("5"))
        .stdout(predicate::str::contains("backend"))
        .stdout(predicate::str::contains("S-1"))
        .stdout(predicate::str::contains("Acceptance criteria here"));
}

#[test]
fn show_renders_markdown_body_by_default_and_plain_opts_out() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args([
            "add",
            "Rendered task",
            "--body",
            "# Heading\n\n**bold** text",
        ])
        .assert()
        .success();

    // 既定では Markdown レンダリング: 見出し記法 (`# `) は消え、本文は残る。
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Heading"))
        .stdout(predicate::str::contains("# Heading").not())
        .stdout(predicate::str::contains("**bold**").not());

    // --plain は生の Markdown をそのまま表示する（オプトアウト手段）。
    pinto(dir.path())
        .args(["show", "T-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Heading"))
        .stdout(predicate::str::contains("**bold**"));
}

#[test]
fn show_presents_rank_as_human_readable_ordinal() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    // 2 番目に追加した PBI は todo 列の #2。
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#2"));

    // --json は rank_ordinal を数値で持つ（内部の rank 文字列も維持）。
    let value = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(value["rank_ordinal"], 2);
    assert!(value["rank"].is_string(), "raw rank string preserved");
}

#[test]
fn show_missing_id_exits_with_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["show", "T-99"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-99"));
}

#[test]
fn show_invalid_id_exits_with_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["show", "not-an-id!"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn show_accepts_multiple_ids_and_preserves_input_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    let output = pinto(dir.path())
        .args(["show", "T-2", "T-1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("show output is UTF-8");
    let second = stdout
        .find("T-2  Second")
        .expect("first requested item shown");
    let separator = stdout[second..]
        .find("\n---\n")
        .map(|offset| second + offset)
        .expect("multiple details have a visible boundary");
    let first = stdout[separator..]
        .find("T-1  First")
        .map(|offset| separator + offset)
        .expect("second requested item shown");
    assert!(second < first, "details follow input order: {stdout}");
}

#[test]
fn show_json_always_returns_an_array_in_input_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    let single = json_stdout(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(single[0]["id"], "T-1");

    let multiple = json_stdout(pinto(dir.path()).args(["show", "T-2", "T-1", "--json"]));
    let items = multiple.as_array().expect("show --json is an array");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["id"], "T-2");
    assert_eq!(items[0]["title"], "Second");
    assert_eq!(items[1]["id"], "T-1");
    assert_eq!(items[1]["title"], "First");
}

#[test]
fn show_plain_multiple_ids_keeps_bodies_and_boundaries() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "First", "--body", "# First body"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second", "--body", "**Second body**"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["show", "T-1", "T-2", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\n---\n"))
        .stdout(predicate::str::contains("# First body"))
        .stdout(predicate::str::contains("**Second body**"));
}

#[test]
fn show_multiple_ids_reports_errors_without_partial_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Existing"])
        .assert()
        .success();

    let output = pinto(dir.path())
        .args(["show", "T-1", "T-404", "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .clone();
    assert!(output.stdout.is_empty(), "no partial JSON is emitted");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("T-404"),
        "the invalid item is named in the error"
    );
}

#[test]
fn show_help_describes_multiple_ids() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["show", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[ID]..."))
        .stdout(predicate::str::contains("one or more PBIs"));
}

#[test]
fn show_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn list_with_one_corrupt_file_among_valid_fails_whole_without_partial_output() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Alpha"]).assert().success(); // T-1
    pinto(dir.path()).args(["add", "Bravo"]).assert().success(); // T-2

    // 正常な T-1 を残し、T-2 だけ壊す。1 件でも壊れると list 全体が失敗し、
    // 部分結果（Alpha）は一切出力しない（並列パースのエラー伝播を結合レベルで固定）。
    std::fs::write(
        dir.path().join(".pinto/tasks/T-2.md"),
        "no frontmatter here",
    )
    .expect("corrupt one task");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("Alpha").not())
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn list_json_outputs_stable_item_schema() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add", "First", "--points", "3", "--label", "backend", "--sprint", "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let items = value.as_array().expect("list --json is an array");
    assert_eq!(items.len(), 2, "both items present");

    // rank 昇順（追加順）。
    let first = &items[0];
    assert_eq!(first["id"], "T-1");
    assert_eq!(first["title"], "First");
    assert_eq!(first["status"], "todo");
    assert_eq!(first["points"], 3);
    assert_eq!(first["labels"], serde_json::json!(["backend"]));
    assert_eq!(first["sprint"], "S-1");
    // 任意フィールドはキーを常に持ち、未設定は null（安定スキーマ）。
    assert!(first.get("assignee").is_some(), "assignee key present");
    assert_eq!(first["assignee"], serde_json::Value::Null);
    assert_eq!(first["parent"], serde_json::Value::Null);
    assert_eq!(first["depends_on"], serde_json::json!([]));
    assert!(first["rank"].is_string(), "rank is a string");
    assert!(first["created"].is_string(), "created is RFC3339 string");
    assert!(first["updated"].is_string(), "updated is RFC3339 string");

    assert_eq!(items[1]["id"], "T-2");
    assert_eq!(items[1]["points"], serde_json::Value::Null);
}

#[test]
fn child_point_aggregation_is_opt_in_and_excludes_done_descendants() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Parent", "--points", "99"])
        .assert()
        .success(); // T-1
    pinto(dir.path())
        .args(["add", "Child", "--points", "3", "--parent", "T-1"])
        .assert()
        .success(); // T-2
    pinto(dir.path())
        .args(["add", "Grandchild", "--points", "4", "--parent", "T-2"])
        .assert()
        .success(); // T-3
    pinto(dir.path())
        .args([
            "add",
            "Completed child",
            "--points",
            "100",
            "--parent",
            "T-1",
        ])
        .assert()
        .success(); // T-4
    pinto(dir.path())
        .args(["move", "T-4", "done"])
        .assert()
        .success();

    let default_list = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(
        default_list[0]["points"], 99,
        "default remains stored points"
    );
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["points"],
        99
    );
    let default_board = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    assert_eq!(default_board["columns"][0]["items"][0]["points"], 99);

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("read config");
    assert!(config.contains("aggregate_children = false"));
    std::fs::write(
        &config_path,
        config.replace("aggregate_children = false", "aggregate_children = true"),
    )
    .expect("enable point aggregation");

    let enabled_list = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(
        enabled_list[0]["points"], 4,
        "nested active leaf is counted once"
    );
    assert_eq!(
        show_json(pinto(dir.path()).args(["show", "T-1", "--json"]))["points"],
        4
    );
    let enabled_board = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    assert_eq!(enabled_board["columns"][0]["items"][0]["points"], 4);
    assert_eq!(
        enabled_list
            .as_array()
            .expect("list array")
            .iter()
            .find(|item| item["id"] == "T-4")
            .expect("completed child")
            .get("points"),
        Some(&serde_json::json!(100)),
        "completed item's own displayed estimate is preserved"
    );
}

#[test]
fn list_json_empty_board_is_empty_array() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let value = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(value, serde_json::json!([]), "empty list is []");
}

#[test]
fn show_json_reports_item_with_links() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Epic"]).assert().success();
    pinto(dir.path()).args(["add", "Story"]).assert().success();
    // Story を Epic の子にし、Epic は Story に依存させる（双方向リンクを作る）。
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dep", "add", "T-1", "T-2"])
        .assert()
        .success();

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["id"], "T-1");
    assert_eq!(value["title"], "Epic");
    assert_eq!(value["depends_on"], serde_json::json!(["T-2"]));
    assert_eq!(value["children"], serde_json::json!(["T-2"]));
    assert_eq!(value["dependents"], serde_json::json!([]));
    assert!(value["body"].is_string(), "body key present");
}

#[test]
fn show_rank_is_sibling_local_and_names_the_parent_for_children() {
    // Rank is sibling-local under hierarchical ordering: a child is numbered
    // among its parent's children and names the parent; a top-level PBI is
    // numbered among the roots.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "PARENT"]).assert().success(); // T-1
    pinto(dir.path())
        .args(["add", "CHILD", "--parent", "T-1"])
        .assert()
        .success(); // T-2
    pinto(dir.path()).args(["add", "TOP"]).assert().success(); // T-3

    pinto(dir.path())
        .args(["show", "T-2", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("#1 under T-1"));
    pinto(dir.path())
        .args(["show", "T-3", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rank:").and(predicate::str::contains("#2 (")));
}

#[test]
fn status_filter_cuts_the_tree_promoting_orphaned_children_to_roots() {
    // When a parent is filtered out (here: moved to done, list scoped to todo),
    // its still-visible children are promoted to top level rather than dropped
    // or nested under an absent parent.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "PARENT"]).assert().success(); // T-1
    pinto(dir.path())
        .args(["add", "CHILD", "--parent", "T-1"])
        .assert()
        .success(); // T-2
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["list", "--status", "todo", "--json"]));
    let ids: Vec<&str> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["T-2"],
        "the orphaned child survives as a top-level row"
    );
}

#[test]
fn list_roots_only_filters_children_using_saved_parent_and_composes_filters() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Parent", "--label", "scope", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Child match",
            "--label",
            "scope",
            "--sprint",
            "S-1",
            "--parent",
            "T-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Root match", "--label", "scope", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();

    let roots = json_stdout(pinto(dir.path()).args(["list", "--roots-only", "--json"]));
    let root_ids: Vec<_> = roots
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(root_ids, ["T-1", "T-3"]);
    assert!(
        roots
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["title"] != "Child match"),
        "child is excluded even though its parent is in another status"
    );

    let filtered = json_stdout(pinto(dir.path()).args([
        "list",
        "--roots-only",
        "--status",
        "todo",
        "--sprint",
        "S-1",
        "--label",
        "scope",
        "--search",
        "match",
        "--json",
    ]));
    let filtered_ids: Vec<_> = filtered
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        filtered_ids,
        ["T-3"],
        "all compatible filters compose with roots-only"
    );

    pinto(dir.path())
        .args(["list", "--roots-only", "--long"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parent"))
        .stdout(predicate::str::contains("Root match"))
        .stdout(predicate::str::contains("Child match").not());
}
