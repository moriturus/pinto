//! Pure key-to-action decisions used by the Kanban event loop.

use super::super::keymap::KeyMap;
use super::{BoardView, ExitMode};
use pinto::kanban_keys::KeyAction;
use ratatui::crossterm::event::{self, KeyCode, KeyModifiers};

/// What a key press means for leaving the view when no popup is open.
///
/// Both leave keys carry the [`ExitMode`] they resolve to, so the confirmation flow can honour
/// the original intent (`q` → [`ExitMode::Quit`], `Q` → [`ExitMode::Shell`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuitIntent {
    /// Not a quit-related key.
    None,
    /// `confirm_quit` is enabled — show the confirmation popup first.
    Confirm(ExitMode),
    /// `confirm_quit` is disabled — leave immediately.
    Leave(ExitMode),
}

/// Decide what pressing `key` means for leaving the view (no popup open).
pub(super) fn quit_intent(keymap: &KeyMap, key: event::KeyEvent, confirm_quit: bool) -> QuitIntent {
    let mode = if keymap.matches(KeyAction::Shell, key) {
        ExitMode::Shell
    } else if keymap.matches(KeyAction::Quit, key) {
        ExitMode::Quit
    } else {
        return QuitIntent::None;
    };
    if confirm_quit {
        QuitIntent::Confirm(mode)
    } else {
        QuitIntent::Leave(mode)
    }
}

/// What a key press means while the details popup is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PopupAction {
    /// Not a popup key.
    None,
    /// Close the popup (`Esc` / `q`).
    Close,
    /// Scroll the body up one line (`Up` / `k`).
    ScrollUp,
    /// Scroll the body down one line (`Down` / `j`).
    ScrollDown,
    /// Select the row above (`K`).
    SelectUp,
    /// Select the row below (`J`).
    SelectDown,
    /// Select the column to the left (`H`).
    SelectLeft,
    /// Select the column to the right (`L`).
    SelectRight,
    /// Edit the shown item with `$EDITOR`, keeping the popup open (`e`).
    Edit,
}

/// Decide what pressing `key` means while the details popup is open.
pub(super) fn popup_action(keymap: &KeyMap, key: event::KeyEvent) -> PopupAction {
    if keymap.matches(KeyAction::PopupClose, key) || keymap.matches(KeyAction::Details, key) {
        PopupAction::Close
    } else if keymap.matches(KeyAction::PopupSelectUp, key) {
        PopupAction::SelectUp
    } else if keymap.matches(KeyAction::PopupSelectDown, key) {
        PopupAction::SelectDown
    } else if keymap.matches(KeyAction::PopupSelectLeft, key) {
        PopupAction::SelectLeft
    } else if keymap.matches(KeyAction::PopupSelectRight, key) {
        PopupAction::SelectRight
    } else if keymap.matches(KeyAction::PopupScrollUp, key) {
        PopupAction::ScrollUp
    } else if keymap.matches(KeyAction::PopupScrollDown, key) {
        PopupAction::ScrollDown
    } else if keymap.matches(KeyAction::Edit, key) {
        PopupAction::Edit
    } else {
        PopupAction::None
    }
}

/// What a key press means while the help window is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HelpKeyAction {
    /// Close the help overlay and consume the toggle key.
    Close,
    /// Scroll the help overlay up, then continue handling the key underneath.
    ScrollUp,
    /// Scroll the help overlay down, then continue handling the key underneath.
    ScrollDown,
    /// No help-specific handling; let the underlying mode process the key.
    PassThrough,
}

/// Decide whether the help overlay has work to do for `key` without making it modal.
pub(super) fn help_key_action(keymap: &KeyMap, key: event::KeyEvent) -> HelpKeyAction {
    if keymap.matches(KeyAction::Help, key) {
        HelpKeyAction::Close
    } else if keymap.matches(KeyAction::PopupScrollUp, key) {
        HelpKeyAction::ScrollUp
    } else if keymap.matches(KeyAction::PopupScrollDown, key) {
        HelpKeyAction::ScrollDown
    } else {
        HelpKeyAction::PassThrough
    }
}

/// Return whether `key` is accepted by the mode underneath the help overlay.
pub(super) fn should_close_help_after_key(
    view: &BoardView,
    keymap: &KeyMap,
    key: event::KeyEvent,
) -> bool {
    if view.is_popup_open() {
        return popup_action(keymap, key) != PopupAction::None;
    }
    if view.is_input_active() {
        let selecting_target = view.is_relation_input()
            && view.input_buffer().is_empty()
            && [
                KeyAction::SelectLeft,
                KeyAction::SelectRight,
                KeyAction::SelectUp,
                KeyAction::SelectDown,
            ]
            .into_iter()
            .any(|action| keymap.matches(action, key));
        return selecting_target || text_entry_key_is_accepted(key);
    }
    if view.is_searching() {
        return text_entry_key_is_accepted(key);
    }

    if view.search_filter().is_some() && keymap.matches(KeyAction::ClearFilter, key) {
        return true;
    }
    [
        KeyAction::Shell,
        KeyAction::Quit,
        KeyAction::SelectLeft,
        KeyAction::SelectRight,
        KeyAction::SelectUp,
        KeyAction::SelectDown,
        KeyAction::MoveLeft,
        KeyAction::MoveRight,
        KeyAction::ReorderUp,
        KeyAction::ReorderDown,
        KeyAction::ToggleExpand,
        KeyAction::Add,
        KeyAction::DependencyAdd,
        KeyAction::DependencyRemove,
        KeyAction::Parent,
        KeyAction::Edit,
        KeyAction::Reload,
        KeyAction::Maximize,
        KeyAction::Search,
        KeyAction::RegexSearch,
        KeyAction::Details,
    ]
    .into_iter()
    .any(|action| keymap.matches(action, key))
}

/// Whether a key is accepted by an active text-entry prompt.
pub(super) fn text_entry_key_is_accepted(key: event::KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => true,
        KeyCode::Char(_) if !key.modifiers.contains(KeyModifiers::CONTROL) => true,
        _ => false,
    }
}
