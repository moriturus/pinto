//! Cycle Time / Lead Time aggregation service.
//!
//! Calculate Cycle Time (start → completion) and Lead Time (creation → completion) from completed
//! item timestamps (`created`, `start_at`, and `done_at`). Return compact summary statistics. The
//! CLI renders the results; this module remains a pure, terminal-independent aggregation service.

use super::open_board;
use crate::backlog::{BacklogItem, ItemId};
use crate::error::Result;
use crate::storage::BacklogItemRepository;
use chrono::{DateTime, Duration, Utc};
use std::path::Path;

/// Optional filters for Cycle/Lead Time aggregation.
///
/// `sprint` selects an assigned sprint; `since` and `until` bound the completion time (`done_at`),
/// inclusively. An unknown sprint produces an empty result rather than an error, matching
/// `list --sprint` and `board --sprint`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleTimeFilter {
    /// Assigned sprint to include, or all sprints when `None`.
    pub sprint: Option<String>,
    /// Inclusive lower bound for completion time (`done_at >= since`).
    pub since: Option<DateTime<Utc>>,
    /// Inclusive upper bound for completion time (`done_at <= until`).
    pub until: Option<DateTime<Utc>>,
}

/// Summary statistics for a [`Duration`] sample.
///
/// Created only when at least one item contributes to the metric; otherwise the corresponding
/// [`CycleTimeReport`] field is `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurationSummary {
    /// Number of items in the sample (at least one).
    pub count: usize,
    /// Mean duration, truncated to whole seconds.
    pub mean: Duration,
    /// Median duration; for an even sample, average the two middle values and truncate to whole
    /// seconds.
    pub median: Duration,
    /// Minimum duration.
    pub min: Duration,
    /// Maximum duration.
    pub max: Duration,
}

/// Results of Cycle Time / Lead Time analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleTimeReport {
    /// Cycle Time summary (`start_at` → `done_at`), or `None` when no item qualifies.
    pub cycle: Option<DurationSummary>,
    /// Lead Time summary (`created` → `done_at`), or `None` when no item qualifies.
    pub lead: Option<DurationSummary>,
    /// Completed PBIs missing `start_at`, so Cycle Time cannot be calculated; IDs are ascending.
    ///
    /// These items still contribute to Lead Time and are reported so missing timestamps are visible.
    pub missing_start: Vec<ItemId>,
    /// Number of completed PBIs after filtering (`done_at` is set).
    pub completed: usize,
}

/// Aggregate Cycle/Lead Time for completed PBIs matching `filter` in `project_dir`.
///
/// Return [`crate::error::Error::NotInitialized`] for an uninitialized board. Reading and pure
/// aggregation are kept separate in `compute_report`.
pub async fn cycle_time(project_dir: &Path, filter: &CycleTimeFilter) -> Result<CycleTimeReport> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let items = BacklogItemRepository::list(&repo).await?;
    Ok(compute_report(&items, filter))
}

/// Aggregate Cycle/Lead Time from completed PBIs without performing I/O.
///
/// Include items with `done_at` that match the sprint and completion-time filters. Cycle Time
/// requires `start_at`; missing values go to `missing_start`. Lead Time uses `created`, which every
/// completed item has.
fn compute_report(items: &[BacklogItem], filter: &CycleTimeFilter) -> CycleTimeReport {
    let mut cycle: Vec<Duration> = Vec::new();
    let mut lead: Vec<Duration> = Vec::new();
    let mut missing_start: Vec<ItemId> = Vec::new();
    let mut completed = 0usize;

    for it in items {
        // Only completed PBIs contribute to either metric.
        let Some(done) = it.done_at else { continue };
        // Apply the optional sprint filter.
        if let Some(sprint) = filter.sprint.as_deref()
            && it.sprint.as_deref() != Some(sprint)
        {
            continue;
        }
        // Apply the inclusive completion-time bounds.
        if filter.since.is_some_and(|since| done < since) {
            continue;
        }
        if filter.until.is_some_and(|until| done > until) {
            continue;
        }

        completed += 1;
        lead.push(done - it.created);
        match it.start_at {
            Some(start) => cycle.push(done - start),
            None => missing_start.push(it.id.clone()),
        }
    }

    // Sort IDs by prefix and number to make the warning order deterministic.
    missing_start.sort_by(|a, b| {
        a.prefix()
            .cmp(b.prefix())
            .then_with(|| a.number().cmp(&b.number()))
    });

    CycleTimeReport {
        cycle: summarize(&mut cycle),
        lead: summarize(&mut lead),
        missing_start,
        completed,
    }
}

