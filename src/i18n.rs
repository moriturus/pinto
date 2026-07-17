//! A lightweight internationalization infrastructure for CLI/TUI.
//!
//! Locale selection follows the POSIX order `LC_ALL`, then `LANG`. English and Japanese are
//! supported; unknown locales safely fall back to English. Fluent FTL resources make it possible
//! to add languages without branching through the call sites.

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource};
use std::env;
use std::str::FromStr;
use std::sync::OnceLock;
use unic_langid::LanguageIdentifier;

const EN_US: &str = "en-US";

const EN_MESSAGES: &str = include_str!("../locales/en-US.ftl");
const JA_MESSAGES: &str = include_str!("../locales/ja-JP.ftl");

/// Display locale currently provided by pinto.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    /// Default display locale.
    English,
    /// Japanese display locale.
    Japanese,
}

/// Identifier for translatable UI messages.
///
/// Messages with values are resolved as Fluent variables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    ErrorPrefix,
    InitializedBoardAt,
    AlreadyInitialized,
    Created,
    NoBacklogItems,
    Moved,
    AcceptanceCriteriaIncomplete,
    Reordered,
    Updated,
    NoChangesTo,
    Archived,
    Deleted,
    CreatedSprint,
    UpdatedSprint,
    DeletedSprint,
    StartedSprint,
    ClosedSprint,
    AssignedToSprint,
    UnassignedFromSprint,
    NoSprints,
    DependencyAdded,
    DependencyCycleWarning,
    DependencyCycleWarningGeneric,
    DependencyRemoved,
    LinkAlreadyLinked,
    LinkAdded,
    LinkNoMatchingCommit,
    LinkRemoved,
    LinkNoNewCommits,
    LinkCommitLinked,
    LinkSummary,
    DodUnset,
    DodUpdated,
    DodCleared,
    DodNoCommonToClear,
    MigrationCompleted,
    MigrationBackendUpdated,
    MigrationAlreadyUsing,
    MigrationSqliteUnavailable,
    WipLimitExceeded,
    OrphanedItemsWarning,
    RebalanceAlreadyBalanced,
    RebalanceDryRun,
    RebalanceCompleted,
    InvalidCapacityOptions,
    FailedToAction,
    ShellParseError,
    SprintAddRequiresItemOrStatus,
    ErrorInvalidItemId,
    ErrorInvalidRank,
    ErrorInvalidAutomationPlan,
    ErrorAutomationPlanSource,
    ErrorUnknownStatus,
    ErrorEmptyTitle,
    ErrorInvalidSearchPattern,
    ErrorInvalidFilterOption,
    ErrorNothingToUpdate,
    ErrorEmptyDod,
    ErrorInvalidTemplateName,
    ErrorTemplateNotFound,
    ErrorTemplateUnreadable,
    ErrorNotFound,
    ErrorReferencedItem,
    ErrorInvalidSprintId,
    ErrorInvalidSprintState,
    ErrorEmptySprintTitle,
    ErrorEmptySprintGoal,
    ErrorInvalidSprintTransition,
    ErrorSprintNotFound,
    ErrorSprintExists,
    ErrorSprintClosed,
    ErrorInvalidSprintPeriod,
    ErrorInvalidDailyWorkHours,
    ErrorInvalidDeductionFactor,
    ErrorInvalidSprintHolidays,
    ErrorSprintCapacityPeriodUnset,
    ErrorSprintCapacityUnset,
    ErrorSprintPeriodUnset,
    ErrorSprintEmpty,
    ErrorNotInSprint,
    ErrorParentCycle,
    ErrorNotInitialized,
    ErrorIo,
    ErrorParse,
    ErrorMissingFrontmatter,
    ErrorUnsupportedSqliteSchema,
    ErrorSelfReference,
    ErrorNotSibling,
    ErrorTask,
    ErrorGit,
    ErrorEditorNotSet,
    ErrorEditorLaunch,
    ErrorEditorInvalid,
    ErrorLocked,
    AlreadyInInteractiveShell,
    KanbanDetailsTitle,
    KanbanPopupHints,
    KanbanHelpTitle,
    KanbanHelpEntries,
    KanbanNoChanges,
    KanbanEditorFailed,
    KanbanEditFailed,
    KanbanWipExceeded,
    KanbanNoEditor,
    KanbanKeyHints,
    KanbanHelpHint,
    KanbanHelpClearFilter,
    KanbanAddTitlePrompt,
    KanbanAddBodyPrompt,
    KanbanAddParentPrompt,
    KanbanAddDependenciesPrompt,
    KanbanDependencyAddPrompt,
    KanbanDependencyRemovePrompt,
    KanbanEmptyTitle,
    KanbanEmptyDependency,
    KanbanDependencyAdded,
    KanbanDependencyRemoved,
    KanbanDependencyCycleWarning,
    KanbanParentPrompt,
    KanbanParentSet,
    KanbanParentCleared,
    KanbanActiveFilter,
    KanbanActiveRegexFilter,
    KanbanDependencyLegend,
    KanbanColumnRange,
    KanbanEmptyColumns,
    KanbanQuitBody,
    KanbanQuitPrompt,
    KanbanNoBody,
    KanbanNoSelection,
    KanbanTerminalInitFailed,
    EditGuidance,
    AutomationCommandValid,
    AutomationCommandInvalid,
    AutomationCommandFailed,
    AutomationCommandSkipped,
    AutomationDryRunCompleted,
    AutomationCompleted,
    AutomationPartialFailure,
    AutomationInvalidCommandArguments,
    AutomationNotExecutedAfterFailure,
    AutomationNotValidatedAfterFailure,
    AutomationDryRunGitInitFailed,
    AutomationDryRunWorkspaceUnavailable,
    AutomationCommandExited,
}

