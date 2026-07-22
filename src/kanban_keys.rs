//! Configurable Kanban key names and platform-neutral key matching.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Actions that can be assigned one or more keys in the user `[tui.key_bindings]` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    /// Leave Kanban mode.
    Quit,
    /// Leave Kanban mode and hand control to the shell.
    Shell,
    /// Select the column to the left.
    SelectLeft,
    /// Select the column to the right.
    SelectRight,
    /// Select the row above.
    SelectUp,
    /// Select the row below.
    SelectDown,
    /// Move the selected item to the column on the left.
    MoveLeft,
    /// Move the selected item to the column on the right.
    MoveRight,
    /// Reorder the selected item upwards.
    ReorderUp,
    /// Reorder the selected item downwards.
    ReorderDown,
    /// Expand or collapse the selected parent item.
    ToggleExpand,
    /// Open the form for adding a new PBI.
    Add,
    /// Open the form for adding a dependency to the selected PBI.
    DependencyAdd,
    /// Open the form for removing a dependency from the selected PBI.
    DependencyRemove,
    /// Open the form for setting or clearing the selected PBI's parent.
    Parent,
    /// Edit the selected item.
    Edit,
    /// Reload the board.
    Reload,
    /// Toggle maximized-column mode.
    Maximize,
    /// Open substring search.
    Search,
    /// Open regular-expression search.
    RegexSearch,
    /// Open or close the selected item's details.
    Details,
    /// Open or close the Kanban help window.
    Help,
    /// Clear an active search filter.
    ClearFilter,
    /// Confirm leaving from the quit dialog.
    ConfirmQuit,
    /// Cancel leaving from the quit dialog.
    CancelQuit,
    /// Close the details popup.
    PopupClose,
    /// Scroll the details popup upwards.
    PopupScrollUp,
    /// Scroll the details popup downwards.
    PopupScrollDown,
    /// Select the row above while the details popup is open.
    PopupSelectUp,
    /// Select the row below while the details popup is open.
    PopupSelectDown,
    /// Select the column to the left while the details popup is open.
    PopupSelectLeft,
    /// Select the column to the right while the details popup is open.
    PopupSelectRight,
}

impl KeyAction {
    /// Every supported action, in the order used by the configuration documentation.
    pub const ALL: &'static [Self] = &[
        Self::Quit,
        Self::Shell,
        Self::SelectLeft,
        Self::SelectRight,
        Self::SelectUp,
        Self::SelectDown,
        Self::MoveLeft,
        Self::MoveRight,
        Self::ReorderUp,
        Self::ReorderDown,
        Self::ToggleExpand,
        Self::Add,
        Self::DependencyAdd,
        Self::DependencyRemove,
        Self::Parent,
        Self::Edit,
        Self::Reload,
        Self::Maximize,
        Self::Search,
        Self::RegexSearch,
        Self::Details,
        Self::Help,
        Self::ClearFilter,
        Self::ConfirmQuit,
        Self::CancelQuit,
        Self::PopupClose,
        Self::PopupScrollUp,
        Self::PopupScrollDown,
        Self::PopupSelectUp,
        Self::PopupSelectDown,
        Self::PopupSelectLeft,
        Self::PopupSelectRight,
    ];

    /// The TOML key used for this action.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Shell => "shell",
            Self::SelectLeft => "select_left",
            Self::SelectRight => "select_right",
            Self::SelectUp => "select_up",
            Self::SelectDown => "select_down",
            Self::MoveLeft => "move_left",
            Self::MoveRight => "move_right",
            Self::ReorderUp => "reorder_up",
            Self::ReorderDown => "reorder_down",
            Self::ToggleExpand => "toggle_expand",
            Self::Add => "add",
            Self::DependencyAdd => "dependency_add",
            Self::DependencyRemove => "dependency_remove",
            Self::Parent => "parent",
            Self::Edit => "edit",
            Self::Reload => "reload",
            Self::Maximize => "maximize",
            Self::Search => "search",
            Self::RegexSearch => "regex_search",
            Self::Details => "details",
            Self::Help => "help",
            Self::ClearFilter => "clear_filter",
            Self::ConfirmQuit => "confirm_quit",
            Self::CancelQuit => "cancel_quit",
            Self::PopupClose => "popup_close",
            Self::PopupScrollUp => "popup_scroll_up",
            Self::PopupScrollDown => "popup_scroll_down",
            Self::PopupSelectUp => "popup_select_up",
            Self::PopupSelectDown => "popup_select_down",
            Self::PopupSelectLeft => "popup_select_left",
            Self::PopupSelectRight => "popup_select_right",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|action| action.name() == name)
    }
}

