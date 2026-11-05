// Derived from vercel-labs/fx `src/ui/footer/picker_presentation.zig`
// (`slashMenuLayoutForCounts`, `composeSlashMenuHeaderRow`,
// `composeSlashMenuRow`), `src/core/shared/list_window.zig`
// (`updateEdgeStart`, `advanceSelection`) and the ASCII case folding of
// `src/core/shared/fuzzy_match.zig` at 412d841d73dd6e1b8de551022728d9c929caae17
// (Apache-2.0, Copyright 2025 Vercel, Inc.); translated from Zig to Rust.
// Ranking, windowing and tests also lifted from 2lab-ai/xfx `src/tui/picker.rs`
// at 93f516b537adf52db208e3a343d9229c5f3d40e2 (Apache-2.0, 2lab).
// Modified by 2lab: generic items, case-insensitive matching, the matched
// part highlighted, a `+N more` row, an empty state, ratatui output.

//! The commands matching what you typed.
//!
//! Ported from fx's slash menu (`src/ui/footer/picker_presentation.zig`,
//! Apache-2.0) by way of xfx's `src/tui/picker.rs`. A dim header says how many
//! match (`Commands 12 · type to filter`) and, when they overflow, which are in
//! view; then one row per match: two cells of indent, the label padded to the
//! widest label in view, the description in dim, and an optional right column
//! (a category, a skill's source) that goes first when the row is narrow.
//! The marked row is bright and bold — fx marks it by weight alone, no caret.
//! The part of each label the query matched is in the accent colour.
//! Rows past the window are counted on a last `+N more` row.
//!
//! Matching is xfx's three tiers — the label **is** the query, **begins**
//! with it, or **contains** it after the sigil — ASCII case-insensitive, as
//! fx folds case. A query that starts with a sigil (`/`, `$`, `@`) matches
//! only labels that start with the same one; within a tier the items keep
//! their order, so the list is a pure function of the query.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::text;
use crate::theme::Theme;

/// The most item rows shown at once (fx `list_window.zig`,
/// `default_max_picker_rows`).
pub const MAX_ROWS: u16 = 6;

/// The header and the blank row under it.
const HEADER_ROWS: usize = 2;
/// Every row's indent, the same for the marked one: there is no caret.
const INDENT: usize = 2;
/// Cells between the label and the description, and before the right column.
const COLUMN_GAP: usize = 3;
/// The least description a row keeps before it gives up its right column.
const MIN_DESCRIPTION: usize = 24;

/// One completion: a label such as `/model`, what it does, and an optional
/// right-hand column (category, source).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub description: String,
    pub meta: String,
}

impl Item {
    pub fn new(label: impl Into<String>, description: impl Into<String>) -> Item {
        Item {
            label: label.into(),
            description: description.into(),
            meta: String::new(),
        }
    }

    pub fn meta(mut self, meta: impl Into<String>) -> Item {
        self.meta = meta.into();
        self
    }
}

/// How well a label answers a query, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rank {
    Exact,
    Prefix,
    Substring,
}

/// One item the query found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Index into the items.
    pub index: usize,
    pub rank: Rank,
    /// The bytes of the label the query matched.
    pub hit: Range<usize>,
}

/// A sigil leads a label (`/model`, `$review`) and is not part of its name.
fn is_sigil(c: char) -> bool {
    c.is_ascii_punctuation()
}

fn starts_with_ci(hay: &str, needle: &str) -> bool {
    hay.len() >= needle.len()
        && hay.as_bytes()[..needle.len()].eq_ignore_ascii_case(needle.as_bytes())
}

/// The first char boundary where `needle` occurs in `hay`, ASCII letters
/// compared without case and everything else exactly.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.len();
    hay.char_indices().map(|(i, _)| i).find(|&i| {
        hay.len() - i >= n && hay.as_bytes()[i..i + n].eq_ignore_ascii_case(needle.as_bytes())
    })
}

