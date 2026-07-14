//! Workflow state.

use std::fmt;

/// Workflow status (Kanban column name).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Status(String);

impl Status {
    /// Create a status, trimming leading and trailing whitespace.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    /// Return the status string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
