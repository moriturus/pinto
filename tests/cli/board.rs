//! Board rendering, filtering, sorting, and WIP output.

use super::common::*;

#[test]
fn board_search_filters_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Keep this"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Drop this"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["board", "--search", "Keep", "--json"]));
    assert_eq!(value["columns"][0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(value["columns"][0]["items"][0]["title"], "Keep this");
}

#[test]
fn board_groups_items_under_columns_in_order() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Alpha"]).assert().success();
    pinto(dir.path()).args(["add", "Beta"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        // 列見出しと件数（T-2 を移動したので todo は 1 件）。
        .stdout(predicate::str::contains("todo (1)"))
        .stdout(predicate::str::contains("in-progress (1)"))
        .stdout(predicate::str::contains("Alpha"))
        .stdout(predicate::str::contains("Beta"))
        // 列は config 順（todo が in-progress より前）。
        .stdout(predicate::str::is_match("(?s)todo.*in-progress").unwrap())
        // 空の列も見出しを出す。
        .stdout(predicate::str::contains("done (0)"))
        .stdout(predicate::str::contains("(empty)"));
}

#[test]
fn board_long_uses_the_list_long_columns_and_details() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Alpha", "--points", "5"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["edit", "T-1", "--assignee", "alice"])
        .assert()
        .success();

    let output = pinto(dir.path())
        .args(["board", "--long"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header = stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some("ID"))
        .expect("long board header");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "CREATED", "UPDATED"
        ]
    );
    assert!(stdout.contains("Alpha"), "title is shown: {stdout}");
    assert!(stdout.contains("alice"), "assignee is shown: {stdout}");
}

#[test]
fn board_long_allows_bare_label_and_sprint_options_without_filtering() {
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
        .args(["board", "--long", "--label", "--sprint"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header = stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some("ID"))
        .expect("long board header");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "LABELS", "SPRINT", "CREATED", "UPDATED"
        ]
    );
    assert!(
        stdout.contains("Backend"),
        "label/sprint options do not filter: {stdout}"
    );
    assert!(
        stdout.contains("Frontend"),
        "label/sprint options do not filter: {stdout}"
    );
}

