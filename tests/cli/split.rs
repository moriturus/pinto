//! CLI `split` flows: relationships, body options, and validation.

use super::common::*;

/// Initialize a board and seed a source PBI with a body.
fn board_with_source(dir: &Path, body: &str) {
    pinto(dir).arg("init").assert().success();
    pinto(dir)
        .args(["add", "Source epic", "--body", body])
        .assert()
        .success();
}

#[test]
fn split_copies_source_body_by_default() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "Deliver the whole flow.");

    pinto(dir.path())
        .args(["split", "T-1", "First slice"])
        .assert()
        .success()
        .stdout(predicate::str::contains("T-1"))
        .stdout(predicate::str::contains("T-2"))
        .stdout(predicate::str::contains("First slice"));

    let slice = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(slice["body"], "Deliver the whole flow.");
    assert_eq!(slice["parent"], serde_json::Value::Null);
    assert_eq!(slice["depends_on"], serde_json::json!([]));
}

#[test]
fn split_creates_one_pbi_per_title() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["split", "T-1", "Slice A", "Slice B", "Slice C"])
        .assert()
        .success();

    for (id, title) in [("T-2", "Slice A"), ("T-3", "Slice B"), ("T-4", "Slice C")] {
        let item = show_json(pinto(dir.path()).args(["show", id, "--json"]));
        assert_eq!(item["title"], title);
    }
}

#[test]
fn split_child_relationship_parents_new_pbis_under_source() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["split", "T-1", "Child slice", "--child"])
        .assert()
        .success();

    let child = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(child["parent"], "T-1");

    let source = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(source["children"], serde_json::json!(["T-2"]));
}

#[test]
fn split_dependency_relationship_makes_source_depend_on_new_pbis() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["split", "T-1", "Blocking spike", "--dependency"])
        .assert()
        .success();

    let source = show_json(pinto(dir.path()).args(["show", "T-1", "--json"]));
    assert_eq!(source["depends_on"], serde_json::json!(["T-2"]));

    let spike = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(spike["dependents"], serde_json::json!(["T-1"]));
    assert_eq!(spike["parent"], serde_json::Value::Null);
}

#[test]
fn split_supports_empty_and_explicit_bodies() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "source body");

    pinto(dir.path())
        .args(["split", "T-1", "Empty slice", "--empty"])
        .assert()
        .success();
    pinto(dir.path())
        .args(["split", "T-1", "Explicit slice", "--body", "custom body"])
        .assert()
        .success();

    let empty = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(empty["body"], "");
    let explicit = show_json(pinto(dir.path()).args(["show", "T-3", "--json"]));
    assert_eq!(explicit["body"], "custom body");
}

#[test]
fn split_uses_a_named_item_template_for_the_body() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "source body");
    let template_dir = dir.path().join(".pinto/templates/item");
    std::fs::create_dir_all(&template_dir).expect("create template dir");
    std::fs::write(template_dir.join("spike.md"), "- [ ] investigate\n").expect("write template");

    pinto(dir.path())
        .args(["split", "T-1", "Templated slice", "--template", "spike"])
        .assert()
        .success();

    let templated = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(templated["body"], "- [ ] investigate\n");
}

#[test]
fn split_reports_a_missing_source_as_a_user_error() {
    let dir = TempDir::new().expect("temp dir");
    pinto(dir.path()).arg("init").assert().success();

    pinto(dir.path())
        .args(["split", "T-404", "Slice"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("T-404"));
}

#[test]
fn split_rejects_conflicting_relationship_flags() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["split", "T-1", "Slice", "--child", "--dependency"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--dependency"));
}

#[test]
fn split_rejects_conflicting_body_flags() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["split", "T-1", "Slice", "--empty", "--body", "x"])
        .assert()
        .failure();
}

#[test]
fn split_alias_creates_new_pbis() {
    let dir = TempDir::new().expect("temp dir");
    board_with_source(dir.path(), "body");

    pinto(dir.path())
        .args(["spl", "T-1", "Aliased slice"])
        .assert()
        .success();

    let item = show_json(pinto(dir.path()).args(["show", "T-2", "--json"]));
    assert_eq!(item["title"], "Aliased slice");
}
