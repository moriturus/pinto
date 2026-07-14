//! Sprint burndown aggregation service.
//!
//! Calculate daily remaining work and the ideal burndown line from the sprint period and assigned
//! item completion times (`done_at`). Rendering belongs to the CLI layer; this module stays a pure,
//! terminal-independent aggregation service.

use super::open_board;
use crate::backlog::BacklogItem;
use crate::error::{Error, Result};
use crate::sprint::SprintId;
use crate::storage::{BacklogItemRepository, SprintRepository};
use chrono::{Duration, NaiveDate};
use std::path::Path;

/// The metric a burndown chart tracks.
///
/// When every assigned PBI has points, remaining work is measured in
/// [`Points`](Self::Points); if even one lacks an estimate, it falls back to
/// [`Count`](Self::Count) so a missing estimate cannot distort the chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BurndownMetric {
    /// Remaining story points (when all PBIs have points).
    Points,
    /// Number of remaining items (when there are PBIs with no points set).
    Count,
}

impl BurndownMetric {
    /// Short name for display/JSON (`points` / `items`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            BurndownMetric::Points => "points",
            BurndownMetric::Count => "items",
        }
    }
}

/// Observation points for one day of burndown.
#[derive(Debug, Clone, PartialEq)]
pub struct BurndownDay {
    /// Target date (UTC).
    pub date: NaiveDate,
    /// Amount of work remaining at the end of the day (total of unfinished work).
    pub remaining: u32,
    /// The remaining work on the ideal line (when consumed linearly over the period).
    pub ideal: f64,
}

/// Sprint burndown summary.
#[derive(Debug, Clone, PartialEq)]
pub struct Burndown {
    /// ID of the target sprint.
    pub sprint_id: SprintId,
    /// Display title of the target sprint.
    pub sprint_title: String,
    /// Aggregated metrics (points or counts).
    pub metric: BurndownMetric,
    /// Total amount of work on the period start date (burndown starting point).
    pub total: u32,
    /// Daily observation points (in ascending order from start date to end date, inclusive).
    pub days: Vec<BurndownDay>,
}

/// Calculate the burndown for sprint `sprint_id` in `project_dir`.
///
/// Use the sprint's planned schedule (`start` / `end`). Return a guidance error when required data
/// is missing:
/// - [`Error::SprintPeriodUnset`] if start and end are not set.
/// - [`Error::SprintEmpty`] if there is no assigned PBI.
///
/// Return [`Error::NotInitialized`] for an uninitialized board or [`Error::SprintNotFound`] when
/// the sprint does not exist.
pub async fn burndown(project_dir: &Path, sprint_id: &SprintId) -> Result<Burndown> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let sprint = SprintRepository::load(&repo, sprint_id).await?;

    let (start, end) = match (sprint.start, sprint.end) {
        (Some(s), Some(e)) => (s.date_naive(), e.date_naive()),
        _ => return Err(Error::SprintPeriodUnset(sprint_id.clone())),
    };
    // Recheck the period in case a user edited the stored sprint manually.
    if end < start {
        return Err(Error::InvalidSprintPeriod { start, end });
    }

    let mut items = BacklogItemRepository::list(&repo).await?;
    items.retain(|it| it.sprint.as_deref() == Some(sprint_id.as_str()));
    if items.is_empty() {
        return Err(Error::SprintEmpty(sprint_id.clone()));
    }

    Ok(compute_burndown(
        sprint.id,
        sprint.title,
        start,
        end,
        &items,
    ))
}

