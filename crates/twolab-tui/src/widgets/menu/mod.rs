//! A context menu: what you can do to this thing, right where you clicked.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `draw_context_menu`,
//! `MenuChrome` and `MenuChrome::hit_item`, with the keys of
//! `on_key_context_menu` (`src/tui/mod.rs`): ↑/k ↓/j move, Enter runs the
//! highlighted item, Esc — or any other key — closes. The popup opens one row
//! under the anchor and returns one rect per item for the mouse.
//!
//! Changes from llmux:
//! - widths are terminal cells, not `chars().count()` (llmux defect §9.1,
//!   `ui.rs:3346`), so a wide title or label never overruns the frame;
//! - the popup flips above the anchor (and to its left) when it would leave
//!   the screen, instead of sliding over the row that was clicked;
//! - separators, disabled items, destructive items and per-item shortcut
//!   keys; the selection skips what cannot run;
//! - a menu taller than the screen scrolls to the selection and counts the
//!   rest (`+N more`) instead of cutting them silently.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, StatefulWidget, Widget};

use crate::input::{Key, KeyInput};
use crate::text::{self, Align};
use crate::theme::{Status, Theme};

/// One row of a menu: an action or a separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem<'a> {
    pub label: Cow<'a, str>,
    /// Shown dim at the right; a single character also runs the item when
    /// typed while the menu is open.
    pub key: Option<Cow<'a, str>>,
    pub disabled: bool,
    /// Drawn in the failure colour (bold in mono): delete, revoke, reset.
    pub destructive: bool,
    separator: bool,
}

impl<'a> MenuItem<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>) -> MenuItem<'a> {
        MenuItem {
            label: label.into(),
            key: None,
            disabled: false,
            destructive: false,
            separator: false,
        }
    }

    /// A rule between groups of items. Never selected, never hit.
    pub fn separator() -> MenuItem<'a> {
        MenuItem {
            separator: true,
            ..MenuItem::new("")
        }
    }

    pub fn key(mut self, key: impl Into<Cow<'a, str>>) -> MenuItem<'a> {
        self.key = Some(key.into());
        self
    }

    /// Shown dim, skipped by the selection, ignored by clicks.
    pub fn disabled(mut self, disabled: bool) -> MenuItem<'a> {
        self.disabled = disabled;
        self
    }

    pub fn destructive(mut self, destructive: bool) -> MenuItem<'a> {
        self.destructive = destructive;
        self
    }

    pub fn is_separator(&self) -> bool {
        self.separator
    }

    /// Can be selected and run.
    pub fn enabled(&self) -> bool {
        !self.separator && !self.disabled
    }

    fn typed_key(&self) -> Option<char> {
        let mut cs = self.key.as_deref()?.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }
}

/// The rect one runnable item was drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuHit {
    pub rect: Rect,
    pub index: usize,
}

/// Where the menu went in its last render.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuLayout {
    /// The whole popup, border included.
    pub area: Rect,
    /// One rect per drawn, runnable item (separators and disabled items
    /// have none).
    pub items: Vec<MenuHit>,
    /// Items scrolled out of a popup the screen is too short for.
    pub hidden: usize,
}

impl MenuLayout {
    /// The item a click lands on.
    pub fn hit(&self, col: u16, row: u16) -> Option<usize> {
        self.items
            .iter()
            .find(|h| {
                row >= h.rect.y && row < h.rect.bottom() && col >= h.rect.x && col < h.rect.right()
            })
            .map(|h| h.index)
    }

    /// Whether a click lands on the popup at all (a click outside closes
    /// it; a click on a separator or a disabled item does nothing).
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row >= self.area.y
            && row < self.area.bottom()
            && col >= self.area.x
            && col < self.area.right()
    }
}

/// What a key did to an open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// The selection moved.
    Moved,
    /// Run this item; the host closes the menu.
    Run(usize),
    /// Close without running anything.
    Close,
    /// Nothing happened (↓ on the last item).
    Ignored,
}

