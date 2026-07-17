//! CLI initialization, errors, storage, and aliases.

use super::common::*;

#[test]
fn cli_help_uses_the_pinto_product_name() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("pinto"));
}

#[test]
fn init_creates_board_files() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));

    assert!(dir.path().join(".pinto/config.toml").is_file());
    assert!(dir.path().join(".pinto/tasks").is_dir());

    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("todo"), "default columns present");
    assert!(config.contains("[project]"), "project section present");
    assert!(
        config.contains("[display]") && config.contains("timezone = \"local\""),
        "display timezone defaults are discoverable"
    );
    assert!(
        config.contains("[tui.key_bindings]"),
        "Kanban keys are discoverable"
    );
    assert!(config.contains("quit ="), "default quit keys are written");
    assert!(config.contains("add ="), "PBI add key is configurable");
    assert!(
        config.contains("dependency_add =") && config.contains("dependency_remove ="),
        "dependency keys are configurable"
    );
    assert!(config.contains("parent ="), "parent key is configurable");
}

#[test]
fn init_twice_is_idempotent() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("Already initialized"));
}

#[test]
fn commands_discover_the_nearest_ancestor_board() {
    let outer = TempDir::new().expect("outer temp dir");
    pinto(outer.path()).arg("init").assert().success();
    pinto(outer.path())
        .args(["add", "Outer item"])
        .assert()
        .success();

    let inner = outer.path().join("workspace");
    std::fs::create_dir_all(&inner).expect("create inner directory");
    pinto(&inner).arg("init").assert().success();
    pinto(&inner).args(["add", "Inner item"]).assert().success();

    pinto(&inner)
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Inner item"));

    let nested = inner.join("src").join("module");
    std::fs::create_dir_all(&nested).expect("create nested directory");

    pinto(&nested)
        .args(["list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inner item"))
        .stdout(predicate::str::contains("Outer item").not());
}

#[test]
fn board_directory_can_be_overridden_by_flag_or_environment() {
    let board = TempDir::new().expect("board temp dir");
    pinto(board.path()).arg("init").assert().success();
    pinto(board.path())
        .args(["add", "Explicit board item"])
        .assert()
        .success();

    let environment_board = TempDir::new().expect("environment board temp dir");
    pinto(environment_board.path())
        .arg("init")
        .assert()
        .success();
    pinto(environment_board.path())
        .args(["add", "Environment board item"])
        .assert()
        .success();

    let worktree = TempDir::new().expect("worktree temp dir");
    pinto(worktree.path())
        .env("PINTO_DIR", environment_board.path())
        .args(["--dir", board.path().to_str().expect("board path"), "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Explicit board item"))
        .stdout(predicate::str::contains("Environment board item").not());

    pinto(worktree.path())
        .env("PINTO_DIR", board.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Explicit board item"));
}

#[test]
fn board_directory_override_accepts_the_pinto_directory() {
    let board = TempDir::new().expect("board temp dir");
    pinto(board.path()).arg("init").assert().success();
    pinto(board.path())
        .args(["add", "Pinto path item"])
        .assert()
        .success();

    let worktree = TempDir::new().expect("worktree temp dir");
    pinto(worktree.path())
        .args([
            "list",
            "--dir",
            board.path().join(".pinto").to_str().expect("board path"),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pinto path item"));
}

#[test]
fn no_board_error_explains_discovery_and_overrides() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("ancestor"))
        .stderr(predicate::str::contains("--dir"))
        .stderr(predicate::str::contains("PINTO_DIR"));
}

#[test]
fn discovery_does_not_cross_a_repository_boundary() {
    let parent = TempDir::new().expect("parent temp dir");
    pinto(parent.path()).arg("init").assert().success();

    let repository = parent.path().join("repository");
    let nested = repository.join("src");
    std::fs::create_dir_all(&nested).expect("create repository directories");
    std::fs::create_dir(repository.join(".git")).expect("mark repository root");

    pinto(&nested)
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("ancestor"));
}

#[test]
fn write_command_recovers_a_stale_lock_file() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(dir.path().join(".pinto/.lock"), "999999999\n").expect("stale lock");

    pinto(dir.path())
        .args(["add", "Recovered write"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"));
}

#[test]
fn ls_alias_works() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Aliased"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("Aliased"));
}

