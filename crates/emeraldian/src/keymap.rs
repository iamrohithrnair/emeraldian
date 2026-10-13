//! Shortcuts remapped in the `[keys]` section of the config.
//!
//! The defaults assume a US layout, where `\` and `]` are keys of their own.
//! On many others they sit behind AltGr, and `Ctrl+\` can't be typed at all —
//! so any app-wide shortcut can be given a key of the user's choosing:
//!
//! ```toml
//! [keys]
//! toggle_left_sidebar = "alt+e"
//! toggle_right_sidebar = "ctrl+alt+o"
//! ```
//!
//! A remapped key is added rather than swapped in: the default keeps working,
//! so a typo in the config can never leave a command unreachable.

use std::collections::BTreeMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

/// The shortcuts that can be remapped, by the name used in the config.
///
/// The app-wide ones only. Keys that edit text, or that vim defines, mean
/// something to the text under the cursor, and moving them would change what
/// typing does rather than where a command lives.
const ACTIONS: &[(&str, Action)] = &[
    ("open_palette", Action::OpenPalette),
    ("open_switcher", Action::OpenSwitcher),
    ("open_search", Action::OpenSearch),
    ("new_note", Action::NewNote),
    ("save", Action::Save),
    ("daily_note", Action::DailyNote),
    ("toggle_mode", Action::ToggleMode),
    ("close_tab", Action::CloseTab),
    ("next_tab", Action::NextTab),
    ("previous_tab", Action::PreviousTab),
    ("rename_note", Action::RenameNote),
    ("open_theme_picker", Action::OpenThemePicker),
    ("toggle_chat", Action::ToggleChat),
    ("cycle_side_panel", Action::CycleSidePanel),
    ("toggle_left_sidebar", Action::ToggleLeftSidebar),
    ("toggle_right_sidebar", Action::ToggleRightSidebar),
    ("open_graph", Action::OpenGraph),
    ("open_local_graph", Action::OpenLocalGraph),
    ("refresh", Action::Refresh),
    ("back", Action::Back),
    ("toggle_vim_mode", Action::ToggleVimMode),
    ("quit", Action::Quit),
];

/// One key with its modifiers, as written in the config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chord {
    code: KeyCode,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Chord {
    /// Reads `"ctrl+alt+e"`, `"alt+F5"`, `"ctrl++"` and the like. Case doesn't
    /// matter, and `option`/`opt`/`meta` are accepted for Alt.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        // A trailing `+` is the key itself, as in `ctrl++`.
        let (mods, key) = match text.strip_suffix("++") {
            Some(mods) => (mods, "+"),
            None => text.rsplit_once('+').unwrap_or(("", text)),
        };

        let mut chord = Self {
            code: key_code(key).ok_or_else(|| format!("\"{key}\" is not a key"))?,
            ctrl: false,
            alt: false,
            shift: false,
        };
        for modifier in mods.split('+').filter(|m| !m.is_empty()) {
            match modifier.to_lowercase().as_str() {
                "ctrl" | "control" => chord.ctrl = true,
                "alt" | "option" | "opt" | "meta" => chord.alt = true,
                "shift" => chord.shift = true,
                other => return Err(format!("\"{other}\" is not a modifier")),
            }
        }

        // A bare letter would go off while typing a note or a search.
        let function_key = matches!(chord.code, KeyCode::F(_));
        if !(chord.ctrl || chord.alt || function_key) {
            return Err("needs Ctrl or Alt, or an F-key, so typing can't set it off".into());
        }
        Ok(chord)
    }

    fn matches(&self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if ctrl != self.ctrl || alt != self.alt {
            return false;
        }
        match (self.code, key.code) {
            (KeyCode::Char(want), KeyCode::Char(got)) => {
                // Shift is part of typing a symbol on most layouts — `+` is
                // `Shift+=` on a US one — so it only counts for letters.
                let shift_counts = want.is_alphabetic() || self.shift;
                want.to_lowercase().eq(got.to_lowercase()) && (!shift_counts || shift == self.shift)
            }
            // Terminals report Shift+Tab as a key of its own.
            (KeyCode::Tab, KeyCode::BackTab) => self.shift,
            (want, got) => want == got && shift == self.shift,
        }
    }
}

