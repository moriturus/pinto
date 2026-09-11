//! Sprint lifecycle services: creation, editing, state transitions, and PBI assignment.

use super::SprintCloseAction;
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::service::open_board_locked;
use crate::sprint::{Sprint, SprintId, SprintSpillover, SprintState};
use crate::sprint_record::SprintRecordKind;
use crate::storage::{Backend, BacklogItemRepository, SprintRecordRepository, SprintRepository};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::path::Path;

use super::SprintDeletionOptions;

type SprintEditPeriod = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

/// Create a sprint on the board in `project_dir` and return the saved [`Sprint`].
///
/// The state is [`crate::sprint::SprintState::Planned`]. `goal` is persisted after the frontmatter
/// as the sprint Markdown body.
/// When `period` is provided, retain the planned start and end dates. Return
/// [`Error::InvalidSprintPeriod`] when the start is after the end, [`Error::SprintExists`] when
/// the ID is already used, [`Error::NotInitialized`] for an uninitialized board, or
/// [`Error::EmptySprintTitle`] for an empty title.
///
/// # Errors
///
/// Returns the validation errors listed above, plus sprint persistence and Git commit errors. All
/// validation and the existing-ID check happen before saving. If saving succeeds but committing
/// fails, the new sprint may remain durable; inspect the board before retrying because the next
/// attempt will report [`Error::SprintExists`].
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
/// Fields set to `None` remain unchanged. Supplying only one schedule endpoint preserves the
/// existing counterpart. Return [`Error::NothingToUpdate`] when no field is supplied,
/// [`Error::EmptySprintTitle`] for a blank title, [`Error::SprintPeriodIncomplete`] when the
/// effective schedule has only one endpoint, [`Error::InvalidSprintPeriod`] for an inverted period,
/// or [`Error::SprintNotFound`] when the sprint does not exist.
///
/// # Errors
///
/// Returns the validation errors listed above, plus persistence and Git commit errors. Validation
/// happens before saving. A save followed by a commit failure may leave the updated sprint
/// durable; inspect it before retrying because the timestamp may already have changed.
pub async fn edit_sprint(
    project_dir: &Path,
    id: &SprintId,
    title: Option<String>,
    goal: Option<String>,
    period: Option<SprintEditPeriod>,
    goal_achieved: Option<bool>,
    clear_goal_achieved: bool,
) -> Result<Sprint> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut sprint = SprintRepository::load(&repo, id).await?;
    let now = Utc::now();
    let schedule_update = period.is_some();
    let period = if let Some((start, end)) = period {
        match (start.or(sprint.start), end.or(sprint.end)) {
            (Some(start), Some(end)) => Some((start, end)),
            _ => return Err(Error::SprintPeriodIncomplete(sprint.id.clone())),
        }
    } else {
        None
    };
    if title.is_some() || goal.is_some() || schedule_update {
        sprint.update_details(title, goal, period, now)?;
    } else if goal_achieved.is_none() && !clear_goal_achieved {
        return Err(Error::NothingToUpdate);
    }
    if clear_goal_achieved {
        sprint.set_goal_achieved(None, now);
    } else if goal_achieved.is_some() {
        sprint.set_goal_achieved(goal_achieved, now);
    }
    SprintRepository::save(&repo, &sprint).await?;
    repo.commit(&format!("pinto: update {}", sprint.id)).await?;
    Ok(sprint)
}

/// Delete a Sprint and clear its assignment from every PBI that references it.
///
/// Existing matching Retro and Review records are protected by default; use
/// [`delete_sprint_with_options`] with [`SprintDeletionOptions::delete_records`] to remove them.
/// The PBIs remain in the backlog. All reads and writes happen while the board lock is held, and
/// Git-backed boards commit the Sprint deletion and assignment changes as one service operation.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`],
/// [`Error::SprintRecordsExist`], persistence errors, or a Git commit error. Assignment clears
/// and Sprint deletion are performed as several durable writes; a failure can leave a partially
/// cleared board. Retrying is not blindly safe after the Sprint itself has been deleted, so
/// inspect the Sprint and assigned PBIs first.
pub async fn delete_sprint(project_dir: &Path, id: &SprintId) -> Result<()> {
    delete_sprint_with_options(project_dir, id, SprintDeletionOptions::default()).await
}

