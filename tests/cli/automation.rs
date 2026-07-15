//! Automation plans, shell commands, and completions.

use super::common::*;
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::Duration;

fn dry_run_workspace_count(pid: u32) -> usize {
    let prefix = format!("pinto-dry-run-{pid}-");
    std::fs::read_dir(std::env::temp_dir())
        .expect("read temporary directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix))
        .count()
}

fn select_git_backend(dir: &Path) {
    let config_path = dir.join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("read config");
    let updated = config.replace("backend = \"file\"", "backend = \"git\"");
    assert_ne!(updated, config, "fixture must start with the file backend");
    std::fs::write(config_path, updated).expect("select Git backend");
}

#[test]
fn completion_bash_generates_script() {
    // 補完はボード初期化に依存しない（どこでも生成できる）。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "bash"])
        .assert()
        .success()
        // bash 補完は `_pinto()` 関数と `complete` 登録を含む。
        .stdout(predicate::str::contains("_pinto"))
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn completion_zsh_generates_script() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "zsh"])
        .assert()
        .success()
        // zsh 補完は `#compdef pinto` で始まる。
        .stdout(predicate::str::contains("#compdef pinto"));
}

#[test]
fn completion_fish_generates_script() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete -c pinto"));
}

#[test]
fn completion_lists_subcommands() {
    // 生成された補完に主要サブコマンドが含まれることを軽く保証する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("board"))
        .stdout(predicate::str::contains("sprint"));
}

#[test]
fn completion_includes_add_relationship_options() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--parent"))
        .stdout(predicate::str::contains("--depends-on"));
}

#[test]
fn completion_unknown_shell_errors_as_user_error() {
    // 未知のシェル名は clap の値解釈エラー。終了コード規約でユーザーエラー(1)へ寄せる。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["completion", "tcsh"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("tcsh"));
}

#[test]
fn shell_executes_commands_from_stdin() {
    // 1 行 1 コマンドで既存サブコマンドを実行し、ボードを開いたまま連続操作できる。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("add \"Hello world\"\nlist\nexit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("Hello world"));
}

#[test]
fn shell_continues_after_command_error() {
    // コマンド実行時エラー（存在しない ID）でループが落ちず、後続コマンドが実行できる。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("show T-999\nadd Recovered\nexit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Recovered"));
}

#[test]
fn shell_continues_after_unknown_command() {
    // 未知のコマンド（clap の解釈エラー）でもループを継続する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("frobnicate\nadd Later\nexit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Later"));
}

#[test]
fn shell_terminates_on_eof() {
    // exit/quit なしでも EOF（stdin の終端 = Ctrl-D 相当）で正常終了する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("add First\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("First"));
}

#[test]
fn shell_terminates_on_quit() {
    // `quit` でも正常終了する（`exit` の別名）。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("add Kept\nquit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Kept"));
}

#[test]
fn shell_rejects_nested_shell() {
    // 入れ子の `shell` は拒否され、後続行（stdin）を横取りせずに継続する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("shell")
        .write_stdin("shell\nadd Nested\nexit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Nested"))
        .stderr(predicate::str::contains("already in an interactive shell"));
}

#[test]
fn automate_executes_a_structured_plan_from_the_cli() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["add","Planned by an agent","--label","ai"],["move","T-1","in-progress"]]}"#,
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-1"))
        .stdout(predicate::str::contains("Moved T-1 to in-progress"))
        .stdout(predicate::str::contains("Automated 2 command(s)."));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["title"], "Planned by an agent");
    assert_eq!(item["status"], "in-progress");
    assert_eq!(item["labels"], serde_json::json!(["ai"]));
}

#[test]
fn automate_reads_a_plan_file_whose_name_starts_with_a_brace() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(
        dir.path().join("{plan.json"),
        r#"{"commands":[["add","From a brace-named file"]]}"#,
    )
    .expect("write plan file");

    pinto(dir.path())
        .args(["automate", "--plan", "{plan.json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-1"));
}

#[test]
fn automate_add_supports_parent_and_multiple_dependencies() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["add","Parent"],["add","Dependency"],["add","Story","--parent","T-1","--depends-on","T-1","T-2"]]}"#;

    pinto(dir.path())
        .args(["automate", "--plan", plan])
        .assert()
        .success()
        .stdout(predicate::str::contains("Automated 3 command(s)."));

    let item = show_json(pinto(dir.path()).args(["show", "T-3", "--json"]));
    assert_eq!(item["parent"], "T-1");
    assert_eq!(item["depends_on"], serde_json::json!(["T-1", "T-2"]));
}

