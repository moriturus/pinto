//! CLI usage surface: help, version, aliases, and argument errors.

use super::common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

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

    // Invalid argument usage is a user error (exit code 1), distinct from internal error 2.
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

    // `add` requires a title; omitting it is a usage error mapped to exit code 1.
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

    // `--points` is numeric; a non-numeric value is a usage error mapped to exit code 1.
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

    // --help produces the requested output successfully (exit code 0) on stdout.
    pinto(dir.path())
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Usage"));
}

#[test]
fn external_subcommands_dispatch_nested_binaries_and_propagate_exit_codes() {
    let board = TempDir::new().expect("board temp dir");
    let executables = TempDir::new().expect("external executable temp dir");
    install_external_command(
        executables.path(),
        "pinto-team-report",
        "Team report - reports team activity.",
    );

    let path = external_path(executables.path());
    let mut command = pinto(board.path());
    command
        .env("PATH", &path)
        .args(["team", "report", "payload"])
        .assert()
        .success()
        .stdout(predicate::str::contains("received:payload"));

    let mut failing = pinto(board.path());
    failing
        .env("PATH", path)
        .args(["team", "report", "fail"])
        .assert()
        .code(7);

    let project = TempDir::new().expect("explicit project temp dir");
    let project_path = project.path().to_str().expect("UTF-8 project path");
    let mut with_project = pinto(board.path());
    with_project
        .env("PATH", external_path(executables.path()))
        .args(["--dir", project_path, "team", "report", "print-dir"])
        .assert()
        .success()
        .stdout(predicate::str::contains(project_path));
}

#[test]
fn external_subcommands_choose_the_longest_matching_prefix() {
    let board = TempDir::new().expect("board temp dir");
    let executables = TempDir::new().expect("external executable temp dir");
    install_external_command(
        executables.path(),
        "pinto-team",
        "Team - handles team commands.",
    );
    install_external_command(
        executables.path(),
        "pinto-team-report",
        "Team report - reports team activity.",
    );

    pinto(board.path())
        .env("PATH", external_path(executables.path()))
        .args(["team", "report", "payload"])
        .assert()
        .success()
        .stdout(predicate::str::contains("received:payload"));
}

