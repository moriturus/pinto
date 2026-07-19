//! Shared persistence contracts for every supported backend.

use chrono::{DateTime, Duration, TimeZone, Utc};
use pinto::backlog::{BacklogItem, ItemId, Status};
use pinto::error::Error;
use pinto::rank::Rank;
use pinto::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use pinto::storage::{Backend, BacklogItemRepository, SprintRepository, StorageBackend};
use tempfile::TempDir;

#[derive(Debug, Clone, PartialEq)]
struct ContractSnapshot {
    item_loaded: BacklogItem,
    items_before_update: Vec<BacklogItem>,
    item_loaded_after_update: BacklogItem,
    archived_items: Vec<BacklogItem>,
    archived_item_loaded: BacklogItem,
    items_after_restore: Vec<BacklogItem>,
    items_after_delete: Vec<BacklogItem>,
    next_item_id: ItemId,
    item_error_codes: Vec<&'static str>,
    sprint_loaded: Sprint,
    sprints_before_update: Vec<Sprint>,
    sprint_loaded_after_update: Sprint,
    sprints_after_delete: Vec<Sprint>,
    sprint_error_codes: Vec<&'static str>,
}

fn timestamp(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0)
        .single()
        .expect("contract timestamp is valid")
}

fn item_id(number: u32) -> ItemId {
    ItemId::try_new("T", number).expect("contract item id is valid")
}

fn full_item(id: u32, rank: &str) -> BacklogItem {
    let mut item = BacklogItem::new(
        item_id(id),
        "契約 item 🧪",
        Status::new("in-progress"),
        Rank::parse(rank).expect("contract rank is valid"),
        timestamp(1_000),
    )
    .expect("contract item is valid");
    item.points = Some(8);
    item.labels = vec!["backend".to_string(), "日本語".to_string()];
    item.assignee = Some("alice".to_string());
    item.sprint = Some("sprint-1".to_string());
    item.parent = Some(item_id(9));
    item.depends_on = vec![item_id(4), item_id(5)];
    item.start_at = Some(timestamp(1_100));
    item.done_at = Some(timestamp(1_200));
    item.commits = vec!["abc123".to_string(), "def456".to_string()];
    item.body = "## 本文\n\n- [ ] Unicode ✅".to_string();
    item.updated = timestamp(1_300);
    item
}

fn full_sprint(id: &str, created: i64) -> Sprint {
    let mut sprint = Sprint::new(
        SprintId::new(id).expect("contract sprint id is valid"),
        "契約 Sprint 🚀",
        timestamp(created),
    )
    .expect("contract sprint is valid");
    sprint.goal = "Ship the parser\n多言語対応".to_string();
    sprint.state = SprintState::Active;
    sprint.start = Some(timestamp(2_000));
    sprint.end = Some(timestamp(2_000) + Duration::days(7));
    sprint.daily_work_hours = Some(6.5);
    sprint.holiday_days = Some(1);
    sprint.deduction_factor = Some(0.8);
    sprint.spillover = SprintSpillover {
        points: 8,
        items: 2,
        unestimated_items: 1,
    };
    sprint.updated = timestamp(created + 100);
    sprint
}

fn error_code<T>(result: std::result::Result<T, Error>) -> &'static str {
    result.map_or_else(|error| error.code(), |_| "ok")
}

fn backend_kinds() -> Vec<StorageBackend> {
    #[cfg(feature = "sqlite")]
    {
        vec![
            StorageBackend::File,
            StorageBackend::Git,
            StorageBackend::Sqlite,
        ]
    }
    #[cfg(not(feature = "sqlite"))]
    {
        vec![StorageBackend::File, StorageBackend::Git]
    }
}

