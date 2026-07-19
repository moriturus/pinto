//! The crate-wide error type.
//!
//! Covers domain-layer validation failures alongside I/O, parsing, and backend errors, so a
//! single `Error`/`Result` flows through every layer.

use crate::backlog::ItemId;
use crate::i18n::{Localizer, Message};
use crate::sprint::{SprintId, SprintState};
use crate::template::TemplateName;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors returned by pinto's domain, service, and persistence layers.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    /// PBI ID format is invalid (expected an ASCII-letter prefix and decimal number).
    #[error("invalid item id: {0:?} (expected `<ASCII_LETTERS>-<NUMBER>`)")]
    InvalidItemId(String),

    /// Invalid rank string (expected non-empty base-36 alphanumeric `0-9a-z`).
    #[error("invalid rank: {0:?} (expected non-empty base-36 chars `0-9a-z`)")]
    InvalidRank(String),

    /// The automation plan is malformed or contains a command that is not permitted.
    #[error(
        "invalid automation plan (expected {{\"commands\":[[\"command\", \"arg\"], ...]}}; API keys are not accepted)"
    )]
    InvalidAutomationPlan,

    /// A plan file or standard-input source could not be read.
    #[error(
        "cannot read automation plan source {path}: {message}; provide inline JSON, a readable file path, or `-` for standard input"
    )]
    AutomationPlanSource { path: PathBuf, message: String },

    /// The transition destination does not exist in the workflow column.
    #[error("unknown status: {0:?} is not a column in the workflow")]
    UnknownStatus(String),

    /// Title is empty.
    #[error("title must not be empty")]
    EmptyTitle,

    /// Search pattern is invalid.
    #[error("invalid search pattern: {0}")]
    InvalidSearchPattern(String),

    /// A filter option was used without the value required by its display mode.
    #[error("invalid filter option: {0}")]
    InvalidFilterOption(String),

    /// No fields were specified to update (empty update of `edit`).
    #[error("no fields to update")]
    NothingToUpdate,

    /// An attempt was made to set an empty string (only spaces) to the common DoD.
    #[error("definition of done must not be empty")]
    EmptyDod,

    /// Template name cannot be used as a safe file name.
    #[error("invalid template name: {0:?} (expected non-empty ASCII alphanumeric, `-`, or `_`)")]
    InvalidTemplateName(String),

    /// The specified template file does not exist.
    #[error("template not found: {kind}/{name} (create {path})")]
    TemplateNotFound {
        kind: &'static str,
        name: TemplateName,
        path: PathBuf,
    },

    /// Cannot read template file as body text.
    #[error("template cannot be read: {path} ({message}; fix or replace this plain-text file)")]
    TemplateUnreadable { path: PathBuf, message: String },

    /// No backlog item exists with the specified ID.
    #[error("backlog item not found: {0}")]
    NotFound(ItemId),

    /// The PBI cannot be physically deleted because active PBIs still refer to it.
    #[error(
        "cannot remove {item}: referenced by {references}; remove these parent/dependency links first"
    )]
    ReferencedItem {
        /// PBI targeted for permanent deletion.
        item: ItemId,
        /// IDs of active PBIs that refer to `item`.
        references: String,
    },

    /// The sprint ID is malformed; it must contain one or more ASCII letters, digits, `-`, or `_`.
    #[error("invalid sprint id: {0:?} (expected non-empty ASCII alphanumeric, `-`, or `_`)")]
    InvalidSprintId(String),

    /// Invalid sprint status string (expected `planned` / `active` / `closed`).
    #[error("invalid sprint state: {0:?} (expected `planned`, `active`, or `closed`)")]
    InvalidSprintState(String),

    /// Sprint title is empty.
    #[error("sprint title must not be empty")]
    EmptySprintTitle,

    /// Sprint goal is required before starting a sprint.
    #[error("sprint goal must be set before starting the sprint")]
    EmptySprintGoal,

    /// The sprint state transition is invalid; only `planned → active → closed` is allowed.
    #[error("invalid sprint transition: {from} -> {to} (allowed: planned -> active -> closed)")]
    InvalidSprintTransition {
        /// Current state.
        from: SprintState,
        /// The state to transition to.
        to: SprintState,
    },

    /// A sprint with the specified ID cannot be found.
    #[error("sprint not found: {0}")]
    SprintNotFound(SprintId),

    /// A sprint with the requested ID already exists.
    ///
    /// Creating a sprint never overwrites an existing sprint. Use `edit`/`remove` to manage the
    /// existing record, or `start`/`close` to advance its state.
    #[error("sprint already exists: {0} (use `sprint edit`/`remove` to manage it)")]
    SprintExists(SprintId),

    /// A PBI cannot be assigned to a Sprint after that Sprint has been closed.
    #[error(
        "cannot assign a PBI to closed sprint {0} (assign it to a planned or active sprint instead; use `sprint unassign {0} <item-id>` to remove an existing assignment)"
    )]
    SprintClosed(SprintId),

    /// The planned sprint date is incorrect (start later than end).
    #[error("invalid sprint period: start {start} is after end {end}")]
    InvalidSprintPeriod {
        /// Planned start date.
        start: chrono::NaiveDate,
        /// Planned end date.
        end: chrono::NaiveDate,
    },

    /// The number of working hours per day is not a finite number greater than or equal to 0.
    #[error("daily work hours must be a finite number greater than or equal to 0 (got {0})")]
    InvalidDailyWorkHours(String),

    /// The deduction rate is not in the range of 0 to 1.
    #[error("deduction factor must be a finite number from 0 to 1 (got {0})")]
    InvalidDeductionFactor(String),

    /// The specified number of holidays exceeds the number of days in the sprint period.
    #[error(
        "holiday days ({holidays}) must not exceed the {calendar_days} calendar day(s) in the sprint period"
    )]
    InvalidSprintHolidays { holidays: u32, calendar_days: u32 },

    /// The sprint period required for capacity setting has not been set.
    #[error(
        "sprint {0} has no start/end dates (set them with `sprint edit {0} --start <YYYY-MM-DD> --end <YYYY-MM-DD>` before setting capacity)"
    )]
    SprintCapacityPeriodUnset(SprintId),

    /// Capacity setting is not set.
    #[error(
        "sprint {0} has no capacity settings (set them with `sprint capacity {0} --daily-hours <HOURS> --holidays <DAYS> --deduction-factor <0..1>`)"
    )]
    SprintCapacityUnset(SprintId),

    /// Planned dates (start and end) are not set for the sprint (burndown period cannot be determined).
    #[error(
        "sprint {0} has no start/end dates (set them with `sprint edit {0} --start <YYYY-MM-DD> --end <YYYY-MM-DD>`)"
    )]
    SprintPeriodUnset(SprintId),

    /// No PBIs are assigned to the sprint (no burndown target).
    #[error("sprint {0} has no assigned items (assign one with `sprint add {0} <item-id>`)")]
    SprintEmpty(SprintId),

    /// The backlog item is not assigned to the specified sprint.
    #[error("{item} is not assigned to sprint {sprint}")]
    NotInSprint {
        /// Target PBI.
        item: ItemId,
        /// The sprint you tried to unassign.
        sprint: SprintId,
    },

    /// A parent link would create a cycle by assigning the item to itself or one of its descendants.
    ///
    /// Parent-child links must remain acyclic so that the hierarchy stays a tree. Dependency links
    /// (`depends_on`) have separate cycle handling and are not rejected by this variant.
    #[error("setting parent of {child} to {parent} would create a cycle")]
    ParentCycle {
        /// PBI attempting to set parent.
        child: ItemId,
        /// Proposed parent; assigning it would create a cycle.
        parent: ItemId,
    },

    /// Board is uninitialized (`.pinto/` is missing).
    #[error(
        "not a pinto board: {path} not found after checking this directory and its ancestors (run `pinto init`, or use `--dir PATH` / `PINTO_DIR`)"
    )]
    NotInitialized {
        /// The expected path of `.pinto/`.
        path: PathBuf,
    },

    /// File I/O failed.
    #[error("i/o error at {path}: {message}")]
    Io {
        /// Target path.
        path: PathBuf,
        /// Message returned by the OS.
        message: String,
    },

    /// Frontmatter or body parsing failed.
    #[error("failed to parse {path}: {message}")]
    Parse {
        /// Target path.
        path: PathBuf,
        /// The message returned by the parser.
        message: String,
    },

    /// Frontmatter delimiter (`+++`) is missing.
    #[error("missing `+++` frontmatter delimiter in {path}")]
    MissingFrontmatter {
        /// Target path.
        path: PathBuf,
    },

    /// The SQLite database uses a schema version this build does not understand.
    #[error(
        "unsupported SQLite schema at {path}: found version {found:?}, but pinto supports version {supported}; upgrade or downgrade pinto to a compatible version, or recreate the database (automatic migration is not available)"
    )]
    UnsupportedSqliteSchema {
        /// SQLite database path.
        path: PathBuf,
        /// Raw value stored in the schema metadata.
        found: String,
        /// Only schema version currently understood by this build.
        supported: u32,
    },

    /// The reorder reference is the same item (`reorder <id> --before/--after <id>` use one ID).
    ///
    /// An item cannot be moved relative to itself.
    #[error("cannot reorder {0} relative to itself")]
    SelfReference(ItemId),

    /// `reorder <item> --before/--after <reference>` names a `reference` that is not a
    /// sibling of `item` (different parent or different column).
    ///
    /// Reorder only changes order **within a sibling group** (same parent and same
    /// status); use `edit --parent` to move an item between groups.
    #[error(
        "cannot reorder {item} relative to {reference}: they are not siblings (same parent and column); use `edit --parent` to regroup"
    )]
    NotSibling { item: ItemId, reference: ItemId },

    /// Execution of a parallel I/O task failed (internal error such as panic).
    #[error("background task failed: {0}")]
    Task(String),

    /// Git backend operation failed (`git` absent, command failure, etc.).
    ///
    /// For example, if `git` is not on `PATH`, install Git or set
    /// `[storage] backend = "file"`.
    #[error("git backend error: {0}")]
    Git(String),

    /// Neither `$EDITOR` nor `$VISUAL` is set, so editing cannot start.
    ///
    /// Set one of those environment variables or provide the content directly with `--body`.
    #[error(
        "no editor configured: set $VISUAL or $EDITOR, or provide content directly (e.g. `pinto add <title> --body ...` or `pinto edit <id> --title ...`)"
    )]
    EditorNotSet,

    /// The configured editor failed to start or terminate normally.
    #[error("failed to launch editor `{editor}`: {message}")]
    EditorLaunch {
        /// The command selected from `$VISUAL` or `$EDITOR`.
        editor: String,
        /// OS startup error or editor exit status.
        message: String,
    },

    /// The edited content is invalid and cannot be applied to the backlog item.
    ///
    /// This includes syntax errors and empty frontmatter titles. The original item remains unchanged;
    /// correct the content and try again.
    #[error("edited content is invalid: {message}")]
    EditorInvalid {
        /// The reason the parser/validator returned.
        message: String,
    },

    /// Another process was holding the board lock (`.pinto/.lock`) and the write could not be serialized.
    ///
    /// An advisory lock prevents simultaneous writes from losing updates through last-writer-wins
    /// behavior. The usual remedy is to wait for the other process. If a crash leaves the lock file
    /// behind, confirm that no `pinto` process is running and remove the file manually.
    #[error(
        "board is locked by another process ({path}); retry shortly, or remove the file if no pinto is running"
    )]
    Locked {
        /// Path of the lock file (`.pinto/.lock`) that could not be obtained.
        path: PathBuf,
    },
}