/// The selection, and where the last render put things.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuState {
    pub selected: usize,
    /// First item row drawn when the popup is too short for all of them.
    pub offset: usize,
    pub layout: MenuLayout,
}

impl MenuState {
    /// Selection on the first runnable item.
    pub fn new(menu: &Menu) -> MenuState {
        let mut s = MenuState::default();
        s.settle(menu);
        s
    }

    /// The item a click lands on (from the last render).
    pub fn hit(&self, col: u16, row: u16) -> Option<usize> {
        self.layout.hit(col, row)
    }

    /// Keep the selection on a runnable item: forward from where it is,
    /// else back.
    fn settle(&mut self, menu: &Menu) {
        let items = &menu.items;
        if items.get(self.selected).is_some_and(MenuItem::enabled) {
            return;
        }
        let from = self.selected.min(items.len());
        if let Some(i) = (from..items.len()).find(|&i| items[i].enabled()) {
            self.selected = i;
        } else if let Some(i) = (0..from).rev().find(|&i| items[i].enabled()) {
            self.selected = i;
        }
    }

    /// llmux's keys: ↑/k and ↓/j move (no wrap), Home/End jump, Enter runs,
    /// an item's own key runs it, Esc and anything else close.
    pub fn on_key(&mut self, menu: &Menu, key: KeyInput) -> MenuAction {
        self.settle(menu);
        let items = &menu.items;
        let runnable = |i: &usize| items[*i].enabled();
        if let Some(c) = key.typed()
            && let Some(i) = items
                .iter()
                .position(|it| it.enabled() && it.typed_key() == Some(c))
        {
            self.selected = i;
            return MenuAction::Run(i);
        }
        let to = match key.key {
            Key::Up | Key::Char('k') => (0..self.selected).rev().find(runnable),
            Key::Down | Key::Char('j') => (self.selected + 1..items.len()).find(runnable),
            Key::Home => (0..items.len()).find(runnable),
            Key::End => (0..items.len()).rev().find(runnable),
            Key::Enter => {
                return if items.get(self.selected).is_some_and(MenuItem::enabled) {
                    MenuAction::Run(self.selected)
                } else {
                    MenuAction::Close
                };
            }
            _ => return MenuAction::Close,
        };
        match to {
            Some(i) if i != self.selected => {
                self.selected = i;
                MenuAction::Moved
            }
            _ => MenuAction::Ignored,
        }
    }
}

/// A popup menu anchored at a cell. Render it over the whole frame: it
/// places itself.
#[derive(Debug, Clone)]
pub struct Menu<'a> {
    title: Option<Cow<'a, str>>,
    items: Vec<MenuItem<'a>>,
    anchor: (u16, u16),
    theme: Theme,
}