/// Delete a Sprint and clear its assignment from every PBI that references it, optionally deleting
/// the matching Retro and Review records as part of the same mutation.
///
/// The child records are checked before any write. Without `delete_records`, an existing child
/// record returns [`Error::SprintRecordsExist`] and leaves the Sprint, PBIs, and child records
/// unchanged. With the option enabled, only records belonging to `id` are deleted. Every PBI that
/// referenced the Sprint has its assignment cleared, and every action PBI whose Retro/Review
/// source pointed at the Sprint has that source link cleared, so no PBI is left referencing a
/// Sprint or child record that no longer exists. Active and archived PBIs are updated
/// symmetrically, so restoring an archived action PBI later never resurrects a dangling reference.
/// Git-backed boards commit all resulting changes once, so the existing undo path can recover the
/// complete operation.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`],
/// [`Error::SprintRecordsExist`], persistence errors, or a Git commit error. Writes before a
/// later persistence or commit failure may remain durable; inspect the board before retrying.
pub async fn delete_sprint_with_options(
    project_dir: &Path,
    id: &SprintId,
    options: SprintDeletionOptions,
) -> Result<()> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    SprintRepository::load(&repo, id).await?;

    let mut existing_kinds = Vec::new();
    for kind in SprintRecordKind::ALL {
        match SprintRecordRepository::load(&repo, kind, id).await {
            Ok(_) => existing_kinds.push(kind),
            Err(Error::SprintRecordNotFound { .. }) => {}
            Err(error) => return Err(error),
        }
    }
    if !options.delete_records && !existing_kinds.is_empty() {
        let records = existing_kinds
            .iter()
            .map(|kind| kind.display_name())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::SprintRecordsExist {
            id: id.clone(),
            records,
        });
    }

    // Clear both the Sprint assignment and any Retro/Review action-source link that points at this
    // Sprint, so deleting a Sprint never leaves a PBI referencing a Sprint or child record that no
    // longer exists. Active and archived PBIs are treated symmetrically: an archived action PBI is
    // updated in place so a later `restore` never resurrects a dangling reference. A single PBI may
    // need both cleared, so decide and save it once.
    let now = Utc::now();
    let references_sprint = |item: &BacklogItem| {
        item.sprint.as_deref() == Some(id.as_str())
            || item
                .source
                .as_ref()
                .is_some_and(|source| source.sprint_id == *id)
    };
    let clear_references = |item: &mut BacklogItem| {
        if item.sprint.as_deref() == Some(id.as_str()) {
            item.sprint = None;
        }
        if item
            .source
            .as_ref()
            .is_some_and(|source| source.sprint_id == *id)
        {
            item.source = None;
        }
        item.updated = now;
    };

    let active = BacklogItemRepository::list(&repo).await?;
    for mut item in active.into_iter().filter(&references_sprint) {
        clear_references(&mut item);
        BacklogItemRepository::save(&repo, &item).await?;
    }
    let archived = BacklogItemRepository::list_archived(&repo).await?;
    for mut item in archived.into_iter().filter(&references_sprint) {
        clear_references(&mut item);
        BacklogItemRepository::save_archived(&repo, &item).await?;
    }

    if options.delete_records {
        for kind in existing_kinds {
            SprintRecordRepository::delete(&repo, kind, id).await?;
        }
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
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`],
/// [`Error::InvalidSprintTransition`], persistence errors, or a Git commit error. Domain
/// validation happens before saving. A save followed by a commit failure may leave the state
/// transition durable; inspect the sprint before retrying.
pub async fn start_sprint(project_dir: &Path, id: &SprintId) -> Result<Sprint> {
    transition_sprint(project_dir, id, Sprint::start).await
}

/// Close the sprint (`active` → `closed`) and return the saved [`Sprint`].
///
/// A rollover target is validated before the first write. Only unfinished PBIs are reassigned or
/// released; completed PBIs remain byte-for-byte equivalent at the domain level. The sprint stores
/// the actual close time and a snapshot of unfinished estimated points and item counts for
/// retrospective display, separate from velocity.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], invalid transition or rollover
/// validation errors, persistence errors, or a Git commit error. Item writes are rolled back when
/// possible if a later write fails, but a rollback failure or a later commit failure can leave
/// durable partial changes. Inspect the sprint and affected PBIs before retrying; a closed sprint
/// cannot be closed again.
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
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], [`Error::SprintClosed`],
/// [`Error::NotFound`], persistence errors, or a Git commit error. The typed sprint ID and item are
/// validated before saving. A later commit failure may leave the assignment durable; retrying the
/// same assignment is safe after inspection because it sets the same sprint.
pub async fn assign_sprint(
    project_dir: &Path,
    sprint_id: &SprintId,
    item_id: &ItemId,
) -> Result<BacklogItem> {
    assign_sprint_raw(project_dir, sprint_id.as_str(), item_id).await
}

/// Assign a PBI from a raw CLI sprint ID, validating its grammar and existence before saving.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::InvalidSprintId`], [`Error::SprintNotFound`],
/// [`Error::SprintClosed`], [`Error::NotFound`], persistence errors, or a Git commit error. Raw ID
/// and record validation happen before saving. If a commit fails after saving, the assignment may
/// remain durable; inspect the item before retrying.
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
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::UnknownStatus`], invalid-limit or sprint validation
/// errors, persistence errors, or a Git commit error. Selection and assignment conflicts are
/// checked before the first save. A failed write or commit is rolled back when possible; if
/// rollback fails, partial assignments may remain. After inspecting the board, retrying is safe
/// because the operation skips already-assigned target members and revalidates conflicts.
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
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::NotFound`], [`Error::NotInSprint`], persistence
/// errors, or a Git commit error. The assignment is checked before saving. A commit failure may
/// leave the unassignment durable; inspect the item before retrying because a second attempt then
/// returns [`Error::NotInSprint`].
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
