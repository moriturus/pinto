//! Unit tests for adding, listing, showing, moving, and removing backlog items.

use super::super::*;
use crate::error::Error;
use crate::service::LabelMatch;
use crate::service::test_support::{add_with, create_sprint_for_test, init_temp};
use crate::sprint::SprintId;
use crate::storage::{BacklogItemRepository, FileRepository};
use chrono::{Duration, Utc};
use tempfile::TempDir;
use tokio::fs;

#[tokio::test]
async fn add_creates_item_with_default_status_and_first_id() {
    let dir = init_temp().await;

    let item = add_item(dir.path(), "First task", NewItem::default())
        .await
        .expect("add succeeds");

    assert_eq!(item.id, ItemId::new("T", 1));
    assert_eq!(item.title, "First task");
    assert_eq!(item.status, Status::new("todo"));

    // It is made permanent.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let loaded = repo.load(&item.id).await.expect("load");
    assert_eq!(loaded, item);
}

#[tokio::test]
async fn add_assigns_unique_incrementing_ids() {
    let dir = init_temp().await;

    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();

    assert_eq!(a.id, ItemId::new("T", 1));
    assert_eq!(b.id, ItemId::new("T", 2));
}

#[tokio::test]
async fn add_sets_optional_fields() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;

    let new = NewItem {
        points: Some(5),
        labels: vec!["backend".to_string(), "urgent".to_string()],
        sprint: Some("S-1".to_string()),
        body: "Acceptance criteria".to_string(),
        parent: None,
        depends_on: Vec::new(),
    };
    let item = add_item(dir.path(), "Configured", new).await.unwrap();

    assert_eq!(item.points, Some(5));
    assert_eq!(item.labels, ["backend", "urgent"]);
    assert_eq!(item.sprint.as_deref(), Some("S-1"));
    assert_eq!(item.body, "Acceptance criteria");
}