impl<'a> Menu<'a> {
    pub fn new(items: impl IntoIterator<Item = MenuItem<'a>>) -> Menu<'a> {
        Menu {
            title: None,
            items: items.into_iter().collect(),
            anchor: (0, 0),
            theme: Theme::default(),
        }
    }

    /// On the top border: what the menu acts on (`ai3@iq.io`).
    pub fn title(mut self, title: impl Into<Cow<'a, str>>) -> Menu<'a> {
        self.title = Some(title.into());
        self
    }

    /// The clicked cell `(col, row)`. The menu opens on the row below it.
    pub fn anchor(mut self, col: u16, row: u16) -> Menu<'a> {
        self.anchor = (col, row);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Menu<'a> {
        self.theme = theme;
        self
    }

    pub fn items(&self) -> &[MenuItem<'a>] {
        &self.items
    }

    /// Cells the widest row needs inside the border: ` label  key `.
    fn content_width(&self) -> usize {
        let items = self
            .items
            .iter()
            .filter(|i| !i.separator)
            .map(|i| {
                let key = i.key.as_deref().map_or(0, |k| text::width(k) + 2);
                text::width(&i.label) + key + 2
            })
            .max()
            .unwrap_or(0);
        let title = self.title.as_deref().map_or(0, |t| text::width(t) + 2);
        items.max(title)
    }

    /// Where the popup goes inside `bounds`: under the anchor row, or above
    /// it when there is no room below; from the anchor column rightwards,
    /// or leftwards when there is no room to the right; clamped to `bounds`.
    pub fn area(&self, bounds: Rect) -> Rect {
        let w = (self.content_width() as u16)
            .saturating_add(2)
            .min(bounds.width);
        let h = (self.items.len() as u16)
            .saturating_add(2)
            .min(bounds.height);
        let (ax, ay) = self.anchor;
        let x = if ax.saturating_add(w) <= bounds.right() {
            ax
        } else if ax.saturating_add(1) >= bounds.x.saturating_add(w) {
            ax + 1 - w
        } else {
            bounds.right() - w
        };
        let below = ay.saturating_add(1);
        let y = if below.saturating_add(h) <= bounds.bottom() {
            below
        } else if ay >= bounds.y.saturating_add(h) {
            ay - h
        } else {
            bounds.bottom() - h
        };
        Rect::new(
            x.clamp(bounds.x, bounds.right() - w),
            y.clamp(bounds.y, bounds.bottom() - h),
            w,
            h,
        )
    }

    fn row_style(&self, item: &MenuItem, selected: bool) -> Style {
        let t = &self.theme;
        let base = if item.disabled {
            t.dim()
        } else if item.destructive {
            t.status(Status::Failed)
        } else {
            t.text()
        };
        if selected {
            let tone = if item.destructive {
                t.status(Status::Failed)
            } else {
                t.accent()
            };
            t.selected().patch(tone).add_modifier(Modifier::BOLD)
        } else {
            base
        }
    }

    /// One action row, exactly `w` cells.
    fn row_line(&self, item: &MenuItem, selected: bool, w: usize) -> Line<'static> {
        let t = &self.theme;
        let style = self.row_style(item, selected);
        let key_style = if selected { style } else { t.dim() };
        let inner = w.saturating_sub(2);
        let key = item.key.as_deref().unwrap_or("");
        let key_w = text::width(key);
        // The label keeps at least half the row; the key gives way first.
        let (label_w, key) = if key_w > 0 && text::width(&item.label) + 2 + key_w <= inner {
            (inner - key_w, key)
        } else {
            (inner, "")
        };
        let mut spans = vec![
            Span::styled(" ", style),
            Span::styled(text::pad(&item.label, label_w, Align::Left), style),
        ];
        if !key.is_empty() {
            spans.push(Span::styled(key.to_string(), key_style));
        }
        if w >= 2 {
            spans.push(Span::styled(" ", style));
        }
        text::truncate_line(Line::from(spans), w)
    }
}