#[test]
fn external_subcommands_forward_argv_and_contract_environment() {
    let board = TempDir::new().expect("board temp dir");
    let executables = TempDir::new().expect("external executable temp dir");
    install_external_command(
        executables.path(),
        "pinto-team-report",
        "Team report - reports team activity.",
    );

    let project = TempDir::new().expect("explicit project temp dir");
    let project_path = project.path().to_str().expect("UTF-8 project path");
    pinto(board.path())
        .env("PATH", external_path(executables.path()))
        .args([
            "--dir",
            project_path,
            "team",
            "report",
            "inspect",
            "value with spaces",
            "--flag",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("args:value with spaces|--flag"))
        .stdout(predicate::str::contains(format!("dir:{project_path}")))
        .stdout(predicate::str::contains(format!(
            "host:{}",
            env!("CARGO_PKG_VERSION")
        )))
        .stdout(predicate::str::contains("contract:1"));
}

#[test]
fn implicit_current_directory_is_not_an_external_search_location() {
    let board = TempDir::new().expect("board temp dir");
    install_external_command(
        board.path(),
        "pinto-team-report",
        "Team report - should not run from the current directory.",
    );

    pinto(board.path())
        .env("PATH", "")
        .args(["team", "report"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("external command"));
}

#[test]
fn help_lists_nested_external_commands_without_a_board() {
    let board = TempDir::new().expect("board temp dir");
    let executables = TempDir::new().expect("external executable temp dir");
    install_external_command(
        executables.path(),
        "pinto-team-report",
        "Team report - reports team activity.",
    );
    install_external_command(
        executables.path(),
        "pinto-add",
        "Add - is provided by the package.",
    );

    let mut command = pinto(board.path());
    let output = command
        .env("PATH", external_path(executables.path()))
        .arg("help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8(output).expect("utf8 help");

    assert!(help.contains("External commands:"));
    assert!(help.contains("team report"));
    assert!(help.contains("Team report - reports team activity."));
    assert!(!help.contains("Add - is provided by the package."));
}

#[test]
fn missing_external_subcommand_is_a_clear_user_error_without_a_panic() {
    let board = TempDir::new().expect("board temp dir");

    pinto(board.path())
        .arg("missing-plugin")
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("external command"));
}

#[test]
fn root_dispatch_prefers_the_official_command_over_a_stale_path_binary() {
    let board = TempDir::new().expect("board temp dir");
    let executables = TempDir::new().expect("external executable temp dir");
    install_external_command(
        executables.path(),
        "pinto-add",
        "Add - delegates the built-in add command.",
    );

    pinto(board.path()).arg("init").assert().success();

    let mut command = pinto(board.path());
    command
        .env("PATH", external_path(executables.path()))
        .args(["add", "delegated"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Created"));

    pinto(board.path())
        .args(["list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("delegated"));
}

fn external_path(directory: &Path) -> std::ffi::OsString {
    let mut paths = vec![directory.to_path_buf()];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).expect("valid external command PATH")
}

fn install_external_command(directory: &Path, name: &str, summary: &str) -> PathBuf {
    let source = directory.join("external-command.rs");
    let body = format!(
        r#"use std::env;

fn main() {{
    match env::args().nth(1).as_deref() {{
        Some("--help") => println!({summary:?}),
        Some("fail") => std::process::exit(7),
        Some("print-dir") => println!("dir:{{}}", env::var("PINTO_DIR").unwrap_or_default()),
        Some("inspect") => {{
            let args = env::args().skip(2).collect::<Vec<_>>().join("|");
            println!("args:{{args}}");
            println!("dir:{{}}", env::var("PINTO_DIR").unwrap_or_default());
            println!("host:{{}}", env::var("PINTO_HOST_VERSION").unwrap_or_default());
            println!(
                "contract:{{}}",
                env::var("PINTO_PLUGIN_CONTRACT_VERSION").unwrap_or_default()
            );
        }},
        Some(argument) => println!("received:{{argument}}"),
        None => println!("received:"),
    }}
}}
"#,
        summary = summary
    );
    fs::write(&source, body).expect("write external command source");

    #[cfg(windows)]
    let path = directory.join(format!("{name}.exe"));
    #[cfg(not(windows))]
    let path = directory.join(name);
    let status = ProcessCommand::new("rustc")
        .args(["--edition", "2024"])
        .arg(&source)
        .arg("-o")
        .arg(&path)
        .status()
        .expect("compile external command");
    assert!(status.success(), "rustc failed for {name}");
    path
}

#[test]
fn new_toplevel_aliases_are_listed_in_help() {
    let dir = TempDir::new().expect("temp dir");
    let out = pinto(dir.path()).arg("--help").assert().success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).expect("utf8");
    for alias in [
        "alias: d]",
        "alias: ln]",
        "alias: dd]",
        "alias: mig]",
        "alias: auto]",
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
        (&["sprint", "--help"], &["alias: rm]", "alias: u]"]),
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

/// Regression guard for P-44: the shared helper must pin a fixed English locale so spawned
/// pinto processes emit English output regardless of the developer's `LC_ALL`/`LANG`. Inspecting
/// the command environment keeps this deterministic across any parent locale, unlike output
/// assertions that only diverge under a Japanese shell.
#[test]
fn helper_pins_english_locale_for_spawned_processes() {
    use std::ffi::OsStr;

    let dir = TempDir::new().expect("temp dir");
    let cmd = pinto(dir.path());
    let english = Some(OsStr::new("en_US.UTF-8"));
    for key in ["LC_ALL", "LANG"] {
        let value = cmd
            .get_envs()
            .find(|(name, _)| *name == OsStr::new(key))
            .map(|(_, value)| value);
        assert_eq!(
            value,
            Some(english),
            "helper should pin {key} to a fixed English locale"
        );
    }
}
