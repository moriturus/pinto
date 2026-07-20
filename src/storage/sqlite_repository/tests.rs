//! Persistence test for `SqliteRepository`. Same behavior as the file backend equivalent test
//! Confirm that SQLite satisfies the same requirements as the file backend, including rank
//! order, NotFound errors, and ID preservation when archiving.

use super::*;
use crate::backlog::{BacklogItem, ItemId, Status};
use crate::rank::Rank;
use crate::sprint::{Sprint, SprintId};
use crate::storage::repository::{BacklogItemRepository, SprintRepository};
use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;

fn ts(secs: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(secs, 0)
        .single()
        .expect("valid timestamp")
}

/// Create a SQLite Repository targeting `.pinto` in the temporary directory.
fn repo() -> (TempDir, SqliteRepository) {
    let dir = TempDir::new().expect("create temp dir");
    let repo = SqliteRepository::new(dir.path().join(".pinto"));
    (dir, repo)
}

/// Generate `count` monotonically increasing ranks in the order of insertion.
fn ranks(count: usize) -> Vec<Rank> {
    let mut out = Vec::with_capacity(count);
    let mut prev: Option<Rank> = None;
    for _ in 0..count {
        let next = Rank::after(prev.as_ref());
        prev = Some(next.clone());
        out.push(next);
    }
    out
}

fn item(n: u32, rank: Rank) -> BacklogItem {
    BacklogItem::new(
        ItemId::new("T", n),
        format!("Item {n}"),
        Status::new("todo"),
        rank,
        ts(1_000),
    )
    .expect("valid item")
}

/// PBI with all fields filled in (including multi-value, optional fields, and body text).
fn full_item() -> BacklogItem {
    let mut it = item(7, Rank::after(None));
    it.title = "Full item".to_string();
    it.status = Status::new("in-progress");
    it.points = Some(5);
    it.labels = vec!["backend".to_string(), "urgent".to_string()];
    it.assignee = Some("alice".to_string());
    it.sprint = Some("S-1".to_string());
    it.parent = Some(ItemId::new("T", 1));
    it.depends_on = vec![ItemId::new("T", 2), ItemId::new("T", 3)];
    it.start_at = Some(ts(2_000));
    it.done_at = Some(ts(3_000));
    it.commits = vec!["abc1234".to_string(), "def5678".to_string()];
    it.body = "Acceptance\n- one\n- two".to_string();
    it.updated = ts(4_000);
    it
}

/// Open DB synchronously for new schema validation (raw access for testing only).
fn raw(repo: &SqliteRepository) -> Connection {
    Connection::open(repo.db_path()).expect("open db")
}

#[tokio::test]
async fn new_database_records_schema_metadata() {
    let (_dir, repo) = repo();
    let items = BacklogItemRepository::list(&repo)
        .await
        .expect("initialize and list");
    assert!(items.is_empty());

    let conn = raw(&repo);
    let version: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .expect("schema version metadata");
    let format: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'format'",
            [],
            |row| row.get(0),
        )
        .expect("format metadata");
    assert_eq!(version, "2");
    assert_eq!(format, "pinto-sqlite");
}

#[tokio::test]
async fn existing_database_with_missing_format_metadata_is_rejected() {
    let (_dir, repo) = repo();
    BacklogItemRepository::list(&repo)
        .await
        .expect("initialize");
    raw(&repo)
        .execute("DELETE FROM metadata WHERE key = 'format'", [])
        .expect("remove format metadata");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("existing database without format metadata must be rejected");
    assert!(err.to_string().contains("missing SQLite format metadata"));
}

#[tokio::test]
async fn malformed_schema_metadata_is_reported_as_corruption() {
    let (_dir, repo) = repo();
    BacklogItemRepository::list(&repo)
        .await
        .expect("initialize");
    raw(&repo)
        .execute(
            "UPDATE metadata SET value = X'01' WHERE key = 'schema_version'",
            [],
        )
        .expect("corrupt schema metadata");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("malformed schema metadata must be rejected");
    assert!(matches!(err, Error::Parse { .. }), "got {err:?}");
    assert!(err.to_string().contains("invalid SQLite metadata"));
}

