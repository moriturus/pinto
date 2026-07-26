//! Split an existing PBI into one or more new PBIs.
//!
//! Splitting keeps the source item and derives fresh PBIs from it, optionally wiring a
//! relationship (parent-child or dependency) between the source and each new item. The new bodies
//! copy the source by default, but the caller may request an empty body or supply explicit text
//! (a template is resolved to explicit text before reaching this layer).

use crate::backlog::{BacklogItem, ItemId, Status};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::service::{open_board_locked, restore_board_after_failure};
use crate::storage::{BacklogItemRepository, BoardRecoveryPoint};
use chrono::Utc;
use std::path::Path;

/// How each new PBI relates to the source item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SplitRelationship {
    /// The new PBIs are independent of the source.
    #[default]
    None,
    /// The source becomes the parent of each new PBI.
    Child,
    /// The source depends on each new PBI (each new PBI must be completed first).
    Dependency,
}

/// Body assigned to every new PBI produced by a split.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SplitBody {
    /// Copy the source item's body.
    #[default]
    Copy,
    /// Start each new PBI with an empty body.
    Empty,
    /// Use the supplied text (a resolved template is passed as explicit text).
    Explicit(String),
}

/// Instructions for [`split_item`].
#[derive(Debug, Clone, Default)]
pub struct SplitSpec {
    /// Titles of the new PBIs to create, in order. Must contain at least one non-blank title.
    pub titles: Vec<String>,
    /// Relationship wired between the source and each new PBI.
    pub relationship: SplitRelationship,
    /// Body assigned to each new PBI.
    pub body: SplitBody,
}

/// Result of [`split_item`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitOutcome {
    /// Source PBI the new items were derived from, reflecting any relationship update.
    pub source: BacklogItem,
    /// Newly persisted PBIs, in the order their titles were supplied.
    pub created: Vec<BacklogItem>,
}

/// Split the PBI `source` into the new PBIs described by `spec`.
///
/// Each new PBI is appended to the backlog with the first workflow column as its status, exactly as
/// [`crate::service::add_item`] does. When `spec.relationship` is [`SplitRelationship::Child`] the
/// source becomes the parent of every new PBI; when it is [`SplitRelationship::Dependency`] the
/// source gains a dependency on every new PBI. Splitting cannot introduce a cycle because the new
/// items carry no other edges, so no cycle warning is produced.
///
/// Return [`Error::NotInitialized`] for an uninitialized board, [`Error::NotFound`] when `source`
/// is absent, or [`Error::EmptyTitle`] when `spec.titles` is empty or contains a blank title.
///
/// # Errors
///
/// Returns [`Error::EmptyTitle`], [`Error::NotInitialized`], [`Error::NotFound`], ID/rank or
/// configuration validation errors, persistence errors, and Git commit errors. Title and source
/// validation occurs before the first write. A failed batch write invokes board recovery; if
/// recovery itself fails, durable partial records may remain. After inspecting the board, retrying
/// is safe only when board recovery has completed; if recovery or the final commit fails, the
/// operation is not idempotent and a blind retry could create another set of PBIs. Inspect the
/// board and either resume from the persisted state or repair it before retrying.
pub async fn split_item(
    project_dir: &Path,
    source: &ItemId,
    spec: SplitSpec,
) -> Result<SplitOutcome> {
    let SplitSpec {
        titles,
        relationship,
        body,
    } = spec;
    // Reject an empty request or any blank title before taking the board lock so a bad request
    // fails fast and a partial split never persists.
    if titles.is_empty() || titles.iter().any(|title| title.trim().is_empty()) {
        return Err(Error::EmptyTitle);
    }

    let (board_dir, repo, config, _lock) = open_board_locked(project_dir).await?;
    let config_path = board_dir.join("config.toml");
    let status = config
        .columns
        .first()
        .map(Status::new)
        .ok_or_else(|| Error::parse(&config_path, "board config has no columns"))?;

    let existing = repo.list().await?;
    let mut source_item = existing
        .iter()
        .find(|item| &item.id == source)
        .cloned()
        .ok_or_else(|| Error::NotFound(source.clone()))?;

    let new_body = match &body {
        SplitBody::Copy => source_item.body.clone(),
        SplitBody::Empty => String::new(),
        SplitBody::Explicit(text) => text.clone(),
    };

    // Reserve the next contiguous ID range before writing. The board lock keeps another writer
    // from consuming the range while this operation prepares its complete record set.
    let first_id = repo.next_id(&config.project.key).await?;
    let mut last_rank = existing.last().map(|item| item.rank.clone());
    let mut created = Vec::with_capacity(titles.len());
    for (index, title) in titles.iter().enumerate() {
        let offset = u32::try_from(index).map_err(|_| {
            Error::InvalidItemId(format!("{}-{}", first_id.prefix(), first_id.number()))
        })?;
        let number = first_id.number().checked_add(offset).ok_or_else(|| {
            Error::InvalidItemId(format!("{}-{}", first_id.prefix(), first_id.number()))
        })?;
        let id = ItemId::try_new(first_id.prefix(), number)?;
        let rank = Rank::after(last_rank.as_ref());
        last_rank = Some(rank.clone());
        let mut item = BacklogItem::new(id, title, status.clone(), rank, Utc::now())?;
        item.body = new_body.clone();
        if relationship == SplitRelationship::Child {
            item.parent = Some(source.clone());
        }
        created.push(item);
    }

    if relationship == SplitRelationship::Dependency {
        for item in &created {
            if !source_item.depends_on.contains(&item.id) {
                source_item.depends_on.push(item.id.clone());
            }
        }
        source_item.updated = Utc::now();
    }

    let mut records = created.clone();
    if relationship == SplitRelationship::Dependency {
        records.push(source_item.clone());
    }
    let recovery = BoardRecoveryPoint::capture(&board_dir).await?;
    if let Err(error) = repo.save_item_batch(&records).await {
        return restore_board_after_failure(recovery, "split", error).await;
    }

    let ids = created
        .iter()
        .map(|item| item.id.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    repo.commit(&format!("pinto: split {source} into {ids}"))
        .await?;

    Ok(SplitOutcome {
        source: source_item,
        created,
    })
}