#[test]
fn no_subcommand_is_usage_error() {
    let dir = TempDir::new().expect("temp dir");

    // 引数の使い方の誤りはユーザーエラー = 終了コード 1（内部エラー 2 と区別する）。
    pinto(dir.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn version_flag_prints_version() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("pinto"));
}

#[test]
fn unknown_flag_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["list", "--nope"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("--nope"));
}

#[test]
fn missing_required_argument_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // `add` はタイトル必須。欠落は使い方の誤り = 1。
    pinto(dir.path())
        .arg("add")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn invalid_option_value_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // `--points` は数値。非数値は使い方の誤り = 1。
    pinto(dir.path())
        .args(["add", "Task", "--points", "abc"])
        .assert()
        .failure()
        .code(1);
}

#[test]
fn unknown_subcommand_is_user_error_code_1() {
    let dir = TempDir::new().expect("temp dir");

    pinto(dir.path())
        .arg("frobnicate")
        .assert()
        .failure()
        .code(1);
}

#[test]
fn help_flag_exits_success_code_0() {
    let dir = TempDir::new().expect("temp dir");

    // --help は要求どおりの表示 = 成功(0)、出力は stdout。
    pinto(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn init_writes_default_file_storage_backend() {
    // `init` は保存先バックエンド（既定 file）を config に明示し、発見可能にする。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("[storage]"), "storage section present");
    assert!(
        config.contains("backend = \"file\""),
        "default backend is file"
    );
}

#[test]
fn unknown_storage_backend_is_a_user_error_not_panic() {
    // 未知のバックエンド値は握り潰さず、直し方の分かるパースエラー（ユーザーエラー = 1）にする。
    // `sqlite` は sqlite 機能を有効化すると正規の値になるため、常に未知である架空の値で検証する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    std::fs::write(
        dir.path().join(".pinto/config.toml"),
        "columns = [\"todo\", \"done\"]\ndone_column = \"done\"\n\n[project]\nname = \"x\"\nkey = \"T\"\n\n[tui]\nconfirm_quit = true\n\n[storage]\nbackend = \"postgres\"\n\n[wip]\nenabled = true\n",
    )
    .expect("write config");

    pinto(dir.path())
        .arg("list")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("unknown variant"))
        .stderr(predicate::str::contains("panicked").not());
}

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
fn file_backend_roundtrips_add_and_list() {
    // 既定（file）でバックエンド抽象化を経ても既存挙動が不変であることを確認する。
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Via backend"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Via backend"));
}

#[test]
fn git_backend_commits_each_change_through_cli() {
    // 一時 git リポジトリ（tempfile 隔離）で、CLI 経由の変更操作ごとにコミットされることを
    // エンドツーエンドに確認する。git 未初期化からの自動 init と、ambient な git
    // identity が無い環境（= 素の CI ランナー）での identity フォールバックも併せて検証する。
    let dir = TempDir::new().expect("temp dir");
    pinto_isolated_git(dir.path())
        .arg("init")
        .assert()
        .success();

    // config.toml で git バックエンドを選択する（手編集で選べる = ユーザーの操作を再現）。
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("write config");

    // 事前に git リポジトリではない。
    assert!(!dir.path().join(".git").exists());

    pinto_isolated_git(dir.path())
        .args(["add", "First task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--points", "3"])
        .assert()
        .success();

    // 自動初期化され、変更操作ごとにコミットが積まれている。
    assert!(
        dir.path().join(".git").exists(),
        "auto-initialized git repo"
    );
    let subjects = git_log_field(dir.path(), "%s");
    assert_eq!(subjects, ["pinto: update T-1", "pinto: add T-1"]);
    // ambient identity が無いので pinto 既定 identity で著者付けされている。
    let authors = git_log_field(dir.path(), "%ae");
    assert_eq!(authors, ["pinto@localhost", "pinto@localhost"]);
}