#[tokio::test]
async fn existing_database_without_schema_metadata_is_rejected() {
    let (_dir, repo) = repo();
    let item = full_item();
    BacklogItemRepository::save(&repo, &item)
        .await
        .expect("save");

    // Simulate a database created before metadata was introduced.
    let conn = raw(&repo);
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL); DROP TABLE metadata;",
    )
    .expect("simulate legacy schema");
    drop(conn);

    let err = BacklogItemRepository::load(&repo, &item.id)
        .await
        .expect_err("schema metadata is required for an existing database");
    assert!(err.to_string().contains("found version \"missing\""));
    assert!(err.to_string().contains("version 2"));
    let conn = raw(&repo);
    let metadata_exists: bool = conn
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'metadata')",
            [],
            |row| row.get(0),
        )
        .expect("inspect metadata table");
    assert!(
        !metadata_exists,
        "rejected databases must not be initialized"
    );
}

#[tokio::test]
async fn metadata_keeps_future_key_value_extensions() {
    let (_dir, repo) = repo();
    BacklogItemRepository::list(&repo)
        .await
        .expect("initialize");
    let conn = raw(&repo);
    conn.execute(
        "INSERT INTO metadata (key, value) VALUES ('created_by', 'pinto-test')",
        [],
    )
    .expect("write extension metadata");
    drop(conn);

    BacklogItemRepository::list(&repo)
        .await
        .expect("read metadata");
    let conn = raw(&repo);
    let value: String = conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'created_by'",
            [],
            |row| row.get(0),
        )
        .expect("extension metadata remains");
    assert_eq!(value, "pinto-test");
}

#[tokio::test]
async fn unsupported_schema_version_returns_actionable_error() {
    let (_dir, repo) = repo();
    BacklogItemRepository::list(&repo)
        .await
        .expect("initialize");
    for found in ["1", "99", "not-a-version"] {
        let conn = raw(&repo);
        conn.execute(
            "UPDATE metadata SET value = ?1 WHERE key = 'schema_version'",
            [found],
        )
        .expect("set unsupported version");
        drop(conn);

        let error = BacklogItemRepository::list(&repo)
            .await
            .expect_err("unsupported schema must not be opened");
        let message = error.to_string();
        assert!(
            message.contains(found),
            "reports stored version {found:?}: {message}"
        );
        assert!(
            message.contains("version 2"),
            "reports supported version: {message}"
        );
        assert!(
            message.contains("upgrade") || message.contains("recreate"),
            "explains how to recover: {message}"
        );
        assert!(
            message.contains("migration"),
            "states that automatic migration is unavailable: {message}"
        );
    }
}

// --- Normalization Schema---

#[tokio::test]
async fn full_item_roundtrips_all_fields() {
    let (_dir, repo) = repo();
    let it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    let loaded = BacklogItemRepository::load(&repo, &it.id)
        .await
        .expect("load");
    assert_eq!(loaded, it);
}

#[tokio::test]
async fn items_table_has_typed_columns_not_markdown() {
    let (_dir, repo) = repo();
    BacklogItemRepository::save(&repo, &full_item())
        .await
        .expect("save");
    let conn = raw(&repo);
    let cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('items')")
        .expect("prepare")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("collect");
    assert!(
        !cols.iter().any(|c| c == "markdown"),
        "markdown blob 列は廃止されている: {cols:?}"
    );
    for expected in [
        "id", "title", "status", "rank", "points", "assignee", "archived",
    ] {
        assert!(
            cols.iter().any(|c| c == expected),
            "型付き列 {expected} が存在する: {cols:?}"
        );
    }
}

