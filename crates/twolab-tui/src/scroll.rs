// Ported from OpenAI Codex `codex-rs/tui/src/bottom_pane/scroll_state.rs` and
// the `ListAction` defaults in `codex-rs/tui/src/keymap.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: public; `ListAction` keeps the six vertical
// moves and maps them from `KeyInput` instead of a configurable keymap;
// moving up from an index past a shrunk list's end clamps into range; the
// crossterm mouse-wheel helper is left behind.

//! Where the cursor is in a list, kept in view: a selected index and the
//! first visible row.
//!
//! Ported from OpenAI Codex (`bottom_pane/scroll_state.rs`, Apache-2.0), the
//! state behind every Codex picker. It knows nothing about the rows: callers
//! pass the current length and the number of visible rows on every call, so
//! a filtered or resized list never leaves the cursor on a row that is gone.
//! ↑/↓ wrap around; page and jump moves clamp at the ends.

use crate::input::{Key, KeyInput};

/// A vertical move through a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ListAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    JumpTop,
    JumpBottom,
}

impl ListAction {
    /// The move a key asks for, with Codex's default list bindings:
    /// ↑ / Ctrl-P / Ctrl-K, ↓ / Ctrl-N / Ctrl-J, PgUp / Ctrl-B, PgDn / Ctrl-F,
    /// Home, End. Plain letters are left to the caller — a list with a
    /// search field types them.
    pub fn from_key(key: KeyInput) -> Option<ListAction> {
        let ctrl = key.mods.ctrl && !key.mods.alt;
        Some(match key.key {
            Key::Up => ListAction::MoveUp,
            Key::Down => ListAction::MoveDown,
            Key::PageUp => ListAction::PageUp,
            Key::PageDown => ListAction::PageDown,
            Key::Home => ListAction::JumpTop,
            Key::End => ListAction::JumpBottom,
            Key::Char('p' | 'k') if ctrl => ListAction::MoveUp,
            Key::Char('n' | 'j') if ctrl => ListAction::MoveDown,
            Key::Char('b') if ctrl => ListAction::PageUp,
            Key::Char('f') if ctrl => ListAction::PageDown,
            _ => return None,
        })
    }
}

/// Generic scroll/selection state for a vertical list menu.
///
/// Encapsulates the common behavior of a selectable list that supports:
/// - Optional selection (None when list is empty)
/// - Wrap-around navigation on Up/Down
/// - Maintaining a scroll window (`scroll_top`) so the selected row stays visible
///
/// Callers own the filtered row count and the visible window size. Every
/// mutation method takes those values instead of caching them here, so list
/// views can apply filters, pagination, or density changes without this helper
/// knowing about their data model. Passing a stale length after filtering would
/// leave selection pointing at the wrong row, so callers should clamp or move
/// through this type immediately after changing their visible row set.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScrollState {
    /// The highlighted row, `None` when the list is empty.
    pub selected_idx: Option<usize>,
    /// The first row in view.
    pub scroll_top: usize,
}

impl ScrollState {
    pub const fn new() -> Self {
        Self {
            selected_idx: None,
            scroll_top: 0,
        }
    }

    /// Apply a vertical list action and keep the resulting selection visible.
    pub fn apply(&mut self, action: ListAction, len: usize, visible_rows: usize) {
        match action {
            ListAction::MoveUp => {
                self.move_up_wrap(len);
                self.ensure_visible(len, visible_rows);
            }
            ListAction::MoveDown => {
                self.move_down_wrap(len);
                self.ensure_visible(len, visible_rows);
            }
            ListAction::PageUp => self.page_up_clamped(len, visible_rows),
            ListAction::PageDown => self.page_down_clamped(len, visible_rows),
            ListAction::JumpTop => self.jump_top(len, visible_rows),
            ListAction::JumpBottom => self.jump_bottom(len, visible_rows),
        }
    }

    /// Reset selection and scroll.
    pub fn reset(&mut self) {
        self.selected_idx = None;
        self.scroll_top = 0;
    }

    /// Clamp selection to be within the [0, len-1] range, or None when empty.
    pub fn clamp_selection(&mut self, len: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        self.selected_idx = Some(self.selected_idx.unwrap_or(0).min(len - 1));
    }