/// How well `label` answers `query`, and which bytes of the label matched.
///
/// A query led by a sigil matches only labels led by the same sigil, and the
/// substring tier looks for what follows it: `/e` is the letter `e` somewhere
/// after a label's slash. A query of just the sigil matches every such label.
pub fn rank(label: &str, query: &str) -> Option<(Rank, Range<usize>)> {
    let query_sigil = query.chars().next().filter(|&c| is_sigil(c));
    let label_sigil = label.chars().next().filter(|&c| is_sigil(c));
    // `off` is where the name starts in the label; `from` is where a hit
    // starts when it reaches back to the label's first byte.
    let (name, q, off, from) = match (query_sigil, label_sigil) {
        (Some(qs), Some(ls)) if qs == ls => (&label[1..], &query[1..], 1, 0),
        (Some(_), _) => return None,
        (None, Some(ls)) => (&label[ls.len_utf8()..], query, ls.len_utf8(), ls.len_utf8()),
        (None, None) => (label, query, 0, 0),
    };
    if q.is_empty() {
        return Some((Rank::Prefix, from..off));
    }
    if name.len() == q.len() && starts_with_ci(name, q) {
        return Some((Rank::Exact, from..off + q.len()));
    }
    if starts_with_ci(name, q) {
        return Some((Rank::Prefix, from..off + q.len()));
    }
    find_ci(name, q).map(|i| (Rank::Substring, off + i..off + i + q.len()))
}

/// Every item `query` finds: exact, then prefix, then substring, each tier
/// in the items' own order.
pub fn matches(items: &[Item], query: &str) -> Vec<Match> {
    let mut found: Vec<Match> = items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            rank(&item.label, query).map(|(rank, hit)| Match { index, rank, hit })
        })
        .collect();
    // Stable: ties keep the items' order.
    found.sort_by_key(|m| m.rank);
    found
}

/// Which match is marked and where the window starts. The window scrolls
/// only when the mark reaches its edge (fx `list_window.zig`); a new query
/// puts the mark back on the first match.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuState {
    selected: usize,
    window_start: usize,
    query: String,
}

impl MenuState {
    pub fn new() -> MenuState {
        MenuState::default()
    }

    /// The marked match, as an index into [`CompletionMenu::matches`].
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The first match in view, as the last render left it.
    pub fn window_start(&self) -> usize {
        self.window_start
    }

    pub fn reset(&mut self) {
        self.selected = 0;
        self.window_start = 0;
    }
}

/// Where a window of `rows` over `count` starts once `selected` is in it,
/// moving from `current` only as far as it has to (fx `updateEdgeStart`).
fn edge_start(current: usize, count: usize, selected: usize, rows: usize) -> usize {
    if count == 0 || rows == 0 || count <= rows {
        return 0;
    }
    let selected = selected.min(count - 1);
    let start = current.min(count - rows);
    if selected < start {
        selected
    } else if selected >= start + rows {
        selected + 1 - rows
    } else {
        start
    }
}

/// The menu's shape in a given height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shape {
    header: bool,
    rows: usize,
    /// A `+N more` row under the items.
    more_row: bool,
    /// `+N` at the end of the only item row (one-row menus).
    more_tail: bool,
}

/// The completion menu.
#[derive(Debug, Clone)]
pub struct CompletionMenu<'a> {
    items: &'a [Item],
    query: &'a str,
    theme: Theme,
    title: String,
    max_rows: u16,
    empty: Option<String>,
}

impl<'a> CompletionMenu<'a> {
    pub fn new(items: &'a [Item], theme: Theme) -> CompletionMenu<'a> {
        CompletionMenu {
            items,
            query: "",
            theme,
            title: "Commands".to_string(),
            max_rows: MAX_ROWS,
            empty: None,
        }
    }

    /// What was typed, sigil included: `/mo`.
    pub fn query(mut self, query: &'a str) -> CompletionMenu<'a> {
        self.query = query;
        self
    }

    /// The header's noun: `Commands 12`.
    pub fn title(mut self, title: impl Into<String>) -> CompletionMenu<'a> {
        self.title = title.into();
        self
    }

    /// The most item rows at once (at least one).
    pub fn max_rows(mut self, rows: u16) -> CompletionMenu<'a> {
        self.max_rows = rows.max(1);
        self
    }

