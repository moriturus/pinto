use super::*;
use crate::backlog::{BacklogItem, ItemId};
use crate::error::Error;
use crate::service::test_support::init_temp;
use crate::service::{NewItem, add_item, move_item};
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use crate::storage::{Backend, BacklogItemRepository, FileRepository, SprintRepository};
use chrono::{DateTime, Utc};

fn sid(s: &str) -> SprintId {
    SprintId::new(s).expect("valid sprint id")
}

#[tokio::test]
async fn sprint_assignment_validation_checks_grammar_and_existence() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    create_sprint(
        dir.path(),
        &sid("S-2"),
        "Sprint 2",
        Some("Ship the sprint".to_string()),
        None,
    )
    .await
    .unwrap();
    start_sprint(dir.path(), &sid("S-2")).await.unwrap();
    close_sprint(dir.path(), &sid("S-2"), SprintCloseAction::Retain)
        .await
        .unwrap();
    let repo = Backend::File(FileRepository::new(dir.path().join(".pinto")));

    assert_eq!(
        validate_sprint_assignment(&repo, "S 1").await.unwrap_err(),
        Error::InvalidSprintId("S 1".to_string())
    );
    assert_eq!(
        validate_sprint_assignment(&repo, "S-9").await.unwrap_err(),
        Error::SprintNotFound(sid("S-9"))
    );
    assert_eq!(
        validate_sprint_assignment(&repo, "S-1")
            .await
            .expect("existing sprint validates"),
        sid("S-1")
    );
    assert_eq!(
        validate_sprint_assignment(&repo, "S-2").await.unwrap_err(),
        Error::SprintClosed(sid("S-2"))
    );
}

/// Create a date and time at midnight UTC (for planned schedule tests).
fn date(y: i32, m: u32, d: u32) -> DateTime<Utc> {
    chrono::NaiveDate::from_ymd_opt(y, m, d)
        .expect("valid date")
        .and_hms_opt(0, 0, 0)
        .expect("valid time")
        .and_utc()
}

#[tokio::test]
async fn create_persists_planned_sprint() {
    let dir = init_temp().await;

    let sprint = create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .expect("create succeeds");
    assert_eq!(sprint.title, "Sprint 1");
    assert_eq!(sprint.state, SprintState::Planned);

    // It is made permanent.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let loaded = SprintRepository::load(&repo, &sid("S-1")).await.unwrap();
    assert_eq!(loaded.title, "Sprint 1");
}

#[tokio::test]
async fn create_stores_goal_as_body() {
    let dir = init_temp().await;
    let sprint = create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        Some("Ship the MVP".to_string()),
        None,
    )
    .await
    .unwrap();
    assert_eq!(sprint.goal, "Ship the MVP");
}

#[tokio::test]
async fn create_rejects_duplicate_id() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "First", None, None)
        .await
        .unwrap();

    let err = create_sprint(dir.path(), &sid("S-1"), "Second", None, None)
        .await
        .unwrap_err();
    assert_eq!(err, Error::SprintExists(sid("S-1")));

    // Existing files will not be overwritten.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        SprintRepository::load(&repo, &sid("S-1"))
            .await
            .unwrap()
            .title,
        "First"
    );
}

#[tokio::test]
async fn create_rejects_empty_title() {
    let dir = init_temp().await;
    let err = create_sprint(dir.path(), &sid("S-1"), "   ", None, None)
        .await
        .unwrap_err();
    assert_eq!(err, Error::EmptySprintTitle);
}

#[tokio::test]
async fn create_stores_planned_period() {
    let dir = init_temp().await;
    let start = date(2026, 7, 6);
    let end = date(2026, 7, 20);
    let sprint = create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        None,
        Some((start, end)),
    )
    .await
    .unwrap();
    assert_eq!(sprint.start, Some(start));
    assert_eq!(sprint.end, Some(end));

    // It is made permanent.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let loaded = SprintRepository::load(&repo, &sid("S-1")).await.unwrap();
    assert_eq!(loaded.start, Some(start));
    assert_eq!(loaded.end, Some(end));
}

