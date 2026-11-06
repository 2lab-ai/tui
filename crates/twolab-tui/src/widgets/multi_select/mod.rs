// Ported from OpenAI Codex `codex-rs/tui/src/bottom_pane/multi_select_picker.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: keys return an `Outcome` instead of firing
// callbacks over an app event channel; matched characters are highlighted
// (Codex computes the positions and drops them); a built-in `k of n
// selected` line replaces the preview callback; an unmet "choose at least
// one" says so instead of ignoring Enter silently; section breaks and the
// configurable keymap are left behind. Styles come from the `Theme`.

//! Which of these to include: a checkbox list with a fuzzy filter.
//!
//! Ported from OpenAI Codex `bottom_pane/multi_select_picker.rs`
//! (Apache-2.0), the picker behind Codex's status-line and feature
//! settings. Each row is `› [x] name   description`; typing filters by a
//! case-insensitive subsequence match (ported with its tests in
//! [`fuzzy_match`]) and the matched characters are highlighted; Space
//! toggles the highlighted row, Enter applies, Esc cancels. A `k of n
//! selected` line keeps the count in view while the filter hides rows.
//! With [`MultiSelect::reorderable`], ←/→ move the highlighted item up or
//! down the list (order matters for, say, an account rotation).

#[cfg(feature = "catalog")]
pub(crate) mod docs;
mod fuzzy;

use std::cell::Cell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

pub use fuzzy::fuzzy_match;

use crate::input::{Key, KeyInput};
use crate::render::Renderable;
use crate::scroll::{ListAction, ScrollState};
use crate::select_list::rows::{self, Frame, Marks, Paint, Row};
use crate::select_list::{RowDisplay, SelectGlyphs};
use crate::text;
use crate::theme::{Level, Theme};

/// Rows shown at once unless the host says otherwise (Codex's
/// `MAX_POPUP_ROWS`).
pub const DEFAULT_MAX_ROWS: usize = 8;

/// One item that can be included or left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiItem {
    /// Returned in [`Outcome::Applied`] when checked.
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub checked: bool,
    /// Whether ←/→ can move it (and move others across it).
    pub orderable: bool,
}

impl MultiItem {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            checked: false,
            orderable: true,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Keep this item where it is when the list is reordered.
    pub fn pinned(mut self) -> Self {
        self.orderable = false;
        self
    }
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not a key this picker uses.
    Ignored,
    /// The cursor, a checkbox, the order or the filter changed; redraw.
    Changed,
    /// Enter: the ids of every checked item, in list order.
    Applied(Vec<String>),
    /// Esc: leave without applying.
    Cancelled,
}

impl Outcome {
    /// Whether anything visible changed.
    pub fn redraw(&self) -> bool {
        !matches!(self, Outcome::Ignored)
    }
}

/// A checkbox list with a fuzzy filter. Owns its items, cursor and filter;
/// draw it with `Widget for &MultiSelect` or as a [`Renderable`].
#[derive(Debug, Clone)]
pub struct MultiSelect {
    items: Vec<MultiItem>,
    title: Option<String>,
    subtitle: Option<String>,
    placeholder: String,
    footer: Option<Line<'static>>,
    query: String,
    /// `(index into items, matched char positions)` in display order.
    filtered: Vec<(usize, Vec<usize>)>,
    state: ScrollState,
    require_selection: bool,
    reorderable: bool,
    /// Enter was refused because nothing is checked.
    refused: bool,
    max_rows: usize,
    display: RowDisplay,
    glyphs: SelectGlyphs,
    theme: Theme,
    shown: Cell<usize>,
}

