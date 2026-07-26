//! Sprint capacity, load-warning, and listing services.

use super::{SprintLoadWarning, SprintLoadWarningKind};
use crate::backlog::BacklogItem;
use crate::error::{Error, Result};
use crate::service::{open_board, open_board_locked};
use crate::sprint::{Sprint, SprintCapacity, SprintId, SprintState};
use crate::storage::{BacklogItemRepository, SprintRepository};
use chrono::Utc;
use std::path::Path;

const VELOCITY_WARNING_RECENT: usize = 5;

/// Calculate the current load warnings for a Sprint without changing board data.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], and persistence or parsing errors
/// while loading the sprint and assigned PBIs. This operation is read-only, leaves no durable
/// partial changes, and is safe to retry after a transient read failure.
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
pub(super) fn sprint_load_warnings_for(
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
    Some(
        crate::service::velocity::compute_velocity(&history, items, VELOCITY_WARNING_RECENT)
            .average_points,
    )
}

/// Return sprints from `project_dir` in ascending creation-time order.
///
/// [`Error::NotInitialized`] if the board is uninitialized.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`] and persistence or parsing errors while loading sprint
/// records. This operation is read-only and safe to retry after a transient read failure.
pub async fn list_sprints(project_dir: &Path) -> Result<Vec<Sprint>> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    SprintRepository::list(&repo).await
}

/// Update sprint capacity settings and return the calculated capacity.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], invalid capacity or period
/// validation errors, persistence errors, or a Git commit error. Validation occurs before saving.
/// If the save succeeds but the commit fails, the settings may remain durable; inspect the sprint
/// before retrying, although applying the same capacity values is otherwise safe.
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
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], [`Error::SprintCapacityUnset`],
/// and persistence or parsing errors. This operation is read-only, leaves no durable partial
/// changes, and is safe to retry after a transient read failure.
pub async fn sprint_capacity(project_dir: &Path, id: &SprintId) -> Result<SprintCapacity> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let sprint = SprintRepository::load(&repo, id).await?;
    sprint
        .capacity()
        .ok_or_else(|| Error::SprintCapacityUnset(id.clone()))
}
