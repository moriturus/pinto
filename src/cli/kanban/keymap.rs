//! Adapter from persisted key expressions to crossterm key events.

use anyhow::Result;
use pinto::kanban_keys::{KeyAction, KeyBindings, KeyCode, KeyStroke, Modifiers};
use ratatui::crossterm::event::{KeyCode as CrosstermKeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

/// Parsed key assignments used by the Kanban event loop.
pub(super) struct KeyMap {
    bindings: HashMap<KeyAction, Vec<KeyStroke>>,
}

impl KeyMap {
    /// Parse and validate the configured key assignments.
    pub(super) fn from_bindings(bindings: &KeyBindings) -> Result<Self> {
        let bindings = bindings.clone().with_defaults();
        bindings
            .validate()
            .map_err(|error| anyhow::anyhow!("invalid Kanban key bindings: {error}"))?;

        let mut parsed = HashMap::with_capacity(KeyAction::ALL.len());
        for action in KeyAction::ALL {
            let keys = bindings
                .keys(*action)
                .iter()
                .map(|key| {
                    KeyStroke::parse(key).map_err(|error| {
                        anyhow::anyhow!("invalid Kanban key binding for {}: {error}", action.name())
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            parsed.insert(*action, keys);
        }
        Ok(Self { bindings: parsed })
    }

    /// Return whether a terminal event matches any key assigned to `action`.
    pub(super) fn matches(&self, action: KeyAction, event: KeyEvent) -> bool {
        let Some((code, modifiers)) = key_event_parts(event) else {
            return false;
        };
        self.bindings
            .get(&action)
            .is_some_and(|keys| keys.iter().any(|key| key.matches(code, modifiers)))
    }

    /// Return the spelling of the first configured key for the footer guide.
    pub(super) fn first(&self, action: KeyAction) -> &str {
        self.bindings
            .get(&action)
            .and_then(|keys| keys.first())
            .map(KeyStroke::display)
            .unwrap_or("?")
    }
}

fn key_event_parts(event: KeyEvent) -> Option<(KeyCode, Modifiers)> {
    let code = match event.code {
        CrosstermKeyCode::Char(character) => KeyCode::Char(character),
        CrosstermKeyCode::Enter => KeyCode::Enter,
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::Tab => KeyCode::Tab,
        CrosstermKeyCode::Backspace => KeyCode::Backspace,
        CrosstermKeyCode::Delete => KeyCode::Delete,
        CrosstermKeyCode::Insert => KeyCode::Insert,
        CrosstermKeyCode::Left => KeyCode::Left,
        CrosstermKeyCode::Right => KeyCode::Right,
        CrosstermKeyCode::Up => KeyCode::Up,
        CrosstermKeyCode::Down => KeyCode::Down,
        CrosstermKeyCode::Home => KeyCode::Home,
        CrosstermKeyCode::End => KeyCode::End,
        CrosstermKeyCode::PageUp => KeyCode::PageUp,
        CrosstermKeyCode::PageDown => KeyCode::PageDown,
        CrosstermKeyCode::F(number) => KeyCode::F(number),
        _ => return None,
    };

    // Printable events carry the resulting character (`A`, `<`, etc.), so their physical Shift
    // key is implicit. Named keys retain Shift so bindings such as `Shift+Left` remain usable.
    let mut modifiers = Modifiers::NONE;
    if event.modifiers.contains(KeyModifiers::SHIFT)
        && !matches!(event.code, CrosstermKeyCode::Char(_))
    {
        modifiers |= Modifiers::SHIFT;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers |= Modifiers::CONTROL;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        modifiers |= Modifiers::ALT;
    }
    if event.modifiers.contains(KeyModifiers::SUPER) {
        modifiers |= Modifiers::SUPER;
    }
    if event.modifiers.contains(KeyModifiers::HYPER) {
        modifiers |= Modifiers::HYPER;
    }
    if event.modifiers.contains(KeyModifiers::META) {
        modifiers |= Modifiers::META;
    }
    Some((code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinto::kanban_keys::{KeyAction, KeyBindings};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn keymap_matches_multiple_keys_and_modifiers() {
        let mut bindings = KeyBindings::default();
        bindings.set(
            KeyAction::Quit,
            vec!["Ctrl+a".to_string(), "Esc".to_string()],
        );
        let keymap = KeyMap::from_bindings(&bindings).expect("valid bindings");

        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)
        ));

        bindings.set(
            KeyAction::Details,
            vec!["Cmd+d".to_string(), "v".to_string()],
        );
        let keymap = KeyMap::from_bindings(&bindings).expect("valid bindings");
        assert!(keymap.matches(
            KeyAction::Details,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::SUPER)
        ));
    }

    #[test]
    fn keymap_matches_terminal_aliases_for_control_and_other_modifiers() {
        let mut bindings = KeyBindings::default();
        bindings.set(KeyAction::RegexSearch, vec!["Ctrl+?".to_string()]);
        let keymap = KeyMap::from_bindings(&bindings).expect("valid regex search binding");

        assert!(keymap.matches(
            KeyAction::RegexSearch,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
        ));
        assert!(keymap.matches(
            KeyAction::RegexSearch,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::CONTROL)
        ));
        assert!(!keymap.matches(
            KeyAction::RegexSearch,
            KeyEvent::new(KeyCode::Char('7'), KeyModifiers::CONTROL)
        ));

        let modifier_cases = [
            ("Alt+?", KeyModifiers::ALT),
            ("Cmd+?", KeyModifiers::SUPER),
            ("Meta+?", KeyModifiers::META),
            ("Hyper+?", KeyModifiers::HYPER),
            ("Ctrl+Alt+?", KeyModifiers::ALT),
        ];
        let mut modifier_bindings = KeyBindings::default();
        modifier_bindings.set(
            KeyAction::Details,
            modifier_cases
                .iter()
                .map(|(binding, _)| (*binding).to_string())
                .collect(),
        );
        modifier_bindings.set(KeyAction::Help, vec!["?".to_string()]);
        let keymap = KeyMap::from_bindings(&modifier_bindings).expect("valid modifier bindings");

        for (binding, modifiers) in modifier_cases {
            assert!(
                keymap.matches(
                    KeyAction::Details,
                    KeyEvent::new(KeyCode::Backspace, modifiers)
                ),
                "binding: {binding}"
            );
        }
        assert!(keymap.matches(
            KeyAction::Help,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)
        ));
        assert!(!keymap.matches(
            KeyAction::Help,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)
        ));

        bindings.set(KeyAction::Details, vec!["Ctrl+A".to_string()]);
        bindings.set(KeyAction::Edit, vec!["Alt+?".to_string()]);
        let keymap = KeyMap::from_bindings(&bindings).expect("valid modified bindings");

        assert!(keymap.matches(
            KeyAction::Details,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)
        ));
        assert!(keymap.matches(
            KeyAction::Edit,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::ALT)
        ));
    }

    #[test]
    fn keymap_exposes_the_first_configured_key_for_guides() {
        let mut bindings = KeyBindings::default();
        bindings.set(
            KeyAction::Details,
            vec!["Cmd+d".to_string(), "v".to_string()],
        );
        let keymap = KeyMap::from_bindings(&bindings).expect("valid bindings");

        assert_eq!(keymap.first(KeyAction::Details), "Cmd+d");
    }

    #[test]
    fn keymap_matches_named_terminal_keys_and_all_modifier_bits() {
        let mut bindings = KeyBindings::default();
        bindings.set(
            KeyAction::Quit,
            [
                "Enter",
                "Esc",
                "Tab",
                "Backspace",
                "Delete",
                "Insert",
                "Left",
                "Right",
                "Up",
                "Down",
                "Home",
                "End",
                "PageUp",
                "PageDown",
                "F12",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        );
        let keymap = KeyMap::from_bindings(&bindings).expect("valid named bindings");

        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Backspace,
            KeyCode::Delete,
            KeyCode::Insert,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
            KeyCode::F(12),
        ] {
            assert!(keymap.matches(KeyAction::Quit, KeyEvent::new(code, KeyModifiers::NONE)));
        }

        let mut modifiers = KeyBindings::default();
        modifiers.set(
            KeyAction::Quit,
            [
                "Shift+Left",
                "Alt+Right",
                "Cmd+Up",
                "Hyper+Down",
                "Meta+Home",
                "A",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        );
        let keymap = KeyMap::from_bindings(&modifiers).expect("valid modifier bindings");

        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Left, KeyModifiers::SHIFT)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Up, KeyModifiers::SUPER)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Down, KeyModifiers::HYPER)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Home, KeyModifiers::META)
        ));
        assert!(keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)
        ));
    }

    #[test]
    fn keymap_rejects_unsupported_terminal_events_and_invalid_bindings() {
        let keymap = KeyMap::from_bindings(&KeyBindings::default()).expect("default bindings");
        assert!(!keymap.matches(
            KeyAction::Quit,
            KeyEvent::new(KeyCode::Null, KeyModifiers::NONE)
        ));

        let mut bindings = KeyBindings::default();
        bindings.set(KeyAction::Quit, vec!["not a key".to_string()]);
        let error = KeyMap::from_bindings(&bindings)
            .err()
            .expect("invalid binding must fail");
        assert!(error.to_string().contains("invalid Kanban key bindings"));
    }
}
