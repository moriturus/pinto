//! Unit tests for the backlog-item use cases.

use super::*;
use crate::error::Error;
use crate::service::test_support::{add_with, init_temp, parent_edit};
use crate::service::{LabelMatch, create_sprint};
use crate::sprint::SprintId;
use crate::storage::{BacklogItemRepository, FileRepository};
use std::path::Path;
use tempfile::TempDir;
use tokio::fs;

async fn create_sprint_for_test(dir: &Path, id: &str) {
    let sprint_id = SprintId::new(id).expect("valid sprint id");
    create_sprint(dir, &sprint_id, id, None, None)
        .await
        .expect("create sprint");
}

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
async fn edit_updates_title_and_persists() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Old title", NewItem::default())
        .await
        .unwrap();

    let edit = ItemEdit {
        title: Some("New title".to_string()),
        ..Default::default()
    };
    let edited = edit_item(dir.path(), &added.id, edit)
        .await
        .expect("edit succeeds");

    assert_eq!(edited.title, "New title");
    assert!(edited.updated >= added.created, "updated advanced");

    // It is made permanent.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.title, "New title");
    assert_eq!(reloaded.updated, edited.updated);
}

#[tokio::test]
async fn edit_updates_each_optional_field() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-2").await;
    let added = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    let edit = ItemEdit {
        points: Some(8),
        labels: Some(vec!["backend".to_string(), "urgent".to_string()]),
        assignee: Some("alice".to_string()),
        sprint: Some("S-2".to_string()),
        body: Some("Acceptance criteria".to_string()),
        ..Default::default()
    };
    let edited = edit_item(dir.path(), &added.id, edit).await.unwrap();

    assert_eq!(edited.points, Some(8));
    assert_eq!(edited.labels, ["backend", "urgent"]);
    assert_eq!(edited.assignee.as_deref(), Some("alice"));
    assert_eq!(edited.sprint.as_deref(), Some("S-2"));
    assert_eq!(edited.body, "Acceptance criteria");
    // Title and status that are not specified will not change.
    assert_eq!(edited.title, "Task");
    assert_eq!(edited.status, added.status);
}

#[tokio::test]
async fn edit_rejects_invalid_or_missing_sprint_without_saving() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    let added = add_item(
        dir.path(),
        "Task",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();

    let invalid = edit_item(
        dir.path(),
        &added.id,
        ItemEdit {
            sprint: Some("S 2".to_string()),
            ..ItemEdit::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(invalid, Error::InvalidSprintId("S 2".to_string()));

    let missing = edit_item(
        dir.path(),
        &added.id,
        ItemEdit {
            sprint: Some("S-9".to_string()),
            ..ItemEdit::default()
        },
    )
    .await
    .unwrap_err();
    assert_eq!(
        missing,
        Error::SprintNotFound(SprintId::new("S-9").unwrap())
    );

    let stored = show_item(dir.path(), &added.id).await.unwrap();
    assert_eq!(stored.sprint.as_deref(), Some("S-1"));
}

#[tokio::test]
async fn edit_leaves_unspecified_fields_unchanged() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    let new = NewItem {
        points: Some(3),
        labels: vec!["keep".to_string()],
        sprint: Some("S-1".to_string()),
        body: "original body".to_string(),
        parent: None,
        depends_on: Vec::new(),
    };
    let added = add_item(dir.path(), "Keep me", new).await.unwrap();

    // Change only the title.
    let edit = ItemEdit {
        title: Some("Renamed".to_string()),
        ..Default::default()
    };
    let edited = edit_item(dir.path(), &added.id, edit).await.unwrap();

    assert_eq!(edited.title, "Renamed");
    assert_eq!(edited.points, Some(3));
    assert_eq!(edited.labels, ["keep"]);
    assert_eq!(edited.sprint.as_deref(), Some("S-1"));
    assert_eq!(edited.body, "original body");
}

#[tokio::test]
async fn edit_with_no_fields_is_rejected() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Untouched", NewItem::default())
        .await
        .unwrap();

    let err = edit_item(dir.path(), &added.id, ItemEdit::default())
        .await
        .unwrap_err();

    assert_eq!(err, Error::NothingToUpdate);
}

#[tokio::test]
async fn edit_rejects_empty_title_and_leaves_item_unchanged() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Original", NewItem::default())
        .await
        .unwrap();

    let edit = ItemEdit {
        title: Some("   ".to_string()),
        ..Default::default()
    };
    let err = edit_item(dir.path(), &added.id, edit).await.unwrap_err();

    assert_eq!(err, Error::EmptyTitle);
    // The title on the disc remains the same.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.title, "Original");
}