#[tokio::test]
async fn add_rejects_invalid_or_missing_sprint_before_allocating_an_id() {
    let dir = init_temp().await;

    let invalid = add_item(
        dir.path(),
        "Malformed",
        NewItem {
            sprint: Some("S 1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(invalid, Error::InvalidSprintId("S 1".to_string()));

    let missing = add_item(
        dir.path(),
        "Missing",
        NewItem {
            sprint: Some("S-9".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        missing,
        Error::SprintNotFound(SprintId::new("S-9").unwrap())
    );
    assert!(
        list_items(dir.path(), &ListFilter::default())
            .await
            .unwrap()
            .is_empty()
    );

    create_sprint_for_test(dir.path(), "S-1").await;
    let item = add_item(
        dir.path(),
        "Valid",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(item.id, ItemId::new("T", 1));
}

#[tokio::test]
async fn add_sets_parent_and_multiple_dependencies() {
    let dir = init_temp().await;
    let parent = add_item(dir.path(), "Parent", NewItem::default())
        .await
        .unwrap();
    let dependency = add_item(dir.path(), "Dependency", NewItem::default())
        .await
        .unwrap();

    let item = add_item(
        dir.path(),
        "Configured relationships",
        NewItem {
            parent: Some(parent.id.clone()),
            depends_on: vec![dependency.id.clone(), parent.id.clone()],
            ..NewItem::default()
        },
    )
    .await
    .expect("add with relationships succeeds");

    assert_eq!(item.parent.as_ref(), Some(&parent.id));
    assert_eq!(item.depends_on, [dependency.id, parent.id]);
}

#[tokio::test]
async fn add_rejects_missing_relationship_targets_without_saving() {
    let dir = init_temp().await;

    let error = add_item(
        dir.path(),
        "Missing parent",
        NewItem {
            parent: Some("T-404".parse().unwrap()),
            ..NewItem::default()
        },
    )
    .await
    .expect_err("missing parent must be rejected");
    assert!(matches!(error, Error::NotFound(_)), "got {error:?}");

    let items = list_items(dir.path(), &ListFilter::default())
        .await
        .expect("list succeeds");
    assert!(items.is_empty(), "failed add must not leave an item");
}

#[tokio::test]
async fn add_rejects_a_self_parent_with_the_same_cycle_error_as_edit() {
    let dir = init_temp().await;

    let error = add_item(
        dir.path(),
        "Self parent",
        NewItem {
            parent: Some("T-1".parse().unwrap()),
            ..NewItem::default()
        },
    )
    .await
    .expect_err("self parent must be rejected");

    assert!(matches!(error, Error::ParentCycle { .. }), "got {error:?}");
}

#[tokio::test]
async fn add_allows_dependency_cycles_with_a_warning_outcome() {
    let dir = init_temp().await;
    let item_id: ItemId = "T-1".parse().unwrap();

    let outcome = add_item_with_outcome(
        dir.path(),
        "Self dependent",
        NewItem {
            depends_on: vec![item_id.clone()],
            ..NewItem::default()
        },
    )
    .await
    .expect("dependency cycles are warnings");

    assert!(outcome.cycle_warning);
    assert_eq!(outcome.item.depends_on, [item_id]);
}

#[tokio::test]
async fn add_appends_in_increasing_rank_order() {
    let dir = init_temp().await;

    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    // Rank increases monotonically in the order of addition.
    assert!(a.rank < b.rank, "{} < {}", a.rank, b.rank);
    assert!(b.rank < c.rank, "{} < {}", b.rank, c.rank);

    // list returns in rank ascending order = addition order.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let ids: Vec<_> = repo
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.id)
        .collect();
    assert_eq!(ids, [a.id, b.id, c.id]);
}

#[tokio::test]
async fn add_rejects_empty_title() {
    let dir = init_temp().await;

    let err = add_item(dir.path(), "   ", NewItem::default())
        .await
        .unwrap_err();

    assert_eq!(err, Error::EmptyTitle);
}

#[tokio::test]
async fn add_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    let err = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap_err();

    assert!(
        matches!(err, Error::NotInitialized { .. }),
        "expected NotInitialized, got {err:?}"
    );
}

#[tokio::test]
async fn list_returns_all_items_in_rank_order() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    let items = list_items(dir.path(), &ListFilter::default())
        .await
        .expect("list succeeds");

    let ids: Vec<_> = items.into_iter().map(|it| it.id).collect();
    assert_eq!(ids, [a.id, b.id, c.id]);
}

#[tokio::test]
async fn list_filters_by_label() {
    let dir = init_temp().await;
    add_with(dir.path(), "Backend task", &["backend"], None).await;
    add_with(dir.path(), "Frontend task", &["frontend"], None).await;
    add_with(dir.path(), "Both", &["backend", "frontend"], None).await;

    let filter = ListFilter {
        labels: vec!["backend".to_string()],
        ..Default::default()
    };
    let titles: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.title)
        .collect();

    assert_eq!(titles, ["Backend task", "Both"]);
}

#[tokio::test]
async fn list_filters_by_multiple_labels_with_any_or_all_matching() {
    let dir = init_temp().await;
    add_with(dir.path(), "Backend task", &["backend"], None).await;
    add_with(dir.path(), "Frontend task", &["frontend"], None).await;
    add_with(dir.path(), "Both", &["backend", "frontend"], None).await;

    let any = ListFilter {
        labels: vec!["backend".to_string(), "frontend".to_string()],
        ..Default::default()
    };
    let any_titles: Vec<_> = list_items(dir.path(), &any)
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.title)
        .collect();
    assert_eq!(any_titles, ["Backend task", "Frontend task", "Both"]);

    let all = ListFilter {
        labels: vec!["backend".to_string(), "frontend".to_string()],
        label_match: LabelMatch::All,
        ..Default::default()
    };
    let all_titles: Vec<_> = list_items(dir.path(), &all)
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.title)
        .collect();
    assert_eq!(all_titles, ["Both"]);
}

#[tokio::test]
async fn list_filters_by_sprint() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    create_sprint_for_test(dir.path(), "S-2").await;
    add_with(dir.path(), "In sprint", &[], Some("S-1")).await;
    add_with(dir.path(), "No sprint", &[], None).await;
    add_with(dir.path(), "Other sprint", &[], Some("S-2")).await;

    let filter = ListFilter {
        sprint: Some("S-1".to_string()),
        ..Default::default()
    };
    let titles: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.title)
        .collect();

    assert_eq!(titles, ["In sprint"]);
}

