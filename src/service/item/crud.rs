//! CRUD operations for backlog items: add, list, show, move, and remove.

use crate::backlog::{AcceptanceCriteriaProgress, BacklogItem, ItemId, Status, Workflow};
use crate::error::{Error, Result};
use crate::rank::Rank;
use crate::service::relations::{validate_dependencies, validate_parent};
use crate::service::{
    LabelMatch, SearchFilter, apply_effective_points, open_board, open_board_locked,
    validate_sprint_assignment,
};
use crate::sprint::Sprint;
use crate::storage::BacklogItemRepository;
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

/// Optional fields for [`add_item`]. Unspecified fields use their domain defaults.
#[derive(Debug, Default, Clone)]
pub struct NewItem {
    /// Story points.
    pub points: Option<u32>,
    /// Labels assigned to the item.
    pub labels: Vec<String>,
    /// Sprint ID assigned to the item.
    pub sprint: Option<String>,
    /// Markdown body, including item-specific Acceptance Criteria.
    pub body: String,
    /// Parent PBI in the hierarchy.
    pub parent: Option<ItemId>,
    /// PBIs that must be completed before this item.
    pub depends_on: Vec<ItemId>,
}

/// Result of adding a PBI, including warning-only dependency-cycle information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddItemOutcome {
    /// Newly persisted PBI.
    pub item: BacklogItem,
    /// Whether one of the requested dependencies creates a cycle.
    pub cycle_warning: bool,
}

/// Add a PBI to the board in `project_dir` and return the saved [`BacklogItem`].
///
/// Prefix the ID with `config.project.key` and use the first workflow column as its status
/// (normally `todo`). Assign the rank after the current backlog. Return [`Error::NotInitialized`]
/// for an uninitialized board or [`Error::EmptyTitle`] when `title` is blank.
pub async fn add_item(project_dir: &Path, title: &str, new: NewItem) -> Result<BacklogItem> {
    Ok(add_item_with_outcome(project_dir, title, new).await?.item)
}

/// Add a PBI and report warning-only dependency cycles.
pub async fn add_item_with_outcome(
    project_dir: &Path,
    title: &str,
    new: NewItem,
) -> Result<AddItemOutcome> {
    let (board_dir, repo, config, _lock) = open_board_locked(project_dir).await?;
    let config_path = board_dir.join("config.toml");
    let status = config
        .columns
        .first()
        .map(Status::new)
        .ok_or_else(|| Error::parse(&config_path, "board config has no columns"))?;

    let NewItem {
        points,
        labels,
        sprint,
        body,
        parent,
        depends_on,
    } = new;
    let sprint = match sprint {
        Some(raw) => Some(validate_sprint_assignment(&repo, &raw).await?.to_string()),
        None => None,
    };

    let id = repo.next_id(&config.project.key).await?;

    // Rank is assigned to the end (all existing ranks are in ascending order, with the maximum number at the end).
    let existing = repo.list().await?;
    let rank = Rank::after(existing.last().map(|it| &it.rank));

    let mut item = BacklogItem::new(id.clone(), title, status, rank, Utc::now())?;
    if let Some(parent) = &parent {
        validate_parent(&existing, &id, parent)?;
    }
    let cycle_warning = validate_dependencies(&existing, &id, &depends_on)?;

    item.points = points;
    item.labels = labels;
    item.sprint = sprint;
    item.body = body;
    item.parent = parent;
    item.depends_on = depends_on
        .into_iter()
        .fold(Vec::new(), |mut unique, dependency| {
            if !unique.contains(&dependency) {
                unique.push(dependency);
            }
            unique
        });

    repo.save(&item).await?;
    repo.commit(&format!("pinto: add {}", item.id)).await?;
    Ok(AddItemOutcome {
        item,
        cycle_warning,
    })
}

