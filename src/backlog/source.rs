//! Structured links from action PBIs to their Sprint Retro or Review source.

use crate::sprint::SprintId;

/// The child record that produced an action PBI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionSourceKind {
    /// Sprint Retro record.
    Retro,
    /// Sprint Review record.
    Review,
}

impl ActionSourceKind {
    /// Return the stable lowercase value used by persistence and JSON output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retro => "retro",
            Self::Review => "review",
        }
    }
}

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