#[test]
fn board_filter_values_work_with_or_without_long() {
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
        .args(["board", "--label", "backend"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Backend"))
        .stdout(predicate::str::contains("Frontend").not());

    let output = pinto(dir.path())
        .args(["board", "--long", "--label", "backend"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header = stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some("ID"))
        .expect("long board label header");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        [
            "ID", "TITLE", "STATUS", "POINTS", "ASSIGNEE", "LABELS", "CREATED", "UPDATED"
        ]
    );
    assert!(
        stdout.contains("Backend"),
        "long label filter keeps match: {stdout}"
    );
    assert!(
        !stdout.contains("Frontend"),
        "long label filter excludes non-match: {stdout}"
    );

    let output = pinto(dir.path())
        .args(["board", "--long", "--sprint", "S-1"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8");
    let header = stdout
        .lines()
        .find(|line| line.split_whitespace().next() == Some("ID"))
        .expect("long board Sprint header");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
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

    for flag in ["--label", "--sprint"] {
        pinto(dir.path())
            .args(["board", flag])
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("requires a value"));
    }
}

#[test]
fn board_status_filter_shows_only_requested_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Alpha"]).assert().success();
    pinto(dir.path()).args(["add", "Beta"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();

    // --status を複数指定 → その列のみ（review / done は現れない）。
    pinto(dir.path())
        .args(["board", "--status", "todo", "--status", "in-progress"])
        .assert()
        .success()
        .stdout(predicate::str::contains("todo (1)"))
        .stdout(predicate::str::contains("in-progress (1)"))
        .stdout(predicate::str::contains("done").not())
        .stdout(predicate::str::contains("review").not());
}

#[test]
fn board_status_filter_unknown_column_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["board", "--status", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nope"));
}

#[test]
fn board_json_status_filter_applies() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Alpha"]).assert().success();

    let value = json_stdout(pinto(dir.path()).args(["board", "--status", "todo", "--json"]));
    let columns = value["columns"].as_array().expect("columns array");
    assert_eq!(columns.len(), 1, "only the requested column: {value}");
    assert_eq!(columns[0]["status"], "todo");
}

#[test]
fn board_roots_only_filters_children_using_saved_parent_and_composes_output_options() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Parent"]).assert().success();
    pinto(dir.path())
        .args(["add", "Child", "--parent", "T-1"])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Root"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["board", "--roots-only", "--json"]));
    let columns = value["columns"].as_array().unwrap();
    let todo = columns
        .iter()
        .find(|column| column["status"] == "todo")
        .unwrap();
    let in_progress = columns
        .iter()
        .find(|column| column["status"] == "in-progress")
        .unwrap();
    assert_eq!(todo["items"].as_array().unwrap().len(), 1);
    assert_eq!(todo["items"][0]["title"], "Root");
    assert_eq!(in_progress["items"][0]["title"], "Parent");
    assert!(
        columns.iter().all(|column| column["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["title"] != "Child")),
        "child is excluded even when its parent is in another board column"
    );

    pinto(dir.path())
        .args([
            "board",
            "--roots-only",
            "--long",
            "--status",
            "todo",
            "--sort",
            "rank",
            "--reverse",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Root"))
        .stdout(predicate::str::contains("Child").not());
}

#[test]
fn board_multiple_statuses_combine_with_sprint_and_label_in_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Todo match", "--label", "backend", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Progress match",
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
            "Wrong status",
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
            "Wrong scope",
            "--label",
            "frontend",
            "--sprint",
            "S-1",
        ])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-3", "done"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args([
        "board",
        "--status",
        "todo",
        "--status",
        "in-progress",
        "--sprint",
        "S-1",
        "--label",
        "backend",
        "--json",
    ]));
    let columns = value["columns"].as_array().unwrap();
    let statuses: Vec<_> = columns
        .iter()
        .map(|column| column["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, ["todo", "in-progress"]);
    assert_eq!(columns[0]["items"].as_array().unwrap().len(), 1);
    assert_eq!(columns[0]["items"][0]["title"], "Todo match");
    assert_eq!(columns[1]["items"].as_array().unwrap().len(), 1);
    assert_eq!(columns[1]["items"][0]["title"], "Progress match");
}

#[test]
fn board_label_filter_supports_multiple_labels_with_or_and_and_matching() {
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
        json_stdout(pinto(dir.path()).args(["board", "--label", "backend", "frontend", "--json"]));
    let or_titles: Vec<_> = or["columns"]
        .as_array()
        .expect("board columns")
        .iter()
        .flat_map(|column| column["items"].as_array().expect("column items"))
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

    let and = json_stdout(pinto(dir.path()).args([
        "board",
        "--status",
        "todo",
        "in-progress",
        "--sprint",
        "S-1",
        "--label",
        "backend",
        "frontend",
        "--all-labels",
        "--json",
    ]));
    let and_titles: Vec<_> = and["columns"]
        .as_array()
        .expect("board columns")
        .iter()
        .flat_map(|column| column["items"].as_array().expect("column items"))
        .map(|item| item["title"].as_str().expect("title"))
        .collect();
    assert_eq!(and_titles, ["Both labels"]);

    let all = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    let all_count: usize = all["columns"]
        .as_array()
        .expect("board columns")
        .iter()
        .map(|column| column["items"].as_array().expect("column items").len())
        .sum();
    assert_eq!(all_count, 6);
}

#[test]
fn board_sort_created_reverse_orders_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    // 追加順 = created 昇順（A, B, C）。
    for t in ["A", "B", "C"] {
        pinto(dir.path()).args(["add", t]).assert().success();
    }

    // --sort created --reverse → 新しい順（C, B, A）。
    let value =
        json_stdout(pinto(dir.path()).args(["board", "--sort", "created", "--reverse", "--json"]));
    let todo = value["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "todo")
        .expect("todo column");
    let titles: Vec<_> = todo["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|it| it["title"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(titles, ["C", "B", "A"], "created desc via --reverse");

    pinto(dir.path())
        .args(["board", "--sort", "done", "--json"])
        .assert()
        .success();
}

#[test]
fn board_sort_rejects_unknown_key() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    // clap の value_enum 検証で弾かれる（解釈エラー → コード 1）。
    pinto(dir.path())
        .args(["board", "--sort", "bogus"])
        .assert()
        .code(1);
}

#[test]
fn board_renders_parent_child_as_tree() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Epic"]).assert().success();
    pinto(dir.path()).args(["add", "Story"]).assert().success();
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success();

    // 親子は同じ列内でツリー表示（子が罫線付きでネスト）。
    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1  Epic"))
        .stdout(predicate::str::contains("└─ T-2  Story"));
}

#[test]
fn board_renders_dependencies_as_flat_markers_not_tree() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Base"]).assert().success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();
    // T-2 が T-1 に依存。依存はツリーのネストにはせず、フラット＋マーカー行で表す
    // Kanban uses the same semantics: the tree represents only parent-child links.
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1  Base"))
        .stdout(predicate::str::contains("T-2  Dependent"))
        .stdout(predicate::str::contains("└─").not());
}