fn assert_git_commit_and_clean(dir: &Path, subject: &str) {
    assert_eq!(
        git_log_field(dir, "%s").first().map(String::as_str),
        Some(subject),
        "the operation must create the expected latest commit"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .expect("git status");
    assert!(
        status.status.success() && status.stdout.is_empty(),
        "operation must leave a clean worktree: {}",
        String::from_utf8_lossy(&status.stdout)
    );
    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir)
        .output()
        .expect("git tree");
    assert!(
        !String::from_utf8_lossy(&tree.stdout)
            .lines()
            .any(|path| path == ".pinto/.lock")
    );
}

#[test]
fn git_backend_commits_item_sprint_dod_and_removal_mutations() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "First task"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: migrate file to git");

    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--title", "Edited task"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["reorder", "T-2", "--top"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");

    pinto_isolated_git(dir.path())
        .args(["move", "T-1", "in-progress"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");
    pinto_isolated_git(dir.path())
        .args(["dep", "rm", "T-2", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-2");

    pinto_isolated_git(dir.path())
        .args(["link", "add", "T-1", "deadbeef"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");
    pinto_isolated_git(dir.path())
        .args(["link", "rm", "T-1", "deadbeef"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args([
            "sprint",
            "new",
            "S-1",
            "Release sprint",
            "--goal",
            "Ship",
            "--start",
            "2025-01-01",
            "--end",
            "2025-01-05",
        ])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: add S-1");

    pinto_isolated_git(dir.path())
        .args([
            "sprint",
            "capacity",
            "S-1",
            "--daily-hours",
            "8",
            "--holidays",
            "0",
            "--deduction-factor",
            "0.1",
        ])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");

    pinto_isolated_git(dir.path())
        .args(["sprint", "add", "S-1", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");
    pinto_isolated_git(dir.path())
        .args(["sprint", "unassign", "S-1", "T-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update T-1");

    pinto_isolated_git(dir.path())
        .args(["sprint", "start", "S-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");
    pinto_isolated_git(dir.path())
        .args(["sprint", "close", "S-1"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update S-1");

    pinto_isolated_git(dir.path())
        .args(["dod", "set", "- [ ] review"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: update common DoD");
    pinto_isolated_git(dir.path())
        .args(["dod", "clear"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: remove common DoD");

    pinto_isolated_git(dir.path())
        .args(["rm", "T-2"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: archive T-2");
    pinto_isolated_git(dir.path())
        .args(["rm", "T-1", "--force"])
        .assert()
        .success();
    assert_git_commit_and_clean(dir.path(), "pinto: remove T-1");
}

#[test]
fn git_backend_warns_before_auto_initializing_a_repository() {
    // Git バックエンドは最初の書き込みで `git init` を実行するため、意図しない初期化を
    // 見逃さないよう、CLI 経由で明確な警告を出す。
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
    .expect("write config");

    pinto_isolated_git(dir.path())
        .args(["add", "First task"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "warning: initializing a Git repository for the selected git backend",
        ));
}

#[test]
fn git_backend_leaves_a_clean_tree_without_transient_lock_in_history() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");

    pinto_isolated_git(dir.path())
        .args(["add", "Tracked task"])
        .assert()
        .success();

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(
        status.status.success() && status.stdout.is_empty(),
        "Git backend must leave a clean worktree: {}",
        String::from_utf8_lossy(&status.stdout)
    );

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));
}

#[test]
fn git_backend_migration_commits_the_backend_switch() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Migrated task"])
        .assert()
        .success();

    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let head_config = std::process::Command::new("git")
        .args(["show", "HEAD:.pinto/config.toml"])
        .current_dir(dir.path())
        .output()
        .expect("read committed config");
    assert!(
        String::from_utf8_lossy(&head_config.stdout).contains("backend = \"git\""),
        "migration backend switch must be in HEAD"
    );
    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[test]
fn git_backend_migration_is_one_commit_boundary() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "First migrated task"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Second migrated task"])
        .assert()
        .success();

    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    assert_eq!(
        git_log_field(dir.path(), "%s"),
        ["pinto: migrate file to git"],
        "one user migration must produce one coherent commit"
    );
}

#[test]
fn git_backend_does_not_commit_preexisting_board_or_outside_changes() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Existing task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let inside = dir.path().join(".pinto/user-note.md");
    std::fs::write(&inside, "keep staged\n").expect("write board note");
    let untracked_inside = dir.path().join(".pinto/untracked-note.md");
    std::fs::write(&untracked_inside, "keep untracked\n").expect("write untracked board note");
    let outside = dir.path().join("user-note.md");
    std::fs::write(&outside, "keep staged outside\n").expect("write outside note");
    run_git(dir.path(), &["add", ".pinto/user-note.md", "user-note.md"]);

    pinto_isolated_git(dir.path())
        .args(["add", "New task"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/user-note.md"));
    assert!(!tree.lines().any(|path| path == ".pinto/untracked-note.md"));
    assert!(!tree.lines().any(|path| path == "user-note.md"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("A  .pinto/user-note.md"))
    );
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("?? .pinto/untracked-note.md"))
    );
    assert!(status.lines().any(|line| line.ends_with("user-note.md")));
}

#[test]
fn git_backend_writes_after_checking_out_a_tracked_lock_file() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");
    std::fs::write(dir.path().join(".pinto/.lock"), "old owner\n").expect("tracked lock fixture");

    run_git(dir.path(), &["init"]);
    run_git(dir.path(), &["config", "user.name", "Fixture"]);
    run_git(
        dir.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    run_git(dir.path(), &["add", ".pinto"]);
    run_git(dir.path(), &["commit", "-m", "fixture with tracked lock"]);

    pinto_isolated_git(dir.path())
        .args(["add", "Write after checkout"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    assert!(status.status.success() && status.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn git_commit_failure_preserves_worktree_change_and_real_index() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Durable task"])
        .assert()
        .success();
    pinto_isolated_git(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success();

    let hooks = TempDir::new().expect("hooks dir");
    editor_script(hooks.path(), "pre-commit", "exit 1");
    run_git(
        dir.path(),
        &[
            "config",
            "core.hooksPath",
            hooks.path().to_str().expect("hooks path"),
        ],
    );

    pinto_isolated_git(dir.path())
        .args(["edit", "T-1", "--title", "Durable but uncommitted"])
        .assert()
        .failure();

    pinto_isolated_git(dir.path())
        .args(["show", "T-1", "--plain"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Durable but uncommitted"));

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .expect("git status");
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.lines().any(|line| line == " M .pinto/tasks/T-1.md"),
        "the failed commit must leave the durable file change in the worktree: {status}"
    );
    assert!(
        std::process::Command::new("git")
            .args(["diff", "--cached", "--quiet"])
            .current_dir(dir.path())
            .status()
            .expect("git diff --cached")
            .success(),
        "the failed commit must not modify the real index"
    );
}

#[test]
fn git_backend_commits_common_dod_and_leaves_no_lock_artifact() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config.replace("backend = \"file\"", "backend = \"git\""),
    )
    .expect("select git backend");
    pinto_isolated_git(dir.path())
        .args(["dod", "set", "- [ ] review"])
        .assert()
        .success();

    let tree = std::process::Command::new("git")
        .args(["ls-tree", "-r", "--name-only", "HEAD"])
        .current_dir(dir.path())
        .output()
        .expect("git tree");
    let tree = String::from_utf8_lossy(&tree.stdout);
    assert!(tree.lines().any(|path| path == ".pinto/dod.md"));
    assert!(!tree.lines().any(|path| path == ".pinto/.lock"));
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

    // 設定ファイルを壊す（手編集・破損を想定）。パースエラーはユーザーエラー = 1。
    // `board` は列定義を読むため config をパースする（`list` は読まない）。
    std::fs::write(dir.path().join(".pinto/config.toml"), "columns = = broken")
        .expect("corrupt config");

    pinto(dir.path())
        .arg("board")
        .assert()
        .failure()
        .code(1)
        // panic ではなく、直し方の分かる整形済みメッセージで終了する。
        .stderr(predicate::str::contains("config.toml"))
        .stderr(predicate::str::contains("panicked").not());
}

#[test]
fn config_missing_required_section_is_user_error_code_1_without_panic() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    // 必須の [tui] セクションを手編集で削除した設定を想定する。
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

    // frontmatter 区切りを欠いた壊れたタスクファイルにする。
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

    // Frontmatter の TOML 自体が壊れた手編集を想定する。
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

#[test]
fn migrate_on_uninitialized_dir_errors() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["migrate", "--to", "file"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("init"));
}

#[test]
fn migrate_to_current_backend_is_noop() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "file"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already using file"));
}

#[test]
fn migrate_to_git_switches_the_backend() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Git task"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["migrate", "--to", "git"])
        .assert()
        .success()
        .stdout(predicate::str::contains("now git"));
}

#[cfg(feature = "sqlite")]
#[test]
fn migrate_to_sqlite_switches_backend_and_persists() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Persisted task"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrated 1 item(s)"))
        .stdout(predicate::str::contains("now sqlite"));

    // config が切り替わり、DB ファイルが生成されている。
    let config = std::fs::read_to_string(dir.path().join(".pinto/config.toml")).expect("config");
    assert!(config.contains("backend = \"sqlite\""), "backend switched");
    assert!(
        dir.path().join(".pinto/board.sqlite3").is_file(),
        "db file created"
    );

    // 以降の `list` は SQLite から読む（ドッグフーディング相当）。
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Persisted task"));

    // 追加操作も SQLite 側に効く。
    pinto(dir.path())
        .args(["add", "Second on sqlite"])
        .assert()
        .success();
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("Second on sqlite"));
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_normalized_schema_roundtrips_via_cli() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success();

    // ラベル（多値）とポイント（型付き任意カラム）を持つ PBI を追加する。
    pinto(dir.path())
        .args([
            "add", "First", "--label", "backend", "--label", "urgent", "--points", "5",
        ])
        .assert()
        .success();
    pinto(dir.path()).args(["add", "Second"]).assert().success();

    // 依存（多値・関連テーブル）を張る。
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    // ラベル・ポイントが list に反映される（型付きカラムから復元）。
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("(5)"))
        .stdout(predicate::str::contains("[backend, urgent]"));

    // 依存が show に反映される。
    pinto(dir.path())
        .args(["show", "T-2"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Depends on:").and(predicate::str::contains("T-1")));

    // アーカイブ（archived フラグ）で list から外れるが、採番は退避 ID を再利用しない。
    pinto(dir.path()).args(["rm", "T-1"]).assert().success();
    pinto(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("First").not())
        .stdout(predicate::str::contains("Second"));
    pinto(dir.path())
        .args(["add", "Third"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-3"));
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_corruption_is_rejected_by_list_and_show() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Visible task"])
        .assert()
        .success();

    let db_path = dir.path().join(".pinto/board.sqlite3");
    let conn = rusqlite::Connection::open(db_path).expect("open sqlite database");
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE items SET title = ' ' WHERE id = 'T-1'", [])
        .expect("corrupt item title");

    for args in [["list"].as_slice(), ["show", "T-1"].as_slice()] {
        pinto(dir.path())
            .args(args)
            .assert()
            .failure()
            .code(1)
            .stderr(predicate::str::contains("empty item title"));
    }
}

#[cfg(not(feature = "sqlite"))]
#[test]
fn migrate_to_sqlite_without_feature_errors() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["migrate", "--to", "sqlite"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--features sqlite"));
}

#[test]
fn new_toplevel_aliases_are_listed_in_help() {
    let dir = TempDir::new().expect("temp dir");
    let out = pinto(dir.path()).arg("--help").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    for alias in [
        "aliases: d]",
        "aliases: ln]",
        "aliases: dd]",
        "aliases: mig]",
        "aliases: auto]",
    ] {
        assert!(
            stdout.contains(alias),
            "top-level help lists alias `{alias}`: {stdout}"
        );
    }
}

#[test]
fn migrate_alias_mig_shows_help() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path())
        .args(["mig", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("-t, --to"));
}

#[test]
fn cli_options_have_short_forms() {
    let dir = TempDir::new().expect("temp dir");
    let cases: &[(&[&str], &[&str])] = &[
        (&["show", "--help"], &["-j, --json"]),
        (&["list", "--help"], &["-j, --json"]),
        (
            &["board", "--help"],
            &[
                "-o, --sort",
                "-f, --no-truncate",
                "-j, --json",
                "-w, --no-wip-check",
                "-P, --roots-only",
                "-a, --all-labels",
            ],
        ),
        (&["move", "--help"], &["-w, --no-wip-check"]),
        (&["remove", "--help"], &["-f, --force"]),
        (&["edit", "--help"], &["-N, --no-parent"]),
        (&["add", "--help"], &["-t, --template"]),
        (&["cycletime", "--help"], &["-j, --json"]),
        (&["rebalance", "--help"], &["-n, --dry-run"]),
        (&["migrate", "--help"], &["-t, --to"]),
        (&["automate", "--help"], &["-p, --plan"]),
        (&["link", "sync", "--help"], &["-s, --since"]),
        (
            &["sprint", "new", "--help"],
            &["-g, --goal", "-t, --template", "-s, --start", "-e, --end"],
        ),
        (
            &["sprint", "edit", "--help"],
            &["-t, --title", "-g, --goal", "-s, --start", "-e, --end"],
        ),
        (
            &["sprint", "close", "--help"],
            &["-r, --rollover <TARGET>", "-u, --release"],
        ),
        (&["sprint", "remove", "--help"], &["Remove a sprint"]),
        (
            &["sprint", "unassign", "--help"],
            &["Unassign a PBI from its sprint"],
        ),
        (&["sprint", "--help"], &["aliases: rm]", "aliases: u]"]),
        (&["sprint", "list", "--help"], &["-j, --json"]),
        (&["sprint", "burndown", "--help"], &["-j, --json"]),
        (&["sprint", "velocity", "--help"], &["-n, --recent"]),
        (
            &["sprint", "capacity", "--help"],
            &[
                "-H, --daily-hours",
                "-d, --holidays",
                "-f, --deduction-factor",
                "-j, --json",
            ],
        ),
        (
            &["list", "--help"],
            &[
                "-F, --search",
                "-R, --regex",
                "-P, --roots-only",
                "-a, --all-labels",
            ],
        ),
        (
            &["board", "--help"],
            &[
                "-F, --search",
                "-R, --regex",
                "-P, --roots-only",
                "-a, --all-labels",
            ],
        ),
        (&["add", "--help"], &["-P, --parent", "-d, --depends-on"]),
    ];

    for (args, expected) in cases {
        let out = pinto(dir.path()).args(*args).assert().success();
        let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
        for needle in *expected {
            assert!(
                stdout.contains(needle),
                "`{args:?}` help lists `{needle}`: {stdout}"
            );
        }
    }
}

#[test]
fn rebalance_cli_shortens_a_narrow_rank_interval() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    for title in ["A", "B"] {
        pinto(dir.path()).args(["add", title]).assert().success();
    }

    for number in 3..=40 {
        let title = format!("Item {number}");
        pinto(dir.path())
            .args(["add", title.as_str()])
            .assert()
            .success();
        let id = format!("T-{number}");
        pinto(dir.path())
            .args(["reorder", id.as_str(), "--before", "T-2"])
            .assert()
            .success();
    }

    let before = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let before_items = before.as_array().expect("list --json is an array");
    assert!(
        before_items
            .iter()
            .any(|item| item["rank"].as_str().unwrap().len() > 2),
        "repeated insertion must create a narrow, long rank interval"
    );
    let before_ids: Vec<_> = before_items
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();

    pinto(dir.path())
        .args(["rebalance", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would rebalance"));
    pinto(dir.path())
        .arg("rebalance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Rebalanced"));

    let after = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let after_items = after.as_array().expect("list --json is an array");
    let after_ids: Vec<_> = after_items
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(after_ids, before_ids, "rebalance preserves backlog order");
    assert!(
        after_items
            .iter()
            .all(|item| item["rank"].as_str().unwrap().len() == 2),
        "a forty-item sibling scope uses two-digit ranks"
    );
}

#[test]
fn rebalance_reports_when_an_empty_board_is_already_balanced() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("rebalance")
        .assert()
        .success()
        .stdout(predicate::str::contains("Ranks already balanced"));
}
