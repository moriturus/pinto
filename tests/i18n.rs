//! Integration tests for locale selection and CLI localization.

use assert_cmd::Command;
use pinto::i18n::{Locale, Message, locale_name_from, localizer_from};
use predicates::prelude::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
fn lc_all_takes_precedence_over_lang() {
    let localizer = localizer_from(Some("en_GB.UTF-8"), Some("en_US.UTF-8"));

    assert_eq!(
        locale_name_from(Some("en_GB.UTF-8"), Some("en_US.UTF-8")),
        Some("en_GB.UTF-8")
    );
    assert_eq!(localizer.language_identifier().to_string(), "en-GB");
    assert_eq!(localizer.locale(), Locale::English);
}

#[test]
fn unsupported_locale_falls_back_to_english() {
    let localizer = localizer_from(Some("fr_FR.UTF-8"), None);

    assert_eq!(localizer.locale(), Locale::English);
    assert_eq!(localizer.language_identifier().to_string(), "en-US");
    assert_eq!(localizer.text(Message::NoBacklogItems), "No backlog items.");
    assert_eq!(
        localizer.text(Message::KanbanDependencyLegend),
        "⊸ depends on  ⊷ depended on by  ⊸! unresolved dependency"
    );
    assert_eq!(localizer.text(Message::KanbanQuitPrompt), "Quit pinto?");
}

#[test]
fn localized_messages_interpolate_fluent_variables() {
    let localizer = localizer_from(Some("en_US.UTF-8"), None);

    assert_eq!(
        localizer.format(
            Message::Created,
            [("id", "T-1"), ("title", "Fluent-backed item")]
        ),
        "Created T-1 Fluent-backed item"
    );
}

#[test]
fn supported_english_locale_is_read_from_environment() {
    let localizer = localizer_from(Some("en_GB.UTF-8"), None);

    assert_eq!(localizer.language_identifier().to_string(), "en-GB");
}

#[test]
fn japanese_locale_uses_the_japanese_bundle() {
    let localizer = localizer_from(Some("ja_JP.UTF-8"), None);

    assert_eq!(localizer.locale(), Locale::Japanese);
    assert_eq!(localizer.language_identifier().to_string(), "ja-JP");
    assert_eq!(
        localizer.text(Message::NoBacklogItems),
        "バックログアイテムはありません。"
    );
    assert_eq!(
        localizer.format(Message::Created, [("id", "T-1"), ("title", "日本語の項目")]),
        "作成しました: T-1 日本語の項目"
    );
}

#[test]
fn cli_uses_the_english_fallback_for_an_unsupported_locale() {
    let dir = TempDir::new().expect("temp dir");
    let mut command = Command::cargo_bin("pinto").expect("binary builds");

    command
        .current_dir(dir.path())
        .args(["init"])
        .env("LC_ALL", "fr_FR.UTF-8")
        .env("LANG", "ja_JP.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized pinto board at"));
}

#[test]
fn board_dependency_legend_uses_english_for_an_unsupported_locale() {
    let dir = TempDir::new().expect("temp dir");
    for arguments in [
        vec!["init"],
        vec!["add", "Base"],
        vec!["add", "Dependent"],
        vec!["dep", "add", "T-2", "T-1"],
    ] {
        let mut command = Command::cargo_bin("pinto").expect("binary builds");
        command
            .current_dir(dir.path())
            .args(arguments)
            .assert()
            .success();
    }

    let mut command = Command::cargo_bin("pinto").expect("binary builds");
    command
        .current_dir(dir.path())
        .arg("board")
        .env("LC_ALL", "fr_FR.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "⊸ depends on  ⊷ depended on by  ⊸! unresolved dependency",
        ));
}

