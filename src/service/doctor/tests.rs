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

#[test]
fn doctor_flags_action_sources_that_lost_their_sprint_or_record() {
    use crate::sprint_record::SprintRecordKind;
    use std::collections::HashSet;

    let (valid, _) = item_record(
        "T-1.md",
        RecordArea::Tasks,
        "id = \"T-1\"\ntitle = \"Ok\"\nstatus = \"todo\"\nrank = \"i\"\n[source]\nkind = \"retro\"\nsprint_id = \"S-1\"",
    );
    let (missing_sprint, _) = item_record(
        "T-2.md",
        RecordArea::Archive,
        "id = \"T-2\"\ntitle = \"Dangling sprint\"\nstatus = \"todo\"\nrank = \"j\"\n[source]\nkind = \"retro\"\nsprint_id = \"S-9\"",
    );
    let (missing_record, _) = item_record(
        "T-3.md",
        RecordArea::Tasks,
        "id = \"T-3\"\ntitle = \"Dangling record\"\nstatus = \"todo\"\nrank = \"k\"\n[source]\nkind = \"review\"\nsprint_id = \"S-1\"",
    );
    let (invalid_kind, _) = item_record(
        "T-4.md",
        RecordArea::Tasks,
        "id = \"T-4\"\ntitle = \"Bad kind\"\nstatus = \"todo\"\nrank = \"l\"\n[source]\nkind = \"note\"\nsprint_id = \"S-1\"",
    );

    let sprints = vec![sprint_record("S-1.md", "id = \"S-1\"\nstate = \"planned\"")];
    // Only a Retro exists for S-1; the Review that T-3 sources is absent.
    let mut child_records = HashSet::new();
    child_records.insert((SprintRecordKind::Retro, "S-1".to_string()));

    let records = vec![valid, missing_sprint, missing_record, invalid_kind];
    let issues = analyze_action_sources(&records, &sprints, &child_records);

    // The valid retro-sourced PBI produces no issue; the other three each produce exactly one.
    assert_eq!(issues.len(), 3, "got {issues:?}");
    assert!(
        issues
            .iter()
            .all(|issue| issue.kind == DoctorIssueKind::DanglingSprint)
    );
    assert!(
        issues.iter().any(|issue| issue.location.contains("T-2.md")
            && issue.detail.contains("missing sprint S-9"))
    );
    assert!(
        issues.iter().any(|issue| issue.location.contains("T-3.md")
            && issue.detail.contains("missing Review S-1"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.location.contains("T-4.md")
                && issue.detail.contains("kind is invalid"))
    );
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

#[cfg(feature = "sqlite")]
fn sqlite_item(number: u32) -> BacklogItem {
    use chrono::Utc;
    BacklogItem::new(
        ItemId::new("T", number),
        "SQLite item",
        crate::backlog::Status::new("todo"),
        Rank::parse("i").expect("rank"),
        Utc::now(),
    )
    .expect("item")
}

#[cfg(feature = "sqlite")]
#[test]
fn analyze_records_flags_duplicate_ranks_among_sqlite_active_items() {
    // Two SQLite active PBIs share the same status, parent, and rank. File and Git report this as a
    // RankAnomaly, and the SQLite active store must join that scope so the three backends agree.
    let board_dir = PathBuf::from(".pinto");
    let first = RawItemRecord::from_item(&board_dir, sqlite_item(1), false);
    let second = RawItemRecord::from_item(&board_dir, sqlite_item(2), false);
    let config = Config::default();
    let issues = analyze_records(&[first, second], &[], &config);
    assert!(
        has_issue_kind(&issues, DoctorIssueKind::RankAnomaly),
        "duplicate ranks among active SQLite PBIs must be a RankAnomaly: {issues:?}"
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn analyze_records_ignores_duplicate_ranks_among_sqlite_archived_items() {
    // doctor does not require rank uniqueness among archived PBIs, so two archived SQLite PBIs that
    // share a rank must not be flagged — matching the file and Git archives.
    let board_dir = PathBuf::from(".pinto");
    let first = RawItemRecord::from_item(&board_dir, sqlite_item(1), true);
    let second = RawItemRecord::from_item(&board_dir, sqlite_item(2), true);
    let config = Config::default();
    let issues = analyze_records(&[first, second], &[], &config);
    assert!(
        !has_issue_kind(&issues, DoctorIssueKind::RankAnomaly),
        "duplicate ranks among archived SQLite PBIs must not be flagged: {issues:?}"
    );
}

#[cfg(feature = "sqlite")]
#[test]
fn analyze_sprints_flags_a_sqlite_goal_outcome_without_a_goal() {
    use chrono::Utc;
    // A SQLite row can pair `goal_achieved` with a blank Goal: the row mapper returns it raw, and
    // doctor keeps it raw (unlike the normalizing read paths). doctor must report it so its health
    // boundary matches what `import` rejects, on SQLite as on File and Git.
    let mut sprint = Sprint::new(
        SprintId::new("S-1").expect("sprint ID"),
        "Sprint",
        Utc::now(),
    )
    .expect("sprint");
    sprint.goal = String::new();
    sprint.goal_achieved = Some(true);
    let record = RawSprintRecord::from_sprint(sprint);
    let issues = analyze_sprints(&[record]);
    assert!(
        issues
            .iter()
            .any(|issue| issue.kind == DoctorIssueKind::MalformedRecord
                && issue
                    .detail
                    .contains("records a goal outcome but has no goal")),
        "a raw SQLite Goal outcome without a Goal must be flagged: {issues:?}"
    );
}

#[cfg(feature = "sqlite")]
#[tokio::test]
async fn doctor_flags_a_sqlite_goal_outcome_without_a_goal_end_to_end() {
    use crate::storage::{SprintRepository, SqliteRepository};
    use chrono::Utc;

    let dir = TempDir::new().expect("temp dir");
    crate::service::init_board(dir.path())
        .await
        .expect("initialize board");
    let board_dir = dir.path().join(".pinto");
    let repository = SqliteRepository::new(board_dir.clone());

    // Save a Sprint that legitimately records a Goal outcome, so the outcome row exists alongside a
    // non-blank Goal.
    let mut sprint = Sprint::new(
        SprintId::new("S-1").expect("sprint ID"),
        "Sprint",
        Utc::now(),
    )
    .expect("sprint");
    sprint.goal = "Ship it".to_string();
    sprint.goal_achieved = Some(true);
    SprintRepository::save(&repository, &sprint)
        .await
        .expect("save sprint");

    // Blank the Goal directly in the database, leaving the recorded outcome behind — the exact
    // hand-edited corruption `import` rejects. The normal read paths would clear the outcome, but
    // doctor must report it instead of calling the board healthy.
    let connection = rusqlite::Connection::open(board_dir.join("board.sqlite3")).expect("open db");
    connection
        .execute("UPDATE sprints SET goal = '' WHERE id = 'S-1'", [])
        .expect("blank the goal");
    drop(connection);

    let mut config = Config::default();
    config.storage.backend = StorageBackend::Sqlite;
    let backend = Backend::Sqlite(repository);
    let inspection = inspect_board(&board_dir, &backend, &config)
        .await
        .expect("inspect SQLite board");
    assert!(
        inspection
            .issues
            .iter()
            .any(|issue| issue.kind == DoctorIssueKind::MalformedRecord
                && issue
                    .detail
                    .contains("records a goal outcome but has no goal")),
        "the end-to-end SQLite read path must surface the raw Goal outcome: {:?}",
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

/// Depth that overflows the default test-thread stack under naive recursion,
/// so cycle inspection must run on an explicit heap stack. See P-50.
const DEEP_GRAPH: usize = 100_000;

#[test]
fn deep_acyclic_chain_reports_no_cycle_without_a_stack_overflow() {
    // T-1 -> T-2 -> ... -> T-DEEP, no back edge.
    let edges: BTreeMap<String, BTreeSet<String>> = (1..DEEP_GRAPH)
        .map(|n| (format!("T-{n}"), BTreeSet::from([format!("T-{}", n + 1)])))
        .collect();

    assert!(
        graph_cycles(&edges).is_empty(),
        "a straight chain has no cycle"
    );
}

#[test]
fn deep_chain_closing_into_a_cycle_is_detected_without_a_stack_overflow() {
    // T-1 -> T-2 -> ... -> T-DEEP -> T-1: one cycle spanning every node.
    let edges: BTreeMap<String, BTreeSet<String>> = (1..=DEEP_GRAPH)
        .map(|n| {
            let next = if n == DEEP_GRAPH { 1 } else { n + 1 };
            (format!("T-{n}"), BTreeSet::from([format!("T-{next}")]))
        })
        .collect();

    let cycles = graph_cycles(&edges);

    assert_eq!(cycles.len(), 1, "exactly one cycle");
    assert_eq!(cycles[0].len(), DEEP_GRAPH, "the cycle spans every node");
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
