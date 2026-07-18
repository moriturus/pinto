//! Sprint creation, state transition, and PBI assignment services.
//!
//! Combines the [`crate::sprint::Sprint`] and [`crate::backlog::BacklogItem`] domain types with the
//! persistence layer. A backlog item's sprint assignment is stored as the sprint ID string in
//! `BacklogItem::sprint`.

use super::{open_board, open_board_locked};
use crate::backlog::{BacklogItem, ItemId};
use crate::error::{Error, Result};
use crate::sprint::{Sprint, SprintCapacity, SprintId, SprintSpillover, SprintState};
use crate::storage::{Backend, BacklogItemRepository, SprintRepository};
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use std::path::Path;

const VELOCITY_WARNING_RECENT: usize = 5;

/// The source of a non-blocking Sprint load warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintLoadWarningKind {
    /// The assigned point total is above the configured capacity-hours threshold.
    Capacity,
    /// The assigned point total is above the historical velocity threshold.
    Velocity,
}

impl SprintLoadWarningKind {
    /// Return the short label used in localized CLI warning messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Velocity => "velocity",
        }
    }

    /// Return the unit attached to the numeric threshold in CLI output.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Capacity => "hours",
            Self::Velocity => "points",
        }
    }
}

/// A non-blocking warning produced when a Sprint's assigned points exceed a threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct SprintLoadWarning {
    /// Which planning comparison was exceeded.
    pub kind: SprintLoadWarningKind,
    /// Sum of estimated points assigned to the Sprint.
    pub points: u32,
    /// Numeric threshold that the assigned points exceeded.
    pub threshold: f64,
}

/// Calculate the current load warnings for a Sprint without changing board data.
pub async fn sprint_load_warnings(
    project_dir: &Path,
    id: &SprintId,
) -> Result<Vec<SprintLoadWarning>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let (sprints, items) = tokio::try_join!(
        SprintRepository::list(&repo),
        BacklogItemRepository::list(&repo)
    )?;
    let target = sprints
        .iter()
        .find(|sprint| sprint.id == *id)
        .ok_or_else(|| Error::SprintNotFound(id.clone()))?;
    Ok(sprint_load_warnings_for(target, &sprints, &items))
}

/// Calculate Sprint load warnings from an already loaded board snapshot.
fn sprint_load_warnings_for(
    target: &Sprint,
    sprints: &[Sprint],
    items: &[BacklogItem],
) -> Vec<SprintLoadWarning> {
    let assigned_points = items
        .iter()
        .filter(|item| item.sprint.as_deref() == Some(target.id.as_str()))
        .filter_map(|item| item.points)
        .fold(0_u32, u32::saturating_add);
    let mut warnings = Vec::with_capacity(2);

    if let Some(capacity) = target.capacity()
        && f64::from(assigned_points) > capacity.hours
    {
        warnings.push(SprintLoadWarning {
            kind: SprintLoadWarningKind::Capacity,
            points: assigned_points,
            threshold: capacity.hours,
        });
    }

    if let Some(threshold) = historical_velocity_threshold(target, sprints, items)
        && f64::from(assigned_points) > threshold
    {
        warnings.push(SprintLoadWarning {
            kind: SprintLoadWarningKind::Velocity,
            points: assigned_points,
            threshold,
        });
    }

    warnings
}

/// Return the average completed points from the target's five most recent closed predecessors.
fn historical_velocity_threshold(
    target: &Sprint,
    sprints: &[Sprint],
    items: &[BacklogItem],
) -> Option<f64> {
    let target_index = sprints.iter().position(|sprint| sprint.id == target.id)?;
    let history: Vec<Sprint> = sprints[..target_index]
        .iter()
        .filter(|sprint| sprint.state == SprintState::Closed)
        .cloned()
        .collect();
    if history.is_empty() {
        return None;
    }
    Some(super::velocity::compute_velocity(&history, items, VELOCITY_WARNING_RECENT).average_points)
}

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

/// How unfinished PBIs are handled when their sprint closes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum SprintCloseAction {
    /// Keep unfinished PBIs assigned to the closed sprint.
    #[default]
    Retain,
    /// Reassign unfinished PBIs to a planned or active sprint.
    Rollover(SprintId),
    /// Clear the sprint assignment from unfinished PBIs.
    Release,
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

/// Return sprints from `project_dir` in ascending creation-time order.
///
/// [`Error::NotInitialized`] if the board is uninitialized.
pub async fn list_sprints(project_dir: &Path) -> Result<Vec<Sprint>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRepository::list(&repo).await
}

/// Update sprint capacity settings and return the calculated capacity.
pub async fn set_sprint_capacity(
    project_dir: &Path,
    id: &SprintId,
    daily_work_hours: f64,
    holiday_days: u32,
    deduction_factor: f64,
) -> Result<SprintCapacity> {
    let (_board_dir, repo, _config, _lock) = open_board_locked(project_dir).await?;
    let mut sprint = SprintRepository::load(&repo, id).await?;
    sprint.set_capacity(daily_work_hours, holiday_days, deduction_factor)?;
    sprint.updated = Utc::now();
    let capacity = sprint
        .capacity()
        .ok_or_else(|| Error::SprintCapacityUnset(id.clone()))?;
    SprintRepository::save(&repo, &sprint).await?;
    repo.commit(&format!("pinto: update {}", sprint.id)).await?;
    Ok(capacity)
}

/// Return the configured capacity for a sprint.
pub async fn sprint_capacity(project_dir: &Path, id: &SprintId) -> Result<SprintCapacity> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let sprint = SprintRepository::load(&repo, id).await?;
    sprint
        .capacity()
        .ok_or_else(|| Error::SprintCapacityUnset(id.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::test_support::init_temp;
    use crate::service::{NewItem, add_item, move_item};
    use crate::sprint::SprintState;
    use crate::storage::{Backend, FileRepository};

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
        assert_eq!(loaded, sprint);
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

        assert!(
            sprint_load_warnings_for(&target, std::slice::from_ref(&target), &items).is_empty()
        );

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

        assert!(
            sprint_load_warnings_for(&target, std::slice::from_ref(&target), &items).is_empty()
        );
    }
}