#[test]
fn board_shows_dependency_markers_with_legend() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Base"]).assert().success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();
    // T-2 が T-1 に依存 → 依存先/依存元マーカーと凡例が出る。
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("board")
        .env("LC_ALL", "en_US.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "⊸ depends on  ⊷ depended on by  ⊸! unresolved dependency",
        ))
        .stdout(predicate::str::contains("⊷ T-2"))
        .stdout(predicate::str::contains("⊸! T-1"));
}

#[test]
fn board_marks_blocked_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Base"]).assert().success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    // T-1 が未完了のうちは T-2 はブロック中（⊸!）。
    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stdout(predicate::str::contains("⊸! T-1"))
        .stdout(predicate::str::contains("⊸ T-1").not());

    // T-1 を完了させるとブロックが解消され、`⊸!` は付かなくなる。
    pinto(dir.path())
        .args(["move", "T-1", "done"])
        .assert()
        .success();
    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stdout(predicate::str::contains("⊸! T-1").not())
        .stdout(predicate::str::contains("⊸ T-1"));
}

#[test]
fn board_json_keeps_flat_items_with_relationship_fields() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Epic"]).assert().success();
    pinto(dir.path()).args(["add", "Story"]).assert().success();
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success();

    // --json は従来どおりフラット＋関係情報（parent）を保つ。
    let value = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    let todo = value["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "todo")
        .expect("todo column");
    let story = todo["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|it| it["id"] == "T-2")
        .expect("T-2 present");
    assert_eq!(story["parent"], "T-1", "relationship preserved in JSON");
}

#[test]
fn board_empty_board_shows_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stdout(predicate::str::contains("todo (0)"))
        .stdout(predicate::str::contains("(empty)"));
}

#[test]
fn board_warns_about_items_in_undefined_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Kept"]).assert().success();
    pinto(dir.path())
        .args(["add", "Stranded"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "review"])
        .assert()
        .success();

    // ユーザーが config を手編集し `review` 列を削除 → T-2 は未定義の列を指す。
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        "columns = [\"todo\", \"in-progress\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"test\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
    )
    .expect("rewrite config");

    pinto(dir.path())
        .arg("board")
        .assert()
        .success() // 表示自体は成功（終了コード 0）。
        // 取り残された PBI は握り潰さず、専用セクションで現在状態を併記。
        .stdout(predicate::str::contains("(!) undefined columns (1)"))
        .stdout(predicate::str::contains("T-2  Stranded  [review]"))
        // 直し方を添えた警告を stderr に出す。
        .stderr(predicate::str::contains("not defined in config.toml"))
        .stderr(predicate::str::contains("T-2"));
}