/// Platform-neutral key codes used by the configuration parser and TUI adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A printable character.
    Char(char),
    /// Enter/Return.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Function key.
    F(u8),
}

/// Modifier flags independent of the terminal backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// Shift modifier for named/non-printable keys.
    pub const SHIFT: Self = Self(1 << 0);
    /// Control modifier.
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt/Option modifier.
    pub const ALT: Self = Self(1 << 2);
    /// Super/Command/Windows modifier.
    pub const SUPER: Self = Self(1 << 3);
    /// Hyper modifier.
    pub const HYPER: Self = Self(1 << 4);
    /// Meta modifier.
    pub const META: Self = Self(1 << 5);

    /// Return whether all bits in `other` are present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    const fn insert(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.insert(rhs)
    }
}

impl std::ops::BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.insert(rhs);
    }
}

/// A parsed key stroke.
#[derive(Debug, Clone)]
pub struct KeyStroke {
    code: KeyCode,
    modifiers: Modifiers,
    /// The spelling used for the on-screen guide.
    display: String,
}

impl PartialEq for KeyStroke {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.modifiers == other.modifiers
    }
}

impl Eq for KeyStroke {}

impl KeyStroke {
    /// Parse a key expression such as `q`, `Esc`, or `Ctrl+a`.
    pub fn parse(input: &str) -> Result<Self, KeySpecError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(KeySpecError::new(input, "the key name must not be empty"));
        }

        let parts: Vec<&str> = input.split('+').collect();
        if parts.iter().any(|part| part.trim().is_empty()) {
            return Err(KeySpecError::new(
                input,
                "use `Plus` for the plus key and separate modifiers with `+`",
            ));
        }

        let key_name = parts.last().copied().unwrap_or(input).trim();
        let code = parse_key_code(key_name).map_err(|reason| KeySpecError::new(input, reason))?;
        let mut modifiers = Modifiers::NONE;
        for modifier_name in &parts[..parts.len().saturating_sub(1)] {
            let modifier_name = modifier_name.trim();
            let modifier = parse_modifier(modifier_name).ok_or_else(|| {
                KeySpecError::new(
                    input,
                    format!(
                        "unknown modifier {modifier_name:?}; use Ctrl, Alt, Shift, Cmd, Meta, or Hyper"
                    ),
                )
            })?;
            modifiers |= modifier;
        }

        if modifiers.contains(Modifiers::SHIFT) && matches!(code, KeyCode::Char(_)) {
            return Err(KeySpecError::new(
                input,
                "write a Shift-modified printable key as its resulting character (use `A` instead of `Shift+a`, or `<` instead of `Shift+,`)",
            ));
        }

        let display = display_key_stroke(code, modifiers);

        Ok(Self {
            code,
            modifiers,
            display,
        })
    }

    /// Return the display spelling used in the footer key guide.
    #[must_use]
    pub fn display(&self) -> &str {
        &self.display
    }

    /// Compare an input event with this configured stroke, including terminal
    /// aliases for ASCII control-byte encodings.
    #[must_use]
    pub fn matches(&self, code: KeyCode, modifiers: Modifiers) -> bool {
        (self.code == code && self.modifiers == modifiers)
            || control_letter_matches(self.code, self.modifiers, code, modifiers)
            || control_character_terminal_alias_matches(self.code, self.modifiers, code, modifiers)
            || question_mark_terminal_alias_matches(self.code, self.modifiers, code, modifiers)
    }
}

