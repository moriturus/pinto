//! Sprint velocity aggregation service.

use super::open_board;
use crate::backlog::BacklogItem;
use crate::error::Result;
use crate::sprint::SprintId;
use crate::storage::{BacklogItemRepository, SprintRepository};
use rayon::prelude::*;
use std::path::Path;

/// Velocity and estimate coverage for one sprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VelocitySprint {
    /// Stable sprint ID.
    pub sprint_id: SprintId,
    /// Sprint display title.
    pub sprint_title: String,
    /// Total points for completed PBIs that have estimates.
    pub points: u32,
    /// Number of completed PBIs, including unestimated items.
    pub completed_items: usize,
    /// Number of completed PBIs without estimates.
    pub unestimated_completed_items: usize,
    /// Number of incomplete PBIs in the sprint.
    pub incomplete_items: usize,
}

/// Velocity summary for the selected recent sprints.
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityReport {
    /// Selected sprints in creation order, limited to the most recent `recent` entries.
    pub sprints: Vec<VelocitySprint>,
    /// Average completed points across the selected sprints.
    pub average_points: f64,
    /// Percentage change from the previous-sprint average to the latest sprint, or `None` when no
    /// comparison is possible.
    pub change_percent: Option<f64>,
}

/// Aggregate velocity for the most recent `recent` sprints in `project_dir`.
///
/// Count only completed PBIs (`done_at` is set) with points in the total. Return unestimated and
/// incomplete counts separately. Compare the latest sprint with the average of the preceding
/// selected sprints; return no percentage when the baseline is zero or unavailable.
pub async fn velocity(project_dir: &Path, recent: usize) -> Result<VelocityReport> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let (sprints, items) = tokio::try_join!(
        SprintRepository::list(&repo),
        BacklogItemRepository::list(&repo),
    )?;
    Ok(compute_velocity(&sprints, &items, recent))
}

/// Calculate velocity from already loaded sprints and PBIs.
fn compute_velocity(
    sprints: &[crate::sprint::Sprint],
    items: &[BacklogItem],
    recent: usize,
) -> VelocityReport {
    let start = sprints.len().saturating_sub(recent);
    let selected = &sprints[start..];
    let rows: Vec<VelocitySprint> = selected
        .par_iter()
        .map(|sprint| {
            let relevant = items
                .iter()
                .filter(|item| item.sprint.as_deref() == Some(sprint.id.as_str()));
            let (points, completed_items, unestimated_completed_items, incomplete_items) = relevant
                .fold((0_u32, 0_usize, 0_usize, 0_usize), |acc, item| {
                    let (points, completed, unestimated, incomplete) = acc;
                    if item.done_at.is_some() {
                        match item.points {
                            Some(value) => (
                                points.saturating_add(value),
                                completed + 1,
                                unestimated,
                                incomplete,
                            ),
                            None => (points, completed + 1, unestimated + 1, incomplete),
                        }
                    } else {
                        (points, completed, unestimated, incomplete + 1)
                    }
                });
            VelocitySprint {
                sprint_id: sprint.id.clone(),
                sprint_title: sprint.title.clone(),
                points,
                completed_items,
                unestimated_completed_items,
                incomplete_items,
            }
        })
        .collect();
    let average_points = if rows.is_empty() {
        0.0
    } else {
        rows.iter().map(|row| f64::from(row.points)).sum::<f64>() / rows.len() as f64
    };
    let change_percent = rows.last().and_then(|latest| {
        let prior = &rows[..rows.len().saturating_sub(1)];
        if prior.is_empty() {
            return None;
        }
        let baseline =
            prior.iter().map(|row| f64::from(row.points)).sum::<f64>() / prior.len() as f64;
        (baseline != 0.0).then(|| (f64::from(latest.points) - baseline) / baseline * 100.0)
    });

    VelocityReport {
        sprints: rows,
        average_points,
        change_percent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::{BacklogItem, ItemId, Status};
    use crate::rank::Rank;
    use crate::sprint::{Sprint, SprintId};
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("valid timestamp")
    }

    fn sprint(id: &str) -> Sprint {
        Sprint::new(SprintId::new(id).expect("valid sprint id"), id, now()).expect("valid sprint")
    }

    fn item(number: u32, sprint: &str, points: Option<u32>, completed: bool) -> BacklogItem {
        let mut item = BacklogItem::new(
            ItemId::new("T", number),
            format!("Item {number}"),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            now(),
        )
        .expect("valid item");
        item.sprint = Some(sprint.to_string());
        item.points = points;
        item.done_at = completed.then(now);
        item
    }

    #[test]
    fn computes_completed_points_and_exposes_unestimated_and_incomplete_counts() {
        let sprints = [sprint("S-1")];
        let items = [
            item(1, "S-1", Some(3), true),
            item(2, "S-1", Some(5), true),
            item(3, "S-1", None, true),
            item(4, "S-1", Some(2), false),
        ];

        let report = compute_velocity(&sprints, &items, 5);

        assert_eq!(report.sprints.len(), 1);
        let row = &report.sprints[0];
        assert_eq!(row.sprint_id.to_string(), "S-1");
        assert_eq!(row.points, 8);
        assert_eq!(row.completed_items, 3);
        assert_eq!(row.unestimated_completed_items, 1);
        assert_eq!(row.incomplete_items, 1);
        assert_eq!(report.average_points, 8.0);
        assert_eq!(
            report.change_percent, None,
            "one sprint has no comparison baseline"
        );
    }

    #[test]
    fn uses_the_most_recent_configured_sprints_and_compares_latest_to_prior_average() {
        let sprints = [sprint("S-1"), sprint("S-2"), sprint("S-3")];
        let items = [
            item(1, "S-1", Some(2), true),
            item(2, "S-2", Some(4), true),
            item(3, "S-3", Some(9), true),
        ];

        let report = compute_velocity(&sprints, &items, 2);

        assert_eq!(
            report
                .sprints
                .iter()
                .map(|row| row.sprint_id.to_string())
                .collect::<Vec<_>>(),
            ["S-2", "S-3"]
        );
        assert_eq!(report.average_points, 6.5);
        assert_eq!(report.change_percent, Some(125.0));
    }

    #[test]
    fn avoids_a_misleading_change_rate_when_the_prior_average_is_zero() {
        let sprints = [sprint("S-1"), sprint("S-2")];
        let items = [item(1, "S-2", Some(3), true)];

        let report = compute_velocity(&sprints, &items, 5);

        assert_eq!(report.change_percent, None);
    }
}