#[tokio::test]
async fn labels_stored_in_relation_table_in_order() {
    let (_dir, repo) = repo();
    let it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    let conn = raw(&repo);
    let labels: Vec<String> = conn
        .prepare("SELECT label FROM item_labels WHERE item_id = ?1 ORDER BY position")
        .expect("prepare")
        .query_map(["T-7"], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("collect");
    assert_eq!(labels, it.labels);
}

#[tokio::test]
async fn dependencies_stored_in_relation_table_in_order() {
    let (_dir, repo) = repo();
    let it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    let conn = raw(&repo);
    let deps: Vec<String> = conn
        .prepare("SELECT depends_on FROM item_dependencies WHERE item_id = ?1 ORDER BY position")
        .expect("prepare")
        .query_map(["T-7"], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("collect");
    assert_eq!(deps, vec!["T-2".to_string(), "T-3".to_string()]);
}

#[tokio::test]
async fn duplicate_label_rejected_by_constraint() {
    let (_dir, repo) = repo();
    BacklogItemRepository::save(&repo, &full_item())
        .await
        .expect("save");
    let conn = raw(&repo);
    // Duplicate labels are rejected by the unique constraint (item_id, label).
    let err = conn.execute(
        "INSERT INTO item_labels (item_id, label, position) VALUES ('T-7', 'backend', 9)",
        [],
    );
    assert!(err.is_err(), "重複ラベルは一意制約で拒否される");
}

#[tokio::test]
async fn deleting_item_cascades_relations() {
    let (_dir, repo) = repo();
    let it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::delete(&repo, &it.id)
        .await
        .expect("delete");
    let conn = raw(&repo);
    let label_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM item_labels WHERE item_id = 'T-7'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    let dep_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM item_dependencies WHERE item_id = 'T-7'",
            [],
            |r| r.get(0),
        )
        .expect("count");
    assert_eq!(label_count, 0, "削除で関連ラベルもカスケード削除される");
    assert_eq!(dep_count, 0, "削除で関連依存もカスケード削除される");
}

#[tokio::test]
async fn archive_sets_flag_and_keeps_row() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::archive(&repo, &it.id)
        .await
        .expect("archive");
    let conn = raw(&repo);
    let archived: i64 = conn
        .query_row("SELECT archived FROM items WHERE id = 'T-1'", [], |r| {
            r.get(0)
        })
        .expect("row still present");
    assert_eq!(archived, 1, "アーカイブ状態は archived 列で表現される");
}

#[tokio::test]
async fn archived_items_can_be_listed_loaded_and_restored_without_changes() {
    let (_dir, repo) = repo();
    let it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::archive(&repo, &it.id)
        .await
        .expect("archive");

    assert_eq!(
        BacklogItemRepository::list_archived(&repo)
            .await
            .expect("list archived"),
        vec![it.clone()]
    );
    assert_eq!(
        BacklogItemRepository::load_archived(&repo, &it.id)
            .await
            .expect("load archived"),
        it
    );
    BacklogItemRepository::restore(&repo, &it.id)
        .await
        .expect("restore");
    assert_eq!(
        BacklogItemRepository::load(&repo, &it.id)
            .await
            .expect("load restored"),
        it
    );
    assert!(
        BacklogItemRepository::list_archived(&repo)
            .await
            .expect("list archived after restore")
            .is_empty()
    );
}

#[tokio::test]
async fn saving_over_an_archived_id_reactivates_it() {
    // The save target is always active. When you resave the saved ID, archived returns to 0,
    // It reappears in the list (preventing ghost hiding when re-migrating from file to sqlite).
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::archive(&repo, &it.id)
        .await
        .expect("archive");
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("re-save");
    let listed = BacklogItemRepository::list(&repo).await.expect("list");
    assert_eq!(listed.len(), 1, "再保存でアクティブへ戻る");
    assert_eq!(listed[0].id, it.id);
}

#[tokio::test]
async fn save_replaces_labels_on_update() {
    let (_dir, repo) = repo();
    let mut it = full_item();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    it.labels = vec!["frontend".to_string()];
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    let loaded = BacklogItemRepository::load(&repo, &it.id)
        .await
        .expect("load");
    assert_eq!(loaded.labels, vec!["frontend".to_string()]);
}

#[tokio::test]
async fn sprint_roundtrips_all_fields() {
    let (_dir, repo) = repo();
    let mut s = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint One", ts(1_000)).unwrap();
    s.goal = "Ship the MVP\nwith tests".to_string();
    s.state = crate::sprint::SprintState::Closed;
    s.closed_at = Some(ts(4_000));
    s.start = Some(ts(1_000));
    s.end = Some(ts(9_000));
    s.spillover = crate::sprint::SprintSpillover {
        points: 8,
        items: 2,
        unestimated_items: 1,
    };
    s.updated = ts(5_000);
    SprintRepository::save(&repo, &s).await.expect("save");
    let loaded = SprintRepository::load(&repo, &s.id).await.expect("load");
    assert_eq!(loaded, s);
    let conn = raw(&repo);
    let cols: Vec<String> = conn
        .prepare("SELECT name FROM pragma_table_info('sprints')")
        .expect("prepare")
        .query_map([], |r| r.get::<_, String>(0))
        .expect("query")
        .collect::<rusqlite::Result<_>>()
        .expect("collect");
    assert!(
        !cols.iter().any(|c| c == "markdown"),
        "sprints も markdown blob を廃止: {cols:?}"
    );
    assert!(
        cols.iter().any(|c| c == "state"),
        "state 列を持つ: {cols:?}"
    );
    assert!(
        cols.iter().any(|column| column == "closed_at"),
        "closed_at column exists: {cols:?}"
    );
    for expected in [
        "spillover_points",
        "spillover_items",
        "unestimated_spillover_items",
    ] {
        assert!(
            cols.iter().any(|column| column == expected),
            "spillover column {expected} exists: {cols:?}"
        );
    }
}