#[tokio::test]
async fn list_filters_by_assignee_without_changing_rank_order() {
    let dir = init_temp().await;
    let first = add_item(dir.path(), "Alice first", NewItem::default())
        .await
        .unwrap();
    let bob = add_item(dir.path(), "Bob", NewItem::default())
        .await
        .unwrap();
    let second = add_item(dir.path(), "Alice second", NewItem::default())
        .await
        .unwrap();

    edit_item(
        dir.path(),
        &first.id,
        ItemEdit {
            assignee: Some("alice".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    edit_item(
        dir.path(),
        &bob.id,
        ItemEdit {
            assignee: Some("bob".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    edit_item(
        dir.path(),
        &second.id,
        ItemEdit {
            assignee: Some("alice".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let filter = ListFilter {
        assignee: Some("alice".to_string()),
        ..Default::default()
    };
    let ids: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();

    assert_eq!(ids, [first.id, second.id]);
}

#[tokio::test]
async fn list_filters_by_status() {
    let dir = init_temp().await;
    // Immediately after addition, all columns are default columns (todo). All cases exist, and 0 cases do not.
    add_item(dir.path(), "One", NewItem::default())
        .await
        .unwrap();
    add_item(dir.path(), "Two", NewItem::default())
        .await
        .unwrap();

    let todo = ListFilter {
        status: vec!["todo".to_string()],
        ..Default::default()
    };
    assert_eq!(list_items(dir.path(), &todo).await.unwrap().len(), 2);

    let done = ListFilter {
        status: vec!["done".to_string()],
        ..Default::default()
    };
    assert!(list_items(dir.path(), &done).await.unwrap().is_empty());
}

#[tokio::test]
async fn list_filters_by_multiple_statuses() {
    let dir = init_temp().await;
    let todo = add_item(dir.path(), "Todo", NewItem::default())
        .await
        .unwrap();
    let progress = add_item(dir.path(), "In progress", NewItem::default())
        .await
        .unwrap();
    move_item(dir.path(), &progress.id, "in-progress")
        .await
        .unwrap();

    let filter = ListFilter {
        status: vec!["todo".to_string(), "in-progress".to_string()],
        ..Default::default()
    };
    let ids: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();

    assert_eq!(ids, [todo.id, progress.id]);
}

#[tokio::test]
async fn list_rejects_unknown_status() {
    let dir = init_temp().await;
    let filter = ListFilter {
        status: vec!["unknown".to_string()],
        ..Default::default()
    };

    let error = list_items(dir.path(), &filter).await.unwrap_err();

    assert!(matches!(error, Error::UnknownStatus(status) if status == "unknown"));
}

#[tokio::test]
async fn list_combines_filters_conjunctively() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    create_sprint_for_test(dir.path(), "S-2").await;
    add_with(dir.path(), "Match", &["backend"], Some("S-1")).await;
    add_with(dir.path(), "Wrong label", &["frontend"], Some("S-1")).await;
    add_with(dir.path(), "Wrong sprint", &["backend"], Some("S-2")).await;

    let filter = ListFilter {
        labels: vec!["backend".to_string()],
        sprint: Some("S-1".to_string()),
        ..Default::default()
    };
    let titles: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.title)
        .collect();

    assert_eq!(titles, ["Match"]);
}

#[tokio::test]
async fn list_filters_by_stale_updated_cutoff_including_boundary_without_writing() {
    let dir = init_temp().await;
    let stale = add_item(dir.path(), "Stale", NewItem::default())
        .await
        .unwrap();
    let boundary = add_item(dir.path(), "Boundary", NewItem::default())
        .await
        .unwrap();
    let fresh = add_item(dir.path(), "Fresh", NewItem::default())
        .await
        .unwrap();

    let cutoff = Utc::now() - Duration::days(7);
    let repo = FileRepository::new(dir.path().join(".pinto"));
    for (item, updated) in [
        (stale, cutoff - Duration::seconds(1)),
        (boundary, cutoff),
        (fresh, cutoff + Duration::seconds(1)),
    ] {
        let mut item = repo.load(&item.id).await.unwrap();
        item.updated = updated;
        repo.save(&item).await.unwrap();
    }
    let before = repo.load(&ItemId::new("T", 1)).await.unwrap();

    let filter = ListFilter {
        stale_before: Some(cutoff),
        ..Default::default()
    };
    let titles: Vec<_> = list_items(dir.path(), &filter)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.title)
        .collect();

    assert_eq!(titles, ["Stale", "Boundary"]);
    assert_eq!(repo.load(&ItemId::new("T", 1)).await.unwrap(), before);
}

#[tokio::test]
async fn list_on_empty_board_returns_empty() {
    let dir = init_temp().await;
    assert!(
        list_items(dir.path(), &ListFilter::default())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn list_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");
    let err = list_items(dir.path(), &ListFilter::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::NotInitialized { .. }),
        "expected NotInitialized, got {err:?}"
    );
}

#[tokio::test]
async fn show_returns_item_by_id() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Detailed", NewItem::default())
        .await
        .unwrap();

    let got = show_item(dir.path(), &added.id)
        .await
        .expect("show succeeds");

    assert_eq!(got, added);
}

#[tokio::test]
async fn show_missing_id_returns_not_found() {
    let dir = init_temp().await;

    let err = show_item(dir.path(), &ItemId::new("T", 99))
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn show_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    let err = show_item(dir.path(), &ItemId::new("T", 1))
        .await
        .unwrap_err();

    assert!(
        matches!(err, Error::NotInitialized { .. }),
        "expected NotInitialized, got {err:?}"
    );
}

#[tokio::test]
async fn item_use_cases_fail_fast_on_filename_frontmatter_id_mismatch() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Corruptible", NewItem::default())
        .await
        .expect("add");
    fs::rename(
        dir.path().join(".pinto/tasks/T-1.md"),
        dir.path().join(".pinto/tasks/T-2.md"),
    )
    .await
    .expect("rename corrupt fixture");

    let operations = [
        list_items(dir.path(), &ListFilter::default())
            .await
            .map(|_| ()),
        show_item(dir.path(), &added.id).await.map(|_| ()),
        move_item(dir.path(), &added.id, "in-progress")
            .await
            .map(|_| ()),
        edit_item(
            dir.path(),
            &added.id,
            ItemEdit {
                title: Some("Updated".to_string()),
                ..ItemEdit::default()
            },
        )
        .await
        .map(|_| ()),
        remove_item(dir.path(), &added.id, false).await.map(|_| ()),
        remove_item(dir.path(), &added.id, true).await.map(|_| ()),
    ];
    for result in operations {
        let error = result.expect_err("corrupt board must stop every item operation");
        assert!(matches!(error, Error::Parse { .. }), "got {error:?}");
        assert!(error.to_string().contains("filename"), "got {error}");
    }
}

#[tokio::test]
async fn move_transitions_to_valid_column_and_persists() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Movable", NewItem::default())
        .await
        .unwrap();
    assert_eq!(added.status, Status::new("todo"));

    let moved = move_item(dir.path(), &added.id, "in-progress")
        .await
        .expect("move succeeds");

    assert_eq!(moved.status, Status::new("in-progress"));
    assert!(moved.updated >= added.created, "updated advanced");

    // It is persisted (confirm by reloading).
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.status, Status::new("in-progress"));
    assert_eq!(reloaded.updated, moved.updated);
}

#[tokio::test]
async fn move_outcome_reports_incomplete_acceptance_criteria_for_done_column() {
    let dir = init_temp().await;
    let added = add_item(
        dir.path(),
        "Incomplete",
        NewItem {
            body: "- [x] shipped\n- [ ] documented".to_string(),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();

    let outcome = move_item_with_outcome(dir.path(), &added.id, "done")
        .await
        .expect("move succeeds");

    assert!(outcome.entered_done_column);
    assert!(outcome.acceptance_criteria.is_incomplete());
    assert_eq!(outcome.acceptance_criteria.to_string(), "1/2");
}

#[tokio::test]
async fn move_to_unknown_column_is_rejected_and_unchanged() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Stay", NewItem::default())
        .await
        .unwrap();

    let err = move_item(dir.path(), &added.id, "archived")
        .await
        .unwrap_err();

    assert_eq!(err, Error::UnknownStatus("archived".to_string()));
    // The state on the disk remains unchanged.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.status, Status::new("todo"));
}

#[tokio::test]
async fn move_missing_id_returns_not_found() {
    let dir = init_temp().await;

    let err = move_item(dir.path(), &ItemId::new("T", 99), "done")
        .await
        .unwrap_err();

    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn move_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");

    let err = move_item(dir.path(), &ItemId::new("T", 1), "done")
        .await
        .unwrap_err();

    assert!(
        matches!(err, Error::NotInitialized { .. }),
        "expected NotInitialized, got {err:?}"
    );
}

#[tokio::test]
async fn remove_archives_by_default_and_hides_from_list() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "Keep", NewItem::default())
        .await
        .unwrap();
    let b = add_item(dir.path(), "Remove me", NewItem::default())
        .await
        .unwrap();

    let outcome = remove_item(dir.path(), &b.id, false)
        .await
        .expect("remove succeeds");

    match outcome {
        RemoveOutcome::Archived(path) => {
            assert!(path.ends_with("archive/T-2.md"), "archived path: {path:?}");
            assert!(path.is_file(), "archived file exists");
        }
        other => panic!("expected Archived, got {other:?}"),
    }

    // The archived item is absent from list, leaving only a.
    let ids: Vec<_> = list_items(dir.path(), &ListFilter::default())
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.id)
        .collect();
    assert_eq!(ids, [a.id]);
    // It is also absent from show.
    assert!(matches!(
        show_item(dir.path(), &b.id).await.unwrap_err(),
        Error::NotFound(_)
    ));
}

