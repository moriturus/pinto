//! Sprint lifecycle services: creation, editing, state transitions, and PBI assignment.

use super::SprintCloseAction;
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::service::open_board_locked;
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use crate::storage::{Backend, BacklogItemRepository, SprintRepository};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::path::Path;

/// Create a sprint on the board in `project_dir` and return the saved [`Sprint`].
///
/// The state is [`crate::sprint::SprintState::Planned`]. `goal` is persisted after the frontmatter
/// as the sprint Markdown body.
/// When `period` is provided, retain the planned start and end dates. Return
/// [`Error::InvalidSprintPeriod`] when the start is after the end, [`Error::SprintExists`] when
/// the ID is already used, [`Error::NotInitialized`] for an uninitialized board, or
/// [`Error::EmptySprintTitle`] for an empty title.
pub async fn create_sprint(
    project_dir: &Path,
    id: &SprintId,
    title: &str,
    goal: Option<String>,
    period: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Sprint> {
    // Reject an inverted period because the burndown time axis would be invalid.
    if let Some((start, end)) = period
        && start > end
    {
        return Err(Error::InvalidSprintPeriod {
            start: start.date_naive(),
            end: end.date_naive(),
        });
    }

    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;

    // Check for an existing ID before saving so creation never overwrites a sprint.
    match SprintRepository::load(&repo, id).await {
        Ok(_) => return Err(Error::SprintExists(id.clone())),
        Err(Error::SprintNotFound(_)) => {}
        Err(e) => return Err(e),
    }

    let mut sprint = Sprint::new(id.clone(), title, Utc::now())?;
    if let Some(goal) = goal {
        sprint.goal = goal;
    }
    if let Some((start, end)) = period {
        sprint.start = Some(start);
        sprint.end = Some(end);
    }
    SprintRepository::save(&repo, &sprint).await?;
    repo.commit(&format!("pinto: add {}", sprint.id)).await?;
    Ok(sprint)
}

/// Update the title, goal, and/or planned period of an existing sprint.
///
/// Fields set to `None` remain unchanged. Return [`Error::NothingToUpdate`] when no field is
/// supplied, [`Error::EmptySprintTitle`] for a blank title, [`Error::InvalidSprintPeriod`] for an
/// inverted period, or [`Error::SprintNotFound`] when the sprint does not exist.
pub async fn edit_sprint(
    project_dir: &Path,
    id: &SprintId,
    title: Option<String>,
    goal: Option<String>,
    period: Option<(DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Sprint> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut sprint = SprintRepository::load(&repo, id).await?;
    sprint.update_details(title, goal, period, Utc::now())?;
    SprintRepository::save(&repo, &sprint).await?;
    repo.commit(&format!("pinto: update {}", sprint.id)).await?;
    Ok(sprint)
}

/// Delete a sprint and clear its assignment from every PBI that references it.
///
/// The PBIs remain in the backlog. All reads and writes happen while the board lock is held, and
/// Git-backed boards commit the sprint deletion and assignment changes as one service operation.
pub async fn delete_sprint(project_dir: &Path, id: &SprintId) -> Result<()> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    SprintRepository::load(&repo, id).await?;

    let now = Utc::now();
    let assigned = BacklogItemRepository::list(&repo)
        .await?
        .into_iter()
        .filter(|item| item.sprint.as_deref() == Some(id.as_str()))
        .collect::<Vec<_>>();
    for mut item in assigned {
        item.sprint = None;
        item.updated = now;
        BacklogItemRepository::save(&repo, &item).await?;
    }

    SprintRepository::delete(&repo, id).await?;
    repo.commit(&format!("pinto: delete {id}")).await?;
    Ok(())
}

/// Start a sprint (`planned` → `active`). Returns the saved [`Sprint`].
///
/// Return [`Error::NotInitialized`] when the board is uninitialized or
/// [`Error::SprintNotFound`] when no sprint with `id` exists. Starting from anything other than
/// `planned` returns [`Error::InvalidSprintTransition`].
pub async fn start_sprint(project_dir: &Path, id: &SprintId) -> Result<Sprint> {
    transition_sprint(project_dir, id, Sprint::start).await
}

/// Close the sprint (`active` → `closed`) and return the saved [`Sprint`].
///
/// A rollover target is validated before the first write. Only unfinished PBIs are reassigned or
/// released; completed PBIs remain byte-for-byte equivalent at the domain level. The sprint stores
/// the actual close time and a snapshot of unfinished estimated points and item counts for
/// retrospective display, separate from velocity.
pub async fn close_sprint(
    project_dir: &Path,
    id: &SprintId,
    action: SprintCloseAction,
) -> Result<Sprint> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let original_sprint = SprintRepository::load(&repo, id).await?;
    if original_sprint.state != SprintState::Active {
        return Err(Error::InvalidSprintTransition {
            from: original_sprint.state,
            to: SprintState::Closed,
        });
    }

    if let SprintCloseAction::Rollover(target) = &action {
        if target == id {
            return Err(Error::InvalidFilterOption(
                "a sprint cannot roll unfinished PBIs over to itself".to_string(),
            ));
        }
        validate_sprint_assignment(&repo, target.as_str()).await?;
    }

    let original_items = BacklogItemRepository::list(&repo)
        .await?
        .into_par_iter()
        .filter(|item| item.sprint.as_deref() == Some(id.as_str()) && item.done_at.is_none())
        .collect::<Vec<_>>();
    let spillover = original_items
        .par_iter()
        .map(|item| SprintSpillover {
            points: item.points.unwrap_or(0),
            items: 1,
            unestimated_items: u32::from(item.points.is_none()),
        })
        .reduce(SprintSpillover::default, |left, right| SprintSpillover {
            points: left.points.saturating_add(right.points),
            items: left.items.saturating_add(right.items),
            unestimated_items: left
                .unestimated_items
                .saturating_add(right.unestimated_items),
        });

    let now = Utc::now();
    let mut sprint = original_sprint.clone();
    sprint.close(now, spillover)?;
    let mut updated_items = if action == SprintCloseAction::Retain {
        Vec::new()
    } else {
        original_items.clone()
    };
    for item in &mut updated_items {
        match &action {
            SprintCloseAction::Retain => {}
            SprintCloseAction::Rollover(target) => item.sprint = Some(target.to_string()),
            SprintCloseAction::Release => item.sprint = None,
        }
        item.updated = now;
    }

    for (index, item) in updated_items.iter().enumerate() {
        if let Err(error) = BacklogItemRepository::save(&repo, item).await {
            rollback_sprint_close(&repo, &original_sprint, &original_items[..=index], &error)
                .await?;
            return Err(error);
        }
    }
    if let Err(error) = SprintRepository::save(&repo, &sprint).await {
        rollback_sprint_close(&repo, &original_sprint, &original_items, &error).await?;
        return Err(error);
    }
    // Match the repository-wide Git failure contract: once durable files are saved, a commit
    // failure leaves them available for inspection and manual recovery.
    repo.commit(&format!("pinto: update {}", sprint.id)).await?;
    Ok(sprint)
}

