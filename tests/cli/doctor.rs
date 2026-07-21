//! `doctor` command integration tests.

use super::common::*;

fn rewrite_item(path: &Path, replacements: &[(&str, &str)]) {
    let mut text = std::fs::read_to_string(path).expect("read item fixture");
    for (from, to) in replacements {
        text = text.replace(from, to);
    }
    std::fs::write(path, text).expect("rewrite item fixture");
}

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

#[test]
fn doctor_fix_renumbers_duplicate_ids_within_active_tasks() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Canonical", "Existing maximum"]);
    let duplicate = dir.path().join(".pinto/tasks/z-merge-copy.md");
    std::fs::copy(dir.path().join(".pinto/tasks/T-1.md"), &duplicate).expect("copy duplicate task");
    rewrite_item(
        &duplicate,
        &[
            ("title = \"Canonical\"", "title = \"Merged copy\""),
            ("rank = \"i\"", "rank = \"k\""),
        ],
    );

    pinto(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renumbered T-1 as T-3"))
        .stdout(predicate::str::contains("healthy"));

    let items = json_stdout(pinto(dir.path()).args(["list", "--json"]));
    let merged = items
        .as_array()
        .expect("item list")
        .iter()
        .find(|item| item["title"] == "Merged copy")
        .expect("renumbered merge copy");
    assert_eq!(merged["id"], "T-3");
    assert!(
        std::fs::read_to_string(dir.path().join(".pinto/issued_ids"))
            .expect("issued history")
            .lines()
            .any(|line| line == "T-3")
    );
}

#[test]
fn doctor_fix_normalizes_the_surviving_duplicate_filename_in_one_run() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Canonical content", "Existing maximum"]);
    let first = dir.path().join(".pinto/tasks/a-merge-copy.md");
    std::fs::rename(dir.path().join(".pinto/tasks/T-1.md"), &first).expect("rename canonical task");
    let duplicate = dir.path().join(".pinto/tasks/z-merge-copy.md");
    std::fs::copy(&first, &duplicate).expect("copy duplicate task");
    rewrite_item(
        &duplicate,
        &[
            (
                "title = \"Canonical content\"",
                "title = \"Renumbered content\"",
            ),
            ("rank = \"i\"", "rank = \"k\""),
        ],
    );

    pinto(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renumbered T-1 as T-3"))
        .stdout(predicate::str::contains("renamed"))
        .stdout(predicate::str::contains("healthy"));

    assert!(dir.path().join(".pinto/tasks/T-1.md").is_file());
    assert!(dir.path().join(".pinto/tasks/T-3.md").is_file());
    assert!(!first.exists());
    assert!(!duplicate.exists());
}

#[test]
fn doctor_fix_renumbers_duplicate_ids_within_the_archive() {
    let dir = TempDir::new().expect("temp dir");
    init_with_items(dir.path(), &["Canonical archive", "Existing maximum"]);
    pinto(dir.path()).args(["rm", "T-1"]).assert().success();
    let duplicate = dir.path().join(".pinto/archive/z-merge-copy.md");
    std::fs::copy(dir.path().join(".pinto/archive/T-1.md"), &duplicate)
        .expect("copy duplicate archive item");
    rewrite_item(
        &duplicate,
        &[(
            "title = \"Canonical archive\"",
            "title = \"Merged archive copy\"",
        )],
    );

    pinto(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renumbered T-1 as T-3"))
        .stdout(predicate::str::contains("healthy"));

    let items = json_stdout(pinto(dir.path()).args(["list", "--archived", "--json"]));
    let merged = items
        .as_array()
        .expect("archived item list")
        .iter()
        .find(|item| item["title"] == "Merged archive copy")
        .expect("renumbered archived merge copy");
    assert_eq!(merged["id"], "T-3");
}

#[test]
fn doctor_fix_repairs_cross_area_merge_lineages_and_relationships() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();
    pinto(dir.path())
        .args(["add", "Canonical parent"])
        .assert()
        .success();
    pinto(dir.path())
        .args([
            "add",
            "Canonical child",
            "--parent",
            "T-1",
            "--depends-on",
            "T-1",
        ])
        .assert()
        .success();

    let parent_copy = dir.path().join(".pinto/archive/z-parent-merge-copy.md");
    std::fs::create_dir_all(dir.path().join(".pinto/archive")).expect("create archive dir");
    std::fs::copy(dir.path().join(".pinto/tasks/T-1.md"), &parent_copy)
        .expect("copy duplicate parent");
    rewrite_item(
        &parent_copy,
        &[("title = \"Canonical parent\"", "title = \"Merged parent\"")],
    );
    let child_copy = dir.path().join(".pinto/archive/z-child-merge-copy.md");
    std::fs::copy(dir.path().join(".pinto/tasks/T-2.md"), &child_copy)
        .expect("copy duplicate child");
    rewrite_item(
        &child_copy,
        &[("title = \"Canonical child\"", "title = \"Merged child\"")],
    );

    pinto(dir.path())
        .args(["doctor", "--fix"])
        .assert()
        .success()
        .stdout(predicate::str::contains("renumbered T-1 as T-3"))
        .stdout(predicate::str::contains("renumbered T-2 as T-4"))
        .stdout(predicate::str::contains("healthy"));

    let archived = json_stdout(pinto(dir.path()).args(["list", "--archived", "--json"]));
    let merged_child = archived
        .as_array()
        .expect("archived item list")
        .iter()
        .find(|item| item["title"] == "Merged child")
        .expect("renumbered merged child");
    assert_eq!(merged_child["id"], "T-4");
    assert_eq!(merged_child["parent"], "T-3");
    assert_eq!(merged_child["depends_on"], serde_json::json!(["T-3"]));
}
