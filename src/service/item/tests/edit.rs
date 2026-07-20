//! Unit tests for field edits, parent changes, and $EDITOR-based editing.

use super::super::*;
use crate::error::Error;
use crate::service::test_support::{create_sprint_for_test, init_temp, parent_edit};
use crate::sprint::SprintId;
use crate::storage::{BacklogItemRepository, FileRepository};
use tempfile::TempDir;

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
