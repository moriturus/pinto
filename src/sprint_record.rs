//! Domain model for records attached to a Sprint.

use crate::sprint::SprintId;
use chrono::{DateTime, Utc};
use std::fmt;

/// The kind of child record attached to a Sprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SprintRecordKind {
    /// Sprint retrospective record.
    Retro,
    /// Sprint review record.
    Review,
}

impl SprintRecordKind {
    /// Every record kind, in a stable order. The single source of truth for code that must sweep
    /// or enumerate all kinds (deletion, board replacement, and interchange).
    pub const ALL: [SprintRecordKind; 2] = [Self::Retro, Self::Review];

    /// Return the stable lowercase value used by persistence, JSON, and CLI commands.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Retro => "retro",
            Self::Review => "review",
        }
    }

    /// Return the human-readable singular name.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Retro => "Retro",
            Self::Review => "Review",
        }
    }

    /// Return the human-readable plural name.
    #[must_use]
    pub const fn plural_name(self) -> &'static str {
        match self {
            Self::Retro => "Retros",
            Self::Review => "Reviews",
        }
    }

    /// Return the on-disk directory name for this record kind.
    #[must_use]
    pub const fn directory(self) -> &'static str {
        self.as_str()
    }
}

impl fmt::Display for SprintRecordKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A kind-tagged record attached to a Sprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SprintRecord {
    /// The record kind, which also selects its storage directory and CLI namespace.
    pub kind: SprintRecordKind,
    /// Stable ID shared with the parent Sprint.
    pub id: SprintId,
    /// Markdown record body.
    pub body: String,
    /// Creation timestamp.
    pub created: DateTime<Utc>,
    /// Last update timestamp.
    pub updated: DateTime<Utc>,
}

impl SprintRecord {
    /// Create an empty or prefilled record for a Sprint.
    #[must_use]
    pub fn new(
        kind: SprintRecordKind,
        id: SprintId,
        body: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            kind,
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
    fn all_lists_every_kind_once() {
        assert_eq!(
            SprintRecordKind::ALL,
            [SprintRecordKind::Retro, SprintRecordKind::Review]
        );
    }

    #[test]
    fn kind_names_match_persistence_and_cli_contracts() {
        assert_eq!(SprintRecordKind::Retro.as_str(), "retro");
        assert_eq!(SprintRecordKind::Retro.display_name(), "Retro");
        assert_eq!(SprintRecordKind::Retro.plural_name(), "Retros");
        assert_eq!(SprintRecordKind::Retro.directory(), "retro");
        assert_eq!(SprintRecordKind::Review.as_str(), "review");
        assert_eq!(SprintRecordKind::Review.display_name(), "Review");
        assert_eq!(SprintRecordKind::Review.plural_name(), "Reviews");
        assert_eq!(SprintRecordKind::Review.directory(), "review");
    }

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
        let mut record = SprintRecord::new(
            SprintRecordKind::Retro,
            SprintId::new("S-1").expect("valid ID"),
            "notes",
            created,
        );

        assert_eq!(record.body, "notes");
        assert_eq!(record.created, created);
        assert_eq!(record.updated, created);

        record.update_body("revised", updated);
        assert_eq!(record.id.as_str(), "S-1");
        assert_eq!(record.body, "revised");
        assert_eq!(record.created, created);
        assert_eq!(record.updated, updated);
    }
}