#[tokio::test]
async fn archived_items_are_listed_and_restored_with_their_original_content() {
    let dir = init_temp().await;
    let archived = add_item(
        dir.path(),
        "Keep all fields",
        NewItem {
            points: Some(5),
            labels: vec!["archive".to_string()],
            body: "original body".to_string(),
            ..NewItem::default()
        },
    )
    .await
    .expect("add");
    remove_item(dir.path(), &archived.id, false)
        .await
        .expect("archive");

    let archived_items = list_items(
        dir.path(),
        &ListFilter {
            archived: true,
            ..Default::default()
        },
    )
    .await
    .expect("list archived");
    assert_eq!(archived_items.as_slice(), std::slice::from_ref(&archived));
    assert_eq!(
        show_archived_item(dir.path(), &archived.id)
            .await
            .expect("show archived"),
        archived
    );
    assert!(
        list_items(dir.path(), &ListFilter::default())
            .await
            .expect("active list")
            .is_empty()
    );

    let restored = restore_item(dir.path(), &archived.id)
        .await
        .expect("restore");
    assert_eq!(restored, archived);
    assert_eq!(
        list_items(dir.path(), &ListFilter::default())
            .await
            .expect("active list after restore"),
        [archived]
    );
    assert!(
        list_items(
            dir.path(),
            &ListFilter {
                archived: true,
                ..Default::default()
            },
        )
        .await
        .expect("archived list after restore")
        .is_empty()
    );
}

