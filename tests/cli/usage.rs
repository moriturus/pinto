//! CLI usage surface: help, version, aliases, and argument errors.

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
