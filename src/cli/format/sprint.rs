//! Sprint text formatting.

use pinto::service::{SprintContext, SprintGoalReport, VelocityReport};
use pinto::sprint::{Sprint, SprintCapacity};
use pinto::timezone::DisplayTimezone;

/// Format Sprint velocity history.
pub(crate) fn format_velocity(report: &VelocityReport, recent: usize) -> String {
    let mut out = format!("Velocity (last {recent} sprints)\n");
    for row in &report.sprints {
        out.push_str(&format!(
            "{}  {} points  completed: {}  unestimated: {}  incomplete: {}  spillover: {} points ({} items, {} unestimated)\n",
            row.sprint_id,
            row.points,
            row.completed_items,
            row.unestimated_completed_items,
            row.incomplete_items,
            row.spillover.points,
            row.spillover.items,
            row.spillover.unestimated_items,
        ));
    }
    out.push_str(&format!("Average: {:.1} points\n", report.average_points));
    match report.change_percent {
        Some(change) => out.push_str(&format!("Change: {change:+.1}% vs prior average\n")),
        None => out.push_str("Change: n/a (need a non-zero prior average)\n"),
    }
    out
}

/// Format Sprint Goal outcomes and the aggregate achievement rate.
pub(crate) fn format_sprint_goal_report(report: &SprintGoalReport, recent: usize) -> String {
    let mut out = format!("Sprint Goal outcomes (last {recent} sprints)\n");
    for row in &report.sprints {
        let outcome = match row.goal_achieved {
            Some(true) => "achieved",
            Some(false) => "not-achieved",
            None => "unevaluated",
        };
        out.push_str(&format!(
            "{}  {}  {}\n",
            row.sprint_id, outcome, row.sprint_title
        ));
    }
    match report.achievement_rate {
        Some(rate) => out.push_str(&format!(
            "Achievement rate: {rate:.1}% ({}/{} evaluated)\n",
            report.achieved_sprints, report.evaluated_sprints
        )),
        None => out.push_str("Achievement rate: n/a (no evaluated Sprint Goals)\n"),
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

/// Format generated parent-Sprint context for a Retro or Review detail view.
pub(crate) fn format_sprint_context(context: &SprintContext, timezone: DisplayTimezone) -> String {
    let mut out = String::from("Sprint Context (generated)\n");
    out.push_str(&format!(
        "Sprint: {}  {}\n",
        context.sprint_id, context.sprint_title
    ));
    if context.goal.trim().is_empty() {
        out.push_str("Goal: unavailable (not set)\n");
    } else {
        out.push_str(&format!("Goal: {}\n", context.goal.replace('\n', " / ")));
    }
    let goal_outcome = match context.goal_achieved {
        Some(true) => "achieved",
        Some(false) => "not-achieved",
        None => "unevaluated",
    };
    out.push_str(&format!("Goal outcome: {goal_outcome}\n"));
    out.push_str(&format!("State: {}\n", context.state));
    match (context.start, context.end) {
        (Some(start), Some(end)) => out.push_str(&format!(
            "Schedule: {} → {}\n",
            timezone.format_datetime(start, "%Y-%m-%d %H:%M"),
            timezone.format_datetime(end, "%Y-%m-%d %H:%M"),
        )),
        _ => out.push_str("Schedule: unavailable (not set)\n"),
    }
    if let Some(closed_at) = context.closed_at {
        out.push_str(&format!(
            "Closed: {}\n",
            timezone.format_datetime(closed_at, "%Y-%m-%d %H:%M"),
        ));
    }

    if let Some(capacity) = &context.capacity {
        out.push_str(&format!(
            "Capacity: {} working days, {:.1} hours\n",
            capacity.working_days, capacity.hours
        ));
    } else {
        out.push_str("Capacity: unavailable (not configured)\n");
    }

    if let Some(velocity) = &context.velocity {
        out.push_str(&format!(
            "Velocity: {} points, {} completed items, {} unestimated, {} incomplete\n",
            velocity.points,
            velocity.completed_items,
            velocity.unestimated_completed_items,
            velocity.incomplete_items,
        ));
    } else {
        out.push_str("Velocity: unavailable (no completed PBIs)\n");
    }

    if let Some(burndown) = &context.burndown {
        if let (Some(first), Some(last)) = (burndown.days.first(), burndown.days.last()) {
            out.push_str(&format!(
                "Burndown: {} metric, total {}, remaining {} ({} → {})\n",
                burndown.metric.as_str(),
                burndown.total,
                last.remaining,
                first.date,
                last.date,
            ));
        } else {
            out.push_str("Burndown: unavailable (no observations)\n");
        }
    } else {
        out.push_str("Burndown: unavailable (period or assigned PBIs are missing)\n");
    }

    if let Some(cycle_time) = &context.cycle_time {
        for line in super::report::format_cycletime(cycle_time).lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
    } else {
        out.push_str("Cycle/Lead time: unavailable (no completed PBIs)\n");
    }

    if let Some(spillover) = context.spillover {
        out.push_str(&format!(
            "Spillover: {} points, {} items, {} unestimated\n",
            spillover.points, spillover.items, spillover.unestimated_items,
        ));
    } else {
        out.push_str("Spillover: unavailable (Sprint is not closed)\n");
    }
    out
}