#[tokio::test]
async fn edit_missing_id_returns_not_found() {
    let dir = init_temp().await;
    let edit = ItemEdit {
        title: Some("x".to_string()),
        ..Default::default()
    };
    let err = edit_item(dir.path(), &ItemId::new("T", 99), edit)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn edit_on_uninitialized_dir_prompts_init() {
    let dir = TempDir::new().expect("temp dir");
    let edit = ItemEdit {
        title: Some("x".to_string()),
        ..Default::default()
    };
    let err = edit_item(dir.path(), &ItemId::new("T", 1), edit)
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

// --- set_parent ---

#[tokio::test]
async fn edit_sets_parent_and_persists() {
    let dir = init_temp().await;
    let epic = add_item(dir.path(), "Epic", NewItem::default())
        .await
        .unwrap();
    let story = add_item(dir.path(), "Story", NewItem::default())
        .await
        .unwrap();

    let updated = edit_item(dir.path(), &story.id, parent_edit(Some(epic.id.clone())))
        .await
        .expect("set parent succeeds");
    assert_eq!(updated.parent.as_ref(), Some(&epic.id));

    // It is made permanent.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        repo.load(&story.id).await.unwrap().parent.as_ref(),
        Some(&epic.id)
    );
}

#[tokio::test]
async fn edit_no_parent_clears_existing_parent() {
    let dir = init_temp().await;
    let epic = add_item(dir.path(), "Epic", NewItem::default())
        .await
        .unwrap();
    let story = add_item(dir.path(), "Story", NewItem::default())
        .await
        .unwrap();
    edit_item(dir.path(), &story.id, parent_edit(Some(epic.id)))
        .await
        .unwrap();

    let cleared = edit_item(dir.path(), &story.id, parent_edit(None))
        .await
        .expect("clear parent succeeds");
    assert_eq!(cleared.parent, None);
}

#[tokio::test]
async fn edit_parent_rejects_cycle_and_leaves_item_unchanged() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    // a ← b (parent of b is a). If the parent of a is set to b, it becomes a cycle.
    edit_item(dir.path(), &b.id, parent_edit(Some(a.id.clone())))
        .await
        .unwrap();

    let err = edit_item(dir.path(), &a.id, parent_edit(Some(b.id)))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::ParentCycle { .. }), "got {err:?}");

    // The parent of a remains unset.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(repo.load(&a.id).await.unwrap().parent, None);
}

#[tokio::test]
async fn edit_parent_with_other_field_is_atomic_on_failure() {
    let dir = init_temp().await;
    let epic = add_item(dir.path(), "Epic", NewItem::default())
        .await
        .unwrap();
    let story = add_item(dir.path(), "Story", NewItem::default())
        .await
        .unwrap();

    // Parent is valid but title is empty → EmptyTitle. Parent changes are also not saved (atomicity).
    let edit = ItemEdit {
        title: Some("  ".to_string()),
        parent: Some(Some(epic.id.clone())),
        ..Default::default()
    };
    let err = edit_item(dir.path(), &story.id, edit).await.unwrap_err();
    assert!(matches!(err, Error::EmptyTitle), "got {err:?}");

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        repo.load(&story.id).await.unwrap().parent,
        None,
        "parent must not be persisted when the edit fails"
    );
}

#[tokio::test]
async fn edit_parent_to_missing_parent_returns_not_found() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let err = edit_item(dir.path(), &a.id, parent_edit(Some(ItemId::new("T", 99))))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

// --- reorder ---

/// ID column in current backlog order (ascending rank).
async fn order(dir: &std::path::Path) -> Vec<ItemId> {
    list_items(dir, &ListFilter::default())
        .await
        .unwrap()
        .into_iter()
        .map(|it| it.id)
        .collect()
}

#[tokio::test]
async fn reorder_before_places_item_immediately_before_reference() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    let moved = reorder_item(dir.path(), &c.id, ReorderTarget::Before(b.id.clone()))
        .await
        .expect("reorder succeeds");

    assert_eq!(order(dir.path()).await, [a.id, c.id, b.id]);
    assert_eq!(moved.status, Status::new("todo"), "status unchanged");
}