#[tokio::test]
async fn create_rejects_period_with_start_after_end() {
    let dir = init_temp().await;
    let start = date(2026, 7, 20);
    let end = date(2026, 7, 6);
    let err = create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        None,
        Some((start, end)),
    )
    .await
    .unwrap_err();
    assert_eq!(
        err,
        Error::InvalidSprintPeriod {
            start: start.date_naive(),
            end: end.date_naive(),
        }
    );
}

#[tokio::test]
async fn create_on_uninitialized_dir_prompts_init() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let err = create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotInitialized { .. }), "got {err:?}");
}

#[tokio::test]
async fn edit_updates_title_goal_and_period() {
    let dir = init_temp().await;
    create_sprint(
        dir.path(),
        &sid("S-1"),
        "Initial title",
        Some("Initial goal".to_string()),
        None,
    )
    .await
    .unwrap();

    let start = date(2026, 8, 3);
    let end = date(2026, 8, 14);
    let edited = edit_sprint(
        dir.path(),
        &sid("S-1"),
        Some("Updated title".to_string()),
        Some("Updated goal".to_string()),
        Some((start, end)),
    )
    .await
    .expect("edit succeeds");

    assert_eq!(edited.title, "Updated title");
    assert_eq!(edited.goal, "Updated goal");
    assert_eq!(edited.start, Some(start));
    assert_eq!(edited.end, Some(end));
}

#[tokio::test]
async fn start_moves_planned_to_active_and_persists() {
    let dir = init_temp().await;
    create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        Some("Ship the sprint".to_string()),
        None,
    )
    .await
    .unwrap();

    let started = start_sprint(dir.path(), &sid("S-1")).await.unwrap();
    assert_eq!(started.state, SprintState::Active);

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        SprintRepository::load(&repo, &sid("S-1"))
            .await
            .unwrap()
            .state,
        SprintState::Active
    );
}

#[tokio::test]
async fn close_moves_active_to_closed() {
    let dir = init_temp().await;
    create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        Some("Ship the sprint".to_string()),
        None,
    )
    .await
    .unwrap();
    start_sprint(dir.path(), &sid("S-1")).await.unwrap();

    let closed = close_sprint(dir.path(), &sid("S-1"), SprintCloseAction::Retain)
        .await
        .unwrap();
    assert_eq!(closed.state, SprintState::Closed);
}

#[tokio::test]
async fn close_rollover_moves_only_unfinished_items_and_snapshots_spillover() {
    let dir = init_temp().await;
    create_sprint(
        dir.path(),
        &sid("S-1"),
        "Source",
        Some("Ship it".to_string()),
        None,
    )
    .await
    .unwrap();
    create_sprint(dir.path(), &sid("S-2"), "Target", None, None)
        .await
        .unwrap();
    let completed = add_item(
        dir.path(),
        "Completed",
        NewItem {
            points: Some(3),
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    let unfinished = add_item(
        dir.path(),
        "Unfinished",
        NewItem {
            points: Some(5),
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    let unestimated = add_item(
        dir.path(),
        "Unestimated",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    let completed = move_item(dir.path(), &completed.id, "done").await.unwrap();
    start_sprint(dir.path(), &sid("S-1")).await.unwrap();

    let closed = close_sprint(
        dir.path(),
        &sid("S-1"),
        SprintCloseAction::Rollover(sid("S-2")),
    )
    .await
    .unwrap();

    assert_eq!(
        closed.spillover,
        SprintSpillover {
            points: 5,
            items: 2,
            unestimated_items: 1,
        }
    );
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &completed.id)
            .await
            .unwrap(),
        completed,
        "completed PBI is not rewritten"
    );
    assert_eq!(
        BacklogItemRepository::load(&repo, &unfinished.id)
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-2")
    );
    assert_eq!(
        BacklogItemRepository::load(&repo, &unestimated.id)
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-2")
    );
}

#[tokio::test]
async fn close_rejects_invalid_rollover_targets_before_mutation() {
    let dir = init_temp().await;
    for (id, title) in [("S-1", "Source"), ("S-2", "Closed target")] {
        create_sprint(
            dir.path(),
            &sid(id),
            title,
            Some("Ship it".to_string()),
            None,
        )
        .await
        .unwrap();
    }
    let item = add_item(
        dir.path(),
        "Unfinished",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();
    start_sprint(dir.path(), &sid("S-1")).await.unwrap();
    start_sprint(dir.path(), &sid("S-2")).await.unwrap();
    close_sprint(dir.path(), &sid("S-2"), SprintCloseAction::Retain)
        .await
        .unwrap();

    assert_eq!(
        close_sprint(
            dir.path(),
            &sid("S-1"),
            SprintCloseAction::Rollover(sid("S-404")),
        )
        .await
        .unwrap_err(),
        Error::SprintNotFound(sid("S-404"))
    );
    assert!(matches!(
        close_sprint(
            dir.path(),
            &sid("S-1"),
            SprintCloseAction::Rollover(sid("S-1")),
        )
        .await
        .unwrap_err(),
        Error::InvalidFilterOption(message) if message.contains("itself")
    ));
    assert_eq!(
        close_sprint(
            dir.path(),
            &sid("S-1"),
            SprintCloseAction::Rollover(sid("S-2")),
        )
        .await
        .unwrap_err(),
        Error::SprintClosed(sid("S-2"))
    );

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        SprintRepository::load(&repo, &sid("S-1"))
            .await
            .unwrap()
            .state,
        SprintState::Active
    );
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-1")
    );
}

#[tokio::test]
async fn close_from_planned_is_rejected() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();

    let err = close_sprint(dir.path(), &sid("S-1"), SprintCloseAction::Retain)
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::InvalidSprintTransition { .. }),
        "got {err:?}"
    );
}