impl Message {
    const fn id(self) -> &'static str {
        match self {
            Self::ErrorPrefix => "error-prefix",
            Self::InitializedBoardAt => "initialized-board-at",
            Self::AlreadyInitialized => "already-initialized",
            Self::Created => "created",
            Self::NoBacklogItems => "no-backlog-items",
            Self::Moved => "moved",
            Self::AcceptanceCriteriaIncomplete => "acceptance-criteria-incomplete",
            Self::Reordered => "reordered",
            Self::Updated => "updated",
            Self::NoChangesTo => "no-changes-to",
            Self::Archived => "archived",
            Self::Deleted => "deleted",
            Self::CreatedSprint => "created-sprint",
            Self::UpdatedSprint => "updated-sprint",
            Self::DeletedSprint => "deleted-sprint",
            Self::StartedSprint => "started-sprint",
            Self::ClosedSprint => "closed-sprint",
            Self::AssignedToSprint => "assigned-to-sprint",
            Self::UnassignedFromSprint => "unassigned-from-sprint",
            Self::NoSprints => "no-sprints",
            Self::DependencyAdded => "dependency-added",
            Self::DependencyCycleWarning => "dependency-cycle-warning",
            Self::DependencyCycleWarningGeneric => "dependency-cycle-warning-generic",
            Self::DependencyRemoved => "dependency-removed",
            Self::LinkAlreadyLinked => "link-already-linked",
            Self::LinkAdded => "link-added",
            Self::LinkNoMatchingCommit => "link-no-matching-commit",
            Self::LinkRemoved => "link-removed",
            Self::LinkNoNewCommits => "link-no-new-commits",
            Self::LinkCommitLinked => "link-commit-linked",
            Self::LinkSummary => "link-summary",
            Self::DodUnset => "dod-unset",
            Self::DodUpdated => "dod-updated",
            Self::DodCleared => "dod-cleared",
            Self::DodNoCommonToClear => "dod-no-common-to-clear",
            Self::MigrationCompleted => "migration-completed",
            Self::MigrationBackendUpdated => "migration-backend-updated",
            Self::MigrationAlreadyUsing => "migration-already-using",
            Self::MigrationSqliteUnavailable => "migration-sqlite-unavailable",
            Self::WipLimitExceeded => "wip-limit-exceeded",
            Self::OrphanedItemsWarning => "orphaned-items-warning",
            Self::RebalanceAlreadyBalanced => "rebalance-already-balanced",
            Self::RebalanceDryRun => "rebalance-dry-run",
            Self::RebalanceCompleted => "rebalance-completed",
            Self::InvalidCapacityOptions => "invalid-capacity-options",
            Self::FailedToAction => "failed-to-action",
            Self::ShellParseError => "shell-parse-error",
            Self::SprintAddRequiresItemOrStatus => "sprint-add-requires-item-or-status",
            Self::ErrorInvalidItemId => "error-invalid-item-id",
            Self::ErrorInvalidRank => "error-invalid-rank",
            Self::ErrorInvalidAutomationPlan => "error-invalid-automation-plan",
            Self::ErrorAutomationPlanSource => "error-automation-plan-source",
            Self::ErrorUnknownStatus => "error-unknown-status",
            Self::ErrorEmptyTitle => "error-empty-title",
            Self::ErrorInvalidSearchPattern => "error-invalid-search-pattern",
            Self::ErrorInvalidFilterOption => "error-invalid-filter-option",
            Self::ErrorNothingToUpdate => "error-nothing-to-update",
            Self::ErrorEmptyDod => "error-empty-dod",
            Self::ErrorInvalidTemplateName => "error-invalid-template-name",
            Self::ErrorTemplateNotFound => "error-template-not-found",
            Self::ErrorTemplateUnreadable => "error-template-unreadable",
            Self::ErrorNotFound => "error-not-found",
            Self::ErrorReferencedItem => "error-referenced-item",
            Self::ErrorInvalidSprintId => "error-invalid-sprint-id",
            Self::ErrorInvalidSprintState => "error-invalid-sprint-state",
            Self::ErrorEmptySprintTitle => "error-empty-sprint-title",
            Self::ErrorEmptySprintGoal => "error-empty-sprint-goal",
            Self::ErrorInvalidSprintTransition => "error-invalid-sprint-transition",
            Self::ErrorSprintNotFound => "error-sprint-not-found",
            Self::ErrorSprintExists => "error-sprint-exists",
            Self::ErrorSprintClosed => "error-sprint-closed",
            Self::ErrorInvalidSprintPeriod => "error-invalid-sprint-period",
            Self::ErrorInvalidDailyWorkHours => "error-invalid-daily-work-hours",
            Self::ErrorInvalidDeductionFactor => "error-invalid-deduction-factor",
            Self::ErrorInvalidSprintHolidays => "error-invalid-sprint-holidays",
            Self::ErrorSprintCapacityPeriodUnset => "error-sprint-capacity-period-unset",
            Self::ErrorSprintCapacityUnset => "error-sprint-capacity-unset",
            Self::ErrorSprintPeriodUnset => "error-sprint-period-unset",
            Self::ErrorSprintEmpty => "error-sprint-empty",
            Self::ErrorNotInSprint => "error-not-in-sprint",
            Self::ErrorParentCycle => "error-parent-cycle",
            Self::ErrorNotInitialized => "error-not-initialized",
            Self::ErrorIo => "error-io",
            Self::ErrorParse => "error-parse",
            Self::ErrorMissingFrontmatter => "error-missing-frontmatter",
            Self::ErrorUnsupportedSqliteSchema => "error-unsupported-sqlite-schema",
            Self::ErrorSelfReference => "error-self-reference",
            Self::ErrorNotSibling => "error-not-sibling",
            Self::ErrorTask => "error-task",
            Self::ErrorGit => "error-git",
            Self::ErrorEditorNotSet => "error-editor-not-set",
            Self::ErrorEditorLaunch => "error-editor-launch",
            Self::ErrorEditorInvalid => "error-editor-invalid",
            Self::ErrorLocked => "error-locked",
            Self::AlreadyInInteractiveShell => "already-in-interactive-shell",
            Self::KanbanDetailsTitle => "kanban-details-title",
            Self::KanbanPopupHints => "kanban-popup-hints",
            Self::KanbanHelpTitle => "kanban-help-title",
            Self::KanbanHelpEntries => "kanban-help-entries",
            Self::KanbanNoChanges => "kanban-no-changes",
            Self::KanbanEditorFailed => "kanban-editor-failed",
            Self::KanbanEditFailed => "kanban-edit-failed",
            Self::KanbanWipExceeded => "kanban-wip-exceeded",
            Self::KanbanNoEditor => "kanban-no-editor",
            Self::KanbanKeyHints => "kanban-key-hints",
            Self::KanbanHelpHint => "kanban-help-hint",
            Self::KanbanHelpClearFilter => "kanban-help-clear-filter",
            Self::KanbanAddTitlePrompt => "kanban-add-title-prompt",
            Self::KanbanAddBodyPrompt => "kanban-add-body-prompt",
            Self::KanbanAddParentPrompt => "kanban-add-parent-prompt",
            Self::KanbanAddDependenciesPrompt => "kanban-add-dependencies-prompt",
            Self::KanbanDependencyAddPrompt => "kanban-dependency-add-prompt",
            Self::KanbanDependencyRemovePrompt => "kanban-dependency-remove-prompt",
            Self::KanbanEmptyTitle => "kanban-empty-title",
            Self::KanbanEmptyDependency => "kanban-empty-dependency",
            Self::KanbanDependencyAdded => "kanban-dependency-added",
            Self::KanbanDependencyRemoved => "kanban-dependency-removed",
            Self::KanbanDependencyCycleWarning => "kanban-dependency-cycle-warning",
            Self::KanbanParentPrompt => "kanban-parent-prompt",
            Self::KanbanParentSet => "kanban-parent-set",
            Self::KanbanParentCleared => "kanban-parent-cleared",
            Self::KanbanActiveFilter => "kanban-active-filter",
            Self::KanbanActiveRegexFilter => "kanban-active-regex-filter",
            Self::KanbanDependencyLegend => "kanban-dependency-legend",
            Self::KanbanColumnRange => "kanban-column-range",
            Self::KanbanEmptyColumns => "kanban-empty-columns",
            Self::KanbanQuitBody => "kanban-quit-body",
            Self::KanbanQuitPrompt => "kanban-quit-prompt",
            Self::KanbanNoBody => "kanban-no-body",
            Self::KanbanNoSelection => "kanban-no-selection",
            Self::KanbanTerminalInitFailed => "kanban-terminal-init-failed",
            Self::EditGuidance => "edit-guidance",
            Self::AutomationCommandValid => "automation-command-valid",
            Self::AutomationCommandInvalid => "automation-command-invalid",
            Self::AutomationCommandFailed => "automation-command-failed",
            Self::AutomationCommandSkipped => "automation-command-skipped",
            Self::AutomationDryRunCompleted => "automation-dry-run-completed",
            Self::AutomationCompleted => "automation-completed",
            Self::AutomationPartialFailure => "automation-partial-failure",
            Self::AutomationInvalidCommandArguments => "automation-invalid-command-arguments",
            Self::AutomationNotExecutedAfterFailure => "automation-not-executed-after-failure",
            Self::AutomationNotValidatedAfterFailure => "automation-not-validated-after-failure",
            Self::AutomationDryRunGitInitFailed => "automation-dry-run-git-init-failed",
            Self::AutomationDryRunWorkspaceUnavailable => {
                "automation-dry-run-workspace-unavailable"
            }
            Self::AutomationCommandExited => "automation-command-exited",
        }
    }
}