#[tokio::test]
async fn save_then_load_roundtrips() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let loaded = BacklogItemRepository::load(&repo, &it.id)
        .await
        .expect("load succeeds");
    assert_eq!(loaded, it);
}

#[tokio::test]
async fn schema_rejects_non_alphabetic_item_prefix() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let conn = raw(&repo);
    for prefix in ["123", "p9", "bug_fix", "PROJ-1"] {
        let err = conn.execute("UPDATE items SET prefix = ?1 WHERE id = 'T-1'", [prefix]);
        assert!(err.is_err(), "prefix {prefix:?} must be rejected");
    }
}

#[tokio::test]
async fn corrupted_prefix_is_rejected_instead_of_becoming_a_path_component() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute(
        "UPDATE items SET id = ?1, prefix = ?2 WHERE id = 'T-1'",
        rusqlite::params!["../outside-1", "../outside"],
    )
    .expect("corrupt row");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("corrupt prefix must be rejected");
    assert!(err.to_string().contains("invalid item id prefix"));
}

#[tokio::test]
async fn corrupted_stored_id_is_rejected_when_it_disagrees_with_components() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    raw(&repo)
        .execute("UPDATE items SET id = 'T-99' WHERE id = 'T-1'", [])
        .expect("corrupt stored id");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("mismatched stored id must be rejected");
    assert!(err.to_string().contains("does not match prefix/number"));
}

#[tokio::test]
async fn corrupted_negative_item_number_is_rejected() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE items SET number = -1 WHERE id = 'T-1'", [])
        .expect("corrupt item number");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("negative item number must be rejected");
    assert!(err.to_string().contains("invalid item number"));
}

#[tokio::test]
async fn corrupted_negative_points_are_rejected() {
    let (_dir, repo) = repo();
    let mut it = item(1, Rank::after(None));
    it.points = Some(3);
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE items SET points = -1 WHERE id = 'T-1'", [])
        .expect("corrupt points");

    let err = BacklogItemRepository::list(&repo)
        .await
        .expect_err("negative points must be rejected");
    assert!(err.to_string().contains("invalid points"));
}

#[tokio::test]
async fn corrupted_empty_item_title_is_rejected_by_the_loader() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE items SET title = '   ' WHERE id = 'T-1'", [])
        .expect("corrupt item title");

    let err = BacklogItemRepository::load(&repo, &it.id)
        .await
        .expect_err("a blank item title must be rejected");
    assert!(err.to_string().contains("empty item title"));
}

#[tokio::test]
async fn corrupted_sprint_period_and_capacity_values_are_rejected() {
    let (_dir, repo) = repo();
    let mut sprint =
        Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
    sprint.start = Some(ts(1_000));
    sprint.end = Some(ts(9_000));
    sprint.daily_work_hours = Some(8.0);
    sprint.holiday_days = Some(1);
    sprint.deduction_factor = Some(0.8);
    SprintRepository::save(&repo, &sprint).await.expect("save");

    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute(
        "UPDATE sprints SET start_at = '1970-01-01T02:30:00Z', end_at = '1970-01-01T00:16:40Z' WHERE id = 'S-1'",
        [],
    )
    .expect("corrupt sprint period");
    drop(conn);

    let err = SprintRepository::load(&repo, &sprint.id)
        .await
        .expect_err("inverted sprint period must be rejected");
    assert!(err.to_string().contains("invalid sprint period"));

    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute(
        "UPDATE sprints SET start_at = '1970-01-01T00:16:40Z', end_at = '1970-01-01T02:30:00Z', holiday_days = 2 WHERE id = 'S-1'",
        [],
    )
    .expect("corrupt sprint capacity");
    drop(conn);

    let err = SprintRepository::load(&repo, &sprint.id)
        .await
        .expect_err("excessive holidays must be rejected");
    assert!(err.to_string().contains("invalid holiday days"));
}

