use super::*;
#[cfg(feature = "sqlite")]
use crate::config::StorageBackend;
#[cfg(feature = "sqlite")]
use crate::rank::Rank;
use crate::storage::{item_issued_ids_path, record_issued_id};
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;
use tokio::fs;

fn item_record(
    path: impl Into<PathBuf>,
    area: RecordArea,
    fields: &str,
) -> (RawItemRecord, String) {
    let path = path.into();
    let text = format!("+++\n{fields}\n+++\n");
    (RawItemRecord::from_document(path, area, text.clone()), text)
}

fn sprint_record(path: impl Into<PathBuf>, fields: &str) -> RawSprintRecord {
    let path = path.into();
    RawSprintRecord::from_document(path, format!("+++\n{fields}\n+++\n"))
}

fn has_issue_kind(issues: &[DoctorIssue], kind: DoctorIssueKind) -> bool {
    issues.iter().any(|issue| issue.kind == kind)
}

#[test]
fn doctor_classifies_malformed_documents_and_relationships() {
    let malformed = RawItemRecord::from_document(
        PathBuf::from("broken.md"),
        RecordArea::Tasks,
        "not frontmatter".to_string(),
    );
    let invalid_toml = RawItemRecord::from_document(
        PathBuf::from("invalid-toml.md"),
        RecordArea::Tasks,
        "+++\nid = [\n+++\n".to_string(),
    );
    let non_table = RawItemRecord::from_document(
        PathBuf::from("non-table.md"),
        RecordArea::Tasks,
        "+++\n[\"not a table\"]\n+++\n".to_string(),
    );
    let (missing_fields, _) = item_record("T-2.md", RecordArea::Tasks, "id = \"T-2\"");
    let (invalid_fields, _) = item_record(
        "T-3.md",
        RecordArea::Tasks,
        "id = \"T-3\"\ntitle = 3\nstatus = 4\nrank = 5\nsprint = 6\nparent = 7\ndepends_on = [8]",
    );
    let (dangling, _) = item_record(
        "T-4.md",
        RecordArea::Tasks,
        "id = \"T-4\"\ntitle = \" \"\nstatus = \"unknown\"\nrank = \"i\"\nsprint = \"S-missing\"\nparent = \"T-99\"\ndepends_on = [\"T-99\", \"not-an-id\"]",
    );
    let (invalid_id, _) = item_record(
        "invalid.md",
        RecordArea::Tasks,
        "id = \"not/an/id\"\ntitle = \"Invalid ID\"\nstatus = \"todo\"\nrank = \"j\"",
    );
    let (missing_id, _) = item_record(
        "missing-id.md",
        RecordArea::Tasks,
        "title = \"Missing ID\"\nstatus = \"todo\"\nrank = \"k\"",
    );
    let (cycle_one, _) = item_record(
        "wrong-name.md",
        RecordArea::Tasks,
        "id = \"T-6\"\ntitle = \"Cycle one\"\nstatus = \"todo\"\nrank = \"l\"\nparent = \"T-7\"\ndepends_on = [\"T-7\"]",
    );
    let (cycle_two, _) = item_record(
        "T-7.md",
        RecordArea::Tasks,
        "id = \"T-7\"\ntitle = \"Cycle two\"\nstatus = \"todo\"\nrank = \"m\"\nparent = \"T-6\"\ndepends_on = [\"T-6\"]",
    );
    let (rank_one, _) = item_record(
        "T-8.md",
        RecordArea::Tasks,
        "id = \"T-8\"\ntitle = \"Rank one\"\nstatus = \"todo\"\nrank = \"n\"",
    );
    let (rank_two, _) = item_record(
        "T-9.md",
        RecordArea::Tasks,
        "id = \"T-9\"\ntitle = \"Rank two\"\nstatus = \"todo\"\nrank = \"n\"",
    );
    let (duplicate_task, _) = item_record(
        "T-5.md",
        RecordArea::Tasks,
        "id = \"T-5\"\ntitle = \"Task copy\"\nstatus = \"todo\"\nrank = \"o\"",
    );
    let (duplicate_archive, _) = item_record(
        "T-5.md",
        RecordArea::Archive,
        "id = \"T-5\"\ntitle = \"Archive copy\"\nstatus = \"todo\"\nrank = \"p\"",
    );

    let records = vec![
        malformed,
        invalid_toml,
        non_table,
        missing_fields,
        invalid_fields,
        dangling,
        invalid_id,
        missing_id,
        cycle_one,
        cycle_two,
        rank_one,
        rank_two,
        duplicate_task,
        duplicate_archive,
    ];
    let sprints = vec![
        RawSprintRecord::from_document(
            PathBuf::from("broken-sprint.md"),
            "not frontmatter".to_string(),
        ),
        sprint_record("invalid-sprint.toml", "id = ["),
        sprint_record("non-table-sprint.md", "[\"not a table\"]"),
        sprint_record("missing-sprint-id.md", "state = \"planned\""),
        sprint_record(
            "invalid-sprint-id.md",
            "id = \"bad/id\"\nstate = \"planned\"",
        ),
        sprint_record("invalid-state.md", "id = \"S-2\"\nstate = \"broken\""),
        sprint_record("missing-state.md", "id = \"S-3\""),
        sprint_record("typed-state.md", "id = \"S-4\"\nstate = 4"),
        sprint_record("S-1.md", "id = \"S-1\"\nstate = \"planned\""),
        sprint_record("duplicate-sprint.md", "id = \"S-1\"\nstate = \"closed\""),
    ];

    let mut config = Config::default();
    let sprint_issues = analyze_sprints(&sprints);
    let item_issues = analyze_records(&records, &sprints, &config);

    for kind in [
        DoctorIssueKind::MalformedRecord,
        DoctorIssueKind::InvalidStatus,
        DoctorIssueKind::DuplicateId,
    ] {
        assert!(
            has_issue_kind(&sprint_issues, kind),
            "missing Sprint issue {kind:?}"
        );
    }
    for kind in [
        DoctorIssueKind::DanglingDependency,
        DoctorIssueKind::DanglingParent,
        DoctorIssueKind::DanglingSprint,
        DoctorIssueKind::ParentCycle,
        DoctorIssueKind::DependencyCycle,
        DoctorIssueKind::DuplicateId,
        DoctorIssueKind::IssuedId,
        DoctorIssueKind::InvalidStatus,
        DoctorIssueKind::RankAnomaly,
        DoctorIssueKind::Collision,
        DoctorIssueKind::MalformedRecord,
        DoctorIssueKind::Filename,
    ] {
        assert!(
            has_issue_kind(&item_issues, kind),
            "missing item issue {kind:?}"
        );
    }

    // Keep this explicit so the test also exercises the normal workflow lookup used by the
    // rank and status checks rather than relying only on Config's default shape.
    config.columns = vec!["todo".to_string(), "done".to_string()];
    assert!(has_issue_kind(
        &analyze_records(&records, &sprints, &config),
        DoctorIssueKind::InvalidStatus
    ));
}