/// Message table used in the execution process.
pub struct Localizer {
    locale: Locale,
    language: LanguageIdentifier,
    bundle: FluentBundle<FluentResource>,
}

impl Localizer {
    /// Determine the locale from the current process environment.
    #[must_use]
    pub fn from_environment() -> Self {
        localizer_from(
            env::var("LC_ALL").ok().as_deref(),
            env::var("LANG").ok().as_deref(),
        )
    }

    /// Returns the actual selected display locale.
    pub const fn locale(&self) -> Locale {
        self.locale
    }

    /// Returns Fluent's locale identifier chosen from an environment variable.
    pub fn language_identifier(&self) -> &LanguageIdentifier {
        &self.language
    }

    /// Resolve Fluent messages without arguments.
    pub fn text(&self, message: Message) -> String {
        self.format(message, [])
    }

    /// Resolve any Fluent message ID.
    ///
    /// Resolve dynamic messages that are difficult to enumerate, such as clap's help, from the
    /// resource bundle. Return `None` for an unknown ID or invalid pattern so callers can keep
    /// their default wording.
    pub fn lookup(&self, id: &str) -> Option<String> {
        let message = self.bundle.get_message(id)?;
        let pattern = message.value()?;
        let mut errors = Vec::new();
        let value = self.bundle.format_pattern(pattern, None, &mut errors);
        errors.is_empty().then(|| value.into_owned())
    }