impl MultiSelect {
    pub fn new(items: Vec<MultiItem>) -> Self {
        let mut picker = Self {
            items,
            title: None,
            subtitle: None,
            placeholder: "Type to search".into(),
            footer: None,
            query: String::new(),
            filtered: Vec::new(),
            state: ScrollState::new(),
            require_selection: false,
            reorderable: false,
            refused: false,
            max_rows: DEFAULT_MAX_ROWS,
            display: RowDisplay::SingleLine,
            glyphs: SelectGlyphs::default(),
            theme: Theme::default(),
            shown: Cell::new(0),
        };
        picker.apply_filter();
        picker
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Dim text in the empty search line.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Replace the default key-hint footer.
    pub fn footer(mut self, footer: Line<'static>) -> Self {
        self.footer = Some(footer);
        self
    }

    /// Refuse Enter while nothing is checked (Esc still cancels).
    pub fn require_selection(mut self) -> Self {
        self.require_selection = true;
        self
    }

    /// Let ←/→ (Ctrl-H/Ctrl-L) move the highlighted item up or down while
    /// the filter is empty.
    pub fn reorderable(mut self) -> Self {
        self.reorderable = true;
        self
    }

    /// Rows shown at once (at least one).
    pub fn max_rows(mut self, rows: usize) -> Self {
        self.max_rows = rows.max(1);
        self.apply_filter();
        self
    }

    /// One line per row (the default) or wrapped descriptions.
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

    /// Start with this filter text.
    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.set_search_query(query);
        self
    }

