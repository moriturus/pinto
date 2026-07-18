//! `doctor` command integration tests.

use super::common::*;

#[test]
fn doctor_reports_a_clean_board_successfully() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("healthy"));
}

#[test]
fn doctor_reports_relationship_state_rank_and_filename_corruption() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["first", "second"]);

    let first_path = dir.path().join(".pinto/tasks/T-1.md");
    let second_path = dir.path().join(".pinto/tasks/T-2.md");
    let first = std::fs::read_to_string(&first_path).expect("first item");
    let second = std::fs::read_to_string(&second_path).expect("second item");
    let first = first
        .replace("status = \"todo\"", "status = \"unknown\"")
        .replace(
            "updated =",
            "parent = \"T-2\"\ndepends_on = [\"T-2\"]\nupdated =",
        );
    let second = second.replace("rank = \"j\"", "rank = \"i0\"").replace(
        "updated =",
        "parent = \"T-1\"\ndepends_on = [\"T-1\"]\nsprint = \"missing\"\nupdated =",
    );
    std::fs::write(dir.path().join(".pinto/tasks/wrong-name.md"), first).expect("write first");
    std::fs::remove_file(first_path).expect("rename first");
    std::fs::write(second_path, second).expect("write second");

    pinto(dir.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("parent cycle"))
        .stdout(predicate::str::contains("dependency cycle"))
        .stdout(predicate::str::contains("invalid workflow state"))
        .stdout(predicate::str::contains("rank anomaly"))
        .stdout(predicate::str::contains("dangling sprint"))
        .stdout(predicate::str::contains("filename mismatch"));
}

#[test]
fn doctor_fix_repairs_only_unambiguous_filename_and_issued_history() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["first"]);
    let item_path = dir.path().join(".pinto/tasks/T-1.md");
    let renamed_path = dir.path().join(".pinto/tasks/renamed.md");
    std::fs::rename(item_path, &renamed_path).expect("rename item");
    std::fs::write(dir.path().join(".pinto/issued_ids"), "").expect("clear history");

    pinto(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fixed:"))
        .stdout(predicate::str::contains("healthy"));
    assert!(dir.path().join(".pinto/tasks/T-1.md").exists());
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".pinto/issued_ids")).expect("history"),
        "T-1\n"
    );
}

#[test]
fn doctor_reports_duplicate_ids_collisions_and_issued_history_errors() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["first", "second"]);
    pinto(dir.path()).args(["rm", "T-2"]).assert().success();
    std::fs::copy(
        dir.path().join(".pinto/tasks/T-1.md"),
        dir.path().join(".pinto/archive/T-1.md"),
    )
    .expect("copy colliding item");
    std::fs::write(
        dir.path().join(".pinto/issued_ids"),
        "T-1\nT-1\nnot-an-id\n",
    )
    .expect("corrupt issued history");

    pinto(dir.path())
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("duplicate ID"))
        .stdout(predicate::str::contains("storage collision"))
        .stdout(predicate::str::contains("issued ID history"));
}
