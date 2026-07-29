//! Structured links from action PBIs to their Sprint Retro or Review source.

use crate::sprint::SprintId;

/// The child record that produced an action PBI.
pub use crate::sprint_record::SprintRecordKind as ActionSourceKind;

/// A machine-readable link from an action PBI to its source child record.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ActionSource {
    /// Source child record kind.
    pub kind: ActionSourceKind,
    /// Sprint ID shared by the source child record and its parent Sprint.
    pub sprint_id: SprintId,
}

impl ActionSource {
    /// Create a source link for a child record belonging to `sprint_id`.
    #[must_use]
    pub const fn new(kind: ActionSourceKind, sprint_id: SprintId) -> Self {
        Self { kind, sprint_id }
    }
}