impl StatefulWidget for &Menu<'_> {
    type State = MenuState;

    fn render(self, bounds: Rect, buf: &mut Buffer, state: &mut MenuState) {
        state.layout = MenuLayout::default();
        state.settle(self);
        let area = self.area(bounds);
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        Clear.render(area, buf);
        buf.set_style(area, Style::new().bg(t.surface_raised));
        let mut block = Block::bordered().border_style(t.border());
        if let Some(title) = self.title.as_deref() {
            let title = text::truncate(title, area.width.saturating_sub(4) as usize);
            block = block.title(Span::styled(format!(" {title} "), t.text()));
        }
        let inner = block.inner(area);
        let rows = inner.height as usize;
        let n = self.items.len();
        // Scroll only when the screen is shorter than the menu.
        if rows == 0 || n <= rows {
            state.offset = 0;
        } else {
            state.offset = state
                .offset
                .min(state.selected)
                .max((state.selected + 1).saturating_sub(rows))
                .min(n - rows);
        }
        let hidden = n.saturating_sub(rows);
        if hidden > 0 {
            block = block.title_bottom(
                Line::from(Span::styled(format!(" +{hidden} more "), t.dim())).right_aligned(),
            );
        }
        block.render(area, buf);
        state.layout.area = area;
        state.layout.hidden = hidden;
        for (row, i) in (state.offset..n).take(rows).enumerate() {
            let item = &self.items[i];
            let y = inner.y + row as u16;
            if item.separator {
                // A rule that joins the side borders: ├────┤.
                let rule = format!("├{}┤", "─".repeat(inner.width as usize));
                buf.set_stringn(area.x, y, rule, area.width as usize, t.border());
                continue;
            }
            let line = self.row_line(item, i == state.selected, inner.width as usize);
            buf.set_line(inner.x, y, &line, inner.width);
            if item.enabled() {
                state.layout.items.push(MenuHit {
                    rect: Rect::new(inner.x, y, inner.width, 1),
                    index: i,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    /// llmux's account menu, plus what the port adds: a disabled item, a
    /// separator, a destructive item.
    fn account_menu() -> Menu<'static> {
        Menu::new([
            MenuItem::new("switch now").key("s"),
            MenuItem::new("pause").key("p"),
            MenuItem::new("resume").disabled(true),
            MenuItem::new("set limit").key("L"),
            MenuItem::separator(),
            MenuItem::new("delete").key("r").destructive(true),
        ])
        .title("ai3@iq.io")
        .theme(Theme::ansi())
    }

    fn render(menu: &Menu, state: &mut MenuState, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        menu.render(area, &mut buf, state);
        buf
    }

    #[test]
    fn opens_one_row_below_the_anchor_with_a_rect_per_runnable_item() {
        let menu = account_menu().anchor(4, 2);
        let mut state = MenuState::new(&menu);
        let buf = render(&menu, &mut state, 40, 14);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[3], "    ┌ ai3@iq.io ────┐");
        assert_eq!(lines[4], "    │ switch now  s │");
        assert_eq!(lines[6], "    │ resume        │");
        assert_eq!(lines[7], "    │ set limit   L │");
        assert_eq!(lines[8], "    ├───────────────┤");
        assert_eq!(lines[9], "    │ delete      r │");
        assert_eq!(lines[10], "    └───────────────┘");
        assert_eq!(state.layout.area, Rect::new(4, 3, 17, 8));
        let rows: Vec<(usize, u16)> = state
            .layout
            .items
            .iter()
            .map(|h| (h.index, h.rect.y))
            .collect();
        // No rect for the disabled item or the separator.
        assert_eq!(rows, vec![(0, 4), (1, 5), (3, 7), (5, 9)]);
        assert_eq!(state.hit(5, 5), Some(1));
        assert_eq!(state.hit(18, 9), Some(5));
        assert_eq!(state.hit(10, 6), None, "disabled");
        assert_eq!(state.hit(10, 8), None, "separator");
        assert!(state.layout.contains(10, 8));
        assert!(!state.layout.contains(3, 5));
        // The selected row is the accent, reversed and bold, edge to edge.
        for x in 5..19 {
            let c = &buf[(x, 4)];
            assert_eq!(c.fg, Color::Cyan);
            assert!(c.modifier.contains(Modifier::REVERSED | Modifier::BOLD));
        }
        assert_eq!(buf[(6, 9)].fg, Color::Red, "destructive");
    }

    #[test]
    fn flips_above_and_left_near_the_bottom_right_edge() {
        let menu = account_menu().anchor(38, 12);
        let mut state = MenuState::new(&menu);
        render(&menu, &mut state, 40, 14);
        // 17 × 8: no room below row 12 or right of column 38, so it ends on
        // the row above the anchor and on the anchor column.
        assert_eq!(state.layout.area, Rect::new(22, 4, 17, 8));
        // Room below but not to the right: only the column flips.
        let menu = account_menu().anchor(38, 1);
        render(&menu, &mut state, 40, 14);
        assert_eq!(state.layout.area, Rect::new(22, 2, 17, 8));
        // No room either way: clamped inside the frame.
        let menu = account_menu().anchor(10, 5);
        render(&menu, &mut state, 40, 10);
        assert_eq!(state.layout.area, Rect::new(10, 2, 17, 8));
    }

    #[test]
    fn keys_follow_llmux_and_skip_what_cannot_run() {
        let menu = account_menu();
        let mut s = MenuState::new(&menu);
        assert_eq!(s.selected, 0);
        assert_eq!(s.on_key(&menu, Key::Up.into()), MenuAction::Ignored);
        assert_eq!(s.on_key(&menu, Key::Down.into()), MenuAction::Moved);
        assert_eq!(s.on_key(&menu, 'j'.into()), MenuAction::Moved);
        assert_eq!(s.selected, 3, "skips the disabled `resume`");
        assert_eq!(s.on_key(&menu, Key::Down.into()), MenuAction::Moved);
        assert_eq!(s.selected, 5, "skips the separator");
        assert_eq!(s.on_key(&menu, Key::Down.into()), MenuAction::Ignored);
        assert_eq!(s.on_key(&menu, 'k'.into()), MenuAction::Moved);
        assert_eq!(s.on_key(&menu, Key::Enter.into()), MenuAction::Run(3));
        assert_eq!(s.on_key(&menu, 'p'.into()), MenuAction::Run(1));
        assert_eq!(s.on_key(&menu, Key::End.into()), MenuAction::Moved);
        assert_eq!(s.selected, 5);
        assert_eq!(s.on_key(&menu, Key::Esc.into()), MenuAction::Close);
        assert_eq!(
            s.on_key(&menu, 'x'.into()),
            MenuAction::Close,
            "any other key"
        );
        // A selection left on a disabled item settles on a runnable one.
        let mut s = MenuState {
            selected: 2,
            ..MenuState::default()
        };
        assert_eq!(s.on_key(&menu, Key::Enter.into()), MenuAction::Run(3));
    }

    /// llmux sized the popup by `chars().count()`: a Korean title of four
    /// syllables got four cells for eight.
    #[test]
    fn wide_titles_and_labels_are_measured_in_cells() {
        let menu = Menu::new([
            MenuItem::new("전환"),
            MenuItem::new("삭제").destructive(true),
        ])
        .title("계정메뉴")
        .anchor(0, 0)
        .theme(Theme::ansi());
        let mut state = MenuState::new(&menu);
        let buf = render(&menu, &mut state, 30, 6);
        assert_eq!(state.layout.area.width, 12);
        assert_eq!(snapshot::lines(&buf)[1], "┌ 계정메뉴 ┐");
        assert_eq!(snapshot::lines(&buf)[2], "│ 전환     │");
    }

    #[test]
    fn a_short_screen_scrolls_to_the_selection_and_counts_the_rest() {
        let menu = account_menu().anchor(0, 0);
        let mut state = MenuState::new(&menu);
        state.on_key(&menu, Key::End.into());
        let buf = render(&menu, &mut state, 30, 5);
        let lines = snapshot::lines(&buf);
        assert_eq!(state.layout.hidden, 3);
        assert!(lines[4].contains("+3 more"), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("delete")), "{lines:?}");
        assert_eq!(state.layout.items.last().map(|h| h.index), Some(5));
        // Down to one cell: no panic.
        for (w, h) in [(1, 1), (2, 2), (5, 3), (12, 2)] {
            render(&menu, &mut state, w, h);
        }
    }

    #[test]
    fn mono_keeps_selection_disabled_and_destructive_apart() {
        let menu = account_menu().anchor(0, 0).theme(Theme::mono());
        let mut state = MenuState::new(&menu);
        let buf = render(&menu, &mut state, 30, 12);
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
        }
        let at = |y: u16| buf[(3, y)].modifier;
        assert!(at(2).contains(Modifier::REVERSED), "selected");
        assert!(at(4).contains(Modifier::DIM), "disabled");
        assert!(at(7).contains(Modifier::BOLD), "destructive");
        assert!(!at(3).intersects(Modifier::REVERSED | Modifier::DIM | Modifier::BOLD));
    }
}