#[test]
fn shell_executes_the_same_automation_command() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("shell")
        .write_stdin("automate --plan '{\"commands\":[[\"add\",\"From shell agent\"]]}'\nexit\n")
        .assert()
        .success()
        .stdout(predicate::str::contains("Created T-1"))
        .stdout(predicate::str::contains("Automated 1 command(s)."));

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["title"], "From shell agent");
}

#[test]
fn automate_rejects_api_keys_without_echoing_or_persisting_them() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let secret = "sk-test-must-not-be-logged";

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            &format!(r#"{{"api_key":"{secret}","commands":[["add","Nope"]]}}"#),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid automation plan"))
        .stderr(predicate::str::contains(secret).not());

    let items = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(items, serde_json::json!([]));
}

#[test]
fn automate_accepts_a_multiline_plan_from_stdin_and_reports_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r##"{"commands":[["add","Multiline","--body","# Heading\n\n- [ ] long body"]]}"##;

    let output = pinto(dir.path())
        .args(["automate", "--plan", "-", "--json"])
        .write_stdin(plan)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "completed");
    assert_eq!(report["commands"][0]["status"], "succeeded");
    assert_eq!(
        report["commands"][0]["created_ids"],
        serde_json::json!(["T-1"])
    );

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(item["body"], "# Heading\n\n- [ ] long body");
}

#[test]
fn automate_accepts_a_plan_file_and_dry_run_does_not_mutate_the_board() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan_path = dir.path().join("plan.json");
    std::fs::write(
        &plan_path,
        r#"{"commands":[["add","Planned one"],["add","Planned two"]]}"#,
    )
    .expect("write plan file");

    let binary = pinto(dir.path()).get_program().to_owned();
    let child = ProcessCommand::new(binary)
        .args([
            "automate",
            "--plan",
            plan_path.to_str().expect("plan path is UTF-8"),
            "--dry-run",
            "--json",
        ])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dry-run");
    let pid = child.id();
    let output = child.wait_with_output().expect("wait dry-run");
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        dry_run_workspace_count(pid),
        0,
        "successful dry-run must clean up"
    );
    let output = output.stdout;
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "dry_run");
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["commands"][0]["status"], "valid");
    assert_eq!(report["commands"][1]["status"], "valid");

    let items = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(
        items,
        serde_json::json!([]),
        "dry-run leaves the board unchanged"
    );
}

#[test]
fn automate_dry_run_in_a_normal_repository_keeps_git_clean_and_cleans_up() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    select_git_backend(dir.path());
    pinto_isolated_git(dir.path())
        .args(["add", "Committed task"])
        .assert()
        .success();

    let head_before = String::from_utf8(
        ProcessCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("git output")
    .trim()
    .to_string();

    let binary = pinto(dir.path()).get_program().to_owned();
    let child = ProcessCommand::new(binary)
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["add","Preview only"]]}"#,
            "--dry-run",
            "--json",
        ])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dry-run");
    let pid = child.id();
    let output = child.wait_with_output().expect("wait dry-run");
    assert!(
        output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        dry_run_workspace_count(pid),
        0,
        "successful dry-run must clean up"
    );
    let output = output.stdout;
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "dry_run");
    assert_eq!(report["commands"][0]["status"], "valid");

    let items = json_stdout(pinto_isolated_git(dir.path()).args(["list", "--json"]));
    assert_eq!(items.as_array().map(Vec::len), Some(1));
    assert_eq!(items[0]["title"], "Committed task");
    assert_eq!(items[0]["status"], "todo");
    let head_after = String::from_utf8(
        ProcessCommand::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(dir.path())
            .output()
            .expect("git rev-parse")
            .stdout,
    )
    .expect("git output")
    .trim()
    .to_string();
    assert_eq!(
        head_after, head_before,
        "dry-run must not commit the source repo"
    );
    let status = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[test]
