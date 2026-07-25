//! Unit tests for splitting a PBI into new PBIs.

use super::super::*;
use crate::backlog::ItemId;
use crate::error::Error;
use crate::service::test_support::init_temp;
use crate::storage::{BacklogItemRepository, FileRepository};

/// Add a source item with a body and return its id.
async fn seed_source(dir: &std::path::Path, title: &str, body: &str) -> ItemId {
    let new = NewItem {
        body: body.to_string(),
        ..NewItem::default()
    };
    add_item(dir, title, new).await.expect("add source").id
}

#[tokio::test]
async fn split_copies_source_body_by_default() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Big story", "- [ ] shared criteria").await;

    let outcome = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["Slice A".to_string()],
            ..SplitSpec::default()
        },
    )
    .await
    .expect("split succeeds");

    assert_eq!(outcome.created.len(), 1);
    let child = &outcome.created[0];
    assert_eq!(child.title, "Slice A");
    assert_eq!(child.body, "- [ ] shared criteria");
    assert_eq!(child.status, Status::new("todo"));
    assert_eq!(child.parent, None);
    assert!(child.depends_on.is_empty());

    // The new item is persisted with the copied body.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let loaded = repo.load(&child.id).await.expect("load child");
    assert_eq!(loaded.body, "- [ ] shared criteria");
}

#[tokio::test]
async fn split_supports_empty_and_explicit_bodies() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Story", "original body").await;

    let empty = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["Empty slice".to_string()],
            body: SplitBody::Empty,
            ..SplitSpec::default()
        },
    )
    .await
    .expect("empty split");
    assert_eq!(empty.created[0].body, "");

    let explicit = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["Explicit slice".to_string()],
            body: SplitBody::Explicit("custom text".to_string()),
            ..SplitSpec::default()
        },
    )
    .await
    .expect("explicit split");
    assert_eq!(explicit.created[0].body, "custom text");
}

#[tokio::test]
async fn split_creates_multiple_items_with_incrementing_ids() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Story", "body").await;

    let outcome = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["A".to_string(), "B".to_string(), "C".to_string()],
            ..SplitSpec::default()
        },
    )
    .await
    .expect("split succeeds");

    let ids: Vec<_> = outcome.created.iter().map(|item| item.id.clone()).collect();
    assert_eq!(
        ids,
        vec![
            ItemId::new("T", 2),
            ItemId::new("T", 3),
            ItemId::new("T", 4)
        ]
    );
    // Ranks must be strictly increasing so the new items keep a stable backlog order.
    assert!(outcome.created[0].rank < outcome.created[1].rank);
    assert!(outcome.created[1].rank < outcome.created[2].rank);
}

#[tokio::test]
async fn split_child_relationship_parents_new_items_under_source() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Parent story", "body").await;

    let outcome = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["Child A".to_string(), "Child B".to_string()],
            relationship: SplitRelationship::Child,
            ..SplitSpec::default()
        },
    )
    .await
    .expect("split succeeds");

    for child in &outcome.created {
        assert_eq!(child.parent.as_ref(), Some(&source));
    }
    // The source is unchanged for a parent-child split.
    assert!(outcome.source.depends_on.is_empty());
}

#[tokio::test]
async fn split_dependency_relationship_makes_source_depend_on_new_items() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Umbrella", "body").await;

    let outcome = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["Piece A".to_string(), "Piece B".to_string()],
            relationship: SplitRelationship::Dependency,
            ..SplitSpec::default()
        },
    )
    .await
    .expect("split succeeds");

    let created_ids: Vec<_> = outcome.created.iter().map(|item| item.id.clone()).collect();
    assert_eq!(outcome.source.depends_on, created_ids);

    // Persisted source reflects the new dependencies.
    let repo = FileRepository::new(dir.path().join(".pinto"));
    let loaded = repo.load(&source).await.expect("load source");
    assert_eq!(loaded.depends_on, created_ids);
    // Dependency splits leave the new items without a parent.
    assert!(outcome.created.iter().all(|item| item.parent.is_none()));
}

#[tokio::test]
async fn split_rejects_missing_source() {
    let dir = init_temp().await;

    let error = split_item(
        dir.path(),
        &ItemId::new("T", 99),
        SplitSpec {
            titles: vec!["Slice".to_string()],
            ..SplitSpec::default()
        },
    )
    .await
    .expect_err("missing source is rejected");
    assert!(matches!(error, Error::NotFound(id) if id == ItemId::new("T", 99)));
}

#[tokio::test]
async fn split_rejects_empty_or_blank_titles() {
    let dir = init_temp().await;
    let source = seed_source(dir.path(), "Story", "body").await;

    let no_titles = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: Vec::new(),
            ..SplitSpec::default()
        },
    )
    .await
    .expect_err("no titles is rejected");
    assert!(matches!(no_titles, Error::EmptyTitle));

    let blank_title = split_item(
        dir.path(),
        &source,
        SplitSpec {
            titles: vec!["   ".to_string()],
            ..SplitSpec::default()
        },
    )
    .await
    .expect_err("blank title is rejected");
    assert!(matches!(blank_title, Error::EmptyTitle));
}