impl fmt::Display for Chord {
    /// Written the way the help and the palette write keys: `Ctrl+Alt+E`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            KeyCode::Char(ch) => write!(f, "{}", ch.to_uppercase()),
            KeyCode::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

fn key_code(name: &str) -> Option<KeyCode> {
    let mut chars = name.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(ch.to_ascii_lowercase()));
    }
    let lower = name.to_lowercase();
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then_some(KeyCode::F(n));
    }
    Some(match lower.as_str() {
        "space" => KeyCode::Char(' '),
        "tab" => KeyCode::Tab,
        "enter" | "return" => KeyCode::Enter,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        _ => return None,
    })
}

/// The `[keys]` section, read.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: Vec<(Chord, Action)>,
    /// What couldn't be understood, worded for the status bar.
    pub problems: Vec<String>,
}

impl Keymap {
    pub fn new(keys: &BTreeMap<String, String>) -> Self {
        let mut keymap = Self::default();
        for (name, key) in keys {
            let Some((_, action)) = ACTIONS.iter().find(|(known, _)| known == name) else {
                keymap.problems.push(format!(
                    "keys.{name} is not a shortcut that can be remapped — the README lists them"
                ));
                continue;
            };
            match Chord::parse(key) {
                Ok(chord) => keymap.bindings.push((chord, action.clone())),
                Err(why) => keymap
                    .problems
                    .push(format!("keys.{name} = \"{key}\": {why}")),
            }
        }
        keymap
    }

    /// The action a key has been given, if any.
    #[must_use]
    pub fn action_for(&self, key: &KeyEvent) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(chord, _)| chord.matches(key))
            .map(|(_, action)| action.clone())
    }

    /// The key an action has been given, written for display.
    #[must_use]
    pub fn label_for(&self, action: &Action) -> Option<String> {
        self.bindings
            .iter()
            .find(|(_, bound)| bound == action)
            .map(|(chord, _)| chord.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap(pairs: &[(&str, &str)]) -> Keymap {
        Keymap::new(
            &pairs
                .iter()
                .map(|(name, key)| ((*name).to_string(), (*key).to_string()))
                .collect(),
        )
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn a_remapped_key_runs_its_action() {
        let keys = keymap(&[("toggle_left_sidebar", "Alt+E")]);
        assert!(keys.problems.is_empty());
        assert_eq!(
            keys.action_for(&key(KeyCode::Char('e'), KeyModifiers::ALT)),
            Some(Action::ToggleLeftSidebar)
        );
        assert_eq!(
            keys.action_for(&key(KeyCode::Char('e'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            keys.label_for(&Action::ToggleLeftSidebar).as_deref(),
            Some("Alt+E")
        );
    }

    #[test]
    fn shift_counts_for_letters_but_not_for_symbols() {
        let keys = keymap(&[("open_search", "ctrl+f"), ("toggle_chat", "ctrl++")]);
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(keys.action_for(&key(KeyCode::Char('F'), ctrl_shift)), None);
        // `+` takes Shift on a US layout and not on a Spanish one.
        assert_eq!(
            keys.action_for(&key(KeyCode::Char('+'), ctrl_shift)),
            Some(Action::ToggleChat)
        );
        assert_eq!(
            keys.action_for(&key(KeyCode::Char('+'), KeyModifiers::CONTROL)),
            Some(Action::ToggleChat)
        );
    }

    #[test]
    fn keys_beyond_letters_parse() {
        assert_eq!(Chord::parse("F5").map(|c| c.to_string()), Ok("F5".into()));
        assert_eq!(
            Chord::parse("option+ctrl+º").map(|c| c.to_string()),
            Ok("Ctrl+Alt+º".into())
        );
        assert!(Chord::parse("ctrl+pagedown").is_ok());

        let keys = keymap(&[("previous_tab", "alt+shift+tab")]);
        assert_eq!(
            keys.action_for(&key(
                KeyCode::BackTab,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )),
            Some(Action::PreviousTab)
        );
    }

    #[test]
    fn mistakes_are_explained_not_ignored() {
        let keys = keymap(&[
            ("toggle_sidebar", "alt+e"),
            ("save", "s"),
            ("quit", "hyper+q"),
            ("new_note", "ctrl+nope"),
        ]);
        assert_eq!(keys.problems.len(), 4, "{:?}", keys.problems);
        assert!(
            keys.problems
                .iter()
                .any(|p| p.contains("keys.toggle_sidebar"))
        );
        assert!(keys.problems.iter().any(|p| p.contains("typing")));
        assert_eq!(
            keys.action_for(&key(KeyCode::Char('s'), KeyModifiers::NONE)),
            None
        );
    }
}
