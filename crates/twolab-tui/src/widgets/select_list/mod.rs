// Ported from OpenAI Codex `codex-rs/tui/src/bottom_pane/list_selection_view.rs`
// and `popup_consts.rs` (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: the generic subset — items, current marker,
// disabled reasons, substring search (immediate or on `/`), numbering and
// digit picks, wrapped or single-line rows, ↑/↓ overflow marks and an `N of M`
// position. Actions are returned as an `Outcome` instead of closures over an
// app event channel; tabs, side panels, toggles, hyperlinks and Codex's view
// stack are left behind. Styles come from the `Theme`.

//! A list to pick one item from: which model, which session, which account.
//!
//! Ported from OpenAI Codex `bottom_pane/list_selection_view.rs` (Apache-2.0),
//! the popup behind `/model`, `/approvals` and every other Codex picker. Each
//! row shows `› 1. name (current)` and a description column aligned across
//! rows; disabled rows say why (`disabled: needs a Max plan`) and are skipped
//! by the cursor. A search line filters by substring. When rows are out of
//! view, `▴`/`▾` marks and an `N of M` position say so. An empty filter says
//! `no match for "…"` instead of showing nothing.
//!
//! Keys: ↑/↓ (Ctrl-P/N) move and wrap, PgUp/PgDn and Home/End clamp, Enter
//! picks, Esc goes back, digits 1–9 pick the Nth enabled row when the search
//! line is not taking keys, `/` opens an on-demand search.

#[cfg(feature = "catalog")]
pub(crate) mod docs;
pub(crate) mod rows;

use std::cell::Cell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

pub use rows::RowDisplay;
use rows::{Marks, Paint, PickerSizes, Row};

use crate::input::{Key, KeyInput};
use crate::render::Renderable;
use crate::scroll::{ListAction, ScrollState};
use crate::text;
use crate::theme::Theme;

/// Rows shown at once unless the host says otherwise (Codex's
/// `MAX_POPUP_ROWS`).
pub const DEFAULT_MAX_ROWS: usize = 8;

/// One choice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectItem {
    pub name: String,
    pub description: Option<String>,
    /// The item in effect now; marked `(current)` and selected first.
    pub is_current: bool,
    /// Why the item cannot be picked. `Some` disables it.
    pub disabled_reason: Option<String>,
    /// What the search matches against; the name when `None`.
    pub search_value: Option<String>,
}

impl SelectItem {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Grey text in the description column.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Mark as the item in effect now.
    pub fn current(mut self) -> Self {
        self.is_current = true;
        self
    }

    /// Disable, saying why.
    pub fn disabled(mut self, reason: impl Into<String>) -> Self {
        self.disabled_reason = Some(reason.into());
        self
    }

    /// Match the search against this text instead of the name.
    pub fn search_value(mut self, value: impl Into<String>) -> Self {
        self.search_value = Some(value.into());
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled_reason.is_some()
    }
}

/// Whether the list has a search line, and when it takes keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Search {
    /// No search: digits pick rows.
    #[default]
    Off,
    /// The search line is always there and every printable key types into it.
    Immediate,
    /// `/` opens the search line; Esc closes it. Digits pick until then.
    OnDemand,
}

/// The cursor and overflow marks. Every default glyph is East-Asian Narrow,
/// so the gutter stays one cell on a CJK terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectGlyphs {
    pub cursor: &'static str,
    pub more_above: &'static str,
    pub more_below: &'static str,
}

impl Default for SelectGlyphs {
    /// `›` (U+203A), `▴` (U+25B4), `▾` (U+25BE) — all Narrow.
    fn default() -> Self {
        Self {
            cursor: "›",
            more_above: "▴",
            more_below: "▾",
        }
    }
}

impl SelectGlyphs {
    /// Plain ASCII: `>`, `^`, `v`.
    pub const fn ascii() -> Self {
        Self {
            cursor: ">",
            more_above: "^",
            more_below: "v",
        }
    }

