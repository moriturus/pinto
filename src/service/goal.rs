//! Sprint Goal outcome and achievement-rate reporting.

use super::open_board;
use crate::error::Result;
use crate::sprint::{Sprint, SprintGoalOutcome, SprintId};
use crate::storage::SprintRepository;
use std::path::Path;

/// One Sprint row in the Goal achievement report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintGoalReportRow {
    /// Stable Sprint ID.
    pub sprint_id: SprintId,
    /// Sprint display title.
    pub sprint_title: String,
    /// Explicit result, or `None` when the result is not evaluated. The write-side invariant
    /// clears a result whenever the Goal is blank.
    pub goal_achieved: SprintGoalOutcome,
}

/// Sprint Goal outcomes and their aggregate achievement rate.
#[derive(Debug, Clone, PartialEq)]
pub struct SprintGoalReport {
    /// Selected Sprints in creation order.
    pub sprints: Vec<SprintGoalReportRow>,
    /// Number of selected Sprints with an evaluated result.
    pub evaluated_sprints: usize,
    /// Number of evaluated Sprints whose result is `true`.
    pub achieved_sprints: usize,
    /// Achievement rate as a percentage, or `None` when no selected Sprint was evaluated.
    pub achievement_rate: Option<f64>,
}

/// Report Sprint Goal outcomes for the most recent `recent` Sprints.
///
/// The write-side Sprint invariant ensures that a stored boolean always accompanies a non-blank
/// Goal, so this report can expose the stored outcome directly.
///
/// # Errors
///
/// Returns [`crate::error::Error::NotInitialized`] or persistence and parsing errors while
/// loading the Sprint records. The operation is read-only and safe to retry after a transient
/// read failure.
pub async fn sprint_goal_report(project_dir: &Path, recent: usize) -> Result<SprintGoalReport> {
    let (_board_dir, repo, _config) = open_board(project_dir).await?;
    let sprints = SprintRepository::list(&repo).await?;
    Ok(compute_sprint_goal_report(&sprints, recent))
}

/// Calculate the Goal report from already loaded Sprints.
pub(crate) fn compute_sprint_goal_report(sprints: &[Sprint], recent: usize) -> SprintGoalReport {
    let start = sprints.len().saturating_sub(recent);
    let rows = sprints[start..]
        .iter()
        .map(|sprint| SprintGoalReportRow {
            sprint_id: sprint.id.clone(),
            sprint_title: sprint.title.clone(),
            goal_achieved: sprint.goal_achieved,
        })
        .collect::<Vec<_>>();
    let evaluated_sprints = rows
        .iter()
        .filter(|row| row.goal_achieved.is_some())
        .count();
    let achieved_sprints = rows
        .iter()
        .filter(|row| row.goal_achieved == Some(true))
        .count();
    let achievement_rate =
        (evaluated_sprints > 0).then(|| achieved_sprints as f64 / evaluated_sprints as f64 * 100.0);

    SprintGoalReport {
        sprints: rows,
        evaluated_sprints,
        achieved_sprints,
        achievement_rate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprint::SprintId;
    use chrono::{DateTime, Utc};

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(0, 0).expect("valid timestamp")
    }

    fn sprint(id: &str, goal: &str, achieved: SprintGoalOutcome) -> Sprint {
        let mut sprint = Sprint::new(SprintId::new(id).expect("valid sprint id"), id, now())
            .expect("valid sprint");
        sprint.goal = goal.to_string();
        sprint.goal_achieved = achieved;
        sprint
    }

    #[test]
    fn computes_rate_from_recorded_boolean_results() {
        let report = compute_sprint_goal_report(
            &[
                sprint("S-1", "Ship it", Some(true)),
                sprint("S-2", "Ship more", Some(false)),
                sprint("S-3", "", None),
                sprint("S-4", "Explore", None),
            ],
            10,
        );

        assert_eq!(report.evaluated_sprints, 2);
        assert_eq!(report.achieved_sprints, 1);
        assert_eq!(report.achievement_rate, Some(50.0));
        assert_eq!(report.sprints[2].goal_achieved, None);
        assert_eq!(report.sprints[3].goal_achieved, None);
    }

    #[test]
    fn reports_no_rate_when_no_goal_is_evaluated() {
        let report = compute_sprint_goal_report(
            &[sprint("S-1", "", None), sprint("S-2", "Explore", None)],
            5,
        );

        assert_eq!(report.evaluated_sprints, 0);
        assert_eq!(report.achieved_sprints, 0);
        assert_eq!(report.achievement_rate, None);
    }
}
