//! Unit tests for reordering and rank rebalancing.

use super::super::*;
use crate::error::Error;
use crate::service::test_support::init_temp;
use crate::storage::BacklogItemRepository;
use std::path::Path;

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

#[tokio::test]
async fn move_repegs_rank_onto_the_destination_scope_tail() {
    // Rank values are unique only within a (status, parent) scope, so a move must
    // not carry a rank into a destination scope that already holds it — the
    // P-24/P-43 production collision. The moved item is re-pegged onto the tail
    // instead, leaving the scope free of duplicates.
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    move_item(dir.path(), &b.id, "in-progress").await.unwrap();
    // Force the exact cross-scope duplicate a per-scope rebalance can leave behind:
    // A (todo) and B (in-progress) both at "i".
    set_rank(dir.path(), &a.id, "i").await;
    set_rank(dir.path(), &b.id, "i").await;

    let moved = move_item(dir.path(), &b.id, "todo").await.unwrap();

    assert_eq!(moved.status, Status::new("todo"), "status changed to todo");
    let a_after = show_item(dir.path(), &a.id).await.unwrap();
    assert_ne!(
        moved.rank, a_after.rank,
        "moved item must not reuse the rank already held in the destination scope"
    );
    assert!(
        a_after.rank < moved.rank,
        "moved item lands at the destination tail ({} < {})",
        a_after.rank,
        moved.rank
    );
    let report = crate::service::doctor(dir.path(), false).await.unwrap();
    assert!(
        !report
            .issues
            .iter()
            .any(|issue| issue.kind == crate::service::DoctorIssueKind::RankAnomaly),
        "move must not create a rank anomaly: {:?}",
        report.issues
    );
}

#[tokio::test]
async fn move_to_the_same_status_preserves_rank() {
    // Moving to the current column does not change scope, so the per-scope
    // invariant already holds and the rank must be left untouched (no re-peg).
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();

    let moved = move_item(dir.path(), &b.id, "todo").await.unwrap();

    assert_eq!(
        moved.rank, b.rank,
        "same-status move must not re-peg the rank"
    );
    let a_after = show_item(dir.path(), &a.id).await.unwrap();
    assert_eq!(a_after.rank, a.rank, "the untouched sibling keeps its rank");
}

#[tokio::test]
async fn move_without_a_rank_collision_keeps_the_carried_rank() {
    // A cross-column move into a scope that does not already hold the item's rank
    // keeps the rank untouched: position travels with the item, and only an actual
    // collision forces a re-peg.
    let dir = init_temp().await;
    let _a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();

    // in-progress is empty, so b's rank cannot collide there.
    let moved = move_item(dir.path(), &b.id, "in-progress").await.unwrap();

    assert_eq!(
        moved.rank, b.rank,
        "a non-colliding move must preserve the carried rank"
    );
    assert_eq!(moved.status, Status::new("in-progress"), "status changed");
}

#[tokio::test]
async fn rebalance_rewrites_scope_with_literal_duplicate_rank_at_canonical_length() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    // Corrupt the board with a literal duplicate at the already-canonical width;
    // the length-only gate must not skip this scope.
    set_rank(dir.path(), &a.id, "i").await;
    set_rank(dir.path(), &b.id, "i").await;

    let outcome = rebalance(dir.path(), false).await.expect("rebalance");

    assert!(
        outcome.changed > 0,
        "a scope containing a literal duplicate rank must be rewritten"
    );
    let a_after = show_item(dir.path(), &a.id).await.unwrap();
    let b_after = show_item(dir.path(), &b.id).await.unwrap();
    assert_ne!(
        a_after.rank, b_after.rank,
        "rebalance must resolve literal duplicate ranks within a scope"
    );
}

#[tokio::test]
async fn doctor_reports_healthy_after_rebalance_resolves_duplicate_ranks() {
    let dir = init_temp().await;
    let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
    let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
    set_rank(dir.path(), &a.id, "i").await;
    set_rank(dir.path(), &b.id, "i").await;

    let before = crate::service::doctor(dir.path(), false).await.unwrap();
    assert!(
        before
            .issues
            .iter()
            .any(|issue| issue.kind == crate::service::DoctorIssueKind::RankAnomaly),
        "duplicate rank must be flagged before repair"
    );

    rebalance(dir.path(), false).await.expect("rebalance");

    let after = crate::service::doctor(dir.path(), false).await.unwrap();
    assert!(
        !after
            .issues
            .iter()
            .any(|issue| issue.kind == crate::service::DoctorIssueKind::RankAnomaly),
        "rebalance must clear the rank anomaly: {:?}",
        after.issues
    );
}