#[tokio::test]
async fn reorder_after_places_item_immediately_after_reference() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    reorder_item(dir.path(), &a.id, ReorderTarget::After(b.id.clone()))
        .await
        .expect("reorder succeeds");

    assert_eq!(order(dir.path()).await, [b.id, a.id, c.id]);
}

#[tokio::test]
async fn reorder_top_moves_within_same_column_to_head() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    reorder_item(dir.path(), &c.id, ReorderTarget::Top)
        .await
        .expect("reorder succeeds");

    assert_eq!(order(dir.path()).await, [c.id, a.id, b.id]);
}

#[tokio::test]
async fn reorder_lone_item_is_noop_and_does_not_bump_updated() {
    let dir = init_temp().await;
    // Only you in the same state: Top / Bottom The position does not change.
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();

    let moved = reorder_item(dir.path(), &a.id, ReorderTarget::Top)
        .await
        .expect("reorder succeeds");

    assert_eq!(moved.rank, a.rank, "rank unchanged");
    assert_eq!(
        moved.updated, a.updated,
        "no-op reorder must not bump updated (no save)"
    );
}

#[tokio::test]
async fn reorder_bottom_moves_within_same_column_to_tail() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();

    reorder_item(dir.path(), &a.id, ReorderTarget::Bottom)
        .await
        .expect("reorder succeeds");

    assert_eq!(order(dir.path()).await, [b.id, c.id, a.id]);
}

#[tokio::test]
async fn reorder_top_is_scoped_to_same_status() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
    // B to done. Todo columns are A and C.
    move_item(dir.path(), &b.id, "done").await.unwrap();

    // Move C to the top of todo column → C, A within todo. It does not affect the position of B(done).
    let moved = reorder_item(dir.path(), &c.id, ReorderTarget::Top)
        .await
        .expect("reorder succeeds");
    assert_eq!(moved.status, Status::new("todo"), "status unchanged");

    let todo: Vec<_> = list_items(
        dir.path(),
        &ListFilter {
            status: vec!["todo".to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|it| it.id)
    .collect();
    assert_eq!(todo, [c.id, a.id]);
}

#[tokio::test]
async fn reorder_relative_to_self_is_rejected() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();

    let err = reorder_item(dir.path(), &a.id, ReorderTarget::Before(a.id.clone()))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::SelfReference(_)), "got {err:?}");
}