/// Filters for [`list_items`]. Empty fields do not filter their corresponding property.
///
/// Multiple status specifications use OR (a PBI matches any selected status); label specifications
/// use [`LabelMatch::Any`] by default and can use [`LabelMatch::All`]; roots-only and other filters
/// are combined with AND. The stale condition matches `updated` timestamps at or before its
/// cutoff. The roots-only condition uses the item's persisted parent link even when that parent is
/// excluded by another filter.
#[derive(Debug, Default, Clone)]
pub struct ListFilter {
    /// Include only PBIs whose persisted parent link is unset.
    pub roots_only: bool,
    /// Read archived PBIs instead of active PBIs.
    pub archived: bool,
    /// Exact workflow statuses to match. An empty list includes every status.
    pub status: Vec<String>,
    /// Exact assigned sprint ID to match.
    pub sprint: Option<String>,
    /// Labels to match. An empty list includes every label set.
    pub labels: Vec<String>,
    /// Matching mode for [`Self::labels`].
    pub label_match: LabelMatch,
    /// Search the item's fields and assigned sprint metadata.
    pub search: Option<SearchFilter>,
    /// Match PBIs whose `updated` timestamp is at or before this UTC cutoff.
    pub stale_before: Option<DateTime<Utc>>,
}

impl ListFilter {
    /// Does `item` match all conditions?
    fn matches(&self, item: &BacklogItem, sprints: &[Sprint]) -> bool {
        if self.roots_only && item.parent.is_some() {
            return false;
        }
        if !self.status.is_empty()
            && !self
                .status
                .iter()
                .any(|status| item.status.as_str() == status)
        {
            return false;
        }
        if let Some(sprint) = &self.sprint
            && item.sprint.as_deref() != Some(sprint.as_str())
        {
            return false;
        }
        if !self.labels.is_empty() && !self.label_match.matches(&item.labels, &self.labels) {
            return false;
        }
        if let Some(cutoff) = self.stale_before
            && item.updated > cutoff
        {
            return false;
        }
        if let Some(search) = &self.search {
            let sprint = item
                .sprint
                .as_deref()
                .and_then(|id| sprints.iter().find(|sprint| sprint.id.as_str() == id));
            if !search.matches(item, sprint) {
                return false;
            }
        }
        true
    }
}

/// Return PBIs from `project_dir` in canonical hierarchical priority order.
///
/// Items are grouped as a parent/child forest: roots in ascending rank order,
/// each parent immediately followed by its subtree (siblings in rank order).
/// A filtered-out or absent parent promotes its children to roots, so the tree
/// is cut cleanly at the filter boundary. See [`crate::service::hierarchical_order`].
///
/// Apply only the conditions specified by `filter`. Return [`Error::NotInitialized`] for an
/// uninitialized board. When `filter.roots_only` is set, items with a persisted parent link remain
/// excluded even if that parent is outside the filtered result.
pub async fn list_items(project_dir: &Path, filter: &ListFilter) -> Result<Vec<BacklogItem>> {
    let (_board_dir, repo, config) = open_board(project_dir).await?;
    for status in &filter.status {
        if !config.columns.iter().any(|column| column == status) {
            return Err(Error::UnknownStatus(status.clone()));
        }
    }
    let mut items = if filter.archived {
        repo.list_archived().await?
    } else {
        repo.list().await?
    };
    apply_effective_points(
        &mut items,
        config.points.aggregate_children,
        &Status::new(&config.done_column),
    );
    let sprints = if filter.search.is_some() {
        crate::storage::SprintRepository::list(&repo).await?
    } else {
        Vec::new()
    };
    // Filter first (preserving canonical backlog order), then flatten the
    // surviving set into hierarchical priority order.
    let filtered: Vec<BacklogItem> = items
        .into_iter()
        .filter(|item| filter.matches(item, &sprints))
        .collect();
    Ok(crate::service::hierarchical(filtered))
}

/// Load PBI `id` from the board in `project_dir`.
///
/// Return [`Error::NotInitialized`] for an uninitialized board or [`Error::NotFound`] when the ID
/// does not exist.
pub async fn show_item(project_dir: &Path, id: &ItemId) -> Result<BacklogItem> {
    show_item_from_store(project_dir, id, false).await
}

/// Load archived PBI `id` from the board in `project_dir`.
pub async fn show_archived_item(project_dir: &Path, id: &ItemId) -> Result<BacklogItem> {
    show_item_from_store(project_dir, id, true).await
}