    /// Resolve messages by giving them Fluent variables.
    ///
    /// If a built-in FTL resource is missing or cannot be formatted, return the message identifier
    /// so a resource problem cannot make the CLI or TUI undisplayable.
    pub fn format<'a>(
        &self,
        message: Message,
        variables: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> String {
        let id = message.id();
        let Some(message) = self.bundle.get_message(id) else {
            return id.to_string();
        };
        let Some(pattern) = message.value() else {
            return id.to_string();
        };
        let mut args = FluentArgs::new();
        for (name, value) in variables {
            args.set(name, value);
        }
        let mut errors = Vec::new();
        let value = self
            .bundle
            .format_pattern(pattern, Some(&args), &mut errors);
        if errors.is_empty() {
            value.into_owned()
        } else {
            id.to_string()
        }
    }
}

/// Prefer `LC_ALL` to `LANG` to choose a non-empty locale name.
///
/// The return value is the candidate before normalization; [`localizer_from`] determines whether
/// it is compatible. Keeping those steps separate makes POSIX precedence and fallback rules easy
/// to test independently.
#[must_use]
pub fn locale_name_from<'a>(lc_all: Option<&'a str>, lang: Option<&'a str>) -> Option<&'a str> {
    lc_all
        .filter(|value| !value.is_empty())
        .or_else(|| lang.filter(|value| !value.is_empty()))
}