#[test]
fn board_warns_when_column_exceeds_wip_limit() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);
    pinto(dir.path())
        .args(["move", "T-1", "in-progress", "--no-wip-check"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress", "--no-wip-check"])
        .assert()
        .success();

    // board 表示自体は成功（コード 0）だが、超過列を stderr で警告。
    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "WIP limit exceeded in 'in-progress'",
        ));

    // --no-wip-check で警告を抑止できる。
    pinto(dir.path())
        .args(["board", "--no-wip-check"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn board_json_output_suppresses_wip_warning() {
    // 機械可読出力（--json）は stdout の JSON のみに保ち、WIP 警告を混ぜない。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path()).args(["add", "B"]).assert().success();
    set_wip_limit(dir.path(), "in-progress", 1, true);
    pinto(dir.path())
        .args(["move", "T-1", "in-progress", "--no-wip-check"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress", "--no-wip-check"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["board", "--json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn board_without_wip_config_has_no_warning() {
    // `init` 既定の `[wip]`（有効・制限なし）では、上限が無いため警告を出さない。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success();
    pinto(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    pinto(dir.path())
        .arg("board")
        .assert()
        .success()
        .stderr(predicate::str::contains("WIP limit").not());
}

#[test]
fn board_without_init_errors_and_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("board")
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn board_scoped_to_sprint_shows_only_assigned_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Sprint One"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "In sprint"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Out of sprint"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["board", "--sprint", "S-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("In sprint"))
        .stdout(predicate::str::contains("Out of sprint").not());
}

#[test]
fn board_json_groups_columns_and_orphaned() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Todo one"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Doing one"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "in-progress"])
        .assert()
        .success();

    let value = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    let columns = value["columns"].as_array().expect("columns array");
    assert_eq!(columns[0]["status"], "todo");
    assert_eq!(columns[0]["items"][0]["id"], "T-1");
    assert_eq!(columns[1]["status"], "in-progress");
    assert_eq!(columns[1]["items"][0]["id"], "T-2");
    assert_eq!(value["orphaned"], serde_json::json!([]));
}

#[test]
fn board_done_column_shows_most_recent_completion_first() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "First"]).assert().success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();
    pinto(dir.path()).args(["add", "Third"]).assert().success();
    // 完了順に done へ（T-1 が最も古く、T-3 が最も新しい完了）。
    for id in ["T-1", "T-2", "T-3"] {
        pinto(dir.path())
            .args(["move", id, "done"])
            .assert()
            .success();
    }

    let value = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    let done = value["columns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "done")
        .expect("done column");
    let ids: Vec<_> = done["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|it| it["id"].as_str().unwrap().to_string())
        .collect();
    // 最新完了が先頭（降順）: T-3, T-2, T-1。
    assert_eq!(
        ids,
        ["T-3", "T-2", "T-1"],
        "done ordered by completion desc"
    );
}

#[test]
fn board_json_reports_orphaned_items() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Kept"]).assert().success();
    pinto(dir.path())
        .args(["add", "Stranded"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-2", "review"])
        .assert()
        .success();

    // ユーザーが config を手編集し `review` 列を削除 → T-2 は未定義の列を指す。
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        "columns = [\"todo\", \"in-progress\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"test\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"file\"\n\n[wip]\nenabled = true\n",
    )
    .expect("rewrite config");

    let value = json_stdout(pinto(dir.path()).args(["board", "--json"]));
    let orphaned = value["orphaned"].as_array().expect("orphaned array");
    assert_eq!(orphaned.len(), 1, "stranded item is orphaned");
    assert_eq!(orphaned[0]["id"], "T-2");
    assert_eq!(orphaned[0]["status"], "review");
}

#[test]
fn board_truncates_long_titles_to_default_width_when_not_a_tty() {
    // 非 TTY（パイプ）では既定幅 80 桁にフォールバックし、長文を省略する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", &"x".repeat(200)])
        .assert()
        .success();

    let out = pinto(dir.path()).arg("board").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    assert!(
        stdout.contains('…'),
        "long title should be truncated: {stdout}"
    );
    let widest = stdout.lines().map(display_cols).max().unwrap();
    assert!(
        widest <= 80,
        "line exceeds default width ({widest}): {stdout}"
    );
}

#[test]
fn board_truncates_fullwidth_titles_by_display_width() {
    // 全角のみのタイトルも表示幅ベースで既定幅に収まるよう切り詰める。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", &"あ".repeat(80)])
        .assert()
        .success();

    let out = pinto(dir.path()).arg("board").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    assert!(stdout.contains('…'), "fullwidth title truncated: {stdout}");
    let widest = stdout.lines().map(display_cols).max().unwrap();
    assert!(widest <= 80, "fullwidth line too wide ({widest}): {stdout}");
}

#[test]
fn board_no_truncate_shows_full_long_title() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let long = "y".repeat(200);
    pinto(dir.path()).args(["add", &long]).assert().success();

    // 既定は省略、--no-truncate は全文表示。
    let out = pinto(dir.path()).arg("board").assert().success();
    let default_stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    assert!(default_stdout.contains('…'), "default truncates");
    assert!(!default_stdout.contains(&long), "default omits full title");

    let out = pinto(dir.path())
        .args(["board", "--no-truncate"])
        .assert()
        .success();
    let full_stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    assert!(
        full_stdout.contains(&long),
        "shows full title: {full_stdout}"
    );
    assert!(!full_stdout.contains('…'), "no ellipsis when full");
}

#[test]
fn board_full_alias_matches_no_truncate() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let long = "z".repeat(200);
    pinto(dir.path()).args(["add", &long]).assert().success();

    // `--full` は `--no-truncate` のエイリアス。
    pinto(dir.path())
        .args(["board", "--full"])
        .assert()
        .success()
        .stdout(predicate::str::contains(&long));
}

// --- list / board / kanban share one canonical order ---

