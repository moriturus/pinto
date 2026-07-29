//! Domain model for the retrospective record attached to a Sprint.

use crate::sprint::SprintId;
use chrono::{DateTime, Utc};

/// A Sprint retrospective record.
///
/// A Retro deliberately has no separate generated ID: its ID is the parent Sprint ID, which
/// makes the one-record-per-Sprint invariant visible in both the domain and the file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintRetro {
    /// Stable ID shared with the parent Sprint.
    pub id: SprintId,
    /// Markdown retrospective body.
    pub body: String,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Last update timestamp.
    pub updated: DateTime<Utc>,
}

impl SprintRetro {
    /// Create an empty or prefilled Retro for a Sprint.
    #[must_use]
    pub fn new(id: SprintId, body: impl Into<String>, now: DateTime<Utc>) -> Self {
        Self {
            id,
            body: body.into(),
            created: now,
            updated: now,
        }
    }

    /// Replace the Markdown body and refresh the update timestamp.
    pub fn update_body(&mut self, body: impl Into<String>, now: DateTime<Utc>) {
        self.body = body.into();
        self.updated = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn new_and_update_preserve_identity_and_timestamps() {
        let created = Utc
            .timestamp_opt(1_000, 0)
            .single()
            .expect("valid timestamp");
        let updated = Utc
            .timestamp_opt(2_000, 0)
            .single()
            .expect("valid timestamp");
        let mut retro = SprintRetro::new(SprintId::new("S-1").expect("valid ID"), "notes", created);

        assert_eq!(retro.body, "notes");
        assert_eq!(retro.created, created);
        assert_eq!(retro.updated, created);

        retro.update_body("revised", updated);
        assert_eq!(retro.id.as_str(), "S-1");
        assert_eq!(retro.body, "revised");
        assert_eq!(retro.created, created);
        assert_eq!(retro.updated, updated);
    }
}