    /// Replace the filter text and refilter.
    pub fn set_search_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.apply_filter();
    }

    pub fn search_query(&self) -> &str {
        &self.query
    }

    /// Every item, in the current order.
    pub fn items(&self) -> &[MultiItem] {
        &self.items
    }

    /// Ids of the checked items, in list order.
    pub fn checked_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.checked)
            .map(|i| i.id.clone())
            .collect()
    }

    /// Index into [`MultiSelect::items`] of the highlighted row.
    pub fn selected(&self) -> Option<usize> {
        self.state
            .selected_idx
            .and_then(|v| self.filtered.get(v))
            .map(|(a, _)| *a)
    }

    /// Rows that pass the filter.
    pub fn visible_len(&self) -> usize {
        self.filtered.len()
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

    /// Applies the current search query to filter and sort items.
    ///
    /// Updates `filtered` to contain only matching items, sorted by fuzzy
    /// match score (then name). Attempts to preserve the current selection
    /// if it still matches the filter.
    fn apply_filter(&mut self) {
        let previously_selected = self.selected();
        let filter = self.query.trim();
        self.filtered = if filter.is_empty() {
            (0..self.items.len()).map(|i| (i, Vec::new())).collect()
        } else {
            let mut matches: Vec<(usize, Vec<usize>, i32)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    fuzzy_match(&item.name, filter).map(|(idx, score)| (i, idx, score))
                })
                .collect();
            matches.sort_by(|a, b| {
                a.2.cmp(&b.2)
                    .then_with(|| self.items[a.0].name.cmp(&self.items[b.0].name))
            });
            matches.into_iter().map(|(i, idx, _)| (i, idx)).collect()
        };
        let len = self.filtered.len();
        self.state.selected_idx = previously_selected
            .and_then(|a| self.filtered.iter().position(|(f, _)| *f == a))
            .or_else(|| (len > 0).then_some(0));
        let page = self.page_rows();
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, page);
    }

    /// Toggles the checkbox of the highlighted item.
    fn toggle_selected(&mut self) -> Outcome {
        let Some(a) = self.selected() else {
            return Outcome::Ignored;
        };
        self.items[a].checked = !self.items[a].checked;
        self.refused = false;
        Outcome::Changed
    }

    /// Collects the ids of all checked items, unless a selection is
    /// required and nothing is checked.
    fn confirm_selection(&mut self) -> Outcome {
        if self.require_selection && !self.items.iter().any(|i| i.checked) {
            self.refused = true;
            return Outcome::Changed;
        }
        Outcome::Applied(self.checked_ids())
    }

    /// Moves the highlighted item up or down the underlying list.
    ///
    /// Only works while the filter is empty and ordering is enabled; a
    /// pinned item neither moves nor lets another cross it.
    fn move_selected_item(&mut self, up: bool) -> Outcome {
        if !self.query.is_empty() {
            return Outcome::Ignored;
        }
        let Some(a) = self.selected() else {
            return Outcome::Ignored;
        };
        let len = self.items.len();
        if !self.items[a].orderable {
            return Outcome::Ignored;
        }
        let b = match up {
            true if a > 0 => a - 1,
            false if a + 1 < len => a + 1,
            _ => return Outcome::Ignored,
        };
        if !self.items[b].orderable {
            return Outcome::Ignored;
        }
        self.items.swap(a, b);
        self.apply_filter();
        if let Some(v) = self.filtered.iter().position(|(f, _)| *f == b) {
            self.state.selected_idx = Some(v);
            let page = self.page_rows();
            self.state.ensure_visible(self.filtered.len(), page);
        }
        Outcome::Changed
    }

    /// Act on a key. Printable characters always feed the filter; Space
    /// toggles.
    pub fn key(&mut self, key: KeyInput) -> Outcome {
        if self.reorderable {
            let ctrl = key.mods.ctrl && !key.mods.alt;
            match key.key {
                Key::Left => return self.move_selected_item(true),
                Key::Right => return self.move_selected_item(false),
                Key::Char('h') if ctrl => return self.move_selected_item(true),
                Key::Char('l') if ctrl => return self.move_selected_item(false),
                _ => {}
            }
        }
        if let Some(action) = ListAction::from_key(key) {
            let before = self.state;
            let len = self.filtered.len();
            let page = self.page_rows();
            self.state.apply(action, len, page);
            return if self.state == before {
                Outcome::Ignored
            } else {
                Outcome::Changed
            };
        }
        match key.key {
            Key::Enter => self.confirm_selection(),
            Key::Esc => Outcome::Cancelled,
            Key::Backspace => {
                if self.query.pop().is_some() {
                    self.apply_filter();
                    Outcome::Changed
                } else {
                    Outcome::Ignored
                }
            }
            _ => match key.typed() {
                Some(' ') => self.toggle_selected(),
                Some(c) if !c.is_control() => {
                    self.query.push(c);
                    self.apply_filter();
                    Outcome::Changed
                }
                _ => Outcome::Ignored,
            },
        }
    }

    fn build_rows(&self) -> Vec<Row> {
        let t = &self.theme;
        self.filtered
            .iter()
            .map(|(a, matches)| {
                let item = &self.items[*a];
                let (mark, style) = if item.checked {
                    ("[x] ", t.accent().add_modifier(Modifier::BOLD))
                } else {
                    ("[ ] ", t.dim())
                };
                Row {
                    gutter: vec![Span::styled(mark, style)],
                    name: item.name.clone(),
                    match_indices: (!matches.is_empty()).then(|| matches.clone()),
                    suffix: None,
                    description: item.description.clone(),
                    disabled_reason: None,
                    disabled: false,
                    meta: Vec::new(),
                }
            })
            .collect()
    }

    fn summary_line(&self) -> Line<'static> {
        let t = &self.theme;
        let k = self.items.iter().filter(|i| i.checked).count();
        let mut spans = vec![Span::styled(
            format!("{k} of {} selected", self.items.len()),
            t.secondary(),
        )];
        if self.refused {
            spans.push(Span::styled(
                "  choose at least one to apply",
                t.level(Level::Warn),
            ));
        } else if !self.query.is_empty() && self.filtered.len() < self.items.len() {
            spans.push(Span::styled(
                format!(
                    "  ({} hidden by the filter)",
                    self.items.len() - self.filtered.len()
                ),
                t.dim(),
            ));
        }
        Line::from(spans)
    }

    fn footer_line(&self) -> Line<'static> {
        if let Some(f) = &self.footer {
            return f.clone();
        }
        let mut pairs = vec![("space", "toggle")];
        if self.reorderable && self.query.is_empty() {
            pairs.push(("left/right", "reorder"));
        }
        pairs.extend([("enter", "apply"), ("esc", "cancel")]);
        rows::hint_line(&self.theme, &pairs)
    }

    fn layout(&self, width: u16) -> (Vec<Row>, Vec<u16>, usize) {
        let rows = self.build_rows();
        let col = rows::desc_col(&rows, width, text::width(self.glyphs.cursor));
        let hs = rows::heights(&rows, col, width, self.display);
        (rows, hs, col)
    }

    fn frame<'a>(&'a self, rows: &'a [Row], heights: &'a [u16], col: usize) -> Frame<'a> {
        let t = &self.theme;
        let mut header = Vec::new();
        if let Some(title) = &self.title {
            header.push(Line::styled(
                title.clone(),
                t.text().add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(sub) = &self.subtitle {
            header.push(Line::styled(sub.clone(), t.secondary()));
        }
        let empty = if self.items.is_empty() {
            "nothing to choose from".to_string()
        } else {
            format!("no match for \"{}\"", self.query)
        };
        Frame {
            header,
            search: Some(rows::search_line(t, None, &self.query, &self.placeholder)),
            rows,
            heights,
            state: &self.state,
            max_rows: self.max_rows,
            paint: Paint {
                theme: t,
                marks: Marks {
                    cursor: self.glyphs.cursor,
                    more_above: self.glyphs.more_above,
                    more_below: self.glyphs.more_below,
                },
                display: self.display,
                desc_col: col,
                selected: self.state.selected_idx,
                columns: Vec::new(),
            },
            empty: Line::styled(empty, t.dim().add_modifier(Modifier::ITALIC)),
            tail: vec![self.summary_line(), self.footer_line()],
        }
    }
}

impl Renderable for MultiSelect {
    fn desired_height(&self, width: u16) -> u16 {
        let (rows, hs, col) = self.layout(width);
        self.frame(&rows, &hs, col).desired_height()
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (rows, hs, col) = self.layout(area.width);
        let painted = self.frame(&rows, &hs, col).render(area, buf);
        self.shown.set(painted.items);
    }

    /// After the filter text.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let (rows, hs, col) = self.layout(area.width);
        let (x, y) = self.frame(&rows, &hs, col).search_origin(area)?;
        let x = x + text::width(&self.query) as u16;
        (x < area.right()).then_some((x, y))
    }
}