    /// Codex's `↑`/`↓`. These arrows are East-Asian *Ambiguous*: two cells
    /// on a CJK terminal set to wide ambiguous glyphs. Opt in knowingly.
    pub const fn arrows() -> Self {
        Self {
            cursor: "›",
            more_above: "↑",
            more_below: "↓",
        }
    }

    fn marks(self) -> Marks {
        Marks {
            cursor: self.cursor,
            more_above: self.more_above,
            more_below: self.more_below,
        }
    }
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a key this list uses.
    Ignored,
    /// The cursor or the search changed; redraw.
    Changed,
    /// The person picked the item at this index into [`SelectList::items`].
    Accepted(usize),
    /// The person backed out (Esc, or Enter on an empty list).
    Cancelled,
}

impl Outcome {
    /// Whether anything visible changed.
    pub fn redraw(self) -> bool {
        !matches!(self, Outcome::Ignored)
    }
}

/// A list to pick one item from. Owns its cursor, scroll and search; draw it
/// with `Widget for &SelectList` or as a [`Renderable`].
#[derive(Debug, Clone)]
pub struct SelectList {
    items: Vec<SelectItem>,
    title: Option<String>,
    subtitle: Option<String>,
    footer: Option<Line<'static>>,
    search: Search,
    search_active: bool,
    placeholder: String,
    query: String,
    empty_message: String,
    numbered: bool,
    max_rows: usize,
    display: RowDisplay,
    glyphs: SelectGlyphs,
    theme: Theme,
    filtered: Vec<usize>,
    state: ScrollState,
    /// Items the last paint showed — the page size for PgUp/PgDn.
    shown: Cell<usize>,
}

