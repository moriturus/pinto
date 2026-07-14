//! Scrum report text formatting.

use pinto::service::{Burndown, CycleTimeReport};

/// Format the Sprint burndown report.
pub(crate) fn format_burndown(chart: &Burndown, max_width: usize) -> String {
    super::format_burndown(chart, max_width)
}

/// Format the cycle and lead-time report.
pub(crate) fn format_cycletime(report: &CycleTimeReport) -> String {
    super::format_cycletime(report)
}
