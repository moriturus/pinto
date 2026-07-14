//! Editing a backlog item: field edits and the `$EDITOR` round-trip.

use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::service::relations::validate_parent;
use crate::service::{open_board, open_board_locked, validate_sprint_assignment};
use crate::storage::BacklogItemRepository;
use chrono::Utc;
use std::path::{Path, PathBuf};

/// Fields to update with [`edit_item`]. A `None` field remains unchanged.
///
/// `labels` replaces the entire existing set of labels if you pass `Some`.
#[derive(Debug, Default, Clone)]
pub struct ItemEdit {
    /// New title; a blank value returns [`Error::EmptyTitle`].
    pub title: Option<String>,
    /// Story points.
    pub points: Option<u32>,
    /// Replacement label set when `Some`.
    pub labels: Option<Vec<String>>,
    /// Assignee.
    pub assignee: Option<String>,
    /// Sprint ID assignment.
    pub sprint: Option<String>,
    /// Markdown body, including Acceptance Criteria.
    pub body: Option<String>,
    /// Parent change: `None` leaves it unchanged, `Some(None)` clears it, and `Some(Some(id))`
    /// assigns the parent.
    ///
    /// Parent-child links remain acyclic; a cycle returns [`Error::ParentCycle`].
    pub parent: Option<Option<ItemId>>,
}

impl ItemEdit {
    /// Are there no fields specified to update?
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.points.is_none()
            && self.labels.is_none()
            && self.assignee.is_none()
            && self.sprint.is_none()
            && self.body.is_none()
            && self.parent.is_none()
    }
}

/// Update the specified fields of PBI `id`, refresh `updated`, and return the saved [`BacklogItem`].
///
/// Fields set to `None` remain unchanged. Validate every field before saving, so an error leaves
/// the on-disk item untouched. Return [`Error::NothingToUpdate`] when no field is supplied,
/// [`Error::EmptyTitle`] for a blank title, [`Error::ParentCycle`] for a cyclic parent, and
/// [`Error::NotFound`] when the item or proposed parent does not exist. Return
/// [`Error::NotInitialized`] for an uninitialized board.
pub async fn edit_item(project_dir: &Path, id: &ItemId, edit: ItemEdit) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;

    if edit.is_empty() {
        return Err(Error::NothingToUpdate);
    }
    if let Some(title) = &edit.title
        && title.trim().is_empty()
    {
        return Err(Error::EmptyTitle);
    }
    let validated_sprint = match edit.sprint.as_deref() {
        Some(raw) => Some(validate_sprint_assignment(&repo, raw).await?),
        None => None,
    };

    let mut item = repo.load(id).await?;

    // Parent validation needs the complete graph; perform it before changing any fields so the
    // update remains atomic.
    if let Some(Some(parent)) = &edit.parent {
        let items = repo.list().await?;
        validate_parent(&items, id, parent)?;
    }

    if let Some(title) = edit.title {
        item.title = title;
    }
    if let Some(points) = edit.points {
        item.points = Some(points);
    }
    if let Some(labels) = edit.labels {
        item.labels = labels;
    }
    if let Some(assignee) = edit.assignee {
        item.assignee = Some(assignee);
    }
    if let Some(sprint) = validated_sprint {
        item.sprint = Some(sprint.to_string());
    }
    if let Some(body) = edit.body {
        item.body = body;
    }
    if let Some(parent) = edit.parent {
        item.parent = parent;
    }
    item.updated = Utc::now();

    repo.save(&item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(item)
}

/// Format PBI `id` as the Markdown template used for `$EDITOR` editing.
///
/// Use the normal `+++` frontmatter format and insert TOML-comment guidance for editable fields.
/// This read-only operation does not acquire the board lock. Return [`Error::NotInitialized`] for
/// an uninitialized board or [`Error::NotFound`] when `id` does not exist.
pub async fn item_edit_template(project_dir: &Path, id: &ItemId) -> Result<String> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let item = repo.load(id).await?;
    let markdown = crate::storage::item_to_markdown(&item)?;
    Ok(with_edit_guidance(&markdown))
}

/// Insert editing guidance (as TOML comments) right after the frontmatter delimiter.
///
/// The guidance is localized through the i18n layer so the selected locale is honored (the
/// hard-coded text used to leak Japanese regardless of `$LANG`). TOML ignores `#` comment
/// lines, so the guidance round-trips harmlessly: `id` / `status` / `rank` / `depends_on` and
/// the timestamps are owned by pinto and cannot be changed here.
fn with_edit_guidance(markdown: &str) -> String {
    let guide = crate::i18n::current().text(crate::i18n::Message::EditGuidance);
    match markdown.split_once('\n') {
        // The frontmatter always opens with `+++`; insert the guidance immediately after it.
        Some((first, rest)) if first == "+++" => format!("{first}\n{guide}\n{rest}"),
        _ => markdown.to_string(),
    }
}

/// Result of [`apply_item_edit`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// The edited contents were reflected and saved (updated PBI).
    ///
    /// Wrap it in `Box` to avoid size difference between variants (`Unchanged` has no data).
    Updated(Box<BacklogItem>),
    /// There was no change in the content and it was not saved.
    Unchanged,
}

/// Validate Markdown edited with `$EDITOR`, apply permitted fields to PBI `id`, and save it.
///
/// Only `title`, `points`, `labels`, `assignee`, `sprint`, `parent`, and the body are editable.
/// pinto retains `id`, `status`, `rank`, `depends_on`, and timestamps; use `move`, `reorder`, and
/// `dep` to change the managed fields.
///
/// Validate everything before saving, so invalid edits do not change on-disk state. Syntax errors
/// and empty titles return [`Error::EditorInvalid`]; a missing parent returns [`Error::NotFound`];
/// a cyclic parent returns [`Error::ParentCycle`]. If no editable field changes, return
/// [`EditOutcome::Unchanged`] without updating the timestamp.
pub async fn apply_item_edit(project_dir: &Path, id: &ItemId, edited: &str) -> Result<EditOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let stored = repo.load(id).await?;

    // Parse the edited text and convert syntax or validation failures into user-correctable errors.
    let display_path = PathBuf::from(format!("{id}.md"));
    let parsed = crate::storage::item_from_markdown(edited, &display_path).map_err(|e| {
        Error::EditorInvalid {
            message: e.to_string(),
        }
    })?;
    let validated_sprint = match parsed.sprint.as_deref() {
        Some(raw) => Some(
            validate_sprint_assignment(&repo, raw)
                .await
                .map_err(|error| Error::EditorInvalid {
                    message: error.to_string(),
                })?,
        ),
        None => None,
    };

    // Copy only editable fields; retain all pinto-managed values from the stored item.
    let mut updated = stored.clone();
    updated.title = parsed.title;
    updated.points = parsed.points;
    updated.labels = parsed.labels;
    updated.assignee = parsed.assignee;
    updated.sprint = validated_sprint.map(|sprint| sprint.to_string());
    updated.body = parsed.body;

    // Validate a changed parent against the full graph; skip the scan when the parent is unchanged.
    if updated.parent != parsed.parent {
        if let Some(parent) = &parsed.parent {
            let items = repo.list().await?;
            validate_parent(&items, id, parent)?;
        }
        updated.parent = parsed.parent;
    }

    // If all editable fields match, avoid writing and leave `updated` unchanged.
    if updated == stored {
        return Ok(EditOutcome::Unchanged);
    }
    updated.updated = Utc::now();
    repo.save(&updated).await?;
    repo.commit(&format!("pinto: update {}", updated.id))
        .await?;
    Ok(EditOutcome::Updated(Box::new(updated)))
}