#[tokio::test]
async fn start_missing_sprint_returns_not_found() {
    let dir = init_temp().await;
    let err = start_sprint(dir.path(), &sid("S-9")).await.unwrap_err();
    assert_eq!(err, Error::SprintNotFound(sid("S-9")));
}

#[tokio::test]
async fn assign_sets_item_sprint_and_persists() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    let assigned = assign_sprint(dir.path(), &sid("S-1"), &item.id)
        .await
        .expect("assign succeeds");
    assert_eq!(assigned.sprint.as_deref(), Some("S-1"));

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-1")
    );
}

#[tokio::test]
async fn assign_to_missing_sprint_returns_not_found() {
    let dir = init_temp().await;
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    let err = assign_sprint(dir.path(), &sid("S-9"), &item.id)
        .await
        .unwrap_err();
    assert_eq!(err, Error::SprintNotFound(sid("S-9")));
    // Not assigned.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn assign_missing_item_returns_not_found() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let err = assign_sprint(dir.path(), &sid("S-1"), &ItemId::new("T", 99))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
}

#[tokio::test]
async fn bulk_assignment_selects_matching_items_in_rank_order_up_to_limit() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    for title in ["First", "Second", "Third"] {
        add_item(dir.path(), title, NewItem::default())
            .await
            .unwrap();
    }
    move_item(dir.path(), &ItemId::new("T", 3), "in-progress")
        .await
        .unwrap();

    let assigned = assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", Some(2))
        .await
        .expect("bulk assignment succeeds");

    assert_eq!(
        assigned
            .iter()
            .map(|item| item.id.to_string())
            .collect::<Vec<_>>(),
        ["T-1", "T-2"]
    );
    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &ItemId::new("T", 1))
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-1")
    );
    assert_eq!(
        BacklogItemRepository::load(&repo, &ItemId::new("T", 2))
            .await
            .unwrap()
            .sprint
            .as_deref(),
        Some("S-1")
    );
    assert_eq!(
        BacklogItemRepository::load(&repo, &ItemId::new("T", 3))
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn bulk_assignment_without_limit_assigns_all_matching_items_and_skips_target_members() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    for title in ["First", "Second", "Third"] {
        add_item(dir.path(), title, NewItem::default())
            .await
            .unwrap();
    }
    assign_sprint(dir.path(), &sid("S-1"), &ItemId::new("T", 1))
        .await
        .unwrap();
    move_item(dir.path(), &ItemId::new("T", 3), "in-progress")
        .await
        .unwrap();

    let assigned = assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", None)
        .await
        .expect("bulk assignment succeeds");

    assert_eq!(
        assigned
            .iter()
            .map(|item| item.id.to_string())
            .collect::<Vec<_>>(),
        ["T-2"]
    );
}

