//! Sprint text formatting.

use pinto::service::VelocityReport;
use pinto::sprint::{Sprint, SprintCapacity};
use pinto::timezone::DisplayTimezone;

/// Format Sprint velocity history.
pub(crate) fn format_velocity(report: &VelocityReport, recent: usize) -> String {
    let mut out = format!("Velocity (last {recent} sprints)\n");
    for row in &report.sprints {
        out.push_str(&format!(
            "{}  {} points  completed: {}  unestimated: {}  incomplete: {}\n",
            row.sprint_id,
            row.points,
            row.completed_items,
            row.unestimated_completed_items,
            row.incomplete_items,
        ));
    }
    out.push_str(&format!("Average: {:.1} points\n", report.average_points));
    match report.change_percent {
        Some(change) => out.push_str(&format!("Change: {change:+.1}% vs prior average\n")),
        None => out.push_str("Change: n/a (need a non-zero prior average)\n"),
    }
    out
}

/// Format the Sprint list with a configured human-readable timestamp timezone.
pub(crate) fn format_sprints_with_timezone(
    sprints: &[Sprint],
    timezone: DisplayTimezone,
) -> String {
    let id_width = sprints
        .iter()
        .map(|s| s.id.as_str().chars().count())
        .max()
        .unwrap_or(0);
    let state_width = sprints
        .iter()
        .map(|s| s.state.as_str().chars().count())
        .max()
        .unwrap_or(0);

    let mut out = String::new();
    for sprint in sprints {
        let mut line = format!(
            "{:<id_width$}  {:<state_width$}  {}",
            sprint.id.as_str(),
            sprint.state.as_str(),
            sprint.title,
        );
        if let (Some(start), Some(end)) = (sprint.start, sprint.end) {
            line.push_str(&format!(
                "  ({} → {})",
                timezone.format_datetime(start, "%Y-%m-%d %H:%M"),
                timezone.format_datetime(end, "%Y-%m-%d %H:%M"),
            ));
        }
        if !sprint.goal.is_empty() {
            line.push_str(&format!("  goal: {}", sprint.goal.replace('\n', " / ")));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Format the capacity summary for one Sprint.
pub(crate) fn format_sprint_capacity(capacity: &SprintCapacity) -> String {
    format!(
        "Working days: {}\nCapacity: {} hours\n",
        capacity.working_days, capacity.hours
    )
}