fn control_letter_matches(
    configured_code: KeyCode,
    configured_modifiers: Modifiers,
    actual_code: KeyCode,
    actual_modifiers: Modifiers,
) -> bool {
    if configured_modifiers != actual_modifiers
        || !configured_modifiers.contains(Modifiers::CONTROL)
    {
        return false;
    }

    match (configured_code, actual_code) {
        (KeyCode::Char(configured), KeyCode::Char(actual)) => {
            configured.is_ascii_alphabetic() && configured.eq_ignore_ascii_case(&actual)
        }
        _ => false,
    }
}

fn control_character_terminal_alias_matches(
    configured_code: KeyCode,
    configured_modifiers: Modifiers,
    actual_code: KeyCode,
    actual_modifiers: Modifiers,
) -> bool {
    let KeyCode::Char(configured) = configured_code else {
        return false;
    };
    if !configured_modifiers.contains(Modifiers::CONTROL) {
        return false;
    }

    let actual_code_matches = match configured {
        '@' => actual_code == KeyCode::Char(' '),
        '[' => actual_code == KeyCode::Esc,
        '\\' => actual_code == KeyCode::Char('4'),
        ']' => actual_code == KeyCode::Char('5'),
        '^' => actual_code == KeyCode::Char('6'),
        '_' => actual_code == KeyCode::Char('7'),
        _ => false,
    };
    if !actual_code_matches {
        return false;
    }

    configured_modifiers == actual_modifiers
        || (actual_code == KeyCode::Esc
            && configured_modifiers == (actual_modifiers | Modifiers::CONTROL))
}

fn question_mark_terminal_alias_matches(
    configured_code: KeyCode,
    configured_modifiers: Modifiers,
    actual_code: KeyCode,
    actual_modifiers: Modifiers,
) -> bool {
    if configured_code != KeyCode::Char('?') || configured_modifiers == Modifiers::NONE {
        return false;
    }

    // Legacy Unix input sends Ctrl+? as DEL. crossterm exposes that byte as
    // Backspace and cannot retain the Control bit. Zellij can instead encode
    // Ctrl+Shift+/ as 0x1f, which crossterm exposes as Ctrl+7. CSI-u can expose
    // the question-mark character with the Control bit. Other modifier bits
    // must remain unchanged.
    if actual_code == KeyCode::Char('7') {
        return configured_modifiers == actual_modifiers;
    }

    actual_code == KeyCode::Backspace
        && (configured_modifiers == actual_modifiers
            || (configured_modifiers.contains(Modifiers::CONTROL)
                && configured_modifiers == (actual_modifiers | Modifiers::CONTROL)))
}

/// A malformed key expression from the configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySpecError {
    input: String,
    reason: String,
}

impl KeySpecError {
    fn new(input: &str, reason: impl Into<String>) -> Self {
        Self {
            input: input.to_string(),
            reason: reason.into(),
        }
    }
}

impl fmt::Display for KeySpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid key {:?}: {}; supported key names include Esc, Enter, Space, arrows, and single characters",
            self.input, self.reason
        )
    }
}

impl std::error::Error for KeySpecError {}

/// Key assignments stored under the user's `[tui.key_bindings]` table.
///
/// Each action accepts one or more key expressions. Missing action entries are filled with the
/// defaults so older user configuration files and partial overrides remain usable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyBindings(BTreeMap<String, Vec<String>>);

impl Default for KeyBindings {
    fn default() -> Self {
        let mut bindings = BTreeMap::new();
        for action in KeyAction::ALL {
            bindings.insert(
                action.name().to_string(),
                default_keys(*action)
                    .iter()
                    .map(|key| (*key).to_string())
                    .collect(),
            );
        }
        Self(bindings)
    }
}