/// Load the sprint, apply a state transition, and save.
///
/// The domain layer validates the transition before the updated sprint is saved, so failures leave
/// the on-disk state unchanged.
async fn transition_sprint(
    project_dir: &Path,
    id: &SprintId,
    transition: impl FnOnce(&mut Sprint, chrono::DateTime<Utc>) -> Result<()>,
) -> Result<Sprint> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut sprint = SprintRepository::load(&repo, id).await?;
    transition(&mut sprint, Utc::now())?;
    SprintRepository::save(&repo, &sprint).await?;
    repo.commit(&format!("pinto: update {}", sprint.id)).await?;
    Ok(sprint)
}

/// Restore sprint and unfinished-PBI contents after a failed close persistence operation.
async fn rollback_sprint_close(
    repo: &Backend,
    original_sprint: &Sprint,
    original_items: &[BacklogItem],
    operation_error: &Error,
) -> Result<()> {
    for item in original_items.iter().rev() {
        if let Err(rollback_error) = BacklogItemRepository::save(repo, item).await {
            return Err(Error::task(format!(
                "{operation_error}; failed to roll back sprint close: {rollback_error}"
            )));
        }
    }
    if let Err(rollback_error) = SprintRepository::save(repo, original_sprint).await {
        return Err(Error::task(format!(
            "{operation_error}; failed to roll back sprint close: {rollback_error}"
        )));
    }
    Ok(())
}

/// Validate a raw sprint assignment while the caller holds the board write lock.
pub(crate) async fn validate_sprint_assignment(repo: &Backend, raw: &str) -> Result<SprintId> {
    let id = SprintId::new(raw)?;
    let sprint = SprintRepository::load(repo, &id).await?;
    if sprint.state == SprintState::Closed {
        return Err(Error::SprintClosed(id));
    }
    Ok(id)
}

/// Assign PBI `item_id` to sprint `sprint_id` and return the saved [`BacklogItem`].
///
/// Validate that the sprint exists and is not closed before assigning, preventing dangling or
/// semantically invalid assignments.
/// Return [`Error::NotInitialized`] when the board is uninitialized, [`Error::SprintNotFound`]
/// when the sprint does not exist, [`Error::SprintClosed`] when it is closed, or
/// [`Error::NotFound`] when the PBI does not exist.
pub async fn assign_sprint(
    project_dir: &Path,
    sprint_id: &SprintId,
    item_id: &ItemId,
) -> Result<BacklogItem> {
    assign_sprint_raw(project_dir, sprint_id.as_str(), item_id).await
}