/// IDs of a `board --json` column, in display order.
fn board_column_ids(value: &serde_json::Value, status: &str) -> Vec<String> {
    value["columns"]
        .as_array()
        .expect("columns array")
        .iter()
        .find(|c| c["status"] == status)
        .expect("column present")["items"]
        .as_array()
        .expect("items array")
        .iter()
        .map(|i| i["id"].as_str().expect("id string").to_string())
        .collect()
}

/// IDs of a `list --json` response, in display order.
fn list_ids(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .expect("list array")
        .iter()
        .map(|i| i["id"].as_str().expect("id string").to_string())
        .collect()
}

/// Fixture: three PBIs completed in rank order, so `done_at` ascending equals
/// rank order and `done_at` descending reverses it — the case that exposes any
/// divergence between the default board sort and `list`.
fn done_in_rank_order(dir: &Path) {
    pinto(dir).arg("init").assert().success();
    for title in ["Alpha", "Bravo", "Charlie"] {
        pinto(dir).args(["add", title]).assert().success();
    }
    for id in ["T-1", "T-2", "T-3"] {
        pinto(dir).args(["move", id, "done"]).assert().success();
    }
}

#[test]
fn list_and_board_agree_within_a_column_under_explicit_rank_sort() {
    // Same filter + explicit `--sort rank` ⇒ identical priority order across
    // views, including the terminal column (no `done_at` exception applies).
    let dir = TempDir::new().expect("temp dir");
    done_in_rank_order(dir.path());

    let list = json_stdout(pinto(dir.path()).args(["list", "--status", "done", "--json"]));
    let board = json_stdout(pinto(dir.path()).args(["board", "--sort", "rank", "--json"]));

    assert_eq!(
        list_ids(&list),
        board_column_ids(&board, "done"),
        "list and board (--sort rank) must share the canonical order"
    );
}

#[test]
fn board_default_terminal_column_sorts_by_completion_not_rank() {
    // Documented exception: the terminal column defaults to `done_at`
    // descending, so the newest completion leads — the reverse of `list`'s
    // rank order. Everything outside the terminal column stays rank-ordered.
    let dir = TempDir::new().expect("temp dir");
    done_in_rank_order(dir.path());

    let list = json_stdout(pinto(dir.path()).args(["list", "--status", "done", "--json"]));
    let board = json_stdout(pinto(dir.path()).arg("board").arg("--json"));

    assert_eq!(list_ids(&list), ["T-1", "T-2", "T-3"], "list is rank order");
    assert_eq!(
        board_column_ids(&board, "done"),
        ["T-3", "T-2", "T-1"],
        "default board terminal column is done_at descending"
    );
}

/// Fixture reproducing the real-backlog shape: a lower-priority parent whose
/// child carries a *higher* raw rank than an unrelated, higher-priority
/// standalone PBI. Flat rank order scatters the child above the standalone;
/// hierarchical order must keep the whole subtree under its parent, below it.
///
/// Insert PARENT, its CHILD, then HIGH; finally send PARENT to the bottom so
/// its rank sits after HIGH while CHILD keeps the lowest (first) rank.
///
/// Flat rank order:   CHILD, HIGH, PARENT
/// Hierarchical order: HIGH, PARENT, CHILD
fn scattered_subtree(dir: &Path) {
    pinto(dir).arg("init").assert().success();
    pinto(dir).args(["add", "PARENT"]).assert().success(); // T-1
    pinto(dir)
        .args(["add", "CHILD", "--parent", "T-1"])
        .assert()
        .success(); // T-2
    pinto(dir).args(["add", "HIGH"]).assert().success(); // T-3
    pinto(dir)
        .args(["reorder", "T-1", "--bottom"])
        .assert()
        .success();
}

#[test]
fn list_orders_child_below_a_higher_priority_standalone() {
    let dir = TempDir::new().expect("temp dir");
    scattered_subtree(dir.path());
    let list = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(
        list_ids(&list),
        ["T-3", "T-1", "T-2"],
        "HIGH first; PARENT's subtree (CHILD) grouped under it, below HIGH"
    );
}

#[test]
fn board_and_list_share_hierarchical_order_within_a_column() {
    let dir = TempDir::new().expect("temp dir");
    scattered_subtree(dir.path());
    let list = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let board = json_stdout(pinto(dir.path()).arg("board").arg("--json"));
    assert_eq!(
        list_ids(&list),
        board_column_ids(&board, "todo"),
        "list and the board's todo column share the hierarchical order"
    );
}