#[tokio::test]
async fn reorder_missing_reference_returns_not_found() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();

    let err = reorder_item(
        dir.path(),
        &a.id,
        ReorderTarget::After(ItemId::new("T", 99)),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn reorder_missing_target_returns_not_found() {
    let dir = init_temp().await;
    let err = reorder_item(dir.path(), &ItemId::new("T", 99), ReorderTarget::Top)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

// --- rebalance ---

/// Overwrite and save the rank of `id` to any (normal) value (for reproducing the bloated state).
async fn set_rank(dir: &Path, id: &ItemId, rank: &str) {
    use crate::storage::FileRepository;
    let repo = FileRepository::new(dir.join(".pinto"));
    let mut item = repo.load(id).await.expect("load");
    item.rank = rank.parse().expect("valid rank");
    repo.save(&item).await.expect("save");
}

/// rank Returns the ID column in ascending order (for order verification).
async fn ids_in_rank_order(dir: &Path) -> Vec<ItemId> {
    list_items(dir, &ListFilter::default())
        .await
        .expect("list")
        .into_iter()
        .map(|it| it.id)
        .collect()
}

#[tokio::test]
async fn rebalance_shortens_ranks_and_preserves_order() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    let c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
    // Expand the rank to a longer rank than the default while maintaining the sort order (a < b < c).
    set_rank(dir.path(), &a.id, "0i").await;
    set_rank(dir.path(), &b.id, "0j").await;
    set_rank(dir.path(), &c.id, "0k").await;

    let order_before = ids_in_rank_order(dir.path()).await;
    let outcome = rebalance(dir.path(), false).await.expect("rebalance");

    assert_eq!(outcome.total, 3);
    assert_eq!(outcome.changed, 3, "all three ranks shortened");
    assert!(
        outcome.after.max_len < outcome.before.max_len,
        "max rank length must shrink ({} -> {})",
        outcome.before.max_len,
        outcome.after.max_len
    );
    assert_eq!(
        ids_in_rank_order(dir.path()).await,
        order_before,
        "rebalance must preserve order"
    );
}

#[tokio::test]
async fn rebalance_is_scoped_to_status_and_parent_siblings() {
    let dir = init_temp().await;
    let root_a = add_item(dir.path(), "Root A", NewItem::default())
        .await
        .unwrap();
    let root_b = add_item(dir.path(), "Root B", NewItem::default())
        .await
        .unwrap();
    let child_a = add_item(
        dir.path(),
        "Child A",
        NewItem {
            parent: Some(root_a.id.clone()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    let child_b = add_item(
        dir.path(),
        "Child B",
        NewItem {
            parent: Some(root_a.id.clone()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    let other_status = add_item(dir.path(), "Other status", NewItem::default())
        .await
        .unwrap();
    move_item(dir.path(), &other_status.id, "in-progress")
        .await
        .unwrap();

    set_rank(dir.path(), &root_a.id, "0i").await;
    set_rank(dir.path(), &root_b.id, "0j").await;
    set_rank(dir.path(), &child_a.id, "0k").await;
    set_rank(dir.path(), &child_b.id, "0l").await;
    set_rank(dir.path(), &other_status.id, "i").await;

    let todo = ListFilter {
        status: vec!["todo".to_string()],
        ..ListFilter::default()
    };
    let todo_order_before: Vec<_> = list_items(dir.path(), &todo)
        .await
        .unwrap()
        .into_iter()
        .map(|item| item.id)
        .collect();
    let other_before = show_item(dir.path(), &other_status.id).await.unwrap();

    let outcome = rebalance(dir.path(), false).await.unwrap();

    assert_eq!(outcome.total, 5);
    assert_eq!(
        outcome.changed, 4,
        "only the two todo sibling groups change"
    );
    assert_eq!(
        list_items(dir.path(), &todo)
            .await
            .unwrap()
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        todo_order_before,
        "each sibling group keeps its relative order"
    );

    for id in [&root_a.id, &root_b.id, &child_a.id, &child_b.id] {
        let item = show_item(dir.path(), id).await.unwrap();
        assert_eq!(item.rank.as_str().len(), 1);
    }
    let other_after = show_item(dir.path(), &other_status.id).await.unwrap();
    assert_eq!(other_after.rank, other_before.rank);
    assert_eq!(other_after.updated, other_before.updated);
}

#[tokio::test]
async fn rebalance_dry_run_reports_but_does_not_write() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    set_rank(dir.path(), &a.id, "0i").await;
    let before = show_item(dir.path(), &a.id).await.unwrap();

    let outcome = rebalance(dir.path(), true).await.expect("rebalance");

    assert_eq!(outcome.changed, 1, "would change one item");
    let after = show_item(dir.path(), &a.id).await.unwrap();
    assert_eq!(after.rank, before.rank, "dry-run must not touch rank");
    assert_eq!(
        after.updated, before.updated,
        "dry-run must not bump updated"
    );
}

#[tokio::test]
async fn rebalance_on_already_short_ranks_changes_nothing() {
    let dir = init_temp().await;
    // Newly added items already fit the shortest width for their sibling scope.
    add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    add_item(dir.path(), "B", NewItem::default()).await.unwrap();

    let outcome = rebalance(dir.path(), false).await.expect("rebalance");
    assert_eq!(outcome.total, 2);
    assert_eq!(outcome.changed, 0, "already-balanced ranks are untouched");
}

// --- $EDITOR Edit (item_edit_template / apply_item_edit)---

#[tokio::test]
async fn editor_template_has_frontmatter_and_guidance() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Template me", NewItem::default())
        .await
        .unwrap();

    let tpl = item_edit_template(dir.path(), &added.id)
        .await
        .expect("template");

    assert!(
        tpl.starts_with("+++\n"),
        "starts with frontmatter delimiter"
    );
    assert!(tpl.contains("# pinto:"), "includes guidance comment");
    assert!(tpl.contains("title = \"Template me\""));
    // It can be parsed even with guidance comments, and editable fields match (no changes) in a round trip.
    let outcome = apply_item_edit(dir.path(), &added.id, &tpl).await.unwrap();
    assert_eq!(outcome, EditOutcome::Unchanged);
}

#[tokio::test]
async fn editor_apply_updates_title_and_body_and_persists() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Before", NewItem::default())
        .await
        .unwrap();

    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    let edited = tpl.replace("title = \"Before\"", "title = \"After\"");
    let edited = format!("{edited}\nRewritten body");

    let outcome = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .expect("apply");
    match outcome {
        EditOutcome::Updated(item) => {
            assert_eq!(item.title, "After");
            assert!(item.body.contains("Rewritten body"), "body applied");
            assert!(item.updated >= added.updated, "updated advanced");
        }
        other => panic!("expected Updated, got {other:?}"),
    }

    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.title, "After");
    assert!(reloaded.body.contains("Rewritten body"));
}

#[tokio::test]
async fn editor_apply_without_changes_returns_unchanged_and_keeps_updated() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Same", NewItem::default())
        .await
        .unwrap();

    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    let outcome = apply_item_edit(dir.path(), &added.id, &tpl)
        .await
        .expect("apply");
    assert_eq!(outcome, EditOutcome::Unchanged);

    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded.updated, added.updated, "updated not bumped");
}

#[tokio::test]
async fn editor_apply_rejects_invalid_content_and_leaves_item_unchanged() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Intact", NewItem::default())
        .await
        .unwrap();

    let err = apply_item_edit(dir.path(), &added.id, "not valid frontmatter\n")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::EditorInvalid { .. }), "got {err:?}");

    let repo = FileRepository::new(dir.path().join(".pinto"));
    let reloaded = repo.load(&added.id).await.unwrap();
    assert_eq!(reloaded, added, "data untouched on invalid edit");
}