/// Assign a PBI from a raw CLI sprint ID, validating its grammar and existence before saving.
pub async fn assign_sprint_raw(
    project_dir: &Path,
    raw_sprint_id: &str,
    item_id: &ItemId,
) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let sprint_id = validate_sprint_assignment(&repo, raw_sprint_id).await?;
    let mut item = BacklogItemRepository::load(&repo, item_id).await?;
    item.sprint = Some(sprint_id.to_string());
    item.updated = Utc::now();
    BacklogItemRepository::save(&repo, &item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(item)
}

/// Assign matching PBIs to a sprint in backlog rank order.
///
/// `status` must be a configured workflow column. When `limit` is `Some`, only the first `limit`
/// matching PBIs are considered; omitting it considers every matching PBI. Items already assigned
/// to the target sprint are skipped without consuming the limit. An item assigned to another
/// sprint causes the operation to fail before any item is saved, so validation errors do not leave
/// a partially assigned set.
pub async fn assign_sprint_by_status(
    project_dir: &Path,
    sprint_id: &SprintId,
    status: &str,
    limit: Option<usize>,
) -> Result<Vec<BacklogItem>> {
    let (_board_dir, repo, config, _lock) = open_board_locked(project_dir).await?;

    if !config.columns.iter().any(|column| column == status) {
        return Err(Error::UnknownStatus(status.to_string()));
    }
    if limit == Some(0) {
        return Err(Error::InvalidFilterOption(
            "--limit must be at least 1".to_string(),
        ));
    }
    validate_sprint_assignment(&repo, sprint_id.as_str()).await?;

    // BacklogItemRepository::list returns canonical rank order. Exclude target-sprint members
    // before applying the limit so rerunning a command fills the requested number of new slots.
    let mut candidates = BacklogItemRepository::list(&repo)
        .await?
        .into_iter()
        .filter(|item| item.status.as_str() == status)
        .filter(|item| item.sprint.as_deref() != Some(sprint_id.as_str()))
        .collect::<Vec<_>>();
    if let Some(limit) = limit {
        candidates.truncate(limit);
    }

    // Validate every selected assignment before the first save. This makes conflicts with another
    // sprint all-or-nothing from the user's perspective.
    if let Some(item) = candidates.iter().find(|item| item.sprint.is_some())
        && let Some(assigned_sprint) = item.sprint.as_deref()
    {
        return Err(Error::InvalidFilterOption(format!(
            "{} is already assigned to sprint {}; remove it before bulk assignment",
            item.id, assigned_sprint
        )));
    }

    let original = candidates.clone();
    let now = Utc::now();
    let mut assigned = Vec::with_capacity(candidates.len());
    for (index, mut item) in candidates.into_iter().enumerate() {
        item.sprint = Some(sprint_id.to_string());
        item.updated = now;
        if let Err(error) = BacklogItemRepository::save(&repo, &item).await {
            rollback_bulk_assignment(&repo, &original[..=index], &error).await?;
            return Err(error);
        }
        assigned.push(item);
    }
    if !assigned.is_empty()
        && let Err(error) = repo
            .commit(&format!(
                "pinto: assign {} item(s) to {}",
                assigned.len(),
                sprint_id
            ))
            .await
    {
        rollback_bulk_assignment(&repo, &original, &error).await?;
        return Err(error);
    }
    Ok(assigned)
}

/// Restore the original item contents after a failed multi-item persistence operation.
async fn rollback_bulk_assignment(
    repo: &Backend,
    original: &[BacklogItem],
    operation_error: &Error,
) -> Result<()> {
    for item in original.iter().rev() {
        if let Err(rollback_error) = BacklogItemRepository::save(repo, item).await {
            return Err(Error::InvalidFilterOption(format!(
                "{operation_error}; failed to roll back bulk assignment: {rollback_error}"
            )));
        }
    }
    Ok(())
}

/// Remove PBI `item_id` from sprint `sprint_id` and return the saved [`BacklogItem`].
///
/// Return [`Error::NotInSprint`] when the item is assigned to another sprint or none. Return
/// [`Error::NotInitialized`] for an uninitialized board or [`Error::NotFound`] when the item does
/// not exist.
pub async fn unassign_sprint(
    project_dir: &Path,
    sprint_id: &SprintId,
    item_id: &ItemId,
) -> Result<BacklogItem> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut item = BacklogItemRepository::load(&repo, item_id).await?;
    if item.sprint.as_deref() != Some(sprint_id.as_str()) {
        return Err(Error::NotInSprint {
            item: item_id.clone(),
            sprint: sprint_id.clone(),
        });
    }
    item.sprint = None;
    item.updated = Utc::now();
    BacklogItemRepository::save(&repo, &item).await?;
    repo.commit(&format!("pinto: update {}", item.id)).await?;
    Ok(item)
}