    /// What the menu says when nothing matches; default `nothing matches {query}`.
    pub fn empty(mut self, text: impl Into<String>) -> CompletionMenu<'a> {
        self.empty = Some(text.into());
        self
    }

    pub fn matches(&self) -> Vec<Match> {
        matches(self.items, self.query)
    }

    /// Rows the whole menu wants: header, blank, the window and `+N more`.
    pub fn height(&self) -> u16 {
        let n = self.matches().len();
        if n == 0 {
            return 1;
        }
        let rows = n.min(usize::from(self.max_rows));
        let more = usize::from(n > rows);
        u16::try_from(HEADER_ROWS + rows + more).unwrap_or(u16::MAX)
    }

    /// A state that belongs to another query starts over.
    fn sync(&self, state: &mut MenuState, count: usize) {
        if state.query != self.query {
            state.query = self.query.to_string();
            state.reset();
        }
        state.selected = state.selected.min(count.saturating_sub(1));
    }

    /// Mark the next match, wrapping past the last.
    pub fn next(&self, state: &mut MenuState) {
        let n = self.matches().len();
        self.sync(state, n);
        if n > 0 {
            state.selected = (state.selected + 1) % n;
        }
    }

    /// Mark the previous match, wrapping past the first.
    pub fn previous(&self, state: &mut MenuState) {
        let n = self.matches().len();
        self.sync(state, n);
        if n > 0 {
            state.selected = (state.selected + n - 1) % n;
        }
    }

    /// The marked item: what Tab completes and Enter runs.
    pub fn selected(&self, state: &MenuState) -> Option<&'a Item> {
        let found = self.matches();
        let i = if state.query == self.query {
            state.selected
        } else {
            0
        };
        found.get(i).or(found.first()).map(|m| &self.items[m.index])
    }

    fn shape(&self, count: usize, height: usize) -> Shape {
        // fx: the header only when the budget is more than two rows.
        let header = height > HEADER_ROWS;
        let avail = if header { height - HEADER_ROWS } else { height };
        let mut rows = avail.min(usize::from(self.max_rows)).min(count);
        let mut more_row = false;
        let mut more_tail = false;
        if count > rows {
            if avail > rows {
                more_row = true;
            } else if rows >= 2 {
                rows -= 1;
                more_row = true;
            } else {
                more_tail = !header;
            }
        }
        Shape {
            header,
            rows,
            more_row,
            more_tail,
        }
    }

    /// The menu as lines for an area `width` × `height`; updates the window.
    pub fn lines(&self, width: u16, height: u16, state: &mut MenuState) -> Vec<Line<'static>> {
        let (w, h) = (usize::from(width), usize::from(height));
        let found = self.matches();
        let n = found.len();
        self.sync(state, n);
        if w == 0 || h == 0 {
            return Vec::new();
        }
        let t = &self.theme;
        if n == 0 {
            let empty = self
                .empty
                .clone()
                .unwrap_or_else(|| format!("nothing matches {}", self.query));
            let line = Line::from(vec![
                Span::raw(" ".repeat(INDENT.min(w))),
                Span::styled(empty, t.dim()),
            ]);
            return vec![text::truncate_line(line, w)];
        }
        let shape = self.shape(n, h);
        state.window_start = edge_start(state.window_start, n, state.selected, shape.rows);
        let start = state.window_start;
        let in_view = &found[start..start + shape.rows];
        let mut out = Vec::with_capacity(h);
        if shape.header {
            out.push(self.header(n, start, shape.rows, w));
            out.push(Line::from(""));
        }
        let label_col = in_view
            .iter()
            .map(|m| text::width(&self.items[m.index].label))
            .max()
            .unwrap_or(0);
        let meta_col = in_view
            .iter()
            .map(|m| text::width(&self.items[m.index].meta))
            .max()
            .unwrap_or(0);
        let hidden = n - shape.rows;
        for (k, m) in in_view.iter().enumerate() {
            let marked = start + k == state.selected;
            let mut line = self.item_line(m, marked, label_col, meta_col, w);
            if shape.more_tail {
                let tail = format!(" +{hidden}");
                let tail_w = text::width(&tail);
                if w > tail_w + INDENT {
                    line = text::truncate_line(line, w - tail_w);
                    let pad = w - tail_w - text::line_width(&line);
                    line.spans.push(Span::raw(" ".repeat(pad)));
                    line.spans.push(Span::styled(tail, t.dim()));
                }
            }
            out.push(line);
        }
        if shape.more_row {
            let more = Line::from(vec![
                Span::raw(" ".repeat(INDENT)),
                Span::styled(format!("+{hidden} more"), t.dim()),
            ]);
            out.push(text::truncate_line(more, w));
        }
        out
    }

    /// `Commands 12 · type to filter`, and `1-6` against the right edge while
    /// the matches overflow (fx `composeSlashMenuHeaderRow`).
    fn header(&self, count: usize, start: usize, rows: usize, w: usize) -> Line<'static> {
        let dim = self.theme.dim();
        let bare = self.query.chars().count() <= 1 && self.query.chars().all(is_sigil);
        let left = if bare {
            format!("{} {count} · type to filter", self.title)
        } else {
            format!("{} {count}", self.title)
        };
        let range = if count > rows {
            format!("{}-{}", start + 1, start + rows)
        } else {
            String::new()
        };
        let range_w = text::width(&range);
        let room = range_w > 0 && w > range_w + 2;
        let left_w = if room { w - range_w - 2 } else { w };
        let left = text::truncate(&left, left_w).into_owned();
        let mut spans = vec![Span::styled(left, dim)];
        if room {
            let pad = w - range_w - text::spans_width(&spans);
            spans.push(Span::raw(" ".repeat(pad)));
            spans.push(Span::styled(range, dim));
        }
        Line::from(spans)
    }

    /// One row (fx `composeSlashMenuRow`): indent, label padded to the label
    /// column, description, and the right column while the description keeps
    /// [`MIN_DESCRIPTION`] cells beside it.
    fn item_line(
        &self,
        m: &Match,
        marked: bool,
        label_col: usize,
        meta_col: usize,
        w: usize,
    ) -> Line<'static> {
        let t = &self.theme;
        let item = &self.items[m.index];
        let bold = |s: Style| {
            if marked {
                s.add_modifier(Modifier::BOLD)
            } else {
                s
            }
        };
        let label_style = if marked {
            t.text().add_modifier(Modifier::BOLD)
        } else {
            t.secondary()
        };
        let mut hit_style = bold(t.accent());
        if t.is_mono() {
            hit_style = hit_style.add_modifier(Modifier::UNDERLINED);
        }
        let desc_style = bold(t.dim());

        if w <= INDENT {
            return Line::from(" ".repeat(w));
        }
        let label = &item.label;
        let hit = m.hit.start.min(label.len())..m.hit.end.min(label.len());
        let parts = [
            (&label[..hit.start], label_style),
            (&label[hit.clone()], hit_style),
            (&label[hit.end..], label_style),
        ];
        let label_line = Line::from(
            parts
                .iter()
                .filter(|(s, _)| !s.is_empty())
                .map(|(s, st)| Span::styled(s.to_string(), *st))
                .collect::<Vec<_>>(),
        );
        let label_line = text::truncate_line(label_line, w - INDENT);
        let label_w = text::line_width(&label_line);
        let mut spans = vec![Span::raw(" ".repeat(INDENT))];
        spans.extend(label_line.spans);

        let desc_start = w.min(INDENT + label_col.max(label_w) + COLUMN_GAP);
        let used = INDENT + label_w;
        if desc_start > used {
            spans.push(Span::raw(" ".repeat(desc_start - used)));
        }
        let meta = item.meta.as_str();
        let meta_start = w.saturating_sub(meta_col);
        let meta_fits = !meta.is_empty() && meta_col > 0 && meta_col <= w;
        let show_meta = if item.description.is_empty() {
            meta_fits && meta_start >= desc_start + COLUMN_GAP
        } else {
            meta_fits && meta_start >= desc_start + MIN_DESCRIPTION + COLUMN_GAP
        };
        let desc_w = if show_meta {
            meta_start - desc_start - COLUMN_GAP
        } else {
            w - desc_start
        };
        if !item.description.is_empty() && desc_w > 0 {
            let desc = text::truncate(&item.description, desc_w).into_owned();
            spans.push(Span::styled(desc, desc_style));
        }
        if show_meta {
            let used = text::spans_width(&spans);
            spans.push(Span::raw(" ".repeat(meta_start.saturating_sub(used))));
            spans.push(Span::styled(meta.to_string(), desc_style));
        }
        text::truncate_line(Line::from(spans), w)
    }
}