#[tokio::test]
async fn corrupted_empty_item_title_is_rejected() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it)
        .await
        .expect("save succeeds");
    let err = raw(&repo)
        .execute("UPDATE items SET title = '   ' WHERE id = 'T-1'", [])
        .expect_err("schema must reject a blank item title");
    assert!(err.to_string().contains("CHECK"));
}

#[tokio::test]
async fn corrupted_negative_sprint_holidays_are_rejected_as_corruption() {
    let (_dir, repo) = repo();
    let mut sprint =
        Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
    sprint.start = Some(ts(1_000));
    sprint.end = Some(ts(9_000));
    sprint.daily_work_hours = Some(8.0);
    sprint.holiday_days = Some(1);
    sprint.deduction_factor = Some(0.8);
    SprintRepository::save(&repo, &sprint).await.expect("save");

    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute("UPDATE sprints SET holiday_days = -1 WHERE id = 'S-1'", [])
        .expect("corrupt holiday count");

    let err = SprintRepository::load(&repo, &sprint.id)
        .await
        .expect_err("negative holiday count must be rejected");
    assert!(err.to_string().contains("invalid holiday days"));
}

#[tokio::test]
async fn corrupted_sprint_title_and_capacity_relation_are_rejected() {
    let (_dir, repo) = repo();
    let mut sprint =
        Sprint::new(SprintId::new("S-1").unwrap(), "Sprint", ts(1_000)).expect("valid sprint");
    sprint.start = Some(ts(1_000));
    sprint.end = Some(ts(9_000));
    sprint.daily_work_hours = Some(8.0);
    sprint.holiday_days = Some(1);
    sprint.deduction_factor = Some(0.8);
    SprintRepository::save(&repo, &sprint).await.expect("save");

    let conn = raw(&repo);
    conn.execute("PRAGMA ignore_check_constraints = ON", [])
        .expect("disable checks for corruption fixture");
    conn.execute(
        "UPDATE sprints SET title = ' ', daily_work_hours = NULL WHERE id = 'S-1'",
        [],
    )
    .expect("corrupt sprint row");

    let err = SprintRepository::list(&repo)
        .await
        .expect_err("invalid sprint row must be rejected");
    assert!(err.to_string().contains("empty sprint title"));
}

#[tokio::test]
async fn save_overwrites_existing_id() {
    let (_dir, repo) = repo();
    let mut it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    it.title = "Renamed".to_string();
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    let loaded = BacklogItemRepository::load(&repo, &it.id)
        .await
        .expect("load");
    assert_eq!(loaded.title, "Renamed");
    // Since it is overwritten, the number remains 1.
    assert_eq!(
        BacklogItemRepository::list(&repo)
            .await
            .expect("list")
            .len(),
        1
    );
}

#[tokio::test]
async fn load_missing_returns_not_found() {
    let (_dir, repo) = repo();
    let err = BacklogItemRepository::load(&repo, &ItemId::new("T", 99))
        .await
        .expect_err("missing");
    assert_eq!(err, Error::NotFound(ItemId::new("T", 99)));
}

#[tokio::test]
async fn list_on_uninitialized_is_empty_not_error() {
    let (_dir, repo) = repo();
    let items = BacklogItemRepository::list(&repo).await.expect("list");
    assert!(items.is_empty());
}

#[tokio::test]
async fn list_returns_all_items_sorted_by_rank() {
    let (_dir, repo) = repo();
    let order = [3u32, 1, 10, 2];
    let rs = ranks(order.len());
    for (n, rank) in order.iter().zip(rs) {
        BacklogItemRepository::save(&repo, &item(*n, rank))
            .await
            .expect("save");
    }
    let items = BacklogItemRepository::list(&repo).await.expect("list");
    let ids: Vec<u32> = items.iter().map(|i| i.id.number()).collect();
    assert_eq!(ids, order.to_vec(), "rank 昇順（= 割当順）で返る");
}

