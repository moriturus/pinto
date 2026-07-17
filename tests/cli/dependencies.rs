//! Parent, dependency, and Git-link relationships.

use super::common::*;

#[test]
fn cli_dependency_operations_share_the_tui_targeting_contract() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Prerequisite"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();
    let linked = json_stdout(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(linked[0]["depends_on"], serde_json::json!(["T-1"]));

    pinto(dir.path())
        .args(["dep", "rm", "T-2", "T-1"])
        .assert()
        .success();
    let unlinked = json_stdout(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(unlinked[0]["depends_on"], serde_json::json!([]));
}

#[test]
fn edit_sets_parent_and_show_reports_both_directions() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Epic", "Story"]);

    // T-2（Story）の親を T-1（Epic）に設定する。
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated T-2"));

    // frontmatter に parent が保存される。
    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-2.md")).expect("task file");
    assert!(task.contains("parent = \"T-1\""));

    // 子側は Parent、親側は Children で双方向に確認できる。
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Parent:      T-1"));
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Children:    T-2"));
}

#[test]
fn edit_no_parent_clears_parent() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Epic", "Story"]);
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["edit", "T-2", "--no-parent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated T-2"));

    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-2.md")).expect("task file");
    assert!(!task.contains("parent ="), "parent field removed");
}

#[test]
fn edit_parent_cycle_exits_with_code_1() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["A", "B"]);
    // A ← B（B の親 A）。A の親を B にすると循環 → エラー。
    pinto(dir.path())
        .args(["edit", "T-2", "--parent", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["edit", "T-1", "--parent", "T-2"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("cycle"));
}

#[test]
fn dep_add_sets_dependency_and_show_reports_both_directions() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Foundation", "Feature"]);

    // T-2（Feature）は T-1（Foundation）に依存する。
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-2 now depends on T-1"));

    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-2.md")).expect("task file");
    assert!(task.contains("depends_on = [\"T-1\"]"));

    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Depends on:  T-1"));
    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Depended by: T-2"));
}

#[test]
fn dep_add_cycle_warns_but_succeeds() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["A", "B"]);
    // B は A に依存。ここで A に「B へ依存」を足すと循環（警告だが成功）。
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["dep", "add", "T-1", "T-2"])
        .assert()
        .success()
        .stderr(predicate::str::contains("cycle"));

    // 循環でも記録される。
    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-1.md")).expect("task file");
    assert!(task.contains("depends_on = [\"T-2\"]"));
}

#[test]
fn dep_rm_removes_dependency() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["A", "B"]);
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["dep", "rm", "T-2", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no longer depends on T-1"));

    let task = std::fs::read_to_string(dir.path().join(".pinto/tasks/T-2.md")).expect("task file");
    assert!(!task.contains("depends_on"), "dependency removed");
}

#[test]
fn dep_add_missing_id_exits_with_code_1() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["A"]);
    pinto(dir.path())
        .args(["dep", "add", "T-1", "T-99"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("T-99"));
}

#[test]
fn link_add_records_commits_and_show_lists_them() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    // Git を用意していないディレクトリでも、手動リンクは素の文字列として記録できる。
    pinto(dir.path())
        .args(["link", "add", "T-1", "abc12345", "def67890"])
        .assert()
        .success()
        .stdout(predicate::str::contains("linked"));

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Commits"))
        .stdout(predicate::str::contains("abc12345"))
        .stdout(predicate::str::contains("def67890"));

    // --json は完全な SHA を配列で持つ（機械可読な契約）。
    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["commits"][0], "abc12345");
    assert_eq!(value["commits"][1], "def67890");
}

#[test]
fn link_rm_unlinks_by_prefix() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();
    pinto(dir.path())
        .args(["link", "add", "T-1", "abc12345", "def67890"])
        .assert()
        .success();

    // 短縮 SHA（前方一致）で外せる。
    pinto(dir.path())
        .args(["link", "rm", "T-1", "abc12"])
        .assert()
        .success()
        .stdout(predicate::str::contains("unlinked"));

    let value = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(value["commits"].as_array().expect("array").len(), 1);
    assert_eq!(value["commits"][0], "def67890");

    pinto(dir.path())
        .args(["link", "add", "T-1", "def67890"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already linked"));
    pinto(dir.path())
        .args(["link", "rm", "T-1", "missing"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no matching commit"));
}

#[test]
fn link_sync_associates_commits_from_messages() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "A"]).assert().success(); // T-1
    pinto(dir.path()).args(["add", "B"]).assert().success(); // T-2

    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.email", "t@example.com"]);
    run_git(dir.path(), &["config", "user.name", "Tester"]);
    run_git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "feat: A (T-1)"],
    );
    run_git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "chore: unrelated"],
    );
    run_git(dir.path(), &["commit", "--allow-empty", "-m", "fix: B T-2"]);

    pinto(dir.path())
        .args(["link", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("T-2"))
        .stdout(predicate::str::contains("Linked 2 commit(s)."));

    // 各 PBI にちょうど 1 件ずつリンクされる。
    let a = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    let b = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(a["commits"].as_array().expect("array").len(), 1);
    assert_eq!(b["commits"].as_array().expect("array").len(), 1);

    // 再走査しても増えない（冪等）。
    pinto(dir.path())
        .args(["link", "sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No new commits linked."));
}

#[test]
fn link_scan_is_rejected_after_rename() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["link", "scan"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn dep_alias_d_adds_dependency() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Base"]).assert().success();
    pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["d", "add", "T-2", "T-1"])
        .assert()
        .success();

    let json = show_json(pinto(dir.path()).args(["show", "T-2", "-j"]));
    assert_eq!(json["depends_on"][0], "T-1");
}

#[test]
fn link_alias_ln_adds_commit() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Item"]).assert().success();

    pinto(dir.path())
        .args(["ln", "add", "T-1", "abc1234"])
        .assert()
        .success();

    let json = show_json(pinto(dir.path()).args(["show", "T-1", "-j"]));
    assert_eq!(json["commits"][0], "abc1234");
}