#[tokio::test]
async fn restore_refuses_an_active_id_collision() {
    let dir = init_temp().await;
    let archived = add_item(dir.path(), "Archived copy", NewItem::default())
        .await
        .expect("add");
    remove_item(dir.path(), &archived.id, false)
        .await
        .expect("archive");

    let active_path = dir.path().join(".pinto/tasks").join("T-1.md");
    let mut active = archived.clone();
    active.title = "Active collision".to_string();
    fs::create_dir_all(active_path.parent().expect("tasks parent"))
        .await
        .expect("create tasks");
    fs::write(
        &active_path,
        crate::storage::item_to_markdown(&active).expect("serialize active collision"),
    )
    .await
    .expect("write active collision");

    let err = restore_item(dir.path(), &archived.id)
        .await
        .expect_err("restore must reject collision");
    assert!(err.to_string().contains("already exists"), "got {err}");
    assert_eq!(
        fs::read_to_string(&active_path)
            .await
            .expect("active copy remains"),
        crate::storage::item_to_markdown(&active).expect("serialize active collision")
    );
}

#[tokio::test]
async fn remove_with_force_deletes_permanently() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "Bye", NewItem::default())
        .await
        .unwrap();

    let outcome = remove_item(dir.path(), &a.id, true)
        .await
        .expect("remove succeeds");
    assert_eq!(outcome, RemoveOutcome::Deleted);

    // Physical deletion does not remain in the archive.
    let archived = dir.path().join(".pinto").join("archive").join("T-1.md");
    assert!(!archived.exists(), "force delete must not archive");
    assert!(matches!(
        show_item(dir.path(), &a.id).await.unwrap_err(),
        Error::NotFound(_)
    ));
}

