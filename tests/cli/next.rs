//! `next` command integration tests.

use super::common::*;

fn seed_candidates(dir: &std::path::Path) {
    pinto(dir).arg("init").assert().success();
    for title in ["Blocked", "Ready", "Completed", "Started", "Second ready"] {
        pinto(dir).args(["add", title]).assert().success();
    }

    pinto(dir).args(["move", "T-3", "done"]).assert().success();
    pinto(dir)
        .args(["move", "T-4", "in-progress"])
        .assert()
        .success();
    pinto(dir)
        .args(["dep", "add", "T-1", "T-4"])
        .assert()
        .success();
    pinto(dir)
        .args(["dep", "add", "T-2", "T-3"])
        .assert()
        .success();
}

#[test]
fn next_returns_the_highest_ranked_ready_item() {
    let dir = TempDir::new().expect("temp dir");
    seed_candidates(dir.path());

    pinto(dir.path())
        .arg("next")
        .assert()
        .success()
        .stdout(predicate::str::contains("T-2"))
        .stdout(predicate::str::contains("Ready"))
        .stdout(predicate::str::contains("Blocked").not())
        .stdout(predicate::str::contains("Completed").not())
        .stdout(predicate::str::contains("Started").not());

    pinto(dir.path())
        .args(["next", "-n", "2"])
        .assert()
        .success()
        .stdout(predicate::str::is_match("(?s)T-2.*T-5").expect("valid output pattern"));
}

#[test]
fn next_filters_by_sprint_and_supports_json() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["sprint", "new", "S-1", "Current"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["sprint", "new", "S-2", "Other"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Current candidate", "--sprint", "S-1"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Other candidate", "--sprint", "S-2"])
        .assert()
        .success();

    let items = json_stdout(pinto(dir.path()).args(["next", "--sprint", "S-1", "--json"]));
    assert_eq!(items.as_array().expect("next JSON is an array").len(), 1);
    assert_eq!(items[0]["title"], "Current candidate");
}

#[test]
fn next_is_read_only() {
    let dir = TempDir::new().expect("temp dir");
    seed_candidates(dir.path());
    let before = json_stdout(pinto(dir.path()).args(["list", "--json"]));

    pinto(dir.path())
        .args(["next", "-n", "2"])
        .assert()
        .success();

    let after = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    assert_eq!(after, before, "next must not update or reorder board data");
}

#[test]
fn next_uses_the_configured_first_and_done_columns() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    let config_path = dir.path().join(".pinto/config.toml");
    let config = std::fs::read_to_string(&config_path).expect("config");
    std::fs::write(
        &config_path,
        config
            .replace("todo", "backlog")
            .replace("in-progress", "doing")
            .replace("review", "reviewing")
            .replace("\"done\"", "\"closed\""),
    )
    .expect("custom workflow");
    pinto(dir.path())
        .args(["add", "Dependency"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["add", "Candidate"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["move", "T-1", "closed"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["dep", "add", "T-2", "T-1"])
        .assert()
        .success();

    pinto(dir.path())
        .arg("next")
        .assert()
        .success()
        .stdout(predicate::str::contains("Candidate"));
}

#[test]
fn next_rejects_a_zero_count() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["next", "--count", "0"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("must be at least 1"));
}