#[test]
fn clap_help_uses_english_for_an_unsupported_locale() {
    let mut command = Command::cargo_bin("pinto").expect("binary builds");
    command
        .args(["--help"])
        .env("LC_ALL", "fr_FR.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("Commands:"))
        .stdout(predicate::str::contains(
            "Initialize a board in the current directory.",
        ))
        .stdout(predicate::str::contains("Options:"));
}

#[test]
fn clap_help_uses_japanese_for_commands_arguments_and_headings() {
    let mut command = Command::cargo_bin("pinto").expect("binary builds");
    command
        .args(["add", "--help"])
        .env("LC_ALL", "ja_JP.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("引数:"))
        .stdout(predicate::str::contains("オプション:"))
        .stdout(predicate::str::contains("PBI のタイトル（必須）。"));
}

#[test]
fn clap_help_localizes_variable_length_label_setting_options() {
    for subcommand in ["add", "edit"] {
        let mut command = Command::cargo_bin("pinto").expect("binary builds");
        command
            .args([subcommand, "--help"])
            .env("LC_ALL", "ja_JP.UTF-8")
            .assert()
            .success()
            .stdout(predicate::str::contains("--label <LABEL>..."))
            .stdout(predicate::str::contains(
                "1つのオプションに複数値を続けるか、オプションを繰り返し指定できる。",
            ));
    }
}

#[test]
fn cli_uses_japanese_messages_for_a_japanese_locale() {
    let dir = TempDir::new().expect("temp dir");
    let mut command = Command::cargo_bin("pinto").expect("binary builds");

    command
        .current_dir(dir.path())
        .args(["init"])
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "en_US.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("pinto ボードを初期化しました:"));
}

/// Regression guard for the localization path: `localize_command`'s `mut_args`
/// may replace help text, but must preserve subcommand aliases and short-option syntax.
#[test]
fn localized_help_preserves_aliases_and_short_options() {
    // Top-level aliases (added here: dep→d, link→ln, dod→dd, migrate→mig, automate→auto).
    let mut top = Command::cargo_bin("pinto").expect("binary builds");
    top.args(["--help"])
        .env("LC_ALL", "ja_JP.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("aliases: d]"))
        .stdout(predicate::str::contains("aliases: ln]"))
        .stdout(predicate::str::contains("aliases: auto]"));

    // Subcommand short options (including board's -o, -f, -j, and -w).
    let mut board = Command::cargo_bin("pinto").expect("binary builds");
    board
        .args(["board", "--help"])
        .env("LC_ALL", "ja_JP.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("-o, --sort"))
        .stdout(predicate::str::contains("-j, --json"))
        .stdout(predicate::str::contains("-w, --no-wip-check"));

    let mut automate = Command::cargo_bin("pinto").expect("binary builds");
    automate
        .args(["automate", "--help"])
        .env("LC_ALL", "ja_JP.UTF-8")
        .assert()
        .success()
        .stdout(predicate::str::contains("自動化プランの JSON Schema"));
}

fn japanese_pinto(dir: &Path) -> Command {
    let mut command = Command::cargo_bin("pinto").expect("binary builds");
    command
        .current_dir(dir)
        .env("LC_ALL", "ja_JP.UTF-8")
        .env("LANG", "ja_JP.UTF-8");
    command
}

fn ftl_keys(resource: &str) -> BTreeSet<String> {
    resource
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim().to_string()))
        .filter(|key| !key.is_empty() && !key.starts_with('#'))
        .collect()
}

#[test]
fn english_and_japanese_catalogs_have_identical_keys() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let english = fs::read_to_string(root.join("locales/en-US.ftl")).expect("English catalog");
    let japanese = fs::read_to_string(root.join("locales/ja-JP.ftl")).expect("Japanese catalog");

    assert_eq!(ftl_keys(&english), ftl_keys(&japanese));
}