impl StatefulWidget for &CompletionMenu<'_> {
    type State = MenuState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut MenuState) {
        if area.is_empty() {
            return;
        }
        for (i, line) in self
            .lines(area.width, area.height, state)
            .iter()
            .enumerate()
        {
            let y = area.y + u16::try_from(i).unwrap_or(u16::MAX);
            if y >= area.bottom() {
                break;
            }
            buf.set_line(area.x, y, line, area.width);
        }
    }
}

impl StatefulWidget for CompletionMenu<'_> {
    type State = MenuState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut MenuState) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl Widget for CompletionMenu<'_> {
    /// The menu with the first match marked.
    fn render(self, area: Rect, buf: &mut Buffer) {
        StatefulWidget::render(&self, area, buf, &mut MenuState::new());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    fn commands() -> Vec<Item> {
        [
            ("/help", "show available slash commands", "General"),
            ("/clear", "clear the conversation", "General"),
            ("/new", "start a fresh session", "Session"),
            ("/resume", "resume a saved session", "Session"),
            ("/rename", "rename the current session", "Session"),
            ("/model", "choose the model and reasoning effort", "Model"),
            (
                "/permissions",
                "choose what runs without asking",
                "Settings",
            ),
            ("/usage", "show token usage and spend", "Usage"),
            ("/quit", "exit the interactive shell", "General"),
        ]
        .into_iter()
        .map(|(l, d, m)| Item::new(l, d).meta(m))
        .collect()
    }

    fn labels(items: &[Item], query: &str) -> Vec<String> {
        matches(items, query)
            .iter()
            .map(|m| items[m.index].label.clone())
            .collect()
    }

    fn draw(menu: &CompletionMenu, w: u16, h: u16, state: &mut MenuState) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        StatefulWidget::render(menu, buf.area, &mut buf, state);
        buf
    }

    #[test]
    fn an_exact_name_ranks_before_a_prefix_before_a_substring() {
        // xfx's THREE_TIERS, without the alias.
        let items: Vec<Item> = ["/annex", "/settle", "/ex", "/exercise", "/exit"]
            .into_iter()
            .map(|l| Item::new(l, ""))
            .collect();
        assert_eq!(
            labels(&items, "/ex"),
            ["/ex", "/exercise", "/exit", "/annex"]
        );
        let ranks: Vec<Rank> = matches(&items, "/ex").iter().map(|m| m.rank).collect();
        assert_eq!(
            ranks,
            [Rank::Exact, Rank::Prefix, Rank::Prefix, Rank::Substring]
        );
        // The slash is not a letter of any name: a second one finds nothing.
        assert!(matches(&items, "//").is_empty());
        // A bare slash finds them all, in order.
        assert_eq!(labels(&items, "/").len(), 5);
    }

    #[test]
    fn matching_folds_ascii_case_and_respects_the_sigil() {
        let items = commands();
        assert_eq!(labels(&items, "/MO"), ["/model"]);
        assert_eq!(rank("/Model", "/mo"), Some((Rank::Prefix, 0..3)));
        assert_eq!(rank("/model", "/MODEL"), Some((Rank::Exact, 0..6)));
        // Substring: the hit is where the stem sits in the label.
        assert_eq!(rank("/permissions", "/MISS"), Some((Rank::Substring, 4..8)));
        // A different sigil is a different menu.
        assert_eq!(rank("/review", "$re"), None);
        assert_eq!(rank("$review", "$re"), Some((Rank::Prefix, 0..3)));
        // A query without a sigil reads past the label's.
        assert_eq!(rank("/model", "mo"), Some((Rank::Prefix, 1..3)));
        // Non-ASCII is compared exactly, and never split.
        assert_eq!(rank("/한글", "/한"), Some((Rank::Prefix, 0..4)));
        assert_eq!(rank("/Éclair", "/é"), None);
    }

    #[test]
    fn typing_narrows_the_list() {
        let items = commands();
        assert_eq!(
            labels(&items, "/m"),
            ["/model", "/resume", "/rename", "/permissions"]
        );
        assert_eq!(labels(&items, "/mo"), ["/model"]);
        assert!(labels(&items, "/deploy").is_empty());
    }

    #[test]
    fn a_bare_slash_has_a_header_a_window_and_a_more_row() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/");
        assert_eq!(menu.height(), 2 + 6 + 1);
        let buf = draw(&menu, 80, 9, &mut MenuState::new());
        let rows = snapshot::lines(&buf);
        assert_eq!(
            rows[0],
            format!("Commands 9 · type to filter{}1-6", " ".repeat(80 - 27 - 3))
        );
        assert_eq!(rows[1], "");
        // Labels padded to the widest in view (`/resume`, 7) plus three; the
        // right column ends at the edge, each entry written from its left.
        assert_eq!(
            rows[2],
            format!(
                "  /help{}show available slash commands{}General",
                " ".repeat(7 - 5 + 3),
                " ".repeat(80 - 7 - (12 + 29))
            )
        );
        assert!(
            rows[7].starts_with("  /model    choose the model"),
            "{:?}",
            rows[7]
        );
        assert_eq!(rows[8], "  +3 more");
    }

    #[test]
    fn the_marked_row_is_bold_and_the_hit_is_accented() {
        let items = commands();
        let t = Theme::dark();
        let menu = CompletionMenu::new(&items, t).query("/m");
        let mut state = MenuState::new();
        let buf = draw(&menu, 80, 8, &mut state);
        let rows = snapshot::lines(&buf);
        assert!(rows[2].starts_with("  /model"), "{rows:?}");
        // `/m` of `/model`: accent, bold on the marked row.
        assert_eq!(buf[(2, 2)].fg, t.accent);
        assert!(buf[(2, 2)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(4, 2)].fg, t.text, "the rest of the marked label");
        assert!(buf[(4, 2)].modifier.contains(Modifier::BOLD));
        // `/resume`: the `m` it contains is the hit, the rest is grey.
        assert!(rows[3].starts_with("  /resume"), "{rows:?}");
        assert_eq!(buf[(2, 3)].fg, t.text_secondary);
        assert_eq!(buf[(7, 3)].fg, t.accent, "the m of /resume");
        assert!(!buf[(2, 3)].modifier.contains(Modifier::BOLD));
        // Descriptions are dim; the marked one keeps the bold.
        let desc_x = 2 + 12 + 3;
        assert_eq!(buf[(desc_x, 2)].fg, t.muted);
        assert!(buf[(desc_x, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn mono_marks_by_weight_and_underlines_the_hit() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::mono()).query("/m");
        let buf = draw(&menu, 80, 8, &mut MenuState::new());
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        assert!(
            buf[(2, 2)]
                .modifier
                .contains(Modifier::UNDERLINED | Modifier::BOLD)
        );
        assert!(buf[(4, 2)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(4, 3)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_window_scrolls_at_its_edge_and_only_there() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/");
        let mut state = MenuState::new();
        for _ in 0..5 {
            menu.next(&mut state);
        }
        let _ = draw(&menu, 80, 9, &mut state);
        assert_eq!((state.selected(), state.window_start()), (5, 0));
        menu.next(&mut state);
        let buf = draw(&menu, 80, 9, &mut state);
        assert_eq!((state.selected(), state.window_start()), (6, 1));
        assert!(snapshot::lines(&buf)[0].ends_with("2-7"));
        // Back up one: the mark moves, the window stays.
        menu.previous(&mut state);
        let _ = draw(&menu, 80, 9, &mut state);
        assert_eq!((state.selected(), state.window_start()), (5, 1));
        // Past the top it wraps to the bottom, taking the window with it.
        let mut state = MenuState::new();
        menu.previous(&mut state);
        let buf = draw(&menu, 80, 9, &mut state);
        assert_eq!((state.selected(), state.window_start()), (8, 3));
        assert!(snapshot::lines(&buf)[7].starts_with("  /quit"));
        assert_eq!(
            menu.selected(&state).map(|i| i.label.as_str()),
            Some("/quit")
        );
    }

    #[test]
    fn a_new_query_puts_the_mark_back_on_the_first_match() {
        let items = commands();
        let mut state = MenuState::new();
        let all = CompletionMenu::new(&items, Theme::ansi()).query("/");
        all.next(&mut state);
        all.next(&mut state);
        assert_eq!(all.selected(&state).unwrap().label, "/new");
        let narrowed = CompletionMenu::new(&items, Theme::ansi()).query("/re");
        assert_eq!(narrowed.selected(&state).unwrap().label, "/resume");
        narrowed.next(&mut state);
        assert_eq!(state.selected(), 1);
        assert_eq!(narrowed.selected(&state).unwrap().label, "/rename");
    }

    #[test]
    fn short_areas_drop_the_header_then_count_what_is_hidden() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/");
        // Two rows: no header, one item and `+N more`.
        let rows = snapshot::lines(&draw(&menu, 60, 2, &mut MenuState::new()));
        assert!(rows[0].starts_with("  /help"), "{rows:?}");
        assert_eq!(rows[1], "  +8 more");
        // One row: the item, with the count at its end.
        let rows = snapshot::lines(&draw(&menu, 60, 1, &mut MenuState::new()));
        assert!(
            rows[0].starts_with("  /help") && rows[0].ends_with(" +8"),
            "{rows:?}"
        );
        // Three rows: the header is back and says the range.
        let rows = snapshot::lines(&draw(&menu, 60, 3, &mut MenuState::new()));
        assert!(
            rows[0].starts_with("Commands 9") && rows[0].ends_with("1-1"),
            "{rows:?}"
        );
        for (w, h) in [(1u16, 1u16), (2, 1), (3, 2), (12, 2), (5, 9), (0, 0)] {
            let _ = draw(&menu, w, h, &mut MenuState::new());
        }
    }

    #[test]
    fn a_filtered_menu_shrinks_and_says_no_range() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/mo");
        assert_eq!(menu.height(), 3);
        let rows = snapshot::lines(&draw(&menu, 80, 3, &mut MenuState::new()));
        assert_eq!(rows[0], "Commands 1");
        assert!(
            rows[2].starts_with("  /model   choose the model"),
            "{rows:?}"
        );
        assert!(rows[2].ends_with("Model"), "{rows:?}");
    }

    #[test]
    fn a_narrow_row_drops_the_right_column_and_cuts_the_description() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/mo");
        let buf = draw(&menu, 40, 3, &mut MenuState::new());
        let row = &snapshot::lines(&buf)[2];
        assert!(!row.contains("Model"), "{row:?}");
        assert!(row.ends_with('…'), "{row:?}");
        assert_eq!(text::width(row), 40);
        for w in 0..90u16 {
            let lines = menu.lines(w, 9, &mut MenuState::new());
            assert!(
                lines.iter().all(|l| text::line_width(l) <= usize::from(w)),
                "{w}"
            );
        }
    }

    #[test]
    fn nothing_matching_says_so() {
        let items = commands();
        let menu = CompletionMenu::new(&items, Theme::ansi()).query("/deploy");
        assert_eq!(menu.height(), 1);
        let rows = snapshot::lines(&draw(&menu, 60, 4, &mut MenuState::new()));
        assert_eq!(rows, ["  nothing matches /deploy", "", "", ""]);
        let custom = menu.clone().empty("no command matches /deploy");
        let rows = snapshot::lines(&draw(&custom, 60, 1, &mut MenuState::new()));
        assert_eq!(rows, ["  no command matches /deploy"]);
        assert!(menu.selected(&MenuState::new()).is_none());
    }
}