    /// Move selection up by one, wrapping to the bottom when necessary.
    pub fn move_up_wrap(&mut self, len: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            Some(idx) if idx > 0 => (idx - 1).min(len - 1),
            Some(_) => len - 1,
            None => 0,
        });
    }

    /// Move selection down by one, wrapping to the top when necessary.
    pub fn move_down_wrap(&mut self, len: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            Some(idx) if idx + 1 < len => idx + 1,
            _ => 0,
        });
    }

    /// Move selection up by one visible page, clamping at the first row.
    ///
    /// Page movement intentionally does not wrap. It mirrors terminal list
    /// behavior where repeated page-up/page-down converges at the nearest edge
    /// while still keeping the selected row visible.
    pub fn page_up_clamped(&mut self, len: usize, visible_rows: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        let step = visible_rows.max(1);
        let current = self.selected_idx.unwrap_or(0).min(len - 1);
        self.selected_idx = Some(current.saturating_sub(step));
        self.ensure_visible(len, visible_rows);
    }

    /// Move selection down by one visible page, clamping at the last row.
    ///
    /// Page movement intentionally does not wrap. It mirrors terminal list
    /// behavior where repeated page-up/page-down converges at the nearest edge
    /// while still keeping the selected row visible.
    pub fn page_down_clamped(&mut self, len: usize, visible_rows: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        let step = visible_rows.max(1);
        let current = self.selected_idx.unwrap_or(0).min(len - 1);
        self.selected_idx = Some(current.saturating_add(step).min(len - 1));
        self.ensure_visible(len, visible_rows);
    }

    /// Jump selection to the first row.
    pub fn jump_top(&mut self, len: usize, visible_rows: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        self.selected_idx = Some(0);
        self.ensure_visible(len, visible_rows);
    }

    /// Jump selection to the last row.
    pub fn jump_bottom(&mut self, len: usize, visible_rows: usize) {
        if self.clear_if_empty(len) {
            return;
        }
        self.selected_idx = Some(len - 1);
        self.ensure_visible(len, visible_rows);
    }

    fn clear_if_empty(&mut self, len: usize) -> bool {
        if len != 0 {
            return false;
        }
        self.selected_idx = None;
        self.scroll_top = 0;
        true
    }

    /// Adjust `scroll_top` so that the current `selected_idx` is visible within
    /// the window of `visible_rows`.
    pub fn ensure_visible(&mut self, len: usize, visible_rows: usize) {
        if len == 0 || visible_rows == 0 {
            self.scroll_top = 0;
            return;
        }
        if let Some(sel) = self.selected_idx {
            if sel < self.scroll_top {
                self.scroll_top = sel;
            } else {
                let bottom = self.scroll_top + visible_rows - 1;
                if sel > bottom {
                    self.scroll_top = sel + 1 - visible_rows;
                }
            }
        } else {
            self.scroll_top = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Upstream: scroll_state.rs
    #[test]
    fn wrap_navigation_and_visibility() {
        let mut s = ScrollState::new();
        let len = 10;
        let vis = 5;

        s.clamp_selection(len);
        assert_eq!(s.selected_idx, Some(0));
        s.ensure_visible(len, vis);
        assert_eq!(s.scroll_top, 0);

        s.apply(ListAction::MoveUp, len, vis);
        assert_eq!(s.selected_idx, Some(len - 1));
        match s.selected_idx {
            Some(sel) => assert!(s.scroll_top <= sel),
            None => panic!("expected Some(selected_idx) after wrap"),
        }

        s.apply(ListAction::MoveDown, len, vis);
        assert_eq!(s.selected_idx, Some(0));
        assert_eq!(s.scroll_top, 0);
    }

    // Upstream: scroll_state.rs
    #[test]
    fn page_and_jump_navigation_clamps() {
        let mut s = ScrollState::new();
        let len = 10;
        let vis = 4;

        s.clamp_selection(len);
        s.apply(ListAction::PageDown, len, vis);
        assert_eq!(s.selected_idx, Some(4));
        assert_eq!(s.scroll_top, 1);

        s.apply(ListAction::PageDown, len, vis);
        assert_eq!(s.selected_idx, Some(8));
        assert_eq!(s.scroll_top, 5);

        s.apply(ListAction::PageDown, len, vis);
        assert_eq!(s.selected_idx, Some(9));
        assert_eq!(s.scroll_top, 6);

        s.apply(ListAction::PageUp, len, vis);
        assert_eq!(s.selected_idx, Some(5));
        assert_eq!(s.scroll_top, 5);

        s.apply(ListAction::JumpTop, len, vis);
        assert_eq!(s.selected_idx, Some(0));
        assert_eq!(s.scroll_top, 0);

        s.apply(ListAction::JumpBottom, len, vis);
        assert_eq!(s.selected_idx, Some(9));
        assert_eq!(s.scroll_top, 6);
    }

    #[test]
    fn empty_lists_clear_and_shrunk_lists_clamp() {
        let mut s = ScrollState {
            selected_idx: Some(7),
            scroll_top: 5,
        };
        for action in [
            ListAction::MoveUp,
            ListAction::MoveDown,
            ListAction::PageUp,
            ListAction::PageDown,
            ListAction::JumpTop,
            ListAction::JumpBottom,
        ] {
            let mut e = s;
            e.apply(action, 0, 4);
            assert_eq!(e, ScrollState::new(), "{action:?}");
        }
        // A filter shrank the list under the cursor: up still lands in range.
        s.apply(ListAction::MoveUp, 3, 4);
        assert_eq!(s.selected_idx, Some(2));
        s.clamp_selection(1);
        assert_eq!(s.selected_idx, Some(0));
        s.ensure_visible(1, 0);
        assert_eq!(s.scroll_top, 0);
    }

    #[test]
    fn keys_map_to_codex_default_list_bindings() {
        assert_eq!(
            ListAction::from_key(Key::Up.into()),
            Some(ListAction::MoveUp)
        );
        assert_eq!(
            ListAction::from_key(KeyInput::ctrl(Key::Char('n'))),
            Some(ListAction::MoveDown)
        );
        assert_eq!(
            ListAction::from_key(KeyInput::ctrl(Key::Char('f'))),
            Some(ListAction::PageDown)
        );
        assert_eq!(
            ListAction::from_key(Key::End.into()),
            Some(ListAction::JumpBottom)
        );
        // Plain letters type into a search field.
        assert_eq!(ListAction::from_key(KeyInput::from('j')), None);
        assert_eq!(ListAction::from_key(Key::Enter.into()), None);
    }
}
