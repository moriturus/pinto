//! Domain model for the review record attached to a Sprint.

use crate::sprint::SprintId;
use chrono::{DateTime, Utc};

/// A Sprint Review record.
///
/// A Review deliberately has no separate generated ID: its ID is the parent Sprint ID, which
/// makes the one-record-per-Sprint invariant visible in both the domain and the file name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintReview {
    /// Stable ID shared with the parent Sprint.
    pub id: SprintId,
    /// Markdown Review body.
    pub body: String,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Last update timestamp.
    pub updated: DateTime<Utc>,
}

impl SprintReview {
    /// Create an empty or prefilled Review for a Sprint.
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
        let mut review =
            SprintReview::new(SprintId::new("S-1").expect("valid ID"), "notes", created);

        assert_eq!(review.body, "notes");
        assert_eq!(review.created, created);
        assert_eq!(review.updated, created);

        review.update_body("revised", updated);
        assert_eq!(review.id.as_str(), "S-1");
        assert_eq!(review.body, "revised");
        assert_eq!(review.created, created);
        assert_eq!(review.updated, updated);
    }
}