/// Construct daily burndown data for `start..=end` from the assigned PBIs.
///
/// Use [`BurndownMetric::Points`] only when every PBI has points; otherwise use
/// [`BurndownMetric::Count`]. Subtract work completed by each date (`done_at` on or before the
/// target date). The ideal line interpolates linearly from the total to zero; a one-day period has
/// an ideal value of zero at its only point. Assume `start <= end`.
fn compute_burndown(
    sprint_id: SprintId,
    sprint_title: String,
    start: NaiveDate,
    end: NaiveDate,
    items: &[BacklogItem],
) -> Burndown {
    let metric = if !items.is_empty() && items.iter().all(|it| it.points.is_some()) {
        BurndownMetric::Points
    } else {
        BurndownMetric::Count
    };
    let weight = |it: &BacklogItem| -> u32 {
        match metric {
            BurndownMetric::Points => it.points.unwrap_or(0),
            BurndownMetric::Count => 1,
        }
    };

    let total: u32 = items.iter().map(weight).sum();
    // 0-based final index (= number of days in period - 1). 0 for a single day.
    let last = (end - start).num_days().max(0);

    let days = (0..=last)
        .map(|i| {
            let date = start + Duration::days(i);
            // Total completed (UTC date of done_at is before the target date) as of the end of the target date.
            let completed: u32 = items
                .iter()
                .filter(|it| it.done_at.is_some_and(|t| t.date_naive() <= date))
                .map(weight)
                .sum();
            let remaining = total.saturating_sub(completed);
            let ideal = if last == 0 {
                0.0
            } else {
                f64::from(total) * (last - i) as f64 / last as f64
            };
            BurndownDay {
                date,
                remaining,
                ideal,
            }
        })
        .collect();

    Burndown {
        sprint_id,
        sprint_title,
        metric,
        total,
        days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::{ItemId, Status};
    use crate::rank::Rank;
    use crate::service::test_support::init_temp;
    use crate::service::{NewItem, add_item, assign_sprint, create_sprint, move_item};
    use chrono::{DateTime, Utc};

    fn sid(s: &str) -> SprintId {
        SprintId::new(s).expect("valid sprint id")
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
    }

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        day(y, m, d)
            .and_hms_opt(12, 0, 0)
            .expect("valid time")
            .and_utc()
    }

    /// Create a test PBI (specify points and done_at arbitrarily).
    fn item(n: u32, points: Option<u32>, done_at: Option<DateTime<Utc>>) -> BacklogItem {
        let mut it = BacklogItem::new(
            ItemId::new("T", n),
            format!("Task {n}"),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            Utc::now(),
        )
        .expect("valid item");
        it.points = points;
        it.done_at = done_at;
        it
    }

    // --- Pure aggregation logic ---

    #[test]
    fn uses_points_metric_when_every_item_is_estimated() {
        let items = [item(1, Some(3), None), item(2, Some(5), None)];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 8),
            &items,
        );
        assert_eq!(b.metric, BurndownMetric::Points);
        assert_eq!(b.total, 8);
    }

    #[test]
    fn falls_back_to_count_metric_when_any_item_unestimated() {
        let items = [item(1, Some(3), None), item(2, None, None)];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 8),
            &items,
        );
        assert_eq!(b.metric, BurndownMetric::Count);
        assert_eq!(b.total, 2, "count metric weights each item as 1");
    }

    #[test]
    fn spans_every_day_inclusive() {
        let items = [item(1, None, None)];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 20),
            &items,
        );
        assert_eq!(b.days.len(), 15, "6th..20th inclusive is 15 days");
        assert_eq!(b.days.first().unwrap().date, day(2026, 7, 6));
        assert_eq!(b.days.last().unwrap().date, day(2026, 7, 20));
    }

    #[test]
    fn remaining_drops_as_items_complete() {
        // 3 completed, 0 completed on the first day, 1 completed on the 2nd day, remaining 2 completed on the 3rd day.
        let items = [
            item(1, None, Some(utc(2026, 7, 7))),
            item(2, None, Some(utc(2026, 7, 8))),
            item(3, None, Some(utc(2026, 7, 8))),
        ];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 8),
            &items,
        );
        let remaining: Vec<u32> = b.days.iter().map(|d| d.remaining).collect();
        assert_eq!(remaining, [3, 2, 0]);
    }

    #[test]
    fn points_metric_burns_down_by_points() {
        let items = [
            item(1, Some(3), Some(utc(2026, 7, 7))),
            item(2, Some(5), None),
        ];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 8),
            &items,
        );
        let remaining: Vec<u32> = b.days.iter().map(|d| d.remaining).collect();
        // Total 8 points. 3pt completed on 7/7 → 8, 5, 5.
        assert_eq!(remaining, [8, 5, 5]);
    }

    #[test]
    fn ideal_line_is_linear_from_total_to_zero() {
        let items: Vec<BacklogItem> = (1..=4).map(|n| item(n, None, None)).collect();
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 10),
            &items,
        );
        let ideals: Vec<f64> = b.days.iter().map(|d| d.ideal).collect();
        // Linear digestion of 4 items in 5 days (0..4): 4, 3, 2, 1, 0.
        assert_eq!(ideals, [4.0, 3.0, 2.0, 1.0, 0.0]);
    }

    #[test]
    fn single_day_sprint_has_one_point_with_zero_ideal() {
        let items = [item(1, None, None)];
        let b = compute_burndown(
            sid("S-1"),
            "S".into(),
            day(2026, 7, 6),
            day(2026, 7, 6),
            &items,
        );
        assert_eq!(b.days.len(), 1);
        assert_eq!(b.days[0].ideal, 0.0);
        assert_eq!(b.days[0].remaining, 1);
    }

    // --- Service layer (information on data shortage/actual data aggregation) ---

    #[tokio::test]
    async fn burndown_reports_period_unset_when_dates_missing() {
        let dir = init_temp().await;
        create_sprint(dir.path(), &sid("S-1"), "Sprint", None, None)
            .await
            .unwrap();
        let item = add_item(dir.path(), "Task", NewItem::default())
            .await
            .unwrap();
        assign_sprint(dir.path(), &sid("S-1"), &item.id)
            .await
            .unwrap();

        let err = burndown(dir.path(), &sid("S-1")).await.unwrap_err();
        assert_eq!(err, Error::SprintPeriodUnset(sid("S-1")));
    }

    #[tokio::test]
    async fn burndown_reports_empty_when_no_items_assigned() {
        let dir = init_temp().await;
        create_sprint(
            dir.path(),
            &sid("S-1"),
            "Sprint",
            None,
            Some((utc(2026, 7, 6), utc(2026, 7, 20))),
        )
        .await
        .unwrap();

        let err = burndown(dir.path(), &sid("S-1")).await.unwrap_err();
        assert_eq!(err, Error::SprintEmpty(sid("S-1")));
    }

    #[tokio::test]
    async fn burndown_missing_sprint_returns_not_found() {
        let dir = init_temp().await;
        let err = burndown(dir.path(), &sid("S-9")).await.unwrap_err();
        assert_eq!(err, Error::SprintNotFound(sid("S-9")));
    }

    #[tokio::test]
    async fn burndown_aggregates_assigned_items_only() {
        let dir = init_temp().await;
        // The period is a relative date that includes "today" as the last day. `move_item` is done_at
        // Since the execution date (today) is set, the completion will not be counted unless the final day is set to today.
        // remaining breaks depending on execution date.
        let now = Utc::now();
        let start = now - Duration::days(2);
        let end = now;
        create_sprint(dir.path(), &sid("S-1"), "Sprint", None, Some((start, end)))
            .await
            .unwrap();
        let a = add_item(dir.path(), "A", NewItem::default()).await.unwrap();
        let b = add_item(dir.path(), "B", NewItem::default()).await.unwrap();
        // C is not assigned (not subject to aggregation).
        let _c = add_item(dir.path(), "C", NewItem::default()).await.unwrap();
        assign_sprint(dir.path(), &sid("S-1"), &a.id).await.unwrap();
        assign_sprint(dir.path(), &sid("S-1"), &b.id).await.unwrap();
        // Move a to completion.
        move_item(dir.path(), &a.id, "done").await.unwrap();

        let result = burndown(dir.path(), &sid("S-1")).await.unwrap();
        assert_eq!(result.metric, BurndownMetric::Count);
        assert_eq!(result.total, 2, "only the two assigned items count");
        // 1 completed, 1 left on the last day.
        assert_eq!(result.days.last().unwrap().remaining, 1);
    }
}