impl Widget for &MultiSelect {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Renderable::render(self, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    fn item(id: &str, orderable: bool) -> MultiItem {
        let i = MultiItem::new(id, id);
        if orderable { i } else { i.pinned() }
    }

    fn ids(p: &MultiSelect) -> Vec<&str> {
        p.items().iter().map(|i| i.id.as_str()).collect()
    }

    fn draw(p: &MultiSelect, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Renderable::render(p, area, &mut buf);
        buf
    }

    // Upstream: `non_orderable_items_cannot_move_or_be_crossed`.
    #[test]
    fn non_orderable_items_cannot_move_or_be_crossed() {
        let mut picker = MultiSelect::new(vec![
            item("theme-colors", false),
            item("model", true),
            item("branch", true),
        ])
        .reorderable();

        picker.key(Key::Right.into());
        assert_eq!(ids(&picker), vec!["theme-colors", "model", "branch"]);

        picker.key(Key::Down.into());
        picker.key(Key::Left.into());
        assert_eq!(ids(&picker), vec!["theme-colors", "model", "branch"]);
    }

    // Upstream: `horizontal_list_keys_reorder_orderable_items`.
    #[test]
    fn horizontal_list_keys_reorder_orderable_items() {
        let mut picker =
            MultiSelect::new(vec![item("model", true), item("branch", true)]).reorderable();

        picker.key(KeyInput::ctrl(Key::Char('l')));
        assert_eq!(ids(&picker), vec!["branch", "model"]);
        assert_eq!(picker.selected(), Some(1), "the cursor follows the item");

        picker.key(KeyInput::ctrl(Key::Char('h')));
        assert_eq!(ids(&picker), vec!["model", "branch"]);
    }

    // Upstream: `searchable_plain_j_updates_query_instead_of_navigating`.
    #[test]
    fn searchable_plain_j_updates_query_instead_of_navigating() {
        let mut picker = MultiSelect::new(vec![item("alpha", true), item("jupiter", true)]);

        picker.key(KeyInput::from('j'));

        assert_eq!(picker.search_query(), "j");
        assert_eq!(picker.visible_len(), 1);
        assert_eq!(picker.selected(), Some(1));
    }

    // Upstream: `page_and_jump_navigation_use_list_keymap` (default bindings).
    #[test]
    fn page_and_jump_navigation() {
        let mut picker = MultiSelect::new(
            (0..12)
                .map(|idx| item(&format!("item-{idx}"), false))
                .collect(),
        );
        picker.key(Key::PageDown.into());
        assert_eq!(picker.selected(), Some(8));
        picker.key(Key::PageUp.into());
        assert_eq!(picker.selected(), Some(0));
        picker.key(Key::End.into());
        assert_eq!(picker.selected(), Some(11));
        picker.key(Key::Home.into());
        assert_eq!(picker.selected(), Some(0));
    }

    // Upstream: `short_picker_preserves_selected_row_when_overflow_spacers_yield`.
    #[test]
    fn short_picker_preserves_selected_row_when_overflow_spacers_yield() {
        let mut picker = MultiSelect::new(
            (0..12)
                .map(|index| item(&format!("field-{index:02}"), true))
                .collect(),
        );
        picker.key(Key::End.into());
        for height in 1..=7 {
            let lines = snapshot::lines(&draw(&picker, 80, height));
            if height >= 2 {
                assert!(
                    lines.iter().any(|l| l == "› [ ] field-11"),
                    "h={height}: {lines:#?}"
                );
            }
            assert_eq!(picker.selected(), Some(11));
        }
        assert_eq!(picker.key(KeyInput::from(' ')), Outcome::Changed);
        assert_eq!(picker.checked_ids(), vec!["field-11"]);
    }

    #[test]
    fn space_toggles_enter_applies_esc_cancels() {
        let mut picker = MultiSelect::new(vec![
            item("a", true).checked(true),
            item("b", true),
            item("c", true),
        ]);
        picker.key(Key::Down.into());
        picker.key(KeyInput::from(' '));
        assert_eq!(
            picker.key(Key::Enter.into()),
            Outcome::Applied(vec!["a".into(), "b".into()])
        );
        assert_eq!(picker.key(Key::Esc.into()), Outcome::Cancelled);
    }

    #[test]
    fn required_selection_says_why_enter_did_nothing() {
        let mut picker = MultiSelect::new(vec![item("a", true)]).require_selection();
        assert_eq!(picker.key(Key::Enter.into()), Outcome::Changed);
        let text = snapshot::text(&draw(&picker, 60, 8));
        assert!(
            text.contains("0 of 1 selected  choose at least one"),
            "{text}"
        );
        picker.key(KeyInput::from(' '));
        assert_eq!(
            picker.key(Key::Enter.into()),
            Outcome::Applied(vec!["a".into()])
        );
    }

    #[test]
    fn filter_sorts_by_score_and_highlights_matches() {
        let mut picker = MultiSelect::new(vec![
            MultiItem::new("read", "read_file"),
            MultiItem::new("fetch", "web_fetch"),
            MultiItem::new("bash", "bash"),
        ]);
        picker.key(KeyInput::from('f'));
        picker.key(KeyInput::from('e'));
        // web_fetch matches "fe" contiguously; read_file is spread.
        let order: Vec<usize> = picker.filtered.iter().map(|(a, _)| *a).collect();
        assert_eq!(order, vec![1, 0]);
        let buf = draw(&picker, 60, 8);
        let lines = snapshot::lines(&buf);
        let y = lines.iter().position(|l| l.contains("web_fetch")).unwrap() as u16;
        // The cursor stays on read_file. In "  [ ] web_fetch" the name starts
        // at cell 6, so the matched 'f' and 'e' are cells 10 and 11.
        assert_eq!(picker.selected(), Some(0));
        for x in [10u16, 11] {
            assert!(buf[(x, y)].modifier.contains(Modifier::BOLD), "x={x}");
        }
        assert!(!buf[(9, y)].modifier.contains(Modifier::BOLD));
        let text = snapshot::text(&buf);
        assert!(text.contains("(1 hidden by the filter)"), "{text}");
        assert_eq!(picker.key(KeyInput::from('z')), Outcome::Changed);
        assert!(snapshot::text(&draw(&picker, 60, 8)).contains("no match for \"fez\""));
    }

    #[test]
    fn reorder_is_off_while_filtering() {
        let mut picker =
            MultiSelect::new(vec![item("model", true), item("branch", true)]).reorderable();
        picker.key(KeyInput::from('b'));
        assert_eq!(picker.key(Key::Right.into()), Outcome::Ignored);
        assert_eq!(ids(&picker), vec!["model", "branch"]);
    }
}