fn automate_dry_run_works_from_a_linked_worktree() {
    let base = TempDir::new().expect("base repository");
    pinto(base.path()).arg("init").assert().success();
    select_git_backend(base.path());
    pinto_isolated_git(base.path())
        .args(["add", "Committed task"])
        .assert()
        .success();

    let linked = base.path().join("linked-worktree");
    let worktree = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "--detach",
            linked.to_str().expect("linked path is UTF-8"),
            "HEAD",
        ])
        .current_dir(base.path())
        .status()
        .expect("git worktree add");
    assert!(worktree.success());
    assert!(
        linked.join(".git").is_file(),
        "fixture must be a linked worktree"
    );

    let plan_path = base.path().join("plan.json");
    std::fs::write(
        &plan_path,
        r#"{"commands":[["add","Previewed in linked worktree"]]}"#,
    )
    .expect("write plan");
    let output = pinto_isolated_git(&linked)
        .args([
            "automate",
            "--plan",
            plan_path.to_str().expect("plan path is UTF-8"),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "dry_run");
    assert_eq!(report["commands"][0]["status"], "valid");
    assert_eq!(
        json_stdout(pinto_isolated_git(&linked).args(["list", "--json"]))[0]["title"],
        "Committed task",
        "linked-worktree dry-run must not mutate its source board"
    );
    let status = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .current_dir(&linked)
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[test]
fn automate_dry_run_waits_for_a_concurrent_writer_before_snapshotting() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let held = runtime
        .block_on(pinto::service::lock_board(dir.path()))
        .expect("hold source board lock");

    let binary = pinto(dir.path()).get_program().to_owned();
    let mut dry_run = ProcessCommand::new(&binary)
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["add","Preview only"]]}"#,
            "--dry-run",
            "--json",
        ])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dry-run");
    let dry_pid = dry_run.id();
    thread::sleep(Duration::from_millis(150));
    assert!(
        dry_run.try_wait().expect("poll dry-run").is_none(),
        "dry-run must wait while a writer owns the board lock"
    );

    let writer = ProcessCommand::new(&binary)
        .args(["add", "Concurrent writer"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn concurrent writer");
    drop(held);
    drop(runtime);

    let dry_output = dry_run.wait_with_output().expect("wait dry-run");
    let writer_output = writer.wait_with_output().expect("wait concurrent writer");
    assert_eq!(
        dry_run_workspace_count(dry_pid),
        0,
        "dry-run must clean up after a successful preview"
    );
    assert!(
        dry_output.status.success(),
        "dry-run failed: {}",
        String::from_utf8_lossy(&dry_output.stderr)
    );
    assert!(
        writer_output.status.success(),
        "writer failed: {}",
        String::from_utf8_lossy(&writer_output.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(&dry_output.stdout).expect("dry-run JSON report");
    assert_eq!(report["status"], "dry_run");
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"]))[0]["title"],
        "Concurrent writer"
    );
}

#[test]
fn automate_dry_run_reports_semantic_failures_without_touching_the_board() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["move","T-404","done"],["add","Never applied"]]}"#;

    let binary = pinto(dir.path()).get_program().to_owned();
    let child = ProcessCommand::new(binary)
        .args(["automate", "--plan", plan, "--dry-run", "--json"])
        .current_dir(dir.path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn semantic failure dry-run");
    let pid = child.id();
    let output = child
        .wait_with_output()
        .expect("wait semantic failure dry-run");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        dry_run_workspace_count(pid),
        0,
        "failed dry-run must clean up"
    );
    let output = output.stdout;
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["commands"][0]["status"], "invalid");
    assert!(
        report["commands"][0]["error"]
            .as_str()
            .expect("error text")
            .contains("T-404")
    );
    assert_eq!(report["commands"][1]["status"], "skipped");
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([])
    );
}

#[test]
fn automate_combines_a_template_with_noninteractive_body_input() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create template directory");
    std::fs::write(
        template_dir.join("default.md"),
        "## Summary\n\n- [ ] template criterion",
    )
    .expect("write template");

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            r###"{"commands":[["add","Combined","--template","default","--body","## Details\n\n- [ ] explicit criterion"]]}"###,
        ])
        .assert()
        .success();

    let item = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(
        item["body"],
        "## Summary\n\n- [ ] template criterion\n\n## Details\n\n- [ ] explicit criterion"
    );
}

#[test]
fn automate_reports_partial_failure_and_skipped_commands_as_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["add","Applied"],["move","T-404","done"],["add","Not applied"]]}"#;

    let output = pinto(dir.path())
        .args(["automate", "--plan", plan, "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "partial_failure");
    assert_eq!(report["commands"][0]["status"], "succeeded");
    assert_eq!(
        report["commands"][0]["created_ids"],
        serde_json::json!(["T-1"])
    );
    assert_eq!(report["commands"][1]["status"], "failed");
    assert!(
        report["commands"][1]["error"]
            .as_str()
            .unwrap()
            .contains("T-404")
    );
    assert_eq!(report["commands"][2]["status"], "skipped");

    let items = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(items.as_array().expect("items array").len(), 1);
    assert_eq!(items[0]["title"], "Applied");
}

#[test]
fn automate_validates_all_commands_before_any_mutation() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["add","Must not apply"],["move","T-1","--unknown"],["add","Also must not apply"]]}"#;

    let output = pinto(dir.path())
        .args(["automate", "--plan", plan, "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["commands"][0]["status"], "valid");
    assert_eq!(report["commands"][1]["status"], "invalid");
    assert_eq!(report["commands"][2]["status"], "valid");
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([])
    );
}