async fn exercise_contract(backend: &Backend) -> ContractSnapshot {
    let item = full_item(1, "a");
    let second_item = full_item(2, "b");
    BacklogItemRepository::save(backend, &item)
        .await
        .expect("save first item");
    BacklogItemRepository::save(backend, &second_item)
        .await
        .expect("save second item");

    let item_loaded = BacklogItemRepository::load(backend, &item.id)
        .await
        .expect("load item");
    let items_before_update = BacklogItemRepository::list(backend)
        .await
        .expect("list items");
    let next_item_id = BacklogItemRepository::next_id(backend, "T")
        .await
        .expect("next item id");

    let mut updated_item = item.clone();
    updated_item.title = "更新済み契約 item".to_string();
    updated_item.body = "更新された本文".to_string();
    updated_item.updated = timestamp(1_400);
    BacklogItemRepository::save(backend, &updated_item)
        .await
        .expect("update item");
    let item_loaded_after_update = BacklogItemRepository::load(backend, &item.id)
        .await
        .expect("load updated item");

    BacklogItemRepository::archive(backend, &item.id)
        .await
        .expect("archive item");
    let archived_items = BacklogItemRepository::list_archived(backend)
        .await
        .expect("list archived items");
    let archived_item_loaded = BacklogItemRepository::load_archived(backend, &item.id)
        .await
        .expect("load archived item");
    BacklogItemRepository::restore(backend, &item.id)
        .await
        .expect("restore item");
    let items_after_restore = BacklogItemRepository::list(backend)
        .await
        .expect("list restored items");
    BacklogItemRepository::delete(backend, &second_item.id)
        .await
        .expect("delete item");
    let items_after_delete = BacklogItemRepository::list(backend)
        .await
        .expect("list after item delete");
    let item_error_codes = vec![
        error_code(BacklogItemRepository::load(backend, &item_id(99)).await),
        error_code(BacklogItemRepository::delete(backend, &item_id(99)).await),
    ];

    let sprint = full_sprint("sprint-1", 3_000);
    let second_sprint = full_sprint("sprint-2", 3_100);
    SprintRepository::save(backend, &sprint)
        .await
        .expect("save first sprint");
    SprintRepository::save(backend, &second_sprint)
        .await
        .expect("save second sprint");
    let sprint_loaded = SprintRepository::load(backend, &sprint.id)
        .await
        .expect("load sprint");
    let sprints_before_update = SprintRepository::list(backend).await.expect("list sprints");

    let mut updated_sprint = sprint.clone();
    updated_sprint.title = "更新済み契約 Sprint".to_string();
    updated_sprint.goal = "Updated goal\n更新された目標".to_string();
    updated_sprint.updated = timestamp(3_300);
    SprintRepository::save(backend, &updated_sprint)
        .await
        .expect("update sprint");
    let sprint_loaded_after_update = SprintRepository::load(backend, &sprint.id)
        .await
        .expect("load updated sprint");
    SprintRepository::delete(backend, &second_sprint.id)
        .await
        .expect("delete sprint");
    let sprints_after_delete = SprintRepository::list(backend)
        .await
        .expect("list after sprint delete");
    let sprint_error_codes = vec![
        error_code(
            SprintRepository::load(
                backend,
                &SprintId::new("missing-sprint").expect("valid missing sprint id"),
            )
            .await,
        ),
        error_code(
            SprintRepository::delete(
                backend,
                &SprintId::new("missing-sprint").expect("valid missing sprint id"),
            )
            .await,
        ),
    ];

    ContractSnapshot {
        item_loaded,
        items_before_update,
        item_loaded_after_update,
        archived_items,
        archived_item_loaded,
        items_after_restore,
        items_after_delete,
        next_item_id,
        item_error_codes,
        sprint_loaded,
        sprints_before_update,
        sprint_loaded_after_update,
        sprints_after_delete,
        sprint_error_codes,
    }
}

#[tokio::test]
async fn all_enabled_backends_share_item_and_sprint_contracts() {
    let kinds = backend_kinds();
    let mut snapshots = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let directory = TempDir::new().expect("create backend test directory");
        let backend = Backend::open(directory.path().join(".pinto"), kind)
            .await
            .expect("open backend");
        snapshots.push((kind, exercise_contract(&backend).await));
    }

    #[cfg(feature = "sqlite")]
    assert_eq!(snapshots.len(), 3, "file, Git, and SQLite must be covered");
    #[cfg(not(feature = "sqlite"))]
    assert_eq!(snapshots.len(), 2, "file and Git must remain available");

    let (_, expected) = snapshots.first().expect("at least one backend");
    for (kind, snapshot) in snapshots.iter().skip(1) {
        assert_eq!(
            snapshot, expected,
            "backend {kind} diverged from the contract"
        );
    }
    assert_eq!(expected.item_error_codes, ["not-found", "not-found"]);
    assert_eq!(
        expected.sprint_error_codes,
        ["sprint-not-found", "sprint-not-found"]
    );
}

#[cfg(not(feature = "sqlite"))]
#[test]
fn sqlite_configuration_value_is_rejected_without_feature() {
    let document: toml::Value = toml::from_str("backend = \"sqlite\"").expect("valid TOML");
    let error = document
        .get("backend")
        .expect("backend value")
        .clone()
        .try_into::<StorageBackend>()
        .expect_err("the default build must not deserialize SQLite");
    assert!(error.to_string().contains("unknown variant"), "{error}");
}

#[cfg(feature = "sqlite")]
#[test]
fn sqlite_configuration_value_is_available_with_feature() {
    let document: toml::Value = toml::from_str("backend = \"sqlite\"").expect("valid TOML");
    assert_eq!(
        document
            .get("backend")
            .expect("backend value")
            .clone()
            .try_into::<StorageBackend>()
            .expect("SQLite is enabled"),
        StorageBackend::Sqlite
    );
}
