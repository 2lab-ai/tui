// Ported from OpenAI Codex `codex-rs/tui/src/bottom_pane/list_selection_view.rs`
// and `popup_consts.rs` (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: the generic subset — items, current marker,
// disabled reasons, substring search (immediate or on `/`), numbering and
// digit picks, wrapped or single-line rows, ↑/↓ overflow marks and an `N of M`
// position. Actions are returned as an `Outcome` instead of closures over an
// app event channel; tabs, side panels, toggles, hyperlinks and Codex's view
// stack are left behind. Styles come from the `Theme`.

// Tree rows, group toggles, meta columns and the detail line derived from
// herdrdev/herdr `src/client/shell/overlays.rs` (`render_navigator_overlay`)
// and `src/client/shell/sidebar.rs` (`workspace_entries`,
// `render_parent_group_toggle`, `render_workspace_rows`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab:
// connectors come from each item's depth in one backward pass (herdr scans
// per row and handles two levels); a folded group says how many rows it
// hides (`+N`); meta columns are host-defined widths with their own
// thresholds and end in `…` instead of being cut silently at 11 cells; a
// search shows matches with their ancestors; ←/→ fold, unfold and walk the
// tree; glyphs are configurable (`▸`/`▾` are Narrow).

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
//!
//! Trees, from herdr's navigator and workspace sidebar (Apache-2.0), opt in
//! per item: [`SelectItem::depth`] indents a row under the one above it with
//! `├─ └─ │` connectors, [`SelectItem::group`] makes a `▾`/`▸` row whose
//! deeper rows fold away (←/→, or [`SelectList::toggle`]), and a folded group
//! says how many rows it hides. [`SelectList::columns`] adds meta columns at
//! the right that appear by width, and [`SelectItem::detail`] a line under
//! the list for the selected row. A list with neither draws as before.

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
use rows::{Frame, Marks, Paint, Row};

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
    /// Tree depth: 0 for a top row; a deeper row belongs to the nearest
    /// shallower row above it.
    pub depth: u8,
    /// A group row: the deeper rows under it fold away.
    pub group: bool,
    /// A folded group (its rows hidden).
    pub collapsed: bool,
    /// Values for the meta columns ([`SelectList::columns`]), in order.
    pub meta: Vec<String>,
    /// A line shown under the list while this row is selected.
    pub detail: Option<String>,
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

    /// Indent under the nearest shallower row above (0 = top level).
    pub fn depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// A group row, open: `▾`, its deeper rows shown.
    pub fn group(mut self) -> Self {
        self.group = true;
        self
    }

    /// A group row, folded: `▸` and `+N` for the rows it hides.
    pub fn collapsed(mut self) -> Self {
        self.group = true;
        self.collapsed = true;
        self
    }

    /// Values for the meta columns, left to right.
    pub fn meta<S: Into<String>>(mut self, values: impl IntoIterator<Item = S>) -> Self {
        self.meta = values.into_iter().map(Into::into).collect();
        self
    }

    /// A line under the list while this row is selected (a path, an id).
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled_reason.is_some()
    }
}

/// A meta column at the right of every row: `width` cells, shown only when
/// the row is at least `min_width` cells wide (herdr's navigator shows its
/// agent column from 36 cells and its state column from 64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Column {
    pub width: u16,
    pub min_width: u16,
}

impl Column {
    pub const fn new(width: u16) -> Self {
        Self {
            width,
            min_width: 0,
        }
    }

    /// Hidden on rows narrower than this.
    pub const fn min_width(mut self, cells: u16) -> Self {
        self.min_width = cells;
        self
    }
}

/// Tree connectors and group toggles. The toggles change with the state, so
/// they are East-Asian Narrow (`▾` U+25BE, `▸` U+25B8). The box-drawing
/// connectors are Ambiguous (two cells on a CJK terminal set to wide
/// ambiguous glyphs) but they are rules, not state; [`TreeGlyphs::ascii`]
/// avoids them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TreeGlyphs {
    /// A row with a later sibling: `├─ `.
    pub branch: &'static str,
    /// The last row of its parent: `└─ `.
    pub last: &'static str,
    /// An ancestor with a later sibling: `│  `.
    pub pipe: &'static str,
    /// An ancestor without one.
    pub blank: &'static str,
    /// An open group.
    pub open: &'static str,
    /// A folded group.
    pub folded: &'static str,
}

