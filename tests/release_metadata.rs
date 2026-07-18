#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::{TempDir, tempdir};

const CHECKER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/check-release-metadata.sh"
);

fn write_fixture_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn run_git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git command failed: {}\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fixture() -> TempDir {
    let root = tempdir().expect("create fixture directory");
    write_fixture_file(
        root.path(),
        "Cargo.toml",
        "[package]\nname = \"pinto-cli\"\nversion = \"0.2.0\"\n",
    );
    for lockfile in ["Cargo.lock", "fuzz/Cargo.lock"] {
        write_fixture_file(
            root.path(),
            lockfile,
            "[[package]]\nname = \"pinto-cli\"\nversion = \"0.2.0\"\n",
        );
    }
    write_fixture_file(
        root.path(),
        "README.md",
        "The latest published release is `0.2.0`.\n\ncargo install pinto-cli --version 0.2.0\n",
    );
    write_fixture_file(
        root.path(),
        "docs/book/src/installation.md",
        "Install the latest published 0.2.0 binary with Cargo:\n\ncargo install pinto-cli --version 0.2.0\n",
    );
    write_fixture_file(
        root.path(),
        "CHANGELOG.md",
        "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-07-17\n",
    );
    write_fixture_file(
        root.path(),
        "docs/stability.md",
        "## SQLite schema v1 to v2 compatibility\n\n### Affected users\n\nUsers with a version 1 database are affected.\n\n### Symptoms\n\nThe version 2 binary reports an unsupported schema error.\n\n### Back up before upgrading\n\nBack up the database before upgrading.\n\n### Downgrade and recovery\n\nDowngrade to a version 1 binary or recover through the file backend.\n",
    );

    run_git(root.path(), &["init", "--quiet"]);
    run_git(
        root.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    run_git(root.path(), &["config", "user.name", "Release test"]);
    run_git(root.path(), &["add", "."]);
    run_git(root.path(), &["commit", "--quiet", "-m", "fixture"]);
    run_git(root.path(), &["tag", "0.2.0"]);
    root
}

fn check(root: &Path) -> Output {
    Command::new("sh")
        .arg(CHECKER)
        .args(["--root", root.to_str().expect("fixture path is UTF-8")])
        .output()
        .expect("run release metadata checker")
}

fn diagnostics(output: &Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn release_metadata_checker_accepts_a_consistent_unreleased_tree() {
    let root = fixture();
    let output = check(root.path());

    assert!(
        output.status.success(),
        "consistent fixture was rejected:\n{}",
        diagnostics(&output)
    );
}

#[test]
fn release_metadata_checker_reports_each_contract_mismatch() {
    let cases: [(&str, &str, &str); 8] = [
        ("package version", "Cargo.toml", "version = \"0.3.0\""),
        ("root lockfile", "Cargo.lock", "version = \"0.3.0\""),
        ("fuzz lockfile", "fuzz/Cargo.lock", "version = \"0.3.0\""),
        (
            "installation example",
            "README.md",
            "cargo install pinto-cli --version 0.3.0",
        ),
        (
            "changelog heading",
            "CHANGELOG.md",
            "## [0.3.0] - 2026-07-17",
        ),
        (
            "future changelog heading",
            "CHANGELOG.md",
            "## [0.3.0] - 2026-07-18",
        ),
        (
            "compatibility documentation",
            "docs/stability.md",
            "### Symptoms\n\n",
        ),
        ("release tag", "TAG:0.2.0", "0.3.0"),
    ];

    for (label, target, replacement) in cases {
        let root = fixture();
        if let Some(tag) = target.strip_prefix("TAG:") {
            run_git(root.path(), &["tag", "--delete", tag]);
            run_git(root.path(), &["tag", replacement]);
        } else {
            let path = root.path().join(target);
            let contents = fs::read_to_string(&path).expect("read fixture file");
            let updated = if label == "compatibility documentation" {
                contents.replace("### Symptoms\n\n", "")
            } else if label == "changelog heading" {
                contents.replace("## [0.2.0] - 2026-07-17", replacement)
            } else if label == "future changelog heading" {
                contents.replace(
                    "## [0.2.0] - 2026-07-17",
                    "## [0.3.0] - 2026-07-18\n\n## [0.2.0] - 2026-07-17",
                )
            } else if label == "installation example" {
                contents.replace("cargo install pinto-cli --version 0.2.0", replacement)
            } else {
                contents.replace("version = \"0.2.0\"", replacement)
            };
            fs::write(path, updated).expect("update fixture file");
        }

        let output = check(root.path());
        let message = diagnostics(&output);
        let expected_label = if label == "future changelog heading" {
            "changelog heading"
        } else {
            label
        };
        assert!(!output.status.success(), "{label} mismatch was accepted");
        assert!(
            message.to_lowercase().contains(expected_label),
            "{label} mismatch was not diagnosed:\n{message}"
        );
    }
}