async fn show_item_from_store(
    project_dir: &Path,
    id: &ItemId,
    archived: bool,
) -> Result<BacklogItem> {
    let (_board_dir, repo, config) = open_board(project_dir).await?;
    let mut items = if archived {
        repo.list_archived().await?
    } else {
        repo.list().await?
    };
    apply_effective_points(
        &mut items,
        config.points.aggregate_children,
        &Status::new(&config.done_column),
    );
    items
        .into_iter()
        .find(|item| &item.id == id)
        .ok_or_else(|| Error::NotFound(id.clone()))
}

/// Move PBI `id` to workflow status `to` and return the saved [`BacklogItem`].
///
/// Only statuses configured in `config.toml` are valid. Reject an unknown status with
/// [`Error::UnknownStatus`] without changing the item. Return [`Error::NotInitialized`] for an
/// uninitialized board or [`Error::NotFound`] when the ID does not exist.
pub async fn move_item(project_dir: &Path, id: &ItemId, to: &str) -> Result<BacklogItem> {
    Ok(move_item_with_outcome(project_dir, id, to).await?.item)
}

/// Result of moving a PBI, including the computed Acceptance Criteria progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveOutcome {
    /// The saved PBI after the transition.
    pub item: BacklogItem,
    /// Progress parsed from the item's unchanged Markdown body.
    pub acceptance_criteria: AcceptanceCriteriaProgress,
    /// Whether the requested destination is the configured completion column.
    pub entered_done_column: bool,
}

/// Move PBI `id` and return the metadata needed by user interfaces for transition warnings.
///
/// The transition is persisted before the outcome is returned. Acceptance Criteria are computed
/// from the existing body and are never written back to the item.
pub async fn move_item_with_outcome(
    project_dir: &Path,
    id: &ItemId,
    to: &str,
) -> Result<MoveOutcome> {
    let (_board_dir, repo, config, _lock) = open_board_locked(project_dir).await?;
    let workflow = Workflow::new(config.columns.iter().map(Status::new));

    let mut item = repo.load(id).await?;
    item.transition_to(Status::new(to), &workflow, Utc::now())?;
    let acceptance_criteria = AcceptanceCriteriaProgress::from_markdown(&item.body);
    let entered_done_column = to == config.done_column;

    repo.save(&item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(MoveOutcome {
        item,
        acceptance_criteria,
        entered_done_column,
    })
}
/// Result of [`remove_item`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveOutcome {
    /// Saved to `archive/` (destination path).
    Archived(PathBuf),
    /// Physically deleted.
    Deleted,
}

/// Return active PBIs that would become dangling references if `target` were deleted.
fn referencing_items(items: &[BacklogItem], target: &ItemId) -> String {
    items
        .iter()
        .filter(|item| {
            &item.id != target
                && (item.parent.as_ref() == Some(target)
                    || item
                        .depends_on
                        .iter()
                        .any(|dependency| dependency == target))
        })
        .map(|item| item.id.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Delete PBI with `id`. The default is archive backup, and if `permanent` is true, physical deletion is performed.
///
/// A non-destructive operation that moves the archive to `.pinto/archive/<id>.md` (it can be tracked with Git and can be restored from the backup location).
/// `permanent` is a hard delete that cannot be undone and requires an explicit flag (`--force`) in the CLI.
/// [`Error::NotInitialized`] if the board is uninitialized, [`Error::NotFound`] if the corresponding ID does not exist.
pub async fn remove_item(
    project_dir: &Path,
    id: &ItemId,
    permanent: bool,
) -> Result<RemoveOutcome> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    if permanent {
        let items = repo.list().await?;
        if items.iter().any(|item| &item.id == id) {
            let references = referencing_items(&items, id);
            if !references.is_empty() {
                return Err(Error::ReferencedItem {
                    item: id.clone(),
                    references,
                });
            }
        }
        BacklogItemRepository::delete(&repo, id).await?;
        repo.commit(&format!("pinto: remove {id}")).await?;
        Ok(RemoveOutcome::Deleted)
    } else {
        let dest = repo.archive(id).await?;
        repo.commit(&format!("pinto: archive {id}")).await?;
        Ok(RemoveOutcome::Archived(dest))
    }
}

/// Restore archived PBI `id` to the active backlog and return the unchanged item.
pub async fn restore_item(project_dir: &Path, id: &ItemId) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    repo.restore(id).await?;
    repo.commit(&format!("pinto: restore {id}")).await?;
    repo.load(id).await
}