#[test]
fn automate_rejects_invalid_item_ids_before_any_mutation() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan =
        r#"{"commands":[["add","Must not apply"],["edit","../outside-1","--title","Changed"]]}"#;

    let output = pinto(dir.path())
        .args(["automate", "--plan", plan, "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["commands"][0]["status"], "valid");
    assert_eq!(report["commands"][1]["status"], "invalid");
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([]),
        "an invalid item ID must prevent every plan command from mutating the board"
    );
}

#[test]
fn automate_validates_invalid_item_ids_in_every_command_shape_before_execution() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = serde_json::json!({
        "commands": [
            ["add", "Must not apply", "--parent", "../outside-1"],
            ["show", "../outside-1"],
            ["move", "../outside-1", "done"],
            ["reorder", "../outside-1", "--top"],
            ["edit", "../outside-1", "--title", "Changed"],
            ["remove", "../outside-1", "--force"],
            ["dep", "add", "../outside-1", "T-2"],
            ["link", "add", "../outside-1", "deadbeef"],
            ["sprint", "add", "S-1", "../outside-1"],
            ["list"],
            ["link", "scan"],
            ["sprint", "new", "S-1", "No apply"]
        ]
    })
    .to_string();
    let output = pinto(dir.path())
        .args(["automate", "--plan", &plan, "--json"])
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output).expect("JSON report");
    assert_eq!(report["status"], "invalid");
    assert_eq!(report["commands"].as_array().expect("commands").len(), 12);
    assert_eq!(
        json_stdout(pinto(dir.path()).args(["list", "--json"])),
        serde_json::json!([]),
        "validation must happen before any command in the plan executes"
    );
}

#[test]
fn automate_human_completion_messages_are_localized_in_english_and_japanese() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["add","Localized"]]}"#;

    pinto(dir.path())
        .args(["automate", "--plan", plan])
        .env("LC_ALL", "en-US")
        .env("LANG", "en-US")
        .assert()
        .success()
        .stdout(predicate::str::contains("Automated 1 command(s)."));

    pinto(dir.path())
        .args(["automate", "--plan", r#"{"commands":[["add","日本語"]]}"#])
        .env("LC_ALL", "ja-JP")
        .env("LANG", "ja-JP")
        .assert()
        .success()
        .stdout(predicate::str::contains("自動化"));
}

#[test]
fn automate_human_failure_messages_are_localized_in_english_and_japanese() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let plan = r#"{"commands":[["move","T-404","done"]]}"#;

    pinto(dir.path())
        .args(["automate", "--plan", plan])
        .env("LC_ALL", "en-US")
        .env("LANG", "en-US")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Command 1 (move): failed"));

    pinto(dir.path())
        .args(["automate", "--plan", plan])
        .env("LC_ALL", "ja-JP")
        .env("LANG", "ja-JP")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("失敗しました"))
        .stderr(predicate::str::contains("自動化を停止"));
}

#[test]
fn automate_help_describes_sources_and_safe_modes() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .args(["automate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("file"))
        .stdout(predicate::str::contains("standard input"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("-j, --json"));
}

#[test]
fn automate_alias_auto_runs_plan_with_short_option() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["auto", "-p", r#"{"commands":[["add","AutoItem"]]}"#])
        .assert()
        .success();

    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("AutoItem"));
}

#[test]
fn automate_human_validation_reports_invalid_commands_without_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["move","T-1","--unknown"]]}"#,
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("invalid"));
}

#[test]
fn automate_human_dry_run_reports_validation_summary() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["add","Planned"]]}"#,
            "--dry-run",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run validated"));
}

#[test]
fn automate_human_failure_reports_skipped_commands() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args([
            "automate",
            "--plan",
            r#"{"commands":[["add","Applied"],["move","T-404","done"],["add","Skipped"]]}"#,
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Command 3"));
}

#[test]
fn automate_missing_plan_file_reports_a_fixable_source_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["automate", "--plan", "does-not-exist.json"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "cannot read automation plan source",
        ));
}