impl KeyBindings {
    /// Return the configured key expressions for an action.
    pub fn keys(&self, action: KeyAction) -> &[String] {
        self.0.get(action.name()).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Override one action. Validation is performed when the user configuration is loaded.
    pub fn set(&mut self, action: KeyAction, keys: Vec<String>) {
        self.0.insert(action.name().to_string(), keys);
    }

    /// Fill omitted actions with their existing defaults.
    #[must_use]
    pub fn with_defaults(mut self) -> Self {
        for action in KeyAction::ALL {
            self.0.entry(action.name().to_string()).or_insert_with(|| {
                default_keys(*action)
                    .iter()
                    .map(|key| (*key).to_string())
                    .collect()
            });
        }
        self
    }

    /// Validate action names, non-empty assignments, and every key expression.
    pub fn validate(&self) -> Result<(), KeyBindingError> {
        for (name, keys) in &self.0 {
            let Some(action) = KeyAction::from_name(name) else {
                return Err(KeyBindingError::UnknownAction {
                    action: name.clone(),
                });
            };
            if keys.is_empty() {
                return Err(KeyBindingError::Empty {
                    action: action.name().to_string(),
                });
            }
            for (index, key) in keys.iter().enumerate() {
                KeyStroke::parse(key).map_err(|source| KeyBindingError::InvalidKey {
                    action: action.name().to_string(),
                    index,
                    source,
                })?;
            }
        }
        Ok(())
    }
}

/// A malformed `[tui.key_bindings]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyBindingError {
    /// The operation name is not supported.
    UnknownAction { action: String },
    /// An operation has no keys assigned.
    Empty { action: String },
    /// One key expression cannot be parsed.
    InvalidKey {
        action: String,
        index: usize,
        source: KeySpecError,
    },
}

impl fmt::Display for KeyBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownAction { action } => write!(
                f,
                "unknown operation {action:?}; use one of: {}",
                KeyAction::ALL
                    .iter()
                    .map(|action| action.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Empty { action } => write!(
                f,
                "operation {action:?} must have at least one key (remove the override to use its default)"
            ),
            Self::InvalidKey {
                action,
                index,
                source,
            } => write!(f, "operation {action:?} key #{index}: {source}"),
        }
    }
}

impl std::error::Error for KeyBindingError {}

fn default_keys(action: KeyAction) -> &'static [&'static str] {
    match action {
        KeyAction::Quit => &["q", "Esc"],
        KeyAction::Shell => &["Q"],
        KeyAction::SelectLeft => &["h", "Left"],
        KeyAction::SelectRight => &["l", "Right"],
        KeyAction::SelectUp => &["k", "Up"],
        KeyAction::SelectDown => &["j", "Down"],
        KeyAction::MoveLeft => &["H", "Shift+Left"],
        KeyAction::MoveRight => &["L", "Shift+Right"],
        KeyAction::ReorderUp => &["K", "Shift+Up"],
        KeyAction::ReorderDown => &["J", "Shift+Down"],
        KeyAction::ToggleExpand => &["Space", "Enter"],
        KeyAction::Add => &["a"],
        KeyAction::DependencyAdd => &["d"],
        KeyAction::DependencyRemove => &["D"],
        KeyAction::Parent => &["p"],
        KeyAction::Edit => &["e"],
        KeyAction::Reload => &["r"],
        KeyAction::Maximize => &["m"],
        KeyAction::Search => &["/"],
        KeyAction::RegexSearch => &["Ctrl+?"],
        KeyAction::Details => &["v"],
        KeyAction::Help => &["?"],
        KeyAction::ClearFilter => &["Esc"],
        KeyAction::ConfirmQuit => &["y", "Y", "q", "Q", "Enter", "Esc"],
        KeyAction::CancelQuit => &["n", "N"],
        KeyAction::PopupClose => &["v", "Esc", "q"],
        KeyAction::PopupScrollUp => &["k", "Up"],
        KeyAction::PopupScrollDown => &["j", "Down"],
        KeyAction::PopupSelectUp => &["K", "Shift+Up"],
        KeyAction::PopupSelectDown => &["J", "Shift+Down"],
        KeyAction::PopupSelectLeft => &["H", "Shift+Left"],
        KeyAction::PopupSelectRight => &["L", "Shift+Right"],
    }
}