impl SelectList {
    pub fn new(items: Vec<SelectItem>) -> Self {
        let mut list = Self {
            items,
            title: None,
            subtitle: None,
            footer: None,
            search: Search::Off,
            search_active: false,
            placeholder: "Type to search".into(),
            query: String::new(),
            empty_message: "nothing to choose from".into(),
            numbered: true,
            max_rows: DEFAULT_MAX_ROWS,
            display: RowDisplay::Wrapped,
            glyphs: SelectGlyphs::default(),
            theme: Theme::default(),
            filtered: Vec::new(),
            state: ScrollState::new(),
            shown: Cell::new(0),
        };
        list.apply_filter();
        list
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Replace the default key-hint footer.
    pub fn footer(mut self, footer: Line<'static>) -> Self {
        self.footer = Some(footer);
        self
    }

    pub fn search(mut self, search: Search) -> Self {
        self.search = search;
        self.search_active = search == Search::Immediate;
        self.apply_filter();
        self
    }

    /// Dim text in an empty search line.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// What an empty list (no items at all) says.
    pub fn empty_message(mut self, message: impl Into<String>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Number enabled rows `1.`, `2.`, … and let digits pick them.
    pub fn numbered(mut self, numbered: bool) -> Self {
        self.numbered = numbered;
        self
    }

    /// Rows shown at once (at least one).
    pub fn max_rows(mut self, rows: usize) -> Self {
        self.max_rows = rows.max(1);
        self.apply_filter();
        self
    }

    pub fn row_display(mut self, display: RowDisplay) -> Self {
        self.display = display;
        self
    }

    pub fn glyphs(mut self, glyphs: SelectGlyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Start with this search text (opens an on-demand search).
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.set_search_query(query);
        self
    }

    pub fn items(&self) -> &[SelectItem] {
        &self.items
    }

    /// Index into [`SelectList::items`] of the highlighted row.
    pub fn selected(&self) -> Option<usize> {
        self.state
            .selected_idx
            .and_then(|v| self.filtered.get(v).copied())
    }

    pub fn selected_item(&self) -> Option<&SelectItem> {
        self.selected().and_then(|i| self.items.get(i))
    }

    /// Rows that pass the search.
    pub fn visible_len(&self) -> usize {
        self.filtered.len()
    }

    pub fn search_query(&self) -> &str {
        &self.query
    }

    /// Replace the search text and refilter (opens an on-demand search).
    pub fn set_search_query(&mut self, query: impl Into<String>) {
        if self.search == Search::Off {
            return;
        }
        self.query = query.into();
        self.search_active = true;
        self.apply_filter();
    }

    fn search_visible(&self) -> bool {
        self.search != Search::Off && self.search_active
    }

    fn enabled(&self, actual: usize) -> bool {
        self.items.get(actual).is_some_and(|i| !i.is_disabled())
    }

    fn visible_disabled(&self, visible: usize) -> bool {
        self.filtered
            .get(visible)
            .is_some_and(|actual| !self.enabled(*actual))
    }

    fn first_enabled_visible(&self) -> Option<usize> {
        self.filtered.iter().position(|a| self.enabled(*a))
    }

    fn page_rows(&self) -> usize {
        let shown = self.shown.get();
        let limit = if shown > 0 {
            self.max_rows.min(shown)
        } else {
            self.max_rows
        };
        limit.min(self.filtered.len().max(1))
    }

    /// Filter by the search text and keep the cursor on the same item when
    /// it survives, else on the current item, else on the first enabled row.
    fn apply_filter(&mut self) {
        let before = self.selected().filter(|a| self.enabled(*a));
        let current = (!self.search_visible() || self.query.is_empty())
            .then(|| {
                self.items
                    .iter()
                    .position(|i| i.is_current && !i.is_disabled())
            })
            .flatten();
        let query = self.query.to_lowercase();
        self.filtered = if self.search_visible() && !query.is_empty() {
            self.items
                .iter()
                .enumerate()
                .filter(|(_, i)| {
                    i.search_value
                        .as_deref()
                        .unwrap_or(&i.name)
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|(n, _)| n)
                .collect()
        } else {
            (0..self.items.len()).collect()
        };
        let find = |a: usize| self.filtered.iter().position(|f| *f == a);
        self.state.selected_idx = before
            .and_then(find)
            .or_else(|| current.and_then(find))
            .or_else(|| self.first_enabled_visible())
            .or_else(|| (!self.filtered.is_empty()).then_some(0));
        let len = self.filtered.len();
        let page = self.page_rows();
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, page);
    }

    fn skip_disabled(&mut self, down: bool, wrap: bool) {
        let len = self.filtered.len();
        let Some(start) = self.state.selected_idx else {
            return;
        };
        if !self.visible_disabled(start) {
            return;
        }
        let next = if wrap {
            (1..len)
                .map(|k| {
                    if down {
                        (start + k) % len
                    } else {
                        (start + len - k) % len
                    }
                })
                .find(|i| !self.visible_disabled(*i))
        } else {
            let fwd = || ((start + 1)..len).find(|i| !self.visible_disabled(*i));
            let back = || (0..start).rev().find(|i| !self.visible_disabled(*i));
            if down {
                fwd().or_else(back)
            } else {
                back().or_else(fwd)
            }
        };
        self.state.selected_idx = next.or(Some(start));
    }

    fn navigate(&mut self, action: ListAction) -> Outcome {
        let before = self.state;
        let len = self.filtered.len();
        let page = self.page_rows();
        self.state.apply(action, len, page);
        match action {
            ListAction::MoveUp => self.skip_disabled(false, true),
            ListAction::MoveDown => self.skip_disabled(true, true),
            ListAction::PageUp | ListAction::JumpBottom => self.skip_disabled(false, false),
            ListAction::PageDown | ListAction::JumpTop => self.skip_disabled(true, false),
        }
        self.state.ensure_visible(len, page);
        if self.state == before {
            Outcome::Ignored
        } else {
            Outcome::Changed
        }
    }

    fn accept(&self) -> Outcome {
        match self.selected() {
            Some(a) if self.enabled(a) => Outcome::Accepted(a),
            Some(_) => Outcome::Ignored,
            None => Outcome::Cancelled,
        }
    }

    /// The Nth enabled item (1-based), counting every item.
    fn enabled_number(&self, n: usize) -> Option<usize> {
        (n > 0)
            .then(|| {
                self.items
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| !i.is_disabled())
                    .nth(n - 1)
                    .map(|(a, _)| a)
            })
            .flatten()
    }

    fn edit_query(&mut self, edit: impl FnOnce(&mut String)) -> Outcome {
        let before = self.query.clone();
        edit(&mut self.query);
        if self.query == before {
            return Outcome::Ignored;
        }
        self.apply_filter();
        Outcome::Changed
    }

    /// Act on a key.
    pub fn key(&mut self, key: KeyInput) -> Outcome {
        if let Some(action) = ListAction::from_key(key) {
            return self.navigate(action);
        }
        let on_demand_active = self.search == Search::OnDemand && self.search_active;
        match key.key {
            Key::Esc if on_demand_active => {
                self.search_active = false;
                self.query.clear();
                self.apply_filter();
                Outcome::Changed
            }
            Key::Esc => Outcome::Cancelled,
            Key::Enter => self.accept(),
            Key::Backspace if self.search_visible() => self.edit_query(|q| {
                q.pop();
            }),
            _ => match key.typed() {
                Some('/') if self.search == Search::OnDemand && !self.search_active => {
                    self.search_active = true;
                    Outcome::Changed
                }
                Some(c) if self.search_visible() && !c.is_control() => {
                    self.edit_query(|q| q.push(c))
                }
                Some(c) if self.numbered && !self.search_visible() => c
                    .to_digit(10)
                    .and_then(|d| self.enabled_number(d as usize))
                    .map_or(Outcome::Ignored, |a| {
                        if let Some(v) = self.filtered.iter().position(|f| *f == a) {
                            self.state.selected_idx = Some(v);
                        }
                        Outcome::Accepted(a)
                    }),
                _ => Outcome::Ignored,
            },
        }
    }

    fn numbers_shown(&self) -> bool {
        self.numbered && !self.search_visible()
    }

    fn build_rows(&self) -> Vec<Row> {
        let enabled_total = self.items.iter().filter(|i| !i.is_disabled()).count();
        let digits = enabled_total.max(1).to_string().len();
        let number_of: Vec<Option<usize>> = {
            let mut n = 0;
            self.items
                .iter()
                .map(|i| {
                    (!i.is_disabled()).then(|| {
                        n += 1;
                        n
                    })
                })
                .collect()
        };
        self.filtered
            .iter()
            .map(|&a| {
                let item = &self.items[a];
                let gutter = if self.numbers_shown() {
                    let label = match number_of[a] {
                        Some(n) => format!("{n:>digits$}. "),
                        None => " ".repeat(digits + 2),
                    };
                    vec![Span::styled(label, self.theme.secondary())]
                } else {
                    Vec::new()
                };
                Row {
                    gutter,
                    name: item.name.clone(),
                    match_indices: None,
                    suffix: item.is_current.then(|| " (current)".to_string()),
                    description: item.description.clone(),
                    disabled_reason: item.disabled_reason.clone(),
                    disabled: item.is_disabled(),
                }
            })
            .collect()
    }

    fn header_lines(&self) -> Vec<Line<'static>> {
        let t = &self.theme;
        let mut v = Vec::new();
        if let Some(title) = &self.title {
            v.push(Line::styled(
                title.clone(),
                t.text().add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(sub) = &self.subtitle {
            v.push(Line::styled(sub.clone(), t.secondary()));
        }
        v
    }

    fn footer_line(&self) -> Line<'static> {
        if let Some(f) = &self.footer {
            return f.clone();
        }
        let mut pairs = vec![("enter", "select"), ("esc", "back")];
        if self.search == Search::OnDemand && self.search_active {
            pairs[1] = ("esc", "stop searching");
        }
        if self.numbers_shown() && self.items.iter().any(|i| !i.is_disabled()) {
            pairs.push(("1-9", "pick"));
        }
        if self.search == Search::OnDemand && !self.search_active {
            pairs.push(("/", "search"));
        }
        rows::hint_line(&self.theme, &pairs)
    }

    /// The selected row's description, when the width hides the column.
    fn note(&self, rows: &[Row], desc_col: usize) -> Option<String> {
        if desc_col > 0 {
            return None;
        }
        self.state
            .selected_idx
            .and_then(|v| rows.get(v))
            .and_then(Row::description_text)
    }

    fn cursor_w(&self) -> usize {
        text::width(self.glyphs.cursor)
    }

    fn empty_line(&self) -> Line<'static> {
        let msg = if self.query.is_empty() {
            self.empty_message.clone()
        } else {
            format!("no match for \"{}\"", self.query)
        };
        Line::styled(msg, self.theme.dim().add_modifier(Modifier::ITALIC))
    }
}

/// Inner width for the header, search and footer (two cells each side).
const INSET: u16 = 2;

impl Renderable for SelectList {
    fn desired_height(&self, width: u16) -> u16 {
        let rows = self.build_rows();
        let col = rows::desc_col(&rows, width, self.cursor_w());
        let hs = rows::heights(&rows, col, width, self.display);
        let header = self.header_lines().len() as u16;
        let list = rows::stable_lines(&hs, self.max_rows);
        let note = u16::from(col == 0 && rows.iter().any(|r| r.description_text().is_some()));
        header + u16::from(header > 0) + u16::from(self.search_visible()) + 2 + list + note + 1
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.shown.set(0);
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        let rows = self.build_rows();
        let col = rows::desc_col(&rows, area.width, self.cursor_w());
        let hs = rows::heights(&rows, col, area.width, self.display);
        let note = self.note(&rows, col);
        let header = self.header_lines();
        let search = u16::from(self.search_visible());

        // The footer and the note yield before the title, search and the
        // selected row.
        let minimum = (header.len() as u16).min(1) + search + 1;
        let mut tail = 1 + u16::from(note.is_some());
        tail = tail.min(area.height.saturating_sub(minimum));
        let body = Rect {
            height: area.height - tail,
            ..area
        };
        let list_lines = rows::stable_lines(&hs, self.max_rows);
        let [head_a, _, search_a, above, list_a, below] = rows::picker_areas(
            body,
            PickerSizes {
                header: header.len() as u16,
                header_gap: 1,
                search,
                rows: list_lines,
            },
        );
        let inset = |r: Rect| Rect {
            x: r.x.saturating_add(INSET),
            width: r.width.saturating_sub(INSET),
            ..r
        };
        for (y, line) in (head_a.y..head_a.bottom()).zip(header) {
            Renderable::render(
                &line,
                inset(Rect {
                    y,
                    height: 1,
                    ..head_a
                }),
                buf,
            );
        }
        if !search_a.is_empty() {
            let label = (self.search == Search::OnDemand).then_some("/ ");
            let line = rows::search_line(t, label, &self.query, &self.placeholder);
            Renderable::render(&line, inset(search_a), buf);
        }
        if !list_a.is_empty() {
            if rows.is_empty() {
                Renderable::render(&self.empty_line(), inset(list_a), buf);
            } else {
                let start = rows::window_start(&hs, &self.state, self.max_rows, list_a.height);
                let painted = rows::paint_rows(
                    list_a,
                    buf,
                    &rows,
                    &hs,
                    start,
                    self.max_rows,
                    &Paint {
                        theme: t,
                        marks: self.glyphs.marks(),
                        display: self.display,
                        desc_col: col,
                        selected: self.state.selected_idx,
                    },
                );
                self.shown.set(painted.items);
                let position = self.state.selected_idx.map(|s| (s + 1, rows.len()));
                rows::paint_overflow(above, below, buf, painted, position, t, self.glyphs.marks());
            }
        }
        let mut y = body.bottom();
        if let Some(note) = note
            && tail > 1
        {
            let line = Line::styled(note, t.secondary());
            Renderable::render(&line, inset(Rect::new(area.x, y, area.width, 1)), buf);
            y += 1;
        }
        if tail > 0 && y < area.bottom() {
            Renderable::render(
                &self.footer_line(),
                inset(Rect::new(area.x, y, area.width, 1)),
                buf,
            );
        }
    }

