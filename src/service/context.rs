//! Generated context for Sprint Retro and Review views.

use super::open_board;
use crate::backlog::BacklogItem;
use crate::error::{Error, Result};
use crate::service::burndown::compute_burndown;
use crate::service::cycletime::{CycleTimeFilter, compute_report};
use crate::service::velocity::compute_velocity;
use crate::sprint::{Sprint, SprintCapacity, SprintId, SprintSpillover, SprintState};
use crate::storage::{BacklogItemRepository, SprintRepository};
use chrono::{DateTime, Utc};
use std::path::Path;

use super::{Burndown, CycleTimeReport, VelocitySprint};

/// Generated parent-Sprint context attached to a Retro or Review view.
///
/// This is a read-only view. It is not persisted with the user-authored Markdown record, so the
/// displayed context always reflects the current Sprint and the existing report semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct SprintContext {
    /// Stable parent Sprint ID.
    pub sprint_id: SprintId,
    /// Parent Sprint title.
    pub sprint_title: String,
    /// Parent Sprint goal text.
    pub goal: String,
    /// Recorded goal outcome, or `None` when it is not recorded. The write-side invariant clears
    /// an outcome whenever the Goal is blank.
    pub goal_achieved: Option<bool>,
    /// Parent Sprint lifecycle state.
    pub state: SprintState,
    /// Planned period start, if configured.
    pub start: Option<DateTime<Utc>>,
    /// Planned period end, if configured.
    pub end: Option<DateTime<Utc>>,
    /// Actual close timestamp, if the Sprint is closed.
    pub closed_at: Option<DateTime<Utc>>,
    /// Configured capacity, if the Sprint has a complete valid capacity configuration.
    pub capacity: Option<SprintCapacity>,
    /// Existing velocity result when completed work makes it meaningful for this Sprint.
    pub velocity: Option<VelocitySprint>,
    /// Existing burndown result when the planned period and assigned PBIs are available.
    pub burndown: Option<Burndown>,
    /// Existing Cycle/Lead Time result when completed work makes it meaningful for this Sprint.
    pub cycle_time: Option<CycleTimeReport>,
    /// Close-time unfinished-work snapshot, or `None` until the Sprint is closed.
    pub spillover: Option<SprintSpillover>,
}

/// Load the parent Sprint and derive its existing delivery context without modifying the board.
///
/// Planned or incomplete delivery metrics remain `None` when there is no qualifying evidence.
/// A closed Sprint uses its stored close-time spillover snapshot and keeps completed PBIs in the
/// existing Sprint assignment, so rollover or release of unfinished PBIs does not erase its
/// historical delivery context.
///
/// # Errors
///
/// Returns [`Error::NotInitialized`], [`Error::SprintNotFound`], an invalid stored Sprint period,
/// or persistence and parsing errors while loading the Sprint and PBIs.
pub async fn sprint_context(project_dir: &Path, id: &SprintId) -> Result<SprintContext> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let (sprint, items) = tokio::try_join!(
        SprintRepository::load(&repo, id),
        BacklogItemRepository::list(&repo),
    )?;
    build_sprint_context(&sprint, &items)
}

/// Derive Sprint context from already loaded Sprint and PBI records.
pub(crate) fn build_sprint_context(
    sprint: &Sprint,
    items: &[BacklogItem],
) -> Result<SprintContext> {
    let assigned = items
        .iter()
        .filter(|item| item.sprint.as_deref() == Some(sprint.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    let velocity = compute_velocity(std::slice::from_ref(sprint), items, 1)
        .sprints
        .into_iter()
        .next()
        .filter(|row| sprint.state == SprintState::Closed || row.completed_items > 0);

    let cycle_report = compute_report(
        items,
        &CycleTimeFilter {
            sprint: Some(sprint.id.to_string()),
            ..CycleTimeFilter::default()
        },
    );
    let cycle_time =
        (sprint.state == SprintState::Closed || cycle_report.completed > 0).then_some(cycle_report);

    let burndown = match (sprint.start, sprint.end) {
        (Some(start), Some(end)) => {
            let start = start.date_naive();
            let end = end.date_naive();
            if end < start {
                return Err(Error::InvalidSprintPeriod { start, end });
            }
            (!assigned.is_empty()).then(|| {
                compute_burndown(
                    sprint.id.clone(),
                    sprint.title.clone(),
                    start,
                    end,
                    &assigned,
                )
            })
        }
        _ => None,
    };

    Ok(SprintContext {
        sprint_id: sprint.id.clone(),
        sprint_title: sprint.title.clone(),
        goal: sprint.goal.clone(),
        goal_achieved: sprint.goal_achieved,
        state: sprint.state,
        start: sprint.start,
        end: sprint.end,
        closed_at: sprint.closed_at,
        capacity: sprint.capacity(),
        velocity,
        burndown,
        cycle_time,
        spillover: (sprint.state == SprintState::Closed).then_some(sprint.spillover),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::{BacklogItem, ItemId, Status};
    use crate::rank::Rank;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(0, 0).single().expect("valid timestamp")
    }

    fn sprint(id: &str) -> Sprint {
        Sprint::new(SprintId::new(id).expect("valid Sprint ID"), id, now()).expect("valid Sprint")
    }

    fn item(number: u32, sprint: &str, points: Option<u32>, done: bool) -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", number),
            format!("Task {number}"),
            Status::new("todo"),
            Rank::between(None, None).expect("open rank bounds"),
            now(),
        )
        .expect("valid item");
        item.sprint = Some(sprint.to_string());
        item.points = points;
        item.done_at = done.then(now);
        item
    }

    #[test]
    fn planned_context_does_not_turn_missing_delivery_into_zeroes() {
        let sprint = sprint("S-1");

        let context = build_sprint_context(&sprint, &[]).expect("context builds");

        assert_eq!(context.state, SprintState::Planned);
        assert_eq!(context.velocity, None);
        assert_eq!(context.burndown, None);
        assert_eq!(context.cycle_time, None);
        assert_eq!(context.spillover, None);
    }

    #[test]
    fn closed_context_retains_completed_work_and_spillover_snapshot() {
        let mut sprint = sprint("S-1");
        sprint.goal = "Ship it".to_string();
        sprint.start = Some(now());
        sprint.end = Some(now() + chrono::Duration::days(2));
        sprint.start(now()).expect("start Sprint");
        sprint
            .close(
                now() + chrono::Duration::days(3),
                SprintSpillover {
                    points: 5,
                    items: 1,
                    unestimated_items: 0,
                },
            )
            .expect("close Sprint");
        let items = [item(1, "S-1", Some(3), true)];

        let context = build_sprint_context(&sprint, &items).expect("context builds");

        assert_eq!(context.spillover.expect("closed spillover").points, 5);
        assert_eq!(context.velocity.expect("closed velocity").points, 3);
        assert_eq!(context.cycle_time.expect("closed cycle time").completed, 1);
        assert!(context.burndown.is_some());
    }
}