/// Create a localizer from the specified environment values.
#[must_use]
pub fn localizer_from(lc_all: Option<&str>, lang: Option<&str>) -> Localizer {
    let (locale, language, messages) = localized_resource(lc_all, lang);
    let resource = match FluentResource::try_new(messages.to_string()) {
        Ok(resource) | Err((resource, _)) => resource,
    };
    let mut bundle = FluentBundle::new_concurrent(vec![language.clone()]);
    bundle.set_use_isolating(false);
    let _ = bundle.add_resource(resource);
    Localizer {
        locale,
        language,
        bundle,
    }
}

/// Normalize `LC_ALL` / `LANG` to BCP 47 language identifiers.
///
/// Determine the corresponding Fluent resource and locale. Preserve the regional component when
/// possible (for example, `en-GB` or `ja-JP`); fall back to the `en-US` resource when the value
/// is missing, unsupported, or syntactically invalid.
fn localized_resource(
    lc_all: Option<&str>,
    lang: Option<&str>,
) -> (Locale, LanguageIdentifier, &'static str) {
    let candidate = locale_name_from(lc_all, lang)
        .and_then(|name| name.split('.').next())
        .map(|name| name.replace('_', "-"))
        .and_then(|name| LanguageIdentifier::from_str(&name).ok());

    match candidate {
        Some(language) if language.language.as_str().eq_ignore_ascii_case("en") => {
            (Locale::English, language, EN_MESSAGES)
        }
        Some(language) if language.language.as_str().eq_ignore_ascii_case("ja") => {
            (Locale::Japanese, language, JA_MESSAGES)
        }
        Some(_) | None => (Locale::English, english_fallback(), EN_MESSAGES),
    }
}

fn english_fallback() -> LanguageIdentifier {
    // `EN_US` is a valid BCP 47 tag fixed at compile time. If the library ever rejects it, keep
    // the CLI running with its default language rather than propagating an impossible error.
    LanguageIdentifier::from_str(EN_US).unwrap_or_default()
}