#[tokio::test]
async fn delete_removes_and_missing_is_not_found() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::delete(&repo, &it.id)
        .await
        .expect("delete");
    assert!(matches!(
        BacklogItemRepository::load(&repo, &it.id).await,
        Err(Error::NotFound(_))
    ));
    assert!(matches!(
        BacklogItemRepository::delete(&repo, &it.id).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn next_id_does_not_reuse_a_physically_deleted_id() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::delete(&repo, &it.id)
        .await
        .expect("delete");

    assert_eq!(
        BacklogItemRepository::next_id(&repo, "T")
            .await
            .expect("next id"),
        ItemId::new("T", 2),
        "a physically deleted ID must remain reserved"
    );
}

#[tokio::test]
async fn archive_moves_item_out_of_active_list() {
    let (_dir, repo) = repo();
    let it = item(1, Rank::after(None));
    BacklogItemRepository::save(&repo, &it).await.expect("save");
    BacklogItemRepository::archive(&repo, &it.id)
        .await
        .expect("archive");
    // After archiving, the item is absent from the active list and load operations.
    assert!(
        BacklogItemRepository::list(&repo)
            .await
            .expect("list")
            .is_empty()
    );
    assert!(matches!(
        BacklogItemRepository::load(&repo, &it.id).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn archive_missing_is_not_found() {
    let (_dir, repo) = repo();
    assert!(matches!(
        BacklogItemRepository::archive(&repo, &ItemId::new("T", 5)).await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn next_id_increments_and_skips_archived() {
    let (_dir, repo) = repo();
    assert_eq!(
        BacklogItemRepository::next_id(&repo, "T")
            .await
            .expect("next"),
        ItemId::new("T", 1)
    );
    for n in 1..=3 {
        BacklogItemRepository::save(&repo, &item(n, Rank::after(None)))
            .await
            .expect("save");
    }
    assert_eq!(
        BacklogItemRepository::next_id(&repo, "T")
            .await
            .expect("next"),
        ItemId::new("T", 4)
    );
    // Archiving T-3 does not release its ID; the next ID remains T-4.
    BacklogItemRepository::archive(&repo, &ItemId::new("T", 3))
        .await
        .expect("archive");
    assert_eq!(
        BacklogItemRepository::next_id(&repo, "T")
            .await
            .expect("next"),
        ItemId::new("T", 4)
    );
}

#[tokio::test]
async fn sprint_save_load_roundtrips() {
    let (_dir, repo) = repo();
    let s = Sprint::new(SprintId::new("S-1").unwrap(), "Sprint 1", ts(1_000)).unwrap();
    SprintRepository::save(&repo, &s).await.expect("save");
    let loaded = SprintRepository::load(&repo, &s.id).await.expect("load");
    assert_eq!(loaded, s);
}

#[tokio::test]
async fn load_missing_sprint_returns_not_found() {
    let (_dir, repo) = repo();
    let id = SprintId::new("S-99").unwrap();
    let err = SprintRepository::load(&repo, &id)
        .await
        .expect_err("missing");
    assert_eq!(err, Error::SprintNotFound(id));
}

#[tokio::test]
async fn delete_sprint_removes_and_missing_is_not_found() {
    let (_dir, repo) = repo();
    let s = Sprint::new(SprintId::new("S-1").unwrap(), "S1", ts(1_000)).unwrap();
    SprintRepository::save(&repo, &s).await.expect("save");
    SprintRepository::delete(&repo, &s.id)
        .await
        .expect("delete");
    assert!(matches!(
        SprintRepository::load(&repo, &s.id).await,
        Err(Error::SprintNotFound(_))
    ));
    assert!(matches!(
        SprintRepository::delete(&repo, &s.id).await,
        Err(Error::SprintNotFound(_))
    ));
}

#[tokio::test]
async fn list_sprints_returns_all_in_creation_order() {
    let (_dir, repo) = repo();
    for (id, secs) in [("S-3", 3_000i64), ("S-1", 1_000), ("S-2", 2_000)] {
        let s = Sprint::new(SprintId::new(id).unwrap(), id, ts(secs)).unwrap();
        SprintRepository::save(&repo, &s).await.expect("save");
    }
    let ids: Vec<String> = SprintRepository::list(&repo)
        .await
        .expect("list")
        .into_iter()
        .map(|s| s.id.as_str().to_string())
        .collect();
    assert_eq!(ids, ["S-1", "S-2", "S-3"]);
}
