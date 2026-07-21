//! Sprint creation, state transition, and PBI assignment services.
//!
//! Combines the [`crate::sprint::Sprint`] and [`crate::backlog::BacklogItem`] domain types with the
//! persistence layer. A backlog item's sprint assignment is stored as the sprint ID string in
//! `BacklogItem::sprint`. Split by concern into `lifecycle` (create/transition/assign) and
//! `capacity` (capacity, load warnings, and listing).

use crate::sprint::SprintId;

mod capacity;
mod lifecycle;

pub use capacity::{list_sprints, set_sprint_capacity, sprint_capacity, sprint_load_warnings};
pub(crate) use lifecycle::validate_sprint_assignment;
pub use lifecycle::{
    assign_sprint, assign_sprint_by_status, assign_sprint_raw, close_sprint, create_sprint,
    delete_sprint, edit_sprint, start_sprint, unassign_sprint,
};

// Referenced by the test module below via `use super::*`.
#[cfg(test)]
use capacity::sprint_load_warnings_for;

/// The source of a non-blocking Sprint load warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprintLoadWarningKind {
    /// The assigned point total is above the configured capacity-hours threshold.
    Capacity,
    /// The assigned point total is above the historical velocity threshold.
    Velocity,
}

impl SprintLoadWarningKind {
    /// Return the short label used in localized CLI warning messages.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Capacity => "capacity",
            Self::Velocity => "velocity",
        }
    }

    /// Return the unit attached to the numeric threshold in CLI output.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Capacity => "hours",
            Self::Velocity => "points",
        }
    }
}

/// A non-blocking warning produced when a Sprint's assigned points exceed a threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct SprintLoadWarning {
    /// Which planning comparison was exceeded.
    pub kind: SprintLoadWarningKind,
    /// Sum of estimated points assigned to the Sprint.
    pub points: u32,
    /// Numeric threshold that the assigned points exceeded.
    pub threshold: f64,
}

/// How unfinished PBIs are handled when their sprint closes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum SprintCloseAction {
    /// Keep unfinished PBIs assigned to the closed sprint.
    #[default]
    Retain,
    /// Reassign unfinished PBIs to a planned or active sprint.
    Rollover(SprintId),
    /// Clear the sprint assignment from unfinished PBIs.
    Release,
}

#[cfg(test)]
mod tests;