impl Default for TreeGlyphs {
    fn default() -> Self {
        Self {
            branch: "├─ ",
            last: "└─ ",
            pipe: "│  ",
            blank: "   ",
            open: "▾",
            folded: "▸",
        }
    }
}

impl TreeGlyphs {
    /// Plain ASCII: `|- `, `` `- ``, `|  `, `-` and `+`.
    pub const fn ascii() -> Self {
        Self {
            branch: "|- ",
            last: "`- ",
            pipe: "|  ",
            blank: "   ",
            open: "-",
            folded: "+",
        }
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
    tree: TreeGlyphs,
    columns: Vec<Column>,
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
            tree: TreeGlyphs::default(),
            columns: Vec::new(),
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

    pub fn tree_glyphs(mut self, glyphs: TreeGlyphs) -> Self {
        self.tree = glyphs;
        self
    }

    /// Meta columns at the right of every row, left to right; each appears
    /// when the row is at least its `min_width` wide. Values come from
    /// [`SelectItem::meta`].
    pub fn columns(mut self, columns: impl IntoIterator<Item = Column>) -> Self {
        self.columns = columns.into_iter().collect();
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

    /// A search text is filtering: folds are set aside so matches show.
    fn searching(&self) -> bool {
        self.search_visible() && !self.query.is_empty()
    }

    /// Whether item `index` is a folded group.
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.items
            .get(index)
            .is_some_and(|i| i.group && i.collapsed)
    }

    /// Fold or unfold group `index`; returns whether anything changed. The
    /// cursor stays on its item when that is still shown, else moves to the
    /// group.
    pub fn set_collapsed(&mut self, index: usize, collapsed: bool) -> bool {
        match self.items.get_mut(index) {
            Some(item) if item.group && item.collapsed != collapsed => {
                item.collapsed = collapsed;
                let keep = self.selected();
                self.apply_filter();
                if keep.is_some_and(|k| !self.filtered.contains(&k))
                    && let Some(v) = self.filtered.iter().position(|a| *a == index)
                {
                    self.state.selected_idx = Some(v);
                }
                true
            }
            _ => false,
        }
    }

    /// Fold an open group, unfold a folded one (a click on its toggle).
    pub fn toggle(&mut self, index: usize) -> bool {
        let folded = self.is_collapsed(index);
        self.set_collapsed(index, !folded)
    }

    /// The nearest shallower item above `index`.
    fn parent(&self, index: usize) -> Option<usize> {
        let depth = self.items.get(index)?.depth;
        (0..index).rev().find(|&i| self.items[i].depth < depth)
    }

    /// For each item, one past its last descendant (rows deeper than it,
    /// right below it). One pass with a stack.
    fn subtree_ends(&self) -> Vec<usize> {
        let n = self.items.len();
        let mut end = vec![n; n];
        let mut stack: Vec<usize> = Vec::new();
        for (i, item) in self.items.iter().enumerate() {
            while let Some(&top) = stack.last() {
                if self.items[top].depth < item.depth {
                    break;
                }
                end[top] = i;
                stack.pop();
            }
            stack.push(i);
        }
        end
    }

    fn is_tree(&self) -> bool {
        self.items.iter().any(|i| i.depth > 0 || i.group)
    }

    /// ←: fold the selected open group, else go to its parent.
    fn tree_left(&mut self) -> Outcome {
        let Some(a) = self.selected() else {
            return Outcome::Ignored;
        };
        if !self.searching() && self.items[a].group && !self.items[a].collapsed {
            self.set_collapsed(a, true);
            return Outcome::Changed;
        }
        self.select_item(self.parent(a))
    }

    /// →: unfold the selected folded group, else go to its first row.
    fn tree_right(&mut self) -> Outcome {
        let Some(a) = self.selected() else {
            return Outcome::Ignored;
        };
        if !self.searching() && self.is_collapsed(a) {
            self.set_collapsed(a, false);
            return Outcome::Changed;
        }
        let depth = self.items[a].depth;
        let next = self
            .filtered
            .iter()
            .position(|f| *f == a)
            .and_then(|v| self.filtered.get(v + 1).copied())
            .filter(|n| self.items[*n].depth > depth);
        self.select_item(next)
    }

    /// Move the cursor to item `actual` when it is shown and enabled.
    fn select_item(&mut self, actual: Option<usize>) -> Outcome {
        let Some(actual) = actual.filter(|a| self.enabled(*a)) else {
            return Outcome::Ignored;
        };
        match self.filtered.iter().position(|f| *f == actual) {
            Some(v) => {
                self.state.selected_idx = Some(v);
                let (len, page) = (self.filtered.len(), self.page_rows());
                self.state.ensure_visible(len, page);
                Outcome::Changed
            }
            None => Outcome::Ignored,
        }
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
            let hit = |i: &SelectItem| {
                i.search_value
                    .as_deref()
                    .unwrap_or(&i.name)
                    .to_lowercase()
                    .contains(&query)
            };
            // Matches, with the rows they sit under so the tree still reads.
            let mut keep = vec![false; self.items.len()];
            for n in (0..self.items.len()).filter(|&n| hit(&self.items[n])) {
                let mut at = Some(n);
                while let Some(i) = at.filter(|i| !keep[*i]) {
                    keep[i] = true;
                    at = self.parent(i);
                }
            }
            (0..self.items.len()).filter(|&n| keep[n]).collect()
        } else {
            // Everything but the rows under a folded group.
            let mut out = Vec::with_capacity(self.items.len());
            let mut fold: Option<u8> = None;
            for (n, item) in self.items.iter().enumerate() {
                match fold {
                    Some(d) if item.depth > d => continue,
                    _ => fold = None,
                }
                out.push(n);
                if item.group && item.collapsed {
                    fold = Some(item.depth);
                }
            }
            out
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
            Key::Left if self.is_tree() => self.tree_left(),
            Key::Right if self.is_tree() => self.tree_right(),
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

    /// The tree gutter of each shown row: connectors from the depths, in one
    /// backward pass, then the group toggle.
    fn tree_gutters(&self) -> Vec<Vec<Span<'static>>> {
        let len = self.filtered.len();
        if !self.is_tree() {
            return vec![Vec::new(); len];
        }
        let t = &self.theme;
        let g = self.tree;
        let depth = |v: usize| self.items[self.filtered[v]].depth as usize;
        let max = (0..len).map(depth).max().unwrap_or(0);
        // Levels with a group row get a toggle column, so leaves line up.
        let mut toggles = vec![false; max + 1];
        for v in 0..len {
            if self.items[self.filtered[v]].group {
                toggles[depth(v)] = true;
            }
        }
        // `below[l]`: a row at depth l further down, before any shallower one.
        let mut below = vec![false; max + 2];
        let mut out = vec![Vec::new(); len];
        for v in (0..len).rev() {
            let d = depth(v);
            let item = &self.items[self.filtered[v]];
            let mut rule = String::new();
            if d > 0 {
                for open in &below[1..d] {
                    rule.push_str(if *open { g.pipe } else { g.blank });
                }
                rule.push_str(if below[d] { g.branch } else { g.last });
            }
            let mut spans = Vec::new();
            if !rule.is_empty() {
                spans.push(Span::styled(rule, t.dim()));
            }
            if item.group {
                let glyph = if item.collapsed { g.folded } else { g.open };
                spans.push(Span::styled(format!("{glyph} "), t.accent()));
            } else if toggles[d] {
                spans.push(Span::raw(" ".repeat(text::width(g.open) + 1)));
            }
            out[v] = spans;
            below[d] = true;
            below[d + 1..].iter_mut().for_each(|b| *b = false);
        }
        out
    }

    fn build_rows(&self) -> Vec<Row> {
        let trees = self.tree_gutters();
        let ends = self.subtree_ends();
        let folds = !self.searching();
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
            .zip(trees)
            .map(|(&a, tree)| {
                let item = &self.items[a];
                let mut gutter = if self.numbers_shown() {
                    let label = match number_of[a] {
                        Some(n) => format!("{n:>digits$}. "),
                        None => " ".repeat(digits + 2),
                    };
                    vec![Span::styled(label, self.theme.secondary())]
                } else {
                    Vec::new()
                };
                gutter.extend(tree);
                let mut suffix = item.is_current.then(|| " (current)".to_string());
                // A folded group says how many rows it hides.
                let hidden = ends[a] - a - 1;
                if folds && item.group && item.collapsed && hidden > 0 {
                    suffix
                        .get_or_insert_with(String::new)
                        .push_str(&format!(" +{hidden}"));
                }
                Row {
                    gutter,
                    name: item.name.clone(),
                    match_indices: None,
                    suffix,
                    description: item.description.clone(),
                    disabled_reason: item.disabled_reason.clone(),
                    disabled: item.is_disabled(),
                    meta: item.meta.clone(),
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
        if self.items.iter().any(|i| i.group) {
            pairs.push(("←/→", "fold"));
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

    /// The selected row's detail line, or an empty one while some other row
    /// has a detail (the height holds still); `None` when no row has one.
    fn detail(&self) -> Option<String> {
        self.items.iter().any(|i| i.detail.is_some()).then(|| {
            self.selected_item()
                .and_then(|i| i.detail.clone())
                .unwrap_or_default()
        })
    }

    /// The meta columns that show on a row `width` cells wide: each one's
    /// index among all the columns (where its values are in a row's meta)
    /// and its width.
    fn shown_columns(&self, width: u16) -> Vec<(usize, u16)> {
        let cols: Vec<(usize, u16)> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, c)| width >= c.min_width)
            .map(|(i, c)| (i, c.width))
            .collect();
        // Never all of the row: the name keeps a few cells.
        if rows::meta_width(&cols).saturating_add(8) > width {
            Vec::new()
        } else {
            cols
        }
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

impl SelectList {
    /// Rows, their heights and the description column at `width` (what
    /// the meta columns leave of it).
    fn layout(&self, width: u16) -> (Vec<Row>, Vec<u16>, usize) {
        let rows = self.build_rows();
        let body = width - rows::meta_width(&self.shown_columns(width));
        let col = rows::desc_col(&rows, body, self.cursor_w());
        let hs = rows::heights(&rows, col, body, self.display);
        (rows, hs, col)
    }

    fn frame<'a>(
        &'a self,
        rows: &'a [Row],
        heights: &'a [u16],
        col: usize,
        note: Option<String>,
        width: u16,
    ) -> Frame<'a> {
        let t = &self.theme;
        let search = self.search_visible().then(|| {
            let label = (self.search == Search::OnDemand).then_some("/ ");
            rows::search_line(t, label, &self.query, &self.placeholder)
        });
        let mut tail = Vec::new();
        if let Some(note) = note {
            tail.push(Line::styled(note, t.secondary()));
        }
        if let Some(detail) = self.detail() {
            tail.push(Line::styled(detail, t.secondary()));
        }
        tail.push(self.footer_line());
        Frame {
            header: self.header_lines(),
            search,
            rows,
            heights,
            state: &self.state,
            max_rows: self.max_rows,
            paint: Paint {
                theme: t,
                marks: self.glyphs.marks(),
                display: self.display,
                desc_col: col,
                selected: self.state.selected_idx,
                columns: self.shown_columns(width),
            },
            empty: self.empty_line(),
            tail,
        }
    }
}

impl Renderable for SelectList {
    fn desired_height(&self, width: u16) -> u16 {
        let (rows, hs, col) = self.layout(width);
        // Reserve the note row whenever some row could need it, so moving
        // the cursor never changes the height.
        let note =
            (col == 0 && rows.iter().any(|r| r.description_text().is_some())).then(String::new);
        self.frame(&rows, &hs, col, note, width).desired_height()
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let (rows, hs, col) = self.layout(area.width);
        let note = self.note(&rows, col);
        let painted = self
            .frame(&rows, &hs, col, note, area.width)
            .render(area, buf);
        self.shown.set(painted.items);
    }

    /// After the search text, when the search line takes keys.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let (rows, hs, col) = self.layout(area.width);
        let (x, y) = self
            .frame(&rows, &hs, col, None, area.width)
            .search_origin(area)?;
        let label = if self.search == Search::OnDemand {
            2
        } else {
            0
        };
        let x = x + label + text::width(&self.query) as u16;
        (x < area.right()).then_some((x, y))
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

    /// herdr's navigator: machines, workspaces, panes.
    fn navigator() -> Vec<SelectItem> {
        vec![
            SelectItem::new("local").group(),
            SelectItem::new("llmux")
                .depth(1)
                .group()
                .detail("~/2lab.ai/llmux · main ↑2"),
            SelectItem::new("claude")
                .depth(2)
                .meta(["claude-opus-5-5", "working"])
                .detail("pane 1 · tab 2 · 14 turns"),
            SelectItem::new("codex").depth(2).meta(["codex"]),
            SelectItem::new("tui").depth(1).collapsed(),
            SelectItem::new("shell")
                .depth(2)
                .meta(["terminal", "shell"]),
            SelectItem::new("review").depth(2),
            SelectItem::new("studio").group(),
            SelectItem::new("deploy").depth(1).meta(["fx", "done"]),
        ]
    }

    fn tree() -> SelectList {
        SelectList::new(navigator())
            .numbered(false)
            .max_rows(12)
            .theme(Theme::ansi())
    }

    fn herdr_columns() -> [Column; 2] {
        [Column::new(11).min_width(36), Column::new(11).min_width(64)]
    }

    #[test]
    fn tree_rows_draw_connectors_toggles_and_fold_counts() {
        let list = tree();
        let h = list.desired_height(40);
        let lines = draw(&list, 40, h);
        assert_eq!(
            &lines[1..8],
            &[
                "› ▾ local",
                "  ├─ ▾ llmux",
                "  │  ├─ claude",
                "  │  └─ codex",
                "  └─ ▸ tui +2",
                "  ▾ studio",
                "  └─   deploy",
            ]
        );
        assert_eq!(
            list.visible_len(),
            7,
            "the folded group's two rows are hidden"
        );
        assert!(lines.last().is_some_and(|l| l.ends_with("←/→ fold")));
        // The toggles change with the state: Narrow, one cell on CJK.
        for g in [TreeGlyphs::default().open, TreeGlyphs::default().folded] {
            assert_eq!(unicode_width::UnicodeWidthStr::width_cjk(g), 1, "{g}");
        }
        let ascii = tree().tree_glyphs(TreeGlyphs::ascii());
        assert_eq!(draw(&ascii, 40, h)[3], "  |  |- claude");
    }

    #[test]
    fn left_and_right_fold_unfold_and_walk_the_tree() {
        let mut list = tree();
        list.key(Key::Down.into());
        assert_eq!(list.selected(), Some(1), "llmux");
        assert_eq!(list.key(Key::Left.into()), Outcome::Changed);
        assert!(list.is_collapsed(1));
        assert_eq!(list.visible_len(), 5);
        assert!(draw(&list, 40, 12).contains(&"› ├─ ▸ llmux +2".to_string()));
        // ↓ steps over the hidden rows.
        list.key(Key::Down.into());
        assert_eq!(list.selected(), Some(4), "tui");
        // ← on a closed group or a leaf goes to the parent.
        assert_eq!(list.key(Key::Left.into()), Outcome::Changed);
        assert_eq!(list.selected(), Some(0), "local");
        assert_eq!(list.key(Key::Left.into()), Outcome::Changed, "folds local");
        assert_eq!(list.visible_len(), 3);
        assert_eq!(list.key(Key::Left.into()), Outcome::Ignored, "top level");
        // → unfolds, then steps to the first row inside.
        assert_eq!(list.key(Key::Right.into()), Outcome::Changed);
        assert_eq!(list.visible_len(), 5);
        assert_eq!(list.key(Key::Right.into()), Outcome::Changed);
        assert_eq!(list.selected(), Some(1));
        assert_eq!(
            list.key(Key::Right.into()),
            Outcome::Changed,
            "unfolds llmux"
        );
        assert_eq!(list.key(Key::Right.into()), Outcome::Changed);
        assert_eq!(list.selected(), Some(2), "claude");
        assert_eq!(list.key(Key::Right.into()), Outcome::Ignored, "a leaf");
        // A click on a toggle: fold the group the cursor is inside; the
        // cursor moves up to the group.
        assert!(list.toggle(1));
        assert_eq!(list.selected(), Some(1));
        assert!(!list.toggle(2), "not a group");
        // Flat lists ignore ←/→.
        let mut flat = SelectList::new(models());
        assert_eq!(flat.key(Key::Left.into()), Outcome::Ignored);
        assert_eq!(flat.key(Key::Right.into()), Outcome::Ignored);
    }

    #[test]
    fn search_shows_matches_with_the_rows_above_them_through_folds() {
        let list = SelectList::new(navigator())
            .numbered(false)
            .search(Search::Immediate)
            .with_query("review");
        let lines = draw(&list, 40, 10);
        // `review` sits in the folded `tui`: shown with its parents.
        assert_eq!(&lines[2..5], &["› ▾ local", "  └─ ▸ tui", "     └─ review"]);
        assert!(list.is_collapsed(4), "the fold is kept for later");
    }

    // herdr cut meta values at 11 cells without a mark (H8): here they end
    // in `…`, and a missing value says `—`.
    #[test]
    fn meta_columns_appear_by_width_and_never_cut_silently() {
        let list = tree().columns(herdr_columns());
        let row = |w: u16| draw(&list, w, list.desired_height(w))[3].clone();
        assert_eq!(
            row(70),
            "  │  ├─ claude                                 claude-opu… working"
        );
        assert_eq!(
            row(50),
            "  │  ├─ claude                         claude-opu…"
        );
        assert_eq!(row(30), "  │  ├─ claude");
        let lines = draw(&list, 70, 12);
        assert!(lines[4].ends_with("codex       —"), "{:?}", lines[4]);
        assert_eq!(lines[1], "› ▾ local", "no meta, no columns");
        // The selected row's columns share its selection.
        let mut list = list;
        list.key(Key::Down.into());
        list.key(Key::Down.into());
        let area = Rect::new(0, 0, 70, 12);
        let mut buf = Buffer::empty(area);
        Renderable::render(&list, area, &mut buf);
        assert!(buf[(60, 3)].modifier.contains(Modifier::REVERSED));
    }

    /// Review #4: a wide first column hidden at a width where the second
    /// shows. The second column's slot drew the first one's value.
    #[test]
    fn a_shown_column_keeps_its_own_values_when_an_earlier_one_hides() {
        let list = SelectList::new(vec![
            SelectItem::new("claude").meta(["hidden-agent", "visible-state"]),
        ])
        .numbered(false)
        .theme(Theme::ansi())
        .columns([
            Column::new(13).min_width(100),
            Column::new(13).min_width(20),
        ]);
        let lines = draw(&list, 40, list.desired_height(40));
        let row = lines
            .iter()
            .find(|l| l.contains("claude"))
            .expect("the row");
        assert!(row.ends_with("visible-state"), "{row:?}");
        assert!(!row.contains("hidden-agent"), "{row:?}");
        // Both show when the row is wide enough, in their own order.
        let lines = draw(&list, 100, list.desired_height(100));
        let row = lines.iter().find(|l| l.contains("claude")).expect("row");
        assert!(row.ends_with("hidden-agent  visible-state"), "{row:?}");
    }

    #[test]
    fn the_detail_line_follows_the_selection_without_moving_the_footer() {
        let mut list = tree();
        let h = list.desired_height(40);
        let footer = draw(&list, 40, h).len() - 1;
        let detail_row = footer - 1;
        assert_eq!(draw(&list, 40, h)[detail_row], "", "local has no detail");
        list.key(Key::Down.into());
        assert_eq!(
            draw(&list, 40, h)[detail_row],
            "  ~/2lab.ai/llmux · main ↑2"
        );
        list.key(Key::Down.into());
        assert_eq!(
            draw(&list, 40, h)[detail_row],
            "  pane 1 · tab 2 · 14 turns"
        );
        assert_eq!(list.desired_height(40), h);
        // Narrow: cut with `…`.
        assert_eq!(draw(&list, 16, h)[detail_row], "  pane 1 · tab …");
    }

    #[test]
    fn mono_tells_open_and_folded_groups_apart_by_glyph() {
        let list = tree().theme(Theme::mono()).columns(herdr_columns());
        let area = Rect::new(0, 0, 70, 12);
        let mut buf = Buffer::empty(area);
        Renderable::render(&list, area, &mut buf);
        for c in buf.content() {
            assert!(
                matches!(c.fg, ratatui::style::Color::Reset)
                    && matches!(c.bg, ratatui::style::Color::Reset),
                "{c:?}"
            );
        }
        let lines = snapshot::lines(&buf);
        assert!(
            lines[2].contains('▾') && lines[5].contains('▸'),
            "{lines:#?}"
        );
    }
}
