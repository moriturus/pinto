//! Definition of Done commands and output.

use super::common::*;

#[test]
fn dod_show_reports_unset_by_default() {
    // `dod` succeeds when the shared DoD is unset and explains how to configure one.
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("dod")
        .assert()
        .success()
        .stdout(predicate::str::contains("No common DoD"));
}

#[test]
fn dod_set_then_show_roundtrips_and_persists_plain_markdown() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["dod", "set", "- [ ] tests pass\n- [ ] reviewed"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated common DoD"));

    pinto(dir.path())
        .arg("dod")
        .assert()
        .success()
        .stdout(predicate::str::contains("- [ ] tests pass"))
        .stdout(predicate::str::contains("- [ ] reviewed"));

    // The DoD is stored as a plain Markdown file at `.pinto/dod.md`.
    let dod = std::fs::read_to_string(dir.path().join(".pinto/dod.md")).expect("dod file");
    assert!(!dod.starts_with("+++"), "no frontmatter: {dod:?}");
    assert!(dod.contains("- [ ] reviewed"));
}

#[test]
fn dod_set_empty_is_user_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["dod", "set", "   "])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("empty"));
}

#[test]
fn dod_clear_removes_common_dod() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["dod", "set", "always green"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["dod", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cleared"));
    pinto(dir.path())
        .arg("dod")
        .assert()
        .success()
        .stdout(predicate::str::contains("No common DoD"));

    pinto(dir.path())
        .args(["dod", "clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No common DoD to clear"));
}

#[test]
fn show_includes_common_dod_alongside_acceptance_criteria() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Task", "--body", "item-specific AC line"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dod", "set", "- [ ] integration test exists"])
        .assert()
        .success();

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        // The PBI's Acceptance Criteria (body) and the shared DoD are shown together.
        .stdout(predicate::str::contains("item-specific AC line"))
        .stdout(predicate::str::contains("Definition of Done (common)"))
        .stdout(predicate::str::contains("integration test exists"));
}

#[test]
fn show_without_common_dod_omits_the_section() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path()).args(["add", "Task"]).assert().success();

    pinto(dir.path())
        .args(["show", "T-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Definition of Done").not());
}

#[test]
fn dod_alias_dd_sets_and_shows() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["dd", "set", "- [ ] reviewed"])
        .assert()
        .success();
    pinto(dir.path())
        .arg("dd")
        .assert()
        .success()
        .stdout(predicate::str::contains("reviewed"));
}