#[tokio::test]
async fn remove_with_force_rejects_parent_reference() {
    let dir = init_temp().await;
    let parent = add_item(dir.path(), "Parent", NewItem::default())
        .await
        .expect("parent");
    let child = add_item(
        dir.path(),
        "Child",
        NewItem {
            parent: Some(parent.id.clone()),
            ..NewItem::default()
        },
    )
    .await
    .expect("child");

    let err = remove_item(dir.path(), &parent.id, true)
        .await
        .expect_err("a referenced parent must not be deleted");
    assert_eq!(
        err,
        Error::ReferencedItem {
            item: parent.id.clone(),
            references: child.id.to_string(),
        }
    );
    assert!(show_item(dir.path(), &parent.id).await.is_ok());
    assert!(show_item(dir.path(), &child.id).await.is_ok());
}

#[tokio::test]
async fn remove_with_force_rejects_dependency_reference() {
    let dir = init_temp().await;
    let dependency = add_item(dir.path(), "Dependency", NewItem::default())
        .await
        .expect("dependency");
    let dependent = add_item(
        dir.path(),
        "Dependent",
        NewItem {
            depends_on: vec![dependency.id.clone()],
            ..NewItem::default()
        },
    )
    .await
    .expect("dependent");

    let err = remove_item(dir.path(), &dependency.id, true)
        .await
        .expect_err("a dependency target must not be deleted");
    assert_eq!(
        err,
        Error::ReferencedItem {
            item: dependency.id.clone(),
            references: dependent.id.to_string(),
        }
    );
}

#[tokio::test]
async fn remove_with_force_does_not_reuse_the_deleted_id() {
    let dir = init_temp().await;
    let deleted = add_item(dir.path(), "Deleted", NewItem::default())
        .await
        .expect("first item");
    remove_item(dir.path(), &deleted.id, true)
        .await
        .expect("force delete");

    let replacement = add_item(dir.path(), "Replacement", NewItem::default())
        .await
        .expect("replacement");
    assert_eq!(replacement.id, ItemId::new("T", 2));
}

#[tokio::test]
async fn archive_allows_a_referenced_item_to_remain_addressable() {
    let dir = init_temp().await;
    let parent = add_item(dir.path(), "Parent", NewItem::default())
        .await
        .expect("parent");
    add_item(
        dir.path(),
        "Child",
        NewItem {
            parent: Some(parent.id.clone()),
            ..NewItem::default()
        },
    )
    .await
    .expect("child");

    assert!(matches!(
        remove_item(dir.path(), &parent.id, false).await,
        Ok(RemoveOutcome::Archived(_))
    ));
}

#[tokio::test]
async fn remove_missing_id_returns_not_found() {
    let dir = init_temp().await;

    let err = remove_item(dir.path(), &ItemId::new("T", 99), false)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "archive: {err:?}");

    let err = remove_item(dir.path(), &ItemId::new("T", 99), true)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "force: {err:?}");
}

#[tokio::test]
async fn remove_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");
    let err = remove_item(dir.path(), &ItemId::new("T", 1), false)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::NotInitialized { .. }),
        "expected NotInitialized, got {err:?}"
    );
}