#[tokio::test]
async fn bulk_assignment_limit_does_not_count_target_members() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    for title in ["Already assigned", "Next in rank"] {
        add_item(dir.path(), title, NewItem::default())
            .await
            .unwrap();
    }
    assign_sprint(dir.path(), &sid("S-1"), &ItemId::new("T", 1))
        .await
        .unwrap();

    let assigned = assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", Some(1))
        .await
        .expect("bulk assignment succeeds");

    assert_eq!(
        assigned
            .iter()
            .map(|item| item.id.to_string())
            .collect::<Vec<_>>(),
        ["T-2"]
    );
}

#[tokio::test]
async fn bulk_assignment_rejects_other_sprint_without_partial_assignment() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    create_sprint(dir.path(), &sid("S-2"), "Sprint 2", None, None)
        .await
        .unwrap();
    for title in ["Already assigned", "Still todo"] {
        add_item(dir.path(), title, NewItem::default())
            .await
            .unwrap();
    }
    assign_sprint(dir.path(), &sid("S-2"), &ItemId::new("T", 1))
        .await
        .unwrap();

    let err = assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", None)
        .await
        .unwrap_err();
    assert!(
        matches!(&err, Error::InvalidFilterOption(message) if message.contains("T-1") && message.contains("S-2")),
        "got {err:?}"
    );

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &ItemId::new("T", 2))
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn bulk_assignment_rejects_unknown_status_and_zero_limit_without_changes() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    assert_eq!(
        assign_sprint_by_status(dir.path(), &sid("S-1"), "missing", None)
            .await
            .unwrap_err(),
        Error::UnknownStatus("missing".to_string())
    );
    assert!(matches!(
        assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", Some(0))
            .await
            .unwrap_err(),
        Error::InvalidFilterOption(message) if message.contains("limit")
    ));

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn bulk_assignment_rejects_missing_or_closed_sprint_without_changes() {
    let dir = init_temp().await;
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    assert_eq!(
        assign_sprint_by_status(dir.path(), &sid("S-9"), "todo", None)
            .await
            .unwrap_err(),
        Error::SprintNotFound(sid("S-9"))
    );

    create_sprint(
        dir.path(),
        &sid("S-1"),
        "Sprint 1",
        Some("Ship it".to_string()),
        None,
    )
    .await
    .unwrap();
    start_sprint(dir.path(), &sid("S-1")).await.unwrap();
    close_sprint(dir.path(), &sid("S-1"), SprintCloseAction::Retain)
        .await
        .unwrap();
    assert_eq!(
        assign_sprint_by_status(dir.path(), &sid("S-1"), "todo", None)
            .await
            .unwrap_err(),
        Error::SprintClosed(sid("S-1"))
    );

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn unassign_clears_item_sprint() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();
    assign_sprint(dir.path(), &sid("S-1"), &item.id)
        .await
        .unwrap();

    let cleared = unassign_sprint(dir.path(), &sid("S-1"), &item.id)
        .await
        .expect("unassign succeeds");
    assert_eq!(cleared.sprint, None);
}

#[tokio::test]
async fn unassign_item_not_in_sprint_returns_error() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let item = add_item(dir.path(), "Task", NewItem::default())
        .await
        .unwrap();

    let err = unassign_sprint(dir.path(), &sid("S-1"), &item.id)
        .await
        .unwrap_err();
    assert_eq!(
        err,
        Error::NotInSprint {
            item: item.id,
            sprint: sid("S-1"),
        }
    );
}

#[tokio::test]
async fn delete_clears_assignments_before_removing_sprint() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "Sprint 1", None, None)
        .await
        .unwrap();
    let item = add_item(
        dir.path(),
        "Assigned task",
        NewItem {
            sprint: Some("S-1".to_string()),
            ..NewItem::default()
        },
    )
    .await
    .unwrap();

    delete_sprint(dir.path(), &sid("S-1"))
        .await
        .expect("delete succeeds");

    let repo = FileRepository::new(dir.path().join(".pinto"));
    assert!(matches!(
        SprintRepository::load(&repo, &sid("S-1")).await,
        Err(Error::SprintNotFound(_))
    ));
    assert_eq!(
        BacklogItemRepository::load(&repo, &item.id)
            .await
            .unwrap()
            .sprint,
        None
    );
}

