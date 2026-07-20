//! Scrum report text formatting.

use super::item::format_ids;
use chrono::Duration;
use pinto::service::{Burndown, CycleTimeReport, DurationSummary};

/// Format the burndown chart into human-readable text.
///
/// Draw one line per day with a horizontal bar (`█`) for remaining work and `┆` for the ideal
/// line. If `max_width` is too narrow for the bar, fall back to a numerical table while keeping
/// the remaining and ideal values readable.
pub(crate) fn format_burndown(chart: &Burndown, max_width: usize) -> String {
    /// Minimum track width required for a bar; narrower output uses the numerical table.
    const MIN_TRACK: usize = 10;
    /// Fixed width outside the bar: date, spacing, borders, and spacing around the value.
    const FIXED: usize = 10 + 1 + 1 + 1 + 1;

    // Align remaining and ideal values to the width of the total, displaying ideal as a rounded integer.
    let value_width = chart.total.to_string().chars().count().max(1);

    let mut out = String::new();
    out.push_str(&format!(
        "{} {} — burndown ({})\n",
        chart.sprint_id,
        chart.sprint_title,
        chart.metric.as_str(),
    ));
    if let (Some(first), Some(last)) = (chart.days.first(), chart.days.last()) {
        out.push_str(&format!(
            "Period {} → {} · total {}\n",
            first.date, last.date, chart.total,
        ));
    }

    // Reserve the fixed fields before deciding whether a bar fits.
    let track = max_width.saturating_sub(FIXED + value_width);
    if track < MIN_TRACK {
        // Keep the `rem` and `ideal` headings wide enough for the total value.
        let rem_w = value_width.max(3);
        let ideal_w = value_width.max(5);
        out.push_str(&format!(
            "{:<10}  {:>rem_w$}  {:>ideal_w$}\n",
            "date", "rem", "ideal",
        ));
        for d in &chart.days {
            out.push_str(&format!(
                "{}  {:>rem_w$}  {:>ideal_w$}\n",
                d.date,
                d.remaining,
                d.ideal.round() as i64,
            ));
        }
        return out;
    }

    // Legend (bar = actual remaining, marker = ideal line).
    out.push_str("█ remaining  ┆ ideal\n");
    for d in &chart.days {
        let cells = scale(d.remaining, chart.total, track);
        let ideal_idx = scale_round(d.ideal, chart.total, track).min(track.saturating_sub(1));
        let mut bar: Vec<char> = (0..track)
            .map(|i| if i < cells { '█' } else { '░' })
            .collect();
        // Draw the ideal marker over the bar so it remains visible at that position.
        bar[ideal_idx] = '┆';
        let bar: String = bar.into_iter().collect();
        out.push_str(&format!(
            "{} │{}│ {:>vw$}\n",
            d.date,
            bar,
            d.remaining,
            vw = value_width,
        ));
    }
    out
}

/// Format the Cycle/Lead Time analysis (`cycletime`) as human-readable text.
///
/// If no items are complete, return a one-line summary. Otherwise, show the count, mean, median,
/// minimum, and maximum for each metric. Durations use compact date/time units. Completed items
/// without `start_at` are listed in a warning because they are excluded from Cycle Time.
pub(crate) fn format_cycletime(report: &CycleTimeReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Cycle/Lead time — {} completed\n",
        report.completed
    ));
    if report.completed == 0 {
        return out;
    }
    if let Some(cycle) = &report.cycle {
        out.push_str(&format!(
            "  cycle (start → done)    {}\n",
            format_summary(cycle)
        ));
    }
    if let Some(lead) = &report.lead {
        out.push_str(&format!(
            "  lead  (created → done)  {}\n",
            format_summary(lead)
        ));
    }
    if !report.missing_start.is_empty() {
        out.push_str(&format!(
            "\n⚠ {} completed item(s) without start time (excluded from cycle time): {}\n",
            report.missing_start.len(),
            format_ids(&report.missing_start),
        ));
    }
    out
}

/// Format one line of summary statistics (`n=… mean … median … min … max …`).
fn format_summary(s: &DurationSummary) -> String {
    format!(
        "n={} mean {} median {} min {} max {}",
        s.count,
        format_duration(s.mean),
        format_duration(s.median),
        format_duration(s.min),
        format_duration(s.max),
    )
}

/// Format a [`Duration`] as a compact, readable date/time value (for example, `2d 4h`, `12h 30m`,
/// `45m`, or `30s`).
///
/// Show at most two units, using enough precision for Cycle/Lead Time analysis. Zero is `0s`, and
/// negative values (such as manually edited `done_at < start_at`) receive a `-` prefix.
pub(super) fn format_duration(d: Duration) -> String {
    let secs = d.num_seconds();
    if secs == 0 {
        return "0s".to_string();
    }
    let sign = if secs < 0 { "-" } else { "" };
    let secs = secs.unsigned_abs();
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let mins = (secs % 3_600) / 60;
    let s = secs % 60;
    let body = if days > 0 {
        if hours > 0 {
            format!("{days}d {hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if mins > 0 {
            format!("{hours}h {mins}m")
        } else {
            format!("{hours}h")
        }
    } else if mins > 0 {
        if s > 0 {
            format!("{mins}m {s}s")
        } else {
            format!("{mins}m")
        }
    } else {
        format!("{s}s")
    };
    format!("{sign}{body}")
}

/// Scale `value / total` to a number of display cells; return zero when `total == 0`.
fn scale(value: u32, total: u32, width: usize) -> usize {
    if total == 0 {
        0
    } else {
        (u64::from(value) * width as u64 / u64::from(total)) as usize
    }
}

/// Scale `value / total` to display cells and round to the nearest cell; return zero when
/// `total == 0`.
fn scale_round(value: f64, total: u32, width: usize) -> usize {
    if total == 0 {
        0
    } else {
        (value / f64::from(total) * width as f64).round() as usize
    }
}