fn parse_modifier(input: &str) -> Option<Modifiers> {
    let modifier = match input.to_ascii_lowercase().as_str() {
        "shift" => Modifiers::SHIFT,
        "ctrl" | "control" => Modifiers::CONTROL,
        "alt" | "option" => Modifiers::ALT,
        "cmd" | "command" | "super" | "windows" | "win" => Modifiers::SUPER,
        "meta" => Modifiers::META,
        "hyper" => Modifiers::HYPER,
        _ => return None,
    };
    Some(modifier)
}

fn parse_key_code(input: &str) -> Result<KeyCode, String> {
    let normalized = input.to_ascii_lowercase();
    let code = match normalized.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "backspace" | "back" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "page_up" | "pgup" => KeyCode::PageUp,
        "pagedown" | "page_down" | "pgdn" => KeyCode::PageDown,
        "plus" => KeyCode::Char('+'),
        _ if normalized.starts_with('f') && normalized[1..].parse::<u8>().is_ok() => {
            let number = normalized[1..]
                .parse::<u8>()
                .map_err(|_| "function key number is invalid".to_string())?;
            if (1..=24).contains(&number) {
                KeyCode::F(number)
            } else {
                return Err("function keys must be between F1 and F24".to_string());
            }
        }
        _ => {
            let mut chars = input.chars();
            let Some(character) = chars.next() else {
                return Err("the key name must not be empty".to_string());
            };
            if chars.next().is_some() {
                return Err(format!(
                    "unknown key {input:?}; use a named key, a single character, or F1-F24"
                ));
            }
            KeyCode::Char(character)
        }
    };
    Ok(code)
}

fn display_key_stroke(code: KeyCode, modifiers: Modifiers) -> String {
    let mut names = Vec::with_capacity(6);
    if modifiers.contains(Modifiers::CONTROL) {
        names.push("Ctrl");
    }
    if modifiers.contains(Modifiers::ALT) {
        names.push("Alt");
    }
    if modifiers.contains(Modifiers::SUPER) {
        names.push("Cmd");
    }
    if modifiers.contains(Modifiers::META) {
        names.push("Meta");
    }
    if modifiers.contains(Modifiers::HYPER) {
        names.push("Hyper");
    }
    if modifiers.contains(Modifiers::SHIFT) {
        names.push("Shift");
    }
    let key = display_key_code(code);
    if names.is_empty() {
        key
    } else {
        format!("{}+{key}", names.join("+"))
    }
}