/// Returns the process-local localizer, initialized from the environment on first use.
///
/// Locale environment changes after the first call do not affect this value; use
/// [`localizer_from`] when a caller needs an explicitly selected locale, such as a test.
#[must_use]
pub fn current() -> &'static Localizer {
    static CURRENT_LOCALIZER: OnceLock<Localizer> = OnceLock::new();
    CURRENT_LOCALIZER.get_or_init(Localizer::from_environment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lc_all_is_selected_before_lang() {
        assert_eq!(locale_name_from(Some("C"), Some("ja_JP.UTF-8")), Some("C"));
    }

    #[test]
    fn empty_lc_all_defers_to_lang() {
        assert_eq!(
            locale_name_from(Some(""), Some("en_US.UTF-8")),
            Some("en_US.UTF-8")
        );
    }

    #[test]
    fn missing_locale_values_use_the_english_fallback() {
        assert_eq!(locale_name_from(None, None), None);
        assert_eq!(locale_name_from(Some(""), Some("")), None);

        for (lc_all, lang) in [(None, None), (Some(""), Some("invalid_locale"))] {
            let localizer = localizer_from(lc_all, lang);
            assert_eq!(localizer.locale(), Locale::English);
            assert_eq!(localizer.language_identifier().to_string(), "en-US");
        }
    }

    #[test]
    fn unsupported_locale_uses_english_messages() {
        let localizer = localizer_from(Some("fr_FR.UTF-8"), None);
        assert_eq!(localizer.locale(), Locale::English);
        assert_eq!(localizer.text(Message::NoBacklogItems), "No backlog items.");
    }

    #[test]
    fn current_reuses_one_localizer_for_the_process_lifetime() {
        assert!(std::ptr::eq(current(), current()));
    }

    #[test]
    fn edit_guidance_is_localized_and_kept_as_toml_comments() {
        // The editor guidance must follow the selected locale (it used to be hard-coded
        // Japanese) and stay as `#` comment lines so TOML round-trips it untouched.
        let en = localizer_from(Some("en_US.UTF-8"), None).text(Message::EditGuidance);
        let ja = localizer_from(Some("ja_JP.UTF-8"), None).text(Message::EditGuidance);

        for guide in [&en, &ja] {
            // Rendered from FTL, not the fallback identifier, and every line is a comment.
            assert_ne!(guide, "edit-guidance", "must resolve from FTL");
            assert!(guide.starts_with("# pinto:"), "opens with a marker comment");
            assert!(
                guide.lines().all(|line| line.starts_with('#')),
                "all lines are TOML comments: {guide:?}"
            );
        }
        assert!(en.contains("saving applies"), "English guidance is English");
        assert!(ja.contains("保存すると"), "Japanese guidance is Japanese");
        assert_ne!(en, ja, "locales produce distinct guidance");
    }

    #[test]
    fn every_message_id_has_a_safe_fallback_and_lookup_handles_unknown_ids() {
        let localizer = localizer_from(Some("en-US"), None);
        let messages = [
            Message::ErrorPrefix,
            Message::InitializedBoardAt,
            Message::AlreadyInitialized,
            Message::Created,
            Message::NoBacklogItems,
            Message::Moved,
            Message::AcceptanceCriteriaIncomplete,
            Message::Reordered,
            Message::Updated,
            Message::NoChangesTo,
            Message::Archived,
            Message::Deleted,
            Message::CreatedSprint,
            Message::UpdatedSprint,
            Message::DeletedSprint,
            Message::StartedSprint,
            Message::ClosedSprint,
            Message::AssignedToSprint,
            Message::UnassignedFromSprint,
            Message::NoSprints,
            Message::DependencyAdded,
            Message::DependencyCycleWarning,
            Message::DependencyCycleWarningGeneric,
            Message::DependencyRemoved,
            Message::LinkAlreadyLinked,
            Message::LinkAdded,
            Message::LinkNoMatchingCommit,
            Message::LinkRemoved,
            Message::LinkNoNewCommits,
            Message::LinkCommitLinked,
            Message::LinkSummary,
            Message::DodUnset,
            Message::DodUpdated,
            Message::DodCleared,
            Message::DodNoCommonToClear,
            Message::MigrationCompleted,
            Message::MigrationBackendUpdated,
            Message::MigrationAlreadyUsing,
            Message::MigrationSqliteUnavailable,
            Message::WipLimitExceeded,
            Message::OrphanedItemsWarning,
            Message::RebalanceAlreadyBalanced,
            Message::RebalanceDryRun,
            Message::RebalanceCompleted,
            Message::InvalidCapacityOptions,
            Message::FailedToAction,
            Message::ShellParseError,
            Message::SprintAddRequiresItemOrStatus,
            Message::ErrorInvalidItemId,
            Message::ErrorInvalidRank,
            Message::ErrorInvalidAutomationPlan,
            Message::ErrorAutomationPlanSource,
            Message::ErrorUnknownStatus,
            Message::ErrorEmptyTitle,
            Message::ErrorInvalidSearchPattern,
            Message::ErrorInvalidFilterOption,
            Message::ErrorNothingToUpdate,
            Message::ErrorEmptyDod,
            Message::ErrorInvalidTemplateName,
            Message::ErrorTemplateNotFound,
            Message::ErrorTemplateUnreadable,
            Message::ErrorNotFound,
            Message::ErrorReferencedItem,
            Message::ErrorInvalidSprintId,
            Message::ErrorInvalidSprintState,
            Message::ErrorEmptySprintTitle,
            Message::ErrorEmptySprintGoal,
            Message::ErrorInvalidSprintTransition,
            Message::ErrorSprintNotFound,
            Message::ErrorSprintExists,
            Message::ErrorSprintClosed,
            Message::ErrorInvalidSprintPeriod,
            Message::ErrorInvalidDailyWorkHours,
            Message::ErrorInvalidDeductionFactor,
            Message::ErrorInvalidSprintHolidays,
            Message::ErrorSprintCapacityPeriodUnset,
            Message::ErrorSprintCapacityUnset,
            Message::ErrorSprintPeriodUnset,
            Message::ErrorSprintEmpty,
            Message::ErrorNotInSprint,
            Message::ErrorParentCycle,
            Message::ErrorNotInitialized,
            Message::ErrorIo,
            Message::ErrorParse,
            Message::ErrorMissingFrontmatter,
            Message::ErrorUnsupportedSqliteSchema,
            Message::ErrorSelfReference,
            Message::ErrorNotSibling,
            Message::ErrorTask,
            Message::ErrorGit,
            Message::ErrorEditorNotSet,
            Message::ErrorEditorLaunch,
            Message::ErrorEditorInvalid,
            Message::ErrorLocked,
            Message::AlreadyInInteractiveShell,
            Message::KanbanDetailsTitle,
            Message::KanbanPopupHints,
            Message::KanbanHelpTitle,
            Message::KanbanHelpEntries,
            Message::KanbanNoChanges,
            Message::KanbanEditorFailed,
            Message::KanbanEditFailed,
            Message::KanbanWipExceeded,
            Message::KanbanNoEditor,
            Message::KanbanKeyHints,
            Message::KanbanHelpHint,
            Message::KanbanHelpClearFilter,
            Message::KanbanAddTitlePrompt,
            Message::KanbanAddBodyPrompt,
            Message::KanbanAddParentPrompt,
            Message::KanbanAddDependenciesPrompt,
            Message::KanbanDependencyAddPrompt,
            Message::KanbanDependencyRemovePrompt,
            Message::KanbanEmptyTitle,
            Message::KanbanEmptyDependency,
            Message::KanbanDependencyAdded,
            Message::KanbanDependencyRemoved,
            Message::KanbanDependencyCycleWarning,
            Message::KanbanParentPrompt,
            Message::KanbanParentSet,
            Message::KanbanParentCleared,
            Message::KanbanActiveFilter,
            Message::KanbanActiveRegexFilter,
            Message::KanbanDependencyLegend,
            Message::KanbanColumnRange,
            Message::KanbanEmptyColumns,
            Message::KanbanQuitBody,
            Message::KanbanQuitPrompt,
            Message::KanbanNoBody,
            Message::KanbanNoSelection,
            Message::KanbanTerminalInitFailed,
            Message::EditGuidance,
            Message::AutomationCommandValid,
            Message::AutomationCommandInvalid,
            Message::AutomationCommandFailed,
            Message::AutomationCommandSkipped,
            Message::AutomationDryRunCompleted,
            Message::AutomationCompleted,
            Message::AutomationPartialFailure,
            Message::AutomationInvalidCommandArguments,
            Message::AutomationNotExecutedAfterFailure,
            Message::AutomationNotValidatedAfterFailure,
            Message::AutomationDryRunGitInitFailed,
            Message::AutomationDryRunWorkspaceUnavailable,
            Message::AutomationCommandExited,
        ];

        for message in messages {
            assert!(!localizer.text(message).is_empty());
        }
        assert!(
            !localizer
                .format(Message::Moved, [("id", "T-1"), ("status", "done")])
                .is_empty()
        );
        assert!(localizer.lookup("missing-message").is_none());
    }
}