#[tokio::test]
async fn doctor_safe_fixes_rename_only_unambiguous_records() {
    let dir = TempDir::new().expect("temp dir");
    let board_dir = dir.path().join(".pinto");
    let tasks_dir = board_dir.join("tasks");
    let archive_dir = board_dir.join("archive");
    fs::create_dir_all(&tasks_dir)
        .await
        .expect("tasks directory");
    fs::create_dir_all(&archive_dir)
        .await
        .expect("archive directory");

    let (rename_active, rename_active_text) = item_record(
        tasks_dir.join("renamed.md"),
        RecordArea::Tasks,
        "id = \"T-1\"\ntitle = \"Rename active\"\nstatus = \"todo\"\nrank = \"i\"",
    );
    let (rename_archive, rename_archive_text) = item_record(
        archive_dir.join("archived.md"),
        RecordArea::Archive,
        "id = \"T-2\"\ntitle = \"Rename archive\"\nstatus = \"todo\"\nrank = \"j\"",
    );
    let (destination_exists, destination_exists_text) = item_record(
        tasks_dir.join("source.md"),
        RecordArea::Tasks,
        "id = \"T-3\"\ntitle = \"Destination exists\"\nstatus = \"todo\"\nrank = \"k\"",
    );
    let (other_area_exists, other_area_exists_text) = item_record(
        archive_dir.join("source.md"),
        RecordArea::Archive,
        "id = \"T-4\"\ntitle = \"Other area exists\"\nstatus = \"todo\"\nrank = \"l\"",
    );
    let (duplicate_one, duplicate_one_text) = item_record(
        tasks_dir.join("duplicate-one.md"),
        RecordArea::Tasks,
        "id = \"T-5\"\ntitle = \"Duplicate one\"\nstatus = \"todo\"\nrank = \"m\"",
    );
    let (duplicate_two, duplicate_two_text) = item_record(
        archive_dir.join("duplicate-two.md"),
        RecordArea::Archive,
        "id = \"T-5\"\ntitle = \"Duplicate two\"\nstatus = \"todo\"\nrank = \"n\"",
    );
    let (already_named, already_named_text) = item_record(
        tasks_dir.join("T-6.md"),
        RecordArea::Tasks,
        "id = \"T-6\"\ntitle = \"Already named\"\nstatus = \"todo\"\nrank = \"o\"",
    );
    let (invalid_id, invalid_id_text) = item_record(
        tasks_dir.join("invalid.md"),
        RecordArea::Tasks,
        "id = \"not-an-id\"\ntitle = \"Invalid ID\"\nstatus = \"todo\"\nrank = \"p\"",
    );
    let fixtures = [
        (rename_active, rename_active_text),
        (rename_archive, rename_archive_text),
        (destination_exists, destination_exists_text),
        (other_area_exists, other_area_exists_text),
        (duplicate_one, duplicate_one_text),
        (duplicate_two, duplicate_two_text),
        (already_named, already_named_text),
        (invalid_id, invalid_id_text),
    ];
    for (record, text) in &fixtures {
        fs::write(&record.path, text).await.expect("write fixture");
    }
    fs::write(tasks_dir.join("T-3.md"), "existing destination")
        .await
        .expect("write active destination");
    fs::write(tasks_dir.join("T-4.md"), "existing other-area destination")
        .await
        .expect("write other-area destination");

    let inspection = Inspection {
        records: fixtures.iter().map(|(record, _)| record.clone()).collect(),
        issues: Vec::new(),
        issued: IssuedHistory {
            path: board_dir.join("issued_ids"),
            ids: HashSet::from([ItemId::new("T", 6)]),
            invalid: Vec::new(),
            duplicates: Vec::new(),
        },
    };
    let fixes = apply_safe_fixes(&board_dir, &inspection)
        .await
        .expect("safe fixes succeed");

    assert!(tasks_dir.join("T-1.md").is_file());
    assert!(!tasks_dir.join("renamed.md").exists());
    assert!(archive_dir.join("T-2.md").is_file());
    assert!(!archive_dir.join("archived.md").exists());
    assert!(
        fixes
            .iter()
            .filter(|fix| fix.description.starts_with("renamed "))
            .count()
            == 2
    );
    let history = fs::read_to_string(board_dir.join("issued_ids"))
        .await
        .expect("issued history");
    for id in ["T-1", "T-2", "T-3", "T-4", "T-5"] {
        assert!(history.lines().any(|line| line == id), "missing {id}");
    }
    assert!(!history.lines().any(|line| line == "T-6"));
}