#[tokio::test]
async fn list_returns_sprints_in_creation_order() {
    let dir = init_temp().await;
    create_sprint(dir.path(), &sid("S-1"), "First", None, None)
        .await
        .unwrap();
    create_sprint(dir.path(), &sid("S-2"), "Second", None, None)
        .await
        .unwrap();

    let ids: Vec<String> = list_sprints(dir.path())
        .await
        .expect("list succeeds")
        .into_iter()
        .map(|s| s.id.as_str().to_string())
        .collect();
    assert_eq!(ids, ["S-1", "S-2"]);
}

#[tokio::test]
async fn list_on_empty_board_is_empty() {
    let dir = init_temp().await;
    assert!(list_sprints(dir.path()).await.unwrap().is_empty());
}

fn sprint_with_capacity(id: &str, hours: f64) -> Sprint {
    let mut sprint = Sprint::new(sid(id), id, date(2026, 7, 1)).expect("valid sprint");
    sprint.start = Some(date(2026, 7, 1));
    sprint.end = Some(date(2026, 7, 1));
    sprint.set_capacity(hours, 0, 1.0).expect("valid capacity");
    sprint
}

fn sprint_with_state(id: &str, state: SprintState) -> Sprint {
    let mut sprint = Sprint::new(sid(id), id, date(2026, 7, 1)).expect("valid sprint");
    sprint.goal = "Ship it".to_string();
    if matches!(state, SprintState::Active | SprintState::Closed) {
        sprint.start(date(2026, 7, 2)).expect("start sprint");
    }
    if state == SprintState::Closed {
        sprint
            .close(date(2026, 7, 3), SprintSpillover::default())
            .expect("close sprint");
    }
    sprint
}

fn item_in_sprint(
    number: u32,
    sprint: &str,
    points: Option<u32>,
    done_at: Option<DateTime<Utc>>,
) -> BacklogItem {
    let mut item = BacklogItem::new(
        ItemId::new("T", number),
        format!("Item {number}"),
        crate::backlog::Status::new("todo"),
        crate::rank::Rank::after(None),
        date(2026, 7, 1),
    )
    .expect("valid item");
    item.sprint = Some(sprint.to_string());
    item.points = points;
    item.done_at = done_at;
    item
}

#[test]
fn sprint_load_warning_respects_capacity_equality_and_ignores_unestimated_points() {
    let target = sprint_with_capacity("S-2", 5.0);
    let items = [
        item_in_sprint(1, "S-2", Some(2), None),
        item_in_sprint(2, "S-2", Some(3), None),
        item_in_sprint(3, "S-2", None, None),
    ];

    assert!(sprint_load_warnings_for(&target, std::slice::from_ref(&target), &items).is_empty());

    let over = [
        item_in_sprint(1, "S-2", Some(3), None),
        item_in_sprint(2, "S-2", Some(3), None),
        item_in_sprint(3, "S-2", None, None),
    ];
    assert_eq!(
        sprint_load_warnings_for(&target, std::slice::from_ref(&target), &over),
        vec![SprintLoadWarning {
            kind: SprintLoadWarningKind::Capacity,
            points: 6,
            threshold: 5.0,
        }]
    );
}

#[test]
fn sprint_load_warning_uses_recent_closed_sprint_velocity() {
    let first = sprint_with_state("S-1", SprintState::Closed);
    let second = sprint_with_state("S-2", SprintState::Closed);
    let target = sprint_with_capacity("S-3", 100.0);
    let sprints = [first, second, target.clone()];
    let items = [
        item_in_sprint(1, "S-1", Some(4), Some(date(2026, 7, 2))),
        item_in_sprint(2, "S-2", Some(6), Some(date(2026, 7, 2))),
        item_in_sprint(3, "S-3", Some(6), None),
    ];

    assert_eq!(
        sprint_load_warnings_for(&target, &sprints, &items),
        vec![SprintLoadWarning {
            kind: SprintLoadWarningKind::Velocity,
            points: 6,
            threshold: 5.0,
        }]
    );
}

#[test]
fn sprint_load_warning_is_empty_without_capacity_or_history() {
    let target = Sprint::new(sid("S-1"), "S-1", date(2026, 7, 1)).expect("valid sprint");
    let items = [item_in_sprint(1, "S-1", Some(8), None)];

    assert!(sprint_load_warnings_for(&target, std::slice::from_ref(&target), &items).is_empty());
}