    /// After the search text, when the search line takes keys.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        if !self.search_visible() || area.is_empty() {
            return None;
        }
        let header = self.header_lines().len() as u16;
        let y = area.y + header + u16::from(header > 0);
        let label = if self.search == Search::OnDemand {
            2
        } else {
            0
        };
        let x = area.x + INSET + label + text::width(&self.query) as u16;
        (y < area.bottom() && x < area.right()).then_some((x, y))
    }
}

impl Widget for &SelectList {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Renderable::render(self, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    fn models() -> Vec<SelectItem> {
        vec![
            SelectItem::new("claude-opus-5-5")
                .description("1M context, $4 / $20 per Mtok")
                .current(),
            SelectItem::new("claude-sonnet-5-5").description("1M context, $2 / $10 per Mtok"),
            SelectItem::new("claude-fable-5-1")
                .description("1M context, $10 / $50 per Mtok")
                .disabled("not on this plan"),
            SelectItem::new("claude-haiku-4-5").description("200k context, $1 / $5 per Mtok"),
        ]
    }

    fn draw(list: &SelectList, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Renderable::render(list, area, &mut buf);
        snapshot::lines(&buf)
    }

    #[test]
    fn model_picker_layout() {
        let list = SelectList::new(models()).title("Select model");
        let h = list.desired_height(72);
        assert_eq!(
            draw(&list, 72, h),
            vec![
                "  Select model",
                "",
                "",
                "› 1. claude-opus-5-5 (current)    1M context, $4 / $20 per Mtok",
                "  2. claude-sonnet-5-5            1M context, $2 / $10 per Mtok",
                "     claude-fable-5-1 (disabled)  1M context, $10 / $50 per Mtok",
                "                                  (disabled: not on this plan)",
                "  3. claude-haiku-4-5             200k context, $1 / $5 per Mtok",
                "",
                "  enter select  esc back  1-9 pick",
            ]
        );
    }

    // Upstream: `disabled_current_rows_skip_default_selection_and_number_shortcuts`.
    #[test]
    fn digits_pick_the_nth_enabled_item_and_skip_disabled() {
        let mut list = SelectList::new(models());
        assert_eq!(list.selected(), Some(0));
        assert_eq!(list.key('3'.into()), Outcome::Accepted(3));
        assert_eq!(list.key('9'.into()), Outcome::Ignored);
        let mut list = SelectList::new(models());
        list.key(Key::Down.into());
        assert_eq!(list.selected(), Some(1));
        list.key(Key::Down.into());
        assert_eq!(list.selected(), Some(3), "the disabled row is skipped");
        list.key(Key::Down.into());
        assert_eq!(list.selected(), Some(0), "down wraps");
        list.key(Key::Up.into());
        assert_eq!(list.selected(), Some(3), "up wraps");
        assert_eq!(list.key(Key::Enter.into()), Outcome::Accepted(3));
        assert_eq!(list.key(Key::Esc.into()), Outcome::Cancelled);
    }

    #[test]
    fn disabled_current_item_is_not_selected_first() {
        let items = vec![
            SelectItem::new("a").current().disabled("gone"),
            SelectItem::new("b"),
        ];
        assert_eq!(SelectList::new(items).selected(), Some(1));
    }

    #[test]
    fn current_item_is_selected_first() {
        let items = vec![SelectItem::new("a"), SelectItem::new("b").current()];
        assert_eq!(SelectList::new(items).selected(), Some(1));
    }

    // Upstream: `enter_with_no_matches_triggers_cancel_callback`.
    #[test]
    fn empty_filter_is_honest_and_enter_cancels() {
        let mut list = SelectList::new(models())
            .search(Search::Immediate)
            .with_query("gemini");
        assert_eq!(list.visible_len(), 0);
        let text = draw(&list, 50, 8).join("\n");
        assert!(text.contains("no match for \"gemini\""), "{text}");
        assert!(text.contains("gemini"), "{text}");
        assert_eq!(list.key(Key::Enter.into()), Outcome::Cancelled);
        for _ in 0..6 {
            list.key(Key::Backspace.into());
        }
        assert_eq!(list.visible_len(), 4);
    }

    // Upstream: `searchable_plain_j_updates_query_instead_of_navigating` (multi-select).
    #[test]
    fn immediate_search_types_digits_and_letters() {
        let mut list = SelectList::new(models()).search(Search::Immediate);
        assert_eq!(list.key('4'.into()), Outcome::Changed);
        assert_eq!(list.search_query(), "4");
        assert_eq!(list.visible_len(), 1);
        assert_eq!(list.selected(), Some(3));
        assert_eq!(list.key('j'.into()), Outcome::Changed);
        assert_eq!(list.visible_len(), 0);
    }

    #[test]
    fn on_demand_search_opens_on_slash_and_closes_on_esc() {
        let mut list = SelectList::new(models()).search(Search::OnDemand);
        assert_eq!(list.key('/'.into()), Outcome::Changed);
        for c in "son".chars() {
            list.key(c.into());
        }
        assert_eq!(list.visible_len(), 1);
        assert_eq!(list.selected(), Some(1));
        assert_eq!(list.key(Key::Esc.into()), Outcome::Changed);
        assert_eq!(list.visible_len(), 4);
        assert_eq!(list.selected(), Some(1), "the cursor stays on the item");
        assert_eq!(list.key('1'.into()), Outcome::Accepted(0));
    }

    #[test]
    fn long_lists_scroll_and_say_where_they_are() {
        let items: Vec<SelectItem> = (1..=30)
            .map(|i| SelectItem::new(format!("session {i:02}")))
            .collect();
        let mut list = SelectList::new(items).max_rows(5);
        let h = list.desired_height(40);
        let first = draw(&list, 40, h);
        assert!(
            first
                .iter()
                .any(|l| l.starts_with('▾') && l.ends_with("1 of 30")),
            "{first:#?}"
        );
        list.key(Key::End.into());
        let last = draw(&list, 40, h);
        assert!(last.iter().any(|l| l.starts_with('▴')), "{last:#?}");
        assert!(last.iter().any(|l| l.ends_with("30 of 30")), "{last:#?}");
        assert!(
            last.iter().any(|l| l.starts_with("› 30. session 30")),
            "{last:#?}"
        );
        // PgUp moves by the rows shown.
        list.key(Key::PageUp.into());
        assert_eq!(list.selected(), Some(24));
    }

    #[test]
    fn selected_row_takes_the_theme_selection_and_mono_has_no_colour() {
        for theme in [Theme::ansi(), Theme::dark(), Theme::mono()] {
            let list = SelectList::new(models()).theme(theme);
            let area = Rect::new(0, 0, 72, 9);
            let mut buf = Buffer::empty(area);
            Renderable::render(&list, area, &mut buf);
            let sel = theme.selected();
            // No title: the overflow spacer is row 0, the cursor row is 1.
            let cell = &buf[(4, 1)];
            assert_eq!(cell.bg, sel.bg.unwrap_or(cell.bg));
            if sel.add_modifier.contains(Modifier::REVERSED) {
                assert!(cell.modifier.contains(Modifier::REVERSED));
            }
            if theme.is_mono() {
                for c in buf.content() {
                    assert!(
                        matches!(c.fg, ratatui::style::Color::Reset)
                            && matches!(c.bg, ratatui::style::Color::Reset),
                        "{c:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn narrow_width_moves_the_selected_description_below() {
        let list = SelectList::new(models());
        let lines = draw(&list, 40, 9);
        assert!(
            lines.contains(&"› 1. claude-opus-5-5 (current)".to_string()),
            "{lines:#?}"
        );
        assert!(
            lines.contains(&"  1M context, $4 / $20 per Mtok".to_string()),
            "{lines:#?}"
        );
    }

    #[test]
    fn tiny_areas_keep_the_selected_row() {
        let list = SelectList::new(models()).title("Select model");
        for h in 1..=6 {
            let lines = draw(&list, 30, h);
            if h >= 2 {
                assert!(
                    lines.iter().any(|l| l.starts_with("› 1.")),
                    "h={h}: {lines:#?}"
                );
            }
        }
        let _ = draw(&list, 1, 1);
    }

    #[test]
    fn cursor_sits_after_the_search_text() {
        let list = SelectList::new(models())
            .title("t")
            .search(Search::Immediate)
            .with_query("op");
        assert_eq!(list.cursor_pos(Rect::new(0, 0, 40, 10)), Some((4, 2)));
        assert_eq!(
            SelectList::new(models()).cursor_pos(Rect::new(0, 0, 40, 10)),
            None
        );
    }
}