impl Error {
    /// Return the stable machine-facing code for this error variant.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidItemId(_) => "invalid-item-id",
            Self::InvalidRank(_) => "invalid-rank",
            Self::InvalidAutomationPlan => "invalid-automation-plan",
            Self::AutomationPlanSource { .. } => "automation-plan-source",
            Self::UnknownStatus(_) => "unknown-status",
            Self::EmptyTitle => "empty-title",
            Self::InvalidSearchPattern(_) => "invalid-search-pattern",
            Self::InvalidFilterOption(_) => "invalid-filter-option",
            Self::NothingToUpdate => "nothing-to-update",
            Self::EmptyDod => "empty-dod",
            Self::InvalidTemplateName(_) => "invalid-template-name",
            Self::TemplateNotFound { .. } => "template-not-found",
            Self::TemplateUnreadable { .. } => "template-unreadable",
            Self::NotFound(_) => "not-found",
            Self::ReferencedItem { .. } => "referenced-item",
            Self::InvalidSprintId(_) => "invalid-sprint-id",
            Self::InvalidSprintState(_) => "invalid-sprint-state",
            Self::EmptySprintTitle => "empty-sprint-title",
            Self::EmptySprintGoal => "empty-sprint-goal",
            Self::InvalidSprintTransition { .. } => "invalid-sprint-transition",
            Self::SprintNotFound(_) => "sprint-not-found",
            Self::SprintExists(_) => "sprint-exists",
            Self::SprintClosed(_) => "sprint-closed",
            Self::InvalidSprintPeriod { .. } => "invalid-sprint-period",
            Self::InvalidDailyWorkHours(_) => "invalid-daily-work-hours",
            Self::InvalidDeductionFactor(_) => "invalid-deduction-factor",
            Self::InvalidSprintHolidays { .. } => "invalid-sprint-holidays",
            Self::SprintCapacityPeriodUnset(_) => "sprint-capacity-period-unset",
            Self::SprintCapacityUnset(_) => "sprint-capacity-unset",
            Self::SprintPeriodUnset(_) => "sprint-period-unset",
            Self::SprintEmpty(_) => "sprint-empty",
            Self::NotInSprint { .. } => "not-in-sprint",
            Self::ParentCycle { .. } => "parent-cycle",
            Self::NotInitialized { .. } => "not-initialized",
            Self::Io { .. } => "io",
            Self::Parse { .. } => "parse",
            Self::MissingFrontmatter { .. } => "missing-frontmatter",
            Self::UnsupportedSqliteSchema { .. } => "unsupported-sqlite-schema",
            Self::SelfReference(_) => "self-reference",
            Self::NotSibling { .. } => "not-sibling",
            Self::Task(_) => "task",
            Self::Git(_) => "git",
            Self::EditorNotSet => "editor-not-set",
            Self::EditorLaunch { .. } => "editor-launch",
            Self::EditorInvalid { .. } => "editor-invalid",
            Self::Locked { .. } => "locked",
        }
    }

    /// Render this error through the selected locale for CLI/TUI boundaries.
    ///
    /// Values originating in the operating system, Git, TOML, or another parser are passed to
    /// the catalog unchanged. They are intentionally not translated because their exact wording
    /// is the actionable diagnostic users need when repairing an external condition. The
    /// `Display` implementation generated by `thiserror` remains the English fallback for
    /// library consumers that do not have a locale boundary. The English catalog is kept equal to
    /// that `Display` output for every variant, so choosing English does not change a diagnostic.
    pub fn localized(&self, localizer: &Localizer) -> String {
        macro_rules! message {
            ($message:expr $(, $name:expr => $value:expr)* $(,)?) => {{
                let values = [$(($name, $value.to_string())),*];
                localizer.format(
                    $message,
                    values.iter().map(|(name, value)| (*name, value.as_str())),
                )
            }};
        }

        match self {
            Self::InvalidItemId(value) => {
                message!(Message::ErrorInvalidItemId, "value" => format!("{value:?}"))
            }
            Self::InvalidRank(value) => {
                message!(Message::ErrorInvalidRank, "value" => format!("{value:?}"))
            }
            Self::InvalidAutomationPlan => localizer.text(Message::ErrorInvalidAutomationPlan),
            Self::AutomationPlanSource {
                path,
                message: detail,
            } => message!(
                Message::ErrorAutomationPlanSource,
                "path" => path.display(),
                "message" => detail,
            ),
            Self::UnknownStatus(status) => message!(
                Message::ErrorUnknownStatus,
                "status" => format!("{status:?}")
            ),
            Self::EmptyTitle => localizer.text(Message::ErrorEmptyTitle),
            Self::InvalidSearchPattern(pattern) => message!(
                Message::ErrorInvalidSearchPattern,
                "pattern" => pattern,
            ),
            Self::InvalidFilterOption(option) => message!(
                Message::ErrorInvalidFilterOption,
                "option" => option,
            ),
            Self::NothingToUpdate => localizer.text(Message::ErrorNothingToUpdate),
            Self::EmptyDod => localizer.text(Message::ErrorEmptyDod),
            Self::InvalidTemplateName(name) => message!(
                Message::ErrorInvalidTemplateName,
                "name" => format!("{name:?}"),
            ),
            Self::TemplateNotFound { kind, name, path } => message!(
                Message::ErrorTemplateNotFound,
                "kind" => kind,
                "name" => name,
                "path" => path.display(),
            ),
            Self::TemplateUnreadable {
                path,
                message: detail,
            } => message!(
                Message::ErrorTemplateUnreadable,
                "path" => path.display(),
                "message" => detail,
            ),
            Self::NotFound(id) => message!(Message::ErrorNotFound, "id" => id),
            Self::ReferencedItem { item, references } => message!(
                Message::ErrorReferencedItem,
                "item" => item,
                "references" => references,
            ),
            Self::InvalidSprintId(id) => message!(
                Message::ErrorInvalidSprintId,
                "id" => format!("{id:?}"),
            ),
            Self::InvalidSprintState(state) => message!(
                Message::ErrorInvalidSprintState,
                "state" => format!("{state:?}"),
            ),
            Self::EmptySprintTitle => localizer.text(Message::ErrorEmptySprintTitle),
            Self::EmptySprintGoal => localizer.text(Message::ErrorEmptySprintGoal),
            Self::InvalidSprintTransition { from, to } => message!(
                Message::ErrorInvalidSprintTransition,
                "from" => from,
                "to" => to,
            ),
            Self::SprintNotFound(id) => message!(Message::ErrorSprintNotFound, "id" => id),
            Self::SprintExists(id) => message!(Message::ErrorSprintExists, "id" => id),
            Self::SprintClosed(id) => message!(Message::ErrorSprintClosed, "id" => id),
            Self::InvalidSprintPeriod { start, end } => message!(
                Message::ErrorInvalidSprintPeriod,
                "start" => start,
                "end" => end,
            ),
            Self::InvalidDailyWorkHours(value) => message!(
                Message::ErrorInvalidDailyWorkHours,
                "value" => value,
            ),
            Self::InvalidDeductionFactor(value) => message!(
                Message::ErrorInvalidDeductionFactor,
                "value" => value,
            ),
            Self::InvalidSprintHolidays {
                holidays,
                calendar_days,
            } => message!(
                Message::ErrorInvalidSprintHolidays,
                "holidays" => holidays,
                "calendar_days" => calendar_days,
            ),
            Self::SprintCapacityPeriodUnset(id) => message!(
                Message::ErrorSprintCapacityPeriodUnset,
                "id" => id,
            ),
            Self::SprintCapacityUnset(id) => {
                message!(Message::ErrorSprintCapacityUnset, "id" => id)
            }
            Self::SprintPeriodUnset(id) => message!(Message::ErrorSprintPeriodUnset, "id" => id),
            Self::SprintEmpty(id) => message!(Message::ErrorSprintEmpty, "id" => id),
            Self::NotInSprint { item, sprint } => message!(
                Message::ErrorNotInSprint,
                "item" => item,
                "sprint" => sprint,
            ),
            Self::ParentCycle { child, parent } => message!(
                Message::ErrorParentCycle,
                "child" => child,
                "parent" => parent,
            ),
            Self::NotInitialized { path } => message!(
                Message::ErrorNotInitialized,
                "path" => path.display(),
            ),
            Self::Io {
                path,
                message: detail,
            } => message!(
                Message::ErrorIo,
                "path" => path.display(),
                "message" => detail,
            ),
            Self::Parse {
                path,
                message: detail,
            } => message!(
                Message::ErrorParse,
                "path" => path.display(),
                "message" => detail,
            ),
            Self::MissingFrontmatter { path } => message!(
                Message::ErrorMissingFrontmatter,
                "path" => path.display(),
            ),
            Self::UnsupportedSqliteSchema {
                path,
                found,
                supported,
            } => message!(
                Message::ErrorUnsupportedSqliteSchema,
                "path" => path.display(),
                "found" => format!("{found:?}"),
                "supported" => supported,
            ),
            Self::SelfReference(id) => message!(Message::ErrorSelfReference, "id" => id),
            Self::NotSibling { item, reference } => message!(
                Message::ErrorNotSibling,
                "item" => item,
                "reference" => reference,
            ),
            Self::Task(detail) => message!(Message::ErrorTask, "message" => detail),
            Self::Git(detail) => message!(Message::ErrorGit, "message" => detail),
            Self::EditorNotSet => localizer.text(Message::ErrorEditorNotSet),
            Self::EditorLaunch {
                editor,
                message: detail,
            } => message!(
                Message::ErrorEditorLaunch,
                "editor" => editor,
                "message" => detail,
            ),
            Self::EditorInvalid { message: detail } => message!(
                Message::ErrorEditorInvalid,
                "message" => detail,
            ),
            Self::Locked { path } => message!(Message::ErrorLocked, "path" => path.display()),
        }
    }

    /// Convert I/O errors to [`Error::Io`] with target path.
    pub(crate) fn io(path: &Path, source: &std::io::Error) -> Self {
        Error::Io {
            path: path.to_path_buf(),
            message: source.to_string(),
        }
    }

    /// Convert parser/serializer errors to [`Error::Parse`] with target path.
    pub(crate) fn parse(path: &Path, message: impl Into<String>) -> Self {
        Error::Parse {
            path: path.to_path_buf(),
            message: message.into(),
        }
    }

    /// Convert asynchronous task join failures ([`tokio::task::JoinError`], etc.) to [`Error::Task`].
    pub(crate) fn task(source: impl std::fmt::Display) -> Self {
        Error::Task(source.to_string())
    }

    /// Is this error caused by the user (bad input or a missing target)?
    ///
    /// The CLI maps user-fixable errors to exit code 1 and unexpected I/O or task failures to
    /// code 2. Add any new user-facing variant here so the classification stays in one place and
    /// no subcommand has to repeat it.
    #[must_use]
    pub fn is_user_error(&self) -> bool {
        matches!(
            self,
            Error::InvalidItemId(_)
                | Error::InvalidRank(_)
                | Error::InvalidAutomationPlan
                | Error::AutomationPlanSource { .. }
                | Error::UnknownStatus(_)
                | Error::EmptyTitle
                | Error::InvalidSearchPattern(_)
                | Error::InvalidFilterOption(_)
                | Error::NothingToUpdate
                | Error::EmptyDod
                | Error::InvalidTemplateName(_)
                | Error::TemplateNotFound { .. }
                | Error::TemplateUnreadable { .. }
                | Error::NotFound(_)
                | Error::ReferencedItem { .. }
                | Error::InvalidSprintId(_)
                | Error::InvalidSprintState(_)
                | Error::EmptySprintTitle
                | Error::EmptySprintGoal
                | Error::InvalidSprintTransition { .. }
                | Error::SprintNotFound(_)
                | Error::SprintExists(_)
                | Error::SprintClosed(_)
                | Error::InvalidSprintPeriod { .. }
                | Error::InvalidDailyWorkHours(_)
                | Error::InvalidDeductionFactor(_)
                | Error::InvalidSprintHolidays { .. }
                | Error::SprintCapacityPeriodUnset(_)
                | Error::SprintCapacityUnset(_)
                | Error::SprintPeriodUnset(_)
                | Error::SprintEmpty(_)
                | Error::NotInSprint { .. }
                | Error::ParentCycle { .. }
                | Error::SelfReference(_)
                | Error::NotSibling { .. }
                | Error::NotInitialized { .. }
                | Error::Parse { .. }
                | Error::MissingFrontmatter { .. }
                | Error::UnsupportedSqliteSchema { .. }
                | Error::EditorNotSet
                | Error::EditorLaunch { .. }
                | Error::EditorInvalid { .. }
                | Error::Locked { .. }
        )
    }
}