/// Create summary statistics from `durations`, returning `None` when it is empty. Sorts the slice
/// in place to calculate the median; callers should not rely on the original order afterward.
fn summarize(durations: &mut [Duration]) -> Option<DurationSummary> {
    if durations.is_empty() {
        return None;
    }
    durations.sort_unstable();
    let count = durations.len();
    let secs: Vec<i64> = durations.iter().map(Duration::num_seconds).collect();
    let sum: i64 = secs.iter().sum();
    let mean = Duration::seconds(sum / count as i64);
    let mid = count / 2;
    let median_secs = if count % 2 == 1 {
        secs[mid]
    } else {
        (secs[mid - 1] + secs[mid]) / 2
    };
    Some(DurationSummary {
        count,
        mean,
        median: Duration::seconds(median_secs),
        min: durations[0],
        max: durations[count - 1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backlog::Status;
    use crate::rank::Rank;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .expect("valid date")
            .and_hms_opt(h, 0, 0)
            .expect("valid time")
            .and_utc()
    }

    /// Create a completed PBI fixture with explicit `created`, `start_at`, and `done_at` values.
    fn item(
        n: u32,
        created: DateTime<Utc>,
        start_at: Option<DateTime<Utc>>,
        done_at: Option<DateTime<Utc>>,
    ) -> BacklogItem {
        let mut it = BacklogItem::new(
            ItemId::new("T", n),
            format!("Task {n}"),
            Status::new("todo"),
            Rank::between(None, None).expect("open bounds produce a rank"),
            created,
        )
        .expect("valid item");
        it.start_at = start_at;
        it.done_at = done_at;
        it
    }

    fn hours(h: i64) -> Duration {
        Duration::hours(h)
    }

    #[test]
    fn computes_cycle_and_lead_from_completed_items() {
        // First item: created 7/1 0h, started 7/1 0h, completed 7/2 0h → cycle 24h, lead 24h.
        // Second item: created 7/1 0h, started 7/2 0h, completed 7/4 0h → cycle 48h, lead 72h.
        let items = [
            item(
                1,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 0)),
            ),
            item(
                2,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 2, 0)),
                Some(utc(2026, 7, 4, 0)),
            ),
        ];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.completed, 2);
        let cycle = r.cycle.expect("cycle summary");
        assert_eq!(cycle.count, 2);
        assert_eq!(cycle.min, hours(24));
        assert_eq!(cycle.max, hours(48));
        assert_eq!(cycle.mean, hours(36));
        let lead = r.lead.expect("lead summary");
        assert_eq!(lead.min, hours(24));
        assert_eq!(lead.max, hours(72));
        assert_eq!(lead.mean, hours(48));
        assert!(r.missing_start.is_empty());
    }

    #[test]
    fn ignores_items_that_are_not_completed() {
        let items = [
            item(
                1,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 0)),
            ),
            // Uncompleted items (no done_at) are not included in the aggregation.
            item(2, utc(2026, 7, 1, 0), Some(utc(2026, 7, 1, 0)), None),
        ];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.completed, 1);
        assert_eq!(r.cycle.expect("cycle").count, 1);
        assert_eq!(r.lead.expect("lead").count, 1);
    }

    #[test]
    fn missing_start_is_listed_and_excluded_from_cycle_but_kept_in_lead() {
        let items = [
            // This item has no start_at: Cycle Time is unavailable, but Lead Time still applies.
            item(7, utc(2026, 7, 1, 0), None, Some(utc(2026, 7, 3, 0))),
            item(
                3,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 0)),
            ),
        ];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.completed, 2);
        // There is only one Cycle with start_at.
        assert_eq!(r.cycle.expect("cycle").count, 1);
        // Lead completed all 2 items.
        assert_eq!(r.lead.expect("lead").count, 2);
        assert_eq!(r.missing_start, vec![ItemId::new("T", 7)]);
    }

    #[test]
    fn median_of_even_count_averages_the_two_middle_values() {
        // cycle: 10h, 20h, 30h, 40h → median (20+30)/2 = 25h
        let items = [
            item(
                1,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 1, 10)),
            ),
            item(
                2,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 1, 20)),
            ),
            item(
                3,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 6)),
            ),
            item(
                4,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 16)),
            ),
        ];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.cycle.expect("cycle").median, Duration::hours(25));
    }

    #[test]
    fn median_of_odd_count_is_the_middle_value() {
        let items = [
            item(
                1,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 1, 10)),
            ),
            item(
                2,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 1, 20)),
            ),
            item(
                3,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 6)),
            ),
        ];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.cycle.expect("cycle").median, Duration::hours(20));
    }

    #[test]
    fn filters_by_sprint() {
        let mut a = item(
            1,
            utc(2026, 7, 1, 0),
            Some(utc(2026, 7, 1, 0)),
            Some(utc(2026, 7, 2, 0)),
        );
        a.sprint = Some("S-1".into());
        let mut b = item(
            2,
            utc(2026, 7, 1, 0),
            Some(utc(2026, 7, 1, 0)),
            Some(utc(2026, 7, 2, 0)),
        );
        b.sprint = Some("S-2".into());
        let items = [a, b];
        let filter = CycleTimeFilter {
            sprint: Some("S-1".into()),
            ..Default::default()
        };
        let r = compute_report(&items, &filter);
        assert_eq!(r.completed, 1, "only S-1 items counted");
    }

    #[test]
    fn filters_by_completion_period() {
        let items = [
            item(
                1,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 2, 0)),
            ),
            item(
                2,
                utc(2026, 7, 1, 0),
                Some(utc(2026, 7, 1, 0)),
                Some(utc(2026, 7, 10, 0)),
            ),
        ];
        // Only those completed after 7/5 → T-2 only.
        let filter = CycleTimeFilter {
            since: Some(utc(2026, 7, 5, 0)),
            ..Default::default()
        };
        let r = compute_report(&items, &filter);
        assert_eq!(r.completed, 1);
        // Only those completed before 7/3 → T-1 only.
        let filter = CycleTimeFilter {
            until: Some(utc(2026, 7, 3, 0)),
            ..Default::default()
        };
        let r = compute_report(&items, &filter);
        assert_eq!(r.completed, 1);
    }

    #[test]
    fn no_completed_items_yields_empty_summaries() {
        let items = [item(1, utc(2026, 7, 1, 0), Some(utc(2026, 7, 1, 0)), None)];
        let r = compute_report(&items, &CycleTimeFilter::default());
        assert_eq!(r.completed, 0);
        assert_eq!(r.cycle, None);
        assert_eq!(r.lead, None);
        assert!(r.missing_start.is_empty());
    }
}
