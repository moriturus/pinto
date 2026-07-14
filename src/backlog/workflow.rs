//! Kanban workflow.

use super::Status;

/// Kanban workflow (ordered set of columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    columns: Vec<Status>,
}

impl Workflow {
    /// Build a workflow from columns (left to right in workflow order).
    pub fn new(columns: impl IntoIterator<Item = Status>) -> Self {
        Self {
            columns: columns.into_iter().collect(),
        }
    }

    /// List of columns.
    #[must_use]
    pub fn columns(&self) -> &[Status] {
        &self.columns
    }

    /// Default state (first column).
    #[must_use]
    pub fn default_status(&self) -> Option<&Status> {
        self.columns.first()
    }

    /// Whether the specified state exists in the workflow.
    #[must_use]
    pub fn contains(&self, status: &Status) -> bool {
        self.columns.contains(status)
    }

    /// Whether a transition from `from` to `to` is valid.
    ///
    /// It is valid when both columns are known; Kanban permits moves between any columns.
    #[must_use]
    pub fn can_transition(&self, from: &Status, to: &Status) -> bool {
        self.contains(from) && self.contains(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workflow() -> Workflow {
        Workflow::new(
            ["todo", "in-progress", "review", "done"]
                .into_iter()
                .map(Status::new),
        )
    }

    #[test]
    fn workflow_default_status_is_first_column() {
        assert_eq!(workflow().default_status(), Some(&Status::new("todo")));
    }

    #[test]
    fn workflow_allows_transition_between_known_columns() {
        let wf = workflow();
        assert!(wf.can_transition(&Status::new("todo"), &Status::new("done")));
        assert!(wf.can_transition(&Status::new("review"), &Status::new("in-progress")));
    }

    #[test]
    fn workflow_rejects_transition_to_unknown_column() {
        let wf = workflow();
        assert!(!wf.can_transition(&Status::new("todo"), &Status::new("archived")));
        assert!(!wf.can_transition(&Status::new("backlog"), &Status::new("done")));
    }
}