#[tokio::test]
async fn doctor_reads_and_classifies_issued_id_history() {
    let dir = TempDir::new().expect("temp dir");
    let board_dir = dir.path().join(".pinto");
    fs::create_dir_all(&board_dir)
        .await
        .expect("board directory");
    fs::write(item_issued_ids_path(&board_dir), "\nT-1\nT-1\nnot-an-id\n")
        .await
        .expect("issued history");

    let history = read_issued_history(&board_dir)
        .await
        .expect("read issued history");
    assert_eq!(history.ids, HashSet::from([ItemId::new("T", 1)]));
    assert_eq!(history.duplicates, vec![(3, "T-1".to_string())]);
    assert_eq!(history.invalid.len(), 1);
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn doctor_inspects_sqlite_records_through_the_backend() {
    use crate::storage::{BacklogItemRepository, SprintRepository, SqliteRepository};
    use chrono::Utc;

    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let board_dir = dir.path().join(".pinto");
    let repository = SqliteRepository::new(board_dir.clone());
    let now = Utc::now();
    let item = BacklogItem::new(
        ItemId::new("T", 1),
        "SQLite item",
        crate::backlog::Status::new("todo"),
        Rank::parse("i").expect("rank"),
        now,
    )
    .expect("item");
    let sprint =
        Sprint::new(SprintId::new("S-1").expect("sprint ID"), "Sprint", now).expect("sprint");
    BacklogItemRepository::save(&repository, &item)
        .await
        .expect("save item");
    SprintRepository::save(&repository, &sprint)
        .await
        .expect("save sprint");
    record_issued_id(&board_dir, &item.id)
        .await
        .expect("record issued ID");

    let mut config = Config::default();
    config.storage.backend = StorageBackend::Sqlite;
    let backend = Backend::Sqlite(repository);
    let inspection = inspect_board(&board_dir, &backend, &config)
        .await
        .expect("inspect SQLite board");
    assert_eq!(inspection.records.len(), 1);
    assert!(
        inspection.issues.is_empty(),
        "issues: {:?}",
        inspection.issues
    );
}

#[test]
fn graph_cycles_are_reported_once_with_stable_members() {
    let edges = BTreeMap::from([
        (
            "T-1".to_string(),
            BTreeSet::from(["T-2".to_string(), "T-3".to_string()]),
        ),
        ("T-2".to_string(), BTreeSet::from(["T-1".to_string()])),
        ("T-3".to_string(), BTreeSet::from(["T-1".to_string()])),
    ]);

    assert_eq!(
        graph_cycles(&edges),
        vec![
            vec!["T-1".to_string(), "T-2".to_string()],
            vec!["T-1".to_string(), "T-3".to_string()],
        ]
    );
}

async fn doctor_with_counted_inspections(project_dir: &Path, fix: bool) -> (DoctorReport, usize) {
    let (board_dir, backend, config) = open_board(project_dir).await.expect("open board");
    let inspections = std::cell::Cell::new(0usize);
    let report = run_doctor_with(&board_dir, &backend, fix, || {
        inspections.set(inspections.get() + 1);
        inspect_board(&board_dir, &backend, &config)
    })
    .await
    .expect("doctor run");
    (report, inspections.get())
}

async fn write_task_fixture(board_dir: &Path, filename: &str, fields: &str) {
    let path = board_dir.join("tasks").join(filename);
    fs::write(&path, format!("+++\n{fields}\n+++\n"))
        .await
        .expect("write task fixture");
}

#[tokio::test]
async fn doctor_without_fix_inspects_the_board_exactly_once() {
    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");

    let (report, inspections) = doctor_with_counted_inspections(dir.path(), false).await;

    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert!(report.fixes.is_empty());
    assert_eq!(inspections, 1);
}

#[tokio::test]
async fn doctor_fix_inspects_once_when_no_fix_is_applied() {
    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let board_dir = dir.path().join(".pinto");
    write_task_fixture(
        &board_dir,
        "T-1.md",
        "id = \"T-1\"\ntitle = \"Dangling parent\"\nstatus = \"todo\"\nrank = \"i\"\nparent = \"T-99\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"",
    )
    .await;
    record_issued_id(&board_dir, &ItemId::new("T", 1))
        .await
        .expect("record issued ID");

    let (report, inspections) = doctor_with_counted_inspections(dir.path(), true).await;

    assert!(has_issue_kind(
        &report.issues,
        DoctorIssueKind::DanglingParent
    ));
    assert!(report.fixes.is_empty(), "fixes: {:?}", report.fixes);
    assert_eq!(inspections, 1);
}

#[tokio::test]
async fn doctor_fix_reinspects_only_once_after_renumbering_duplicates() {
    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let board_dir = dir.path().join(".pinto");
    let canonical = "id = \"T-1\"\ntitle = \"Canonical\"\nstatus = \"todo\"\nrank = \"i\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"";
    write_task_fixture(&board_dir, "T-1.md", canonical).await;
    write_task_fixture(
        &board_dir,
        "z-copy.md",
        &canonical.replace("rank = \"i\"", "rank = \"j\""),
    )
    .await;
    record_issued_id(&board_dir, &ItemId::new("T", 1))
        .await
        .expect("record issued ID");

    let (report, inspections) = doctor_with_counted_inspections(dir.path(), true).await;

    assert!(
        report
            .fixes
            .iter()
            .any(|fix| fix.description.starts_with("renumbered ")),
        "fixes: {:?}",
        report.fixes
    );
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(inspections, 2);
}

#[tokio::test]
async fn doctor_fix_reinspects_once_after_safe_fixes() {
    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let board_dir = dir.path().join(".pinto");
    write_task_fixture(
        &board_dir,
        "renamed.md",
        "id = \"T-1\"\ntitle = \"Rename me\"\nstatus = \"todo\"\nrank = \"i\"\ncreated = \"1970-01-01T00:00:00Z\"\nupdated = \"1970-01-01T00:00:00Z\"",
    )
    .await;
    record_issued_id(&board_dir, &ItemId::new("T", 1))
        .await
        .expect("record issued ID");

    let (report, inspections) = doctor_with_counted_inspections(dir.path(), true).await;

    assert!(
        report
            .fixes
            .iter()
            .any(|fix| fix.description.starts_with("renamed ")),
        "fixes: {:?}",
        report.fixes
    );
    assert!(report.issues.is_empty(), "issues: {:?}", report.issues);
    assert_eq!(inspections, 2);
}

#[tokio::test]
async fn service_scan_keeps_diagnosing_after_malformed_record() {
    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let path = dir.path().join(".pinto/tasks/broken.md");
    fs::write(&path, "this is not frontmatter")
        .await
        .expect("write malformed item");

    let report = doctor(dir.path(), false).await.expect("scan board");

    assert!(report.issues.iter().any(|issue| {
        issue.kind == DoctorIssueKind::MalformedRecord
            && std::path::Path::new(&issue.location)
                .file_name()
                .and_then(|name| name.to_str())
                == path.file_name().and_then(|name| name.to_str())
    }));
}