#[tokio::test]
async fn editor_apply_rejects_empty_title() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Has title", NewItem::default())
        .await
        .unwrap();

    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    let edited = tpl.replace("title = \"Has title\"", "title = \"\"");
    let err = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::EditorInvalid { .. }), "got {err:?}");
}

#[tokio::test]
async fn editor_apply_ignores_managed_fields() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Managed", NewItem::default())
        .await
        .unwrap();

    // Even if you rewrite status / rank / id, it will not be reflected, only the editable title will be reflected.
    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    let edited = tpl
        .replace("status = \"todo\"", "status = \"done\"")
        .replace(
            &format!("id = \"{}\"", added.id),
            &format!("id = \"{}-999\"", added.id.prefix()),
        )
        .replace("title = \"Managed\"", "title = \"Managed v2\"");

    let outcome = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .expect("apply");
    match outcome {
        EditOutcome::Updated(item) => {
            assert_eq!(item.id, added.id, "id preserved");
            assert_eq!(item.status, added.status, "status preserved");
            assert_eq!(item.rank, added.rank, "rank preserved");
            assert_eq!(item.title, "Managed v2", "title applied");
        }
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[tokio::test]
async fn editor_apply_can_clear_optional_field() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    let new = NewItem {
        sprint: Some("S-1".to_string()),
        ..NewItem::default()
    };
    let added = add_item(dir.path(), "Assigned", new).await.unwrap();

    // Delete sprint line = return to unset (editing not possible with field specification CLI).
    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    assert!(tpl.contains("sprint = \"S-1\""));
    let edited: String = tpl
        .lines()
        .filter(|l| !l.starts_with("sprint = "))
        .collect::<Vec<_>>()
        .join("\n");
    let edited = format!("{edited}\n");

    let outcome = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .expect("apply");
    match outcome {
        EditOutcome::Updated(item) => assert_eq!(item.sprint, None, "sprint cleared"),
        other => panic!("expected Updated, got {other:?}"),
    }
}

#[tokio::test]
async fn editor_apply_rejects_missing_sprint_without_saving() {
    let dir = init_temp().await;
    create_sprint_for_test(dir.path(), "S-1").await;
    let added = add_item(
        dir.path(),
        "Assigned",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();

    let template = item_edit_template(dir.path(), &added.id).await.unwrap();
    let edited = template.replace("sprint = \"S-1\"", "sprint = \"S-9\"");
    let error = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        Error::EditorInvalid { message } if message.contains("sprint not found")
    ));

    let stored = show_item(dir.path(), &added.id).await.unwrap();
    assert_eq!(stored.sprint.as_deref(), Some("S-1"));
}

#[tokio::test]
async fn editor_apply_rejects_missing_parent() {
    let dir = init_temp().await;
    let added = add_item(dir.path(), "Child", NewItem::default())
        .await
        .unwrap();

    let tpl = item_edit_template(dir.path(), &added.id).await.unwrap();
    let edited = tpl.replace("title = \"Child\"", "title = \"Child\"\nparent = \"T-404\"");
    let err = apply_item_edit(dir.path(), &added.id, &edited)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}