/// Common crate `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    /// Variants classified as user-fixable (exit code 1).
    #[test]
    fn user_facing_variants_are_user_errors() {
        let user: &[Error] = &[
            Error::InvalidItemId("x".into()),
            Error::InvalidRank("".into()),
            Error::Parse {
                path: PathBuf::from("f"),
                message: "bad".into(),
            },
            Error::MissingFrontmatter {
                path: PathBuf::from("f"),
            },
            Error::UnknownStatus("archived".into()),
            Error::EmptyTitle,
            Error::NothingToUpdate,
            Error::EmptyDod,
            Error::NotFound(ItemId::new("T", 1)),
            Error::ReferencedItem {
                item: ItemId::new("T", 1),
                references: "T-2".into(),
            },
            Error::InvalidSprintId("S 1".into()),
            Error::InvalidSprintState("archived".into()),
            Error::EmptySprintTitle,
            Error::EmptySprintGoal,
            Error::InvalidSprintTransition {
                from: SprintState::Active,
                to: SprintState::Active,
            },
            Error::SprintNotFound(SprintId::new("S-1").unwrap()),
            Error::SprintExists(SprintId::new("S-1").unwrap()),
            Error::SprintClosed(SprintId::new("S-1").unwrap()),
            Error::InvalidSprintPeriod {
                start: chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
                end: chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
            },
            Error::SprintPeriodUnset(SprintId::new("S-1").unwrap()),
            Error::SprintEmpty(SprintId::new("S-1").unwrap()),
            Error::NotInSprint {
                item: ItemId::new("T", 1),
                sprint: SprintId::new("S-1").unwrap(),
            },
            Error::ParentCycle {
                child: ItemId::new("T", 1),
                parent: ItemId::new("T", 2),
            },
            Error::SelfReference(ItemId::new("T", 1)),
            Error::NotSibling {
                item: ItemId::new("T", 1),
                reference: ItemId::new("T", 2),
            },
            Error::EditorNotSet,
            Error::EditorLaunch {
                editor: "missing-editor".into(),
                message: "not found".into(),
            },
            Error::EditorInvalid {
                message: "empty title".into(),
            },
            Error::NotInitialized {
                path: PathBuf::from(".pinto"),
            },
            Error::Locked {
                path: PathBuf::from(".pinto/.lock"),
            },
            Error::UnsupportedSqliteSchema {
                path: PathBuf::from(".pinto/board.sqlite3"),
                found: "99".into(),
                supported: 1,
            },
        ];
        for e in user {
            assert!(e.is_user_error(), "expected user error: {e:?}");
        }
    }

    /// Variants classified as internal or unexpected (exit code 2), such as I/O and task failures
    /// that cannot be fixed with user input.
    #[test]
    fn internal_variants_are_not_user_errors() {
        let internal: &[Error] = &[
            Error::Io {
                path: PathBuf::from("f"),
                message: "boom".into(),
            },
            Error::Task("panicked".into()),
            Error::Git("git not found".into()),
        ];
        for e in internal {
            assert!(!e.is_user_error(), "expected internal error: {e:?}");
        }
    }

    #[test]
    fn domain_errors_have_stable_codes_and_localized_bodies() {
        let japanese = crate::i18n::localizer_from(Some("ja_JP.UTF-8"), None);
        assert_eq!(Error::EmptyTitle.code(), "empty-title");
        assert_eq!(Error::Git("git failed".into()).code(), "git");
        assert_eq!(
            Error::EmptyTitle.localized(&japanese),
            "タイトルは空にできません。"
        );

        let external = Error::Parse {
            path: PathBuf::from("config.toml"),
            message: "expected newline".into(),
        };
        let rendered = external.localized(&japanese);
        assert!(rendered.contains("config.toml"));
        assert!(rendered.contains("expected newline"));
        assert!(rendered.contains("解析"));
    }

    #[test]
    fn english_localization_preserves_display_quoting_for_special_values() {
        let english = crate::i18n::localizer_from(Some("en_US.UTF-8"), None);
        let errors = [
            Error::InvalidItemId("bad\"id".into()),
            Error::InvalidRank("bad\nrank".into()),
            Error::UnknownStatus("two words".into()),
            Error::InvalidTemplateName("a\"b".into()),
            Error::InvalidSprintId("S \"1".into()),
            Error::InvalidSprintState("unknown state".into()),
            Error::UnsupportedSqliteSchema {
                path: PathBuf::from("board.sqlite3"),
                found: "1\"2".into(),
                supported: 1,
            },
        ];

        for error in errors {
            assert_eq!(
                error.localized(&english),
                error.to_string(),
                "special values must use the same quoting at the English boundary: {error:?}"
            );
        }
    }

    #[test]
    fn every_error_variant_resolves_from_the_catalog() {
        let english = crate::i18n::localizer_from(Some("en_US.UTF-8"), None);
        let japanese = crate::i18n::localizer_from(Some("ja_JP.UTF-8"), None);
        let template = TemplateName::new("item").unwrap();
        let sprint = SprintId::new("S-1").unwrap();
        let errors = [
            Error::InvalidItemId("bad".into()),
            Error::InvalidRank("!".into()),
            Error::InvalidAutomationPlan,
            Error::AutomationPlanSource {
                path: PathBuf::from("plan.json"),
                message: "permission denied".into(),
            },
            Error::UnknownStatus("blocked".into()),
            Error::EmptyTitle,
            Error::InvalidSearchPattern("[".into()),
            Error::InvalidFilterOption("--label".into()),
            Error::NothingToUpdate,
            Error::EmptyDod,
            Error::InvalidTemplateName("../item".into()),
            Error::TemplateNotFound {
                kind: "item",
                name: template.clone(),
                path: PathBuf::from(".pinto/templates/item/item.md"),
            },
            Error::TemplateUnreadable {
                path: PathBuf::from("item.md"),
                message: "invalid UTF-8".into(),
            },
            Error::NotFound(ItemId::new("T", 1)),
            Error::ReferencedItem {
                item: ItemId::new("T", 1),
                references: "T-2".into(),
            },
            Error::InvalidSprintId("S 1".into()),
            Error::InvalidSprintState("paused".into()),
            Error::EmptySprintTitle,
            Error::EmptySprintGoal,
            Error::InvalidSprintTransition {
                from: SprintState::Active,
                to: SprintState::Planned,
            },
            Error::SprintNotFound(sprint.clone()),
            Error::SprintExists(sprint.clone()),
            Error::SprintClosed(sprint.clone()),
            Error::InvalidSprintPeriod {
                start: chrono::NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
                end: chrono::NaiveDate::from_ymd_opt(2026, 7, 6).unwrap(),
            },
            Error::InvalidDailyWorkHours("NaN".into()),
            Error::InvalidDeductionFactor("2".into()),
            Error::InvalidSprintHolidays {
                holidays: 8,
                calendar_days: 5,
            },
            Error::SprintCapacityPeriodUnset(sprint.clone()),
            Error::SprintCapacityUnset(sprint.clone()),
            Error::SprintPeriodUnset(sprint.clone()),
            Error::SprintEmpty(sprint.clone()),
            Error::NotInSprint {
                item: ItemId::new("T", 1),
                sprint: sprint.clone(),
            },
            Error::ParentCycle {
                child: ItemId::new("T", 1),
                parent: ItemId::new("T", 2),
            },
            Error::NotInitialized {
                path: PathBuf::from(".pinto"),
            },
            Error::Io {
                path: PathBuf::from("file"),
                message: "permission denied".into(),
            },
            Error::Parse {
                path: PathBuf::from("file"),
                message: "invalid TOML".into(),
            },
            Error::MissingFrontmatter {
                path: PathBuf::from("task.md"),
            },
            Error::UnsupportedSqliteSchema {
                path: PathBuf::from("board.sqlite3"),
                found: "99".into(),
                supported: 1,
            },
            Error::SelfReference(ItemId::new("T", 1)),
            Error::NotSibling {
                item: ItemId::new("T", 1),
                reference: ItemId::new("T", 2),
            },
            Error::Task("join failed".into()),
            Error::Git("git failed".into()),
            Error::EditorNotSet,
            Error::EditorLaunch {
                editor: "missing-editor".into(),
                message: "not found".into(),
            },
            Error::EditorInvalid {
                message: "empty title".into(),
            },
            Error::Locked {
                path: PathBuf::from(".pinto/.lock"),
            },
        ];

        let mismatches: Vec<_> = errors
            .iter()
            .filter_map(|error| {
                let localized = error.localized(&english);
                let display = error.to_string();
                (localized != display)
                    .then(|| format!("{error:?}: localized={localized:?}, display={display:?}"))
            })
            .collect();
        assert!(
            mismatches.is_empty(),
            "English localization must be the library Display fallback:\n{}",
            mismatches.join("\n")
        );

        for error in errors {
            let rendered = error.localized(&japanese);
            assert!(
                !rendered.starts_with("error-"),
                "missing catalog entry: {error:?}"
            );
        }
    }
}