fn display_key_code(code: KeyCode) -> String {
    match code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char('+') => "Plus".to_string(),
        KeyCode::Char(character) => character.to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::F(number) => format!("F{number}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_and_platform_modifiers_are_parsed_and_displayed() {
        let control = KeyStroke::parse("Ctrl+a").expect("control key is valid");
        assert!(control.matches(KeyCode::Char('a'), Modifiers::CONTROL));
        assert_eq!(control.display(), "Ctrl+a");

        let command = KeyStroke::parse("Cmd+q").expect("command key is valid");
        assert!(command.matches(KeyCode::Char('q'), Modifiers::SUPER));
        assert_eq!(command.display(), "Cmd+q");
    }

    #[test]
    fn terminal_aliases_match_modified_printable_bindings() {
        let control_letter = KeyStroke::parse("Ctrl+A").expect("control letter is valid");
        assert!(control_letter.matches(KeyCode::Char('a'), Modifiers::CONTROL));

        let unix_control_aliases = [
            ("Ctrl+@", KeyCode::Char(' '), Modifiers::CONTROL),
            ("Ctrl+[", KeyCode::Esc, Modifiers::NONE),
            ("Ctrl+\\", KeyCode::Char('4'), Modifiers::CONTROL),
            ("Ctrl+]", KeyCode::Char('5'), Modifiers::CONTROL),
            ("Ctrl+^", KeyCode::Char('6'), Modifiers::CONTROL),
            ("Ctrl+_", KeyCode::Char('7'), Modifiers::CONTROL),
        ];
        for (input, code, modifiers) in unix_control_aliases {
            let stroke = KeyStroke::parse(input).expect("control punctuation is valid");
            assert!(stroke.matches(code, modifiers), "input: {input}");
        }

        let regex_search = KeyStroke::parse("Ctrl+?").expect("regex search key is valid");
        assert!(regex_search.matches(KeyCode::Backspace, Modifiers::NONE));
        assert!(regex_search.matches(KeyCode::Char('?'), Modifiers::CONTROL));
        assert!(regex_search.matches(KeyCode::Char('7'), Modifiers::CONTROL));

        let alt_regex_search = KeyStroke::parse("Alt+?").expect("Alt binding is valid");
        assert!(alt_regex_search.matches(KeyCode::Backspace, Modifiers::ALT));

        let combined_regex_search =
            KeyStroke::parse("Ctrl+Alt+?").expect("combined binding is valid");
        assert!(combined_regex_search.matches(KeyCode::Backspace, Modifiers::ALT));
    }

    #[test]
    fn default_bindings_keep_multiple_existing_keys() {
        let bindings = KeyBindings::default();

        assert_eq!(
            bindings.keys(KeyAction::Quit),
            ["q".to_string(), "Esc".to_string()]
        );
        assert_eq!(
            bindings.keys(KeyAction::SelectLeft),
            ["h".to_string(), "Left".to_string()]
        );
        assert!(
            KeyAction::ALL
                .iter()
                .all(|action| !bindings.keys(*action).is_empty())
        );
    }

    #[test]
    fn invalid_key_spec_explains_the_supported_syntax() {
        let error = KeyStroke::parse("Controlled+a")
            .expect_err("unknown modifier must be rejected")
            .to_string();

        assert!(error.contains("Ctrl"));
        assert!(error.contains("Alt"));
        assert!(error.contains("Cmd"));
    }

    #[test]
    fn parses_named_keys_aliases_and_function_keys() {
        let cases = [
            ("Esc", KeyCode::Esc),
            ("Escape", KeyCode::Esc),
            ("Enter", KeyCode::Enter),
            ("Return", KeyCode::Enter),
            ("Space", KeyCode::Char(' ')),
            ("Tab", KeyCode::Tab),
            ("Backspace", KeyCode::Backspace),
            ("Back", KeyCode::Backspace),
            ("Delete", KeyCode::Delete),
            ("Del", KeyCode::Delete),
            ("Insert", KeyCode::Insert),
            ("Ins", KeyCode::Insert),
            ("Left", KeyCode::Left),
            ("Right", KeyCode::Right),
            ("Up", KeyCode::Up),
            ("Down", KeyCode::Down),
            ("Home", KeyCode::Home),
            ("End", KeyCode::End),
            ("PageUp", KeyCode::PageUp),
            ("page_up", KeyCode::PageUp),
            ("PgUp", KeyCode::PageUp),
            ("PageDown", KeyCode::PageDown),
            ("page_down", KeyCode::PageDown),
            ("PgDn", KeyCode::PageDown),
            ("Plus", KeyCode::Char('+')),
            ("F1", KeyCode::F(1)),
            ("F24", KeyCode::F(24)),
        ];

        for (input, expected) in cases {
            let stroke = KeyStroke::parse(input).expect("named key is valid");
            assert!(stroke.matches(expected, Modifiers::NONE), "input: {input}");
        }
        assert_eq!(
            KeyStroke::parse("Space").expect("space key").display(),
            "Space"
        );
        assert_eq!(
            KeyStroke::parse("Plus").expect("plus key").display(),
            "Plus"
        );
        assert_eq!(KeyStroke::parse(".").expect("character key").display(), ".");
        assert_eq!(
            KeyStroke::parse("F1").expect("function key").display(),
            "F1"
        );
    }

    #[test]
    fn parses_modifier_aliases_and_displays_combined_modifiers() {
        let aliases = [
            ("Shift+Left", Modifiers::SHIFT),
            ("Ctrl+Left", Modifiers::CONTROL),
            ("Control+Left", Modifiers::CONTROL),
            ("Alt+Left", Modifiers::ALT),
            ("Option+Left", Modifiers::ALT),
            ("Cmd+Left", Modifiers::SUPER),
            ("Command+Left", Modifiers::SUPER),
            ("Super+Left", Modifiers::SUPER),
            ("Windows+Left", Modifiers::SUPER),
            ("Win+Left", Modifiers::SUPER),
            ("Meta+Left", Modifiers::META),
            ("Hyper+Left", Modifiers::HYPER),
        ];

        for (input, expected) in aliases {
            let stroke = KeyStroke::parse(input).expect("modifier alias is valid");
            assert!(stroke.matches(KeyCode::Left, expected), "input: {input}");
        }

        let combined = KeyStroke::parse("Ctrl+Alt+Cmd+Meta+Hyper+Shift+PageDown")
            .expect("combined modifiers are valid");
        assert_eq!(combined.display(), "Ctrl+Alt+Cmd+Meta+Hyper+Shift+PageDown");
        assert!(combined.matches(
            KeyCode::PageDown,
            Modifiers::CONTROL
                | Modifiers::ALT
                | Modifiers::SUPER
                | Modifiers::META
                | Modifiers::HYPER
                | Modifiers::SHIFT
        ));
    }

    #[test]
    fn rejects_malformed_key_expressions() {
        for input in [
            "",
            "+",
            "Ctrl+",
            "Unknown+a",
            "Shift+a",
            "F0",
            "F25",
            "Fabc",
            "left right",
        ] {
            assert!(KeyStroke::parse(input).is_err(), "input: {input:?}");
        }
        assert!(
            KeyStroke::parse("Ctrl+")
                .expect_err("missing key")
                .to_string()
                .contains("Plus")
        );
        assert!(
            KeyStroke::parse("F25")
                .expect_err("function key range")
                .to_string()
                .contains("F1 and F24")
        );
    }

    #[test]
    fn key_bindings_fill_defaults_and_report_validation_errors() {
        let empty = KeyBindings(BTreeMap::new());
        assert!(empty.keys(KeyAction::Quit).is_empty());
        let completed = empty.with_defaults();
        assert!(completed.validate().is_ok());
        assert_eq!(completed.keys(KeyAction::Quit), ["q", "Esc"]);

        let mut unknown = KeyBindings(BTreeMap::new());
        unknown
            .0
            .insert("unknown".to_string(), vec!["q".to_string()]);
        let error = unknown.validate().expect_err("unknown action must fail");
        assert!(matches!(error, KeyBindingError::UnknownAction { .. }));
        assert!(error.to_string().contains("unknown operation"));

        let mut empty_action = KeyBindings(BTreeMap::new());
        empty_action.0.insert("quit".to_string(), Vec::new());
        let error = empty_action
            .validate()
            .expect_err("empty assignment must fail");
        assert!(matches!(error, KeyBindingError::Empty { .. }));
        assert!(error.to_string().contains("at least one key"));

        let mut invalid = KeyBindings(BTreeMap::new());
        invalid
            .0
            .insert("quit".to_string(), vec!["not a key".to_string()]);
        let error = invalid.validate().expect_err("invalid key must fail");
        assert!(matches!(
            error,
            KeyBindingError::InvalidKey { index: 0, .. }
        ));
        assert!(error.to_string().contains("key #0"));
    }
}