#[test]
fn pinto_owned_cli_messages_follow_the_japanese_locale() {
    let dir = TempDir::new().expect("temp dir");
    japanese_pinto(dir.path()).arg("init").assert().success();
    japanese_pinto(dir.path())
        .args(["add", "Prerequisite"])
        .assert()
        .success();
    japanese_pinto(dir.path())
        .args(["add", "Dependent"])
        .assert()
        .success();

    japanese_pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-2 は T-1 に依存します。"));
    japanese_pinto(dir.path())
        .args(["dep", "add", "T-1", "T-2"])
        .assert()
        .success()
        .stderr(predicate::str::contains("警告:"));

    japanese_pinto(dir.path())
        .args(["link", "add", "T-1", "abc12345"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1: リンクしました abc12345"));

    japanese_pinto(dir.path())
        .args(["dod", "set", "- [ ] tests pass"])
        .assert()
        .success()
        .stdout(predicate::str::contains("共通 DoD を更新しました。"));
    japanese_pinto(dir.path())
        .args(["dod", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("共通 DoD を解除しました。"));

    japanese_pinto(dir.path())
        .args(["migrate", "--to", "file"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "file を使用中です。移行は不要です。",
        ));

    japanese_pinto(dir.path())
        .args(["add", ""])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "エラー: タイトルは空にできません。",
        ));
}

#[test]
fn localized_cli_keeps_json_output_machine_readable() {
    let dir = TempDir::new().expect("temp dir");
    japanese_pinto(dir.path()).arg("init").assert().success();
    japanese_pinto(dir.path())
        .args(["add", "日本語の項目"])
        .assert()
        .success();

    let output = japanese_pinto(dir.path())
        .args(["list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let items: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON output");
    assert_eq!(items[0]["title"], "日本語の項目");
}

#[test]
fn japanese_locale_preserves_external_parse_diagnostics() {
    let dir = TempDir::new().expect("temp dir");
    japanese_pinto(dir.path()).arg("init").assert().success();
    fs::write(dir.path().join(".pinto/config.toml"), "columns = = broken").expect("corrupt config");

    let stderr = japanese_pinto(dir.path())
        .arg("board")
        .assert()
        .failure()
        .code(1)
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&stderr);
    assert!(stderr.contains("config.toml"));
    assert!(stderr.contains("expected") || stderr.contains("invalid"));
}

#[test]
fn cli_commands_do_not_keep_known_pinto_owned_messages_inline() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let commands = fs::read_to_string(root.join("src/cli/commands.rs")).expect("commands source");
    for phrase in [
        "now depends on",
        "introduces a cycle",
        "already linked (no change)",
        "no matching commit to unlink",
        "No new commits linked.",
        "No common DoD set.",
        "Updated common DoD.",
        "Cleared common DoD.",
        "No common DoD to clear.",
        "Migrated {items} item(s)",
        "Storage backend is now",
        "Already using {backend}",
        "item(s) are in columns not defined",
        "WIP limit exceeded in",
        "Ranks already balanced",
        "Would rebalance",
        "Rebalanced {}/{} item(s)",
        "daily-hours, holidays, and deduction-factor",
    ] {
        assert!(
            !commands.contains(phrase),
            "pinto-owned text remains inline: {phrase}"
        );
    }
}

fn run_locale_command(dir: &Path, locale: &str, args: &[&str]) -> std::process::Output {
    let mut command = Command::cargo_bin("pinto").expect("binary builds");
    command
        .current_dir(dir)
        .args(args)
        .env("LC_ALL", locale)
        .env("LANG", locale);
    command.output().expect("run localized CLI command")
}

fn assert_locale_command(dir: &Path, locale: &str, args: &[&str], success: bool) {
    let output = run_locale_command(dir, locale, args);
    assert_eq!(
        output.status.success(),
        success,
        "unexpected status for {locale} {args:?}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.contains("error-") && !output.contains("dependency-cycle-warning"),
        "catalog fallback leaked for {locale} {args:?}: {output}"
    );
}

#[test]
fn representative_subcommands_work_in_both_supported_locales() {
    for locale in ["en_US.UTF-8", "ja_JP.UTF-8"] {
        let dir = TempDir::new().expect("temp dir");
        let commands: &[(&[&str], bool)] = &[
            (&["init"], true),
            (&["init"], true),
            (&["add", "First"], true),
            (&["add", "Second"], true),
            (&["list"], true),
            (&["list", "--json"], true),
            (&["export", "--json"], true),
            (&["show", "T-1"], true),
            (&["move", "T-1", "in-progress"], true),
            (&["reorder", "T-1", "--top"], true),
            (&["edit", "T-1", "--title", "Updated"], true),
            (&["dep", "add", "T-2", "T-1"], true),
            (&["dep", "rm", "T-2", "T-1"], true),
            (&["link", "add", "T-1", "abc123"], true),
            (&["link", "rm", "T-1", "abc123"], true),
            (&["dod", "set", "- [ ] reviewed"], true),
            (&["dod"], true),
            (&["dod", "clear"], true),
            (&["sprint", "new", "S-1", "Sprint", "--goal", "Ship"], true),
            (&["sprint", "list"], true),
            (&["sprint", "capacity", "S-1"], false),
            (&["sprint", "add", "S-1", "T-2"], true),
            (&["sprint", "start", "S-1"], true),
            (&["sprint", "close", "S-1"], true),
            (&["sprint", "unassign", "S-1", "T-2"], true),
            (&["sprint", "velocity"], true),
            (&["sprint", "burndown", "S-1"], false),
            (&["board"], true),
            (&["cycletime"], true),
            (&["rebalance"], true),
            (&["migrate", "--to", "file"], true),
            (
                &[
                    "automate",
                    "--dry-run",
                    "--plan",
                    r#"{"commands":[["add","Dry run"]]}"#,
                ],
                true,
            ),
            (&["completion", "bash"], true),
        ];
        for (args, success) in commands {
            assert_locale_command(dir.path(), locale, args, *success);
        }
    }
}
