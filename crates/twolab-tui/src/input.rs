//! Keys, without a terminal library.
//!
//! Stateful widgets (text input, selection list, pager) take a [`KeyInput`]
//! so they work the same under crossterm, termwiz or the browser catalog.
//! With the `crossterm` feature, `KeyInput::from(KeyEvent)` converts.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyInput {
    pub key: Key,
    pub mods: Mods,
}

impl KeyInput {
    pub const fn new(key: Key) -> KeyInput {
        KeyInput {
            key,
            mods: Mods {
                ctrl: false,
                alt: false,
                shift: false,
            },
        }
    }

    pub const fn ctrl(key: Key) -> KeyInput {
        KeyInput {
            key,
            mods: Mods {
                ctrl: true,
                alt: false,
                shift: false,
            },
        }
    }

    /// A printable character typed without Ctrl or Alt.
    pub fn typed(&self) -> Option<char> {
        match self.key {
            Key::Char(c) if !self.mods.ctrl && !self.mods.alt => Some(c),
            _ => None,
        }
    }
}

impl From<Key> for KeyInput {
    fn from(key: Key) -> Self {
        KeyInput::new(key)
    }
}

impl From<char> for KeyInput {
    fn from(c: char) -> Self {
        KeyInput::new(Key::Char(c))
    }
}

#[cfg(feature = "crossterm")]
mod crossterm_conv {
    use super::{Key, KeyInput, Mods};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    impl KeyInput {
        /// `None` for keys the widgets never act on (F-keys, media keys…).
        pub fn from_crossterm(ev: &KeyEvent) -> Option<KeyInput> {
            let key = match ev.code {
                KeyCode::Char(c) => Key::Char(c),
                KeyCode::Enter => Key::Enter,
                KeyCode::Esc => Key::Esc,
                KeyCode::Backspace => Key::Backspace,
                KeyCode::Delete => Key::Delete,
                KeyCode::Tab => Key::Tab,
                KeyCode::BackTab => Key::BackTab,
                KeyCode::Left => Key::Left,
                KeyCode::Right => Key::Right,
                KeyCode::Up => Key::Up,
                KeyCode::Down => Key::Down,
                KeyCode::Home => Key::Home,
                KeyCode::End => Key::End,
                KeyCode::PageUp => Key::PageUp,
                KeyCode::PageDown => Key::PageDown,
                _ => return None,
            };
            Some(KeyInput {
                key,
                mods: Mods {
                    ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
                    alt: ev.modifiers.contains(KeyModifiers::ALT),
                    shift: ev.modifiers.contains(KeyModifiers::SHIFT),
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ignores_control_chords() {
        assert_eq!(KeyInput::from('a').typed(), Some('a'));
        assert_eq!(KeyInput::ctrl(Key::Char('a')).typed(), None);
        assert_eq!(KeyInput::new(Key::Enter).typed(), None);
    }
}
