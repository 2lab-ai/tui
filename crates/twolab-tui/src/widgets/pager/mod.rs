// Ported from OpenAI Codex `codex-rs/tui/src/pager_overlay.rs` (`PagerView`,
// `StaticOverlay`, the navigation hints and their tests)
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: keys come in as `KeyInput` with Codex's
// default pager bindings plus Esc, and return an `Outcome`; the scroll offset
// is clamped from the last paint so rendering takes `&self`; content
// positions are relative to the content area (Codex compares them with the
// area's absolute `y`); lines wrap through `WrappedLine`, measured exactly
// and cached per width; the bottom rule survives widths narrower than its
// label; one hint row; glyphs are configurable and Narrow by default; styles
// come from the `Theme`. The transcript and analytics overlays stay behind.

// Drag selection derived from herdrdev/herdr `src/selection.rs` (`Phase`,
// `Selection::{anchor, drag, finish, ordered, visible_rects, contains}` and
// their tests), `src/client/shell/mouse.rs` (`selection_edge_scroll_lines`)
// and `src/ui/panes.rs` (`render_selection_highlight`); `notes` derived from
// `src/ui/release_notes.rs` (`release_notes_inline_spans`,
// `release_notes_lines` and their tests); at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab:
// rows are content rows of the pager (herdr: scrollback rows of a terminal
// pane); the selected text is read from the drawn cells grapheme by grapheme,
// a wide glyph whole; double-click selects a word locally (herdr asks its
// server); edge autoscroll happens per drag event, the host repeating the
// last drag on its own timer (no clock here); the highlight style is a
// builder option (REVERSED by default) instead of a colour mixed from the
// host background; clipboard writing is left to `crate::osc52`. `notes`
// measures nothing itself (herdr's per-line `chars().count()` widths were
// unused and wrong for wide text), takes `#` to `###` headings and indented
// bullets, and its default bullet and gutter glyphs are East-Asian Narrow.

//! The full text, scrollable: a request body, a log, a transcript.
//!
//! Ported from OpenAI Codex `pager_overlay.rs` (Apache-2.0), the overlay
//! Codex opens for diffs, approvals and static text. It tells the person
//! where they are in something longer than the screen:
//!
//! ```text
//! / Request body / / / / / / / / / / /
//! {"model": "claude-opus-5-5",
//!  "max_tokens": 64000,
//! ~
//! ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌
//! q/esc close  up/down scroll  space/pgdn page  home/end jump
//! ```
//!
//! A title rule, the content (lines wrap; nothing is cut), `~` past the end,
//! a bottom rule with how far down you are, and the keys. Heights are
//! measured once per width and cached, so a long transcript scrolls without
//! re-wrapping every line every frame.
//!
//! From herdr (Apache-2.0): a mouse drag selects text and the selection
//! stays on its rows while the pager scrolls ([`Pager::mouse_down`],
//! [`Pager::mouse_drag`], [`Pager::mouse_up`], [`Pager::select_word`] for a
//! double-click; at most three highlight rects); [`Pager::selected_text`]
//! reads it back by cells and [`Pager::copy_sequence`] wraps it in OSC 52.
//! [`notes`] turns release-notes markdown (`###` headings, `- ` bullets,
//! `` `code` ``, fenced blocks) into lines for the pager.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::cell::Cell;
use std::fmt;

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::input::{Key, KeyInput};
use crate::osc52;
use crate::render::{CachedRenderable, Renderable, render_offset_content};
use crate::text;
use crate::theme::Theme;

/// Where a mouse selection is in its life (herdr's `Phase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionPhase {
    /// The button is down and has not moved: so far just a click. Nothing
    /// is highlighted.
    Anchored,
    /// The pointer moved off the anchor cell: the cells are highlighted.
    Dragging,
    /// Released after a drag (or a double-click word): highlighted, ready
    /// to copy.
    Done,
}

/// A selection in content coordinates: `(row, col)` with the row counted
/// from the top of the content (not the screen) and the column in cells.
/// Scrolling the pager does not move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    anchor: (usize, u16),
    cursor: (usize, u16),
    phase: SelectionPhase,
}

impl Selection {
    /// A button press at a cell: anchored, not visible yet.
    pub const fn anchor(row: usize, col: u16) -> Selection {
        Selection {
            anchor: (row, col),
            cursor: (row, col),
            phase: SelectionPhase::Anchored,
        }
    }

    /// A finished selection from `start` to `end`, both cells included, in
    /// either order (select all, a word, a search hit).
    pub const fn range(start: (usize, u16), end: (usize, u16)) -> Selection {
        Selection {
            anchor: start,
            cursor: end,
            phase: SelectionPhase::Done,
        }
    }

    /// Extend to `(row, col)` while the button is down; highlighting starts
    /// once the cursor leaves the anchor cell.
    pub fn drag(&mut self, row: usize, col: u16) {
        if self.phase == SelectionPhase::Done {
            return;
        }
        self.cursor = (row, col);
        if self.cursor != self.anchor {
            self.phase = SelectionPhase::Dragging;
        }
    }

    /// Release: `true` when the person dragged (a selection to copy),
    /// `false` for a plain click.
    pub fn finish(&mut self) -> bool {
        match self.phase {
            SelectionPhase::Dragging => {
                self.phase = SelectionPhase::Done;
                true
            }
            SelectionPhase::Done => true,
            SelectionPhase::Anchored => false,
        }
    }

    pub const fn phase(&self) -> SelectionPhase {
        self.phase
    }

    /// Highlighted (dragging or done).
    pub fn is_visible(&self) -> bool {
        self.phase != SelectionPhase::Anchored
    }

    /// Pressed and released without moving.
    pub fn is_just_click(&self) -> bool {
        self.phase == SelectionPhase::Anchored
    }

    /// Start and end in reading order.
    pub fn ordered(&self) -> ((usize, u16), (usize, u16)) {
        let (a, c) = (self.anchor, self.cursor);
        if a.0 < c.0 || (a.0 == c.0 && a.1 <= c.1) {
            (a, c)
        } else {
            (c, a)
        }
    }

    /// Whether content cell `(row, col)` is selected.
    pub fn contains(&self, row: usize, col: u16) -> bool {
        if !self.is_visible() {
            return false;
        }
        let ((sr, sc), (er, ec)) = self.ordered();
        if row < sr || row > er {
            false
        } else if sr == er {
            col >= sc && col <= ec
        } else if row == sr {
            col >= sc
        } else if row == er {
            col <= ec
        } else {
            true
        }
    }

    /// The selected cells inside `view` when content row `top` is its first
    /// row, as at most three rects (first row, the rows between, last row),
    /// each clamped inside `view`. Empty rects for parts out of view.
    pub fn rects(&self, view: Rect, top: usize) -> [Rect; 3] {
        let mut out = [Rect::default(); 3];
        if !self.is_visible() || view.is_empty() {
            return out;
        }
        let ((sr, sc), (er, ec)) = self.ordered();
        let bottom = top + view.height as usize;
        let rect = |first: usize, end: usize, left: u16, right: u16| {
            let (first, end) = (first.max(top), end.min(bottom));
            let (left, right) = (left.min(view.width), right.min(view.width));
            if first >= end || left >= right {
                Rect::default()
            } else {
                Rect::new(
                    view.x + left,
                    view.y + (first - top) as u16,
                    right - left,
                    (end - first) as u16,
                )
            }
        };
        if sr == er {
            out[0] = rect(sr, sr + 1, sc, ec.saturating_add(1));
        } else {
            out[0] = rect(sr, sr + 1, sc, view.width);
            out[1] = rect(sr + 1, er, 0, view.width);
            out[2] = rect(er, er + 1, 0, ec.saturating_add(1));
        }
        out
    }
}

/// Rows to scroll when a drag is `distance` rows past the content's top or
/// bottom edge: 3 per row of distance, 3 to 15 (herdr's
/// `selection_edge_scroll_lines`).
pub fn edge_scroll_rows(distance: u16) -> usize {
    usize::from(distance).saturating_mul(3).clamp(3, 15)
}

/// Graphemes a word is made of for a double-click: anything but spaces and
/// brackets, quotes and separators, so `zhuge@2lab.ai` or `src/ui.rs:42`
/// select whole.
fn is_word(g: &str) -> bool {
    !g.chars().any(|c| {
        c.is_whitespace()
            || matches!(
                c,
                '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '"' | '\'' | '`' | ',' | ';' | '|'
            )
    })
}

/// The graphemes drawn on row `y` of `buf`: `(x, cells, symbol)`, a wide
/// glyph once with its full width (the way a terminal backend walks cells).
fn row_cells(buf: &Buffer, y: u16) -> Vec<(u16, u16, String)> {
    let area = buf.area;
    let mut out = Vec::new();
    let mut x = area.x;
    while x < area.right() {
        let cell = &buf[(x, y)];
        let w = cell.cell_width().max(1);
        let sym = if cell.diff_option == CellDiffOption::Skip {
            " ".to_string()
        } else {
            cell.symbol().to_string()
        };
        out.push((x, w, sym));
        x = x.saturating_add(w);
    }
    out
}

/// One line that wraps at the width it is given (ratatui's word wrap,
/// leading spaces kept). Its height is ratatui's own row count for that
/// width (`Paragraph::line_count`), so it is exact. Wide spaces are made
/// wrap-safe first ([`text::wrap_safe`]). Scrolling paints only the visible
/// rows.
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedLine(pub Line<'static>);

impl WrappedLine {
    fn paragraph(&self) -> Paragraph<'static> {
        Paragraph::new(text::wrap_safe_line(self.0.clone())).wrap(Wrap { trim: false })
    }
}

impl Renderable for WrappedLine {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Widget::render(self.paragraph(), area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        // ratatui's own count, from the same word wrapper the render uses.
        u16::try_from(self.paragraph().line_count(width).max(1)).unwrap_or(u16::MAX)
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        Widget::render(self.paragraph().scroll((scroll_offset, 0)), area, buf);
        true
    }
}

/// The rule glyphs. Every default is East-Asian Narrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagerGlyphs {
    /// Repeated across the title row (Codex: `/ `).
    pub title_fill: &'static str,
    /// Repeated across the bottom rule.
    pub rule: &'static str,
    /// Marks each row past the end.
    pub eof: &'static str,
}

impl Default for PagerGlyphs {
    /// `/ `, `╌` (U+254C, Narrow), `~`.
    fn default() -> Self {
        Self {
            title_fill: "/ ",
            rule: "╌",
            eof: "~",
        }
    }
}

impl PagerGlyphs {
    /// Plain ASCII: `/ `, `-`, `~`.
    pub const fn ascii() -> Self {
        Self {
            title_fill: "/ ",
            rule: "-",
            eof: "~",
        }
    }

    /// Codex's solid `─` rule. U+2500 is East-Asian *Ambiguous*: two cells on
    /// a CJK terminal set to wide ambiguous glyphs. Opt in knowingly.
    pub const fn box_drawing() -> Self {
        Self {
            title_fill: "/ ",
            rule: "─",
            eof: "~",
        }
    }
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a pager key, or the view was already at that end.
    Ignored,
    /// The view moved; redraw.
    Scrolled,
    /// `q`, Esc or Ctrl-C: close the pager.
    Closed,
}

/// A scrollable, read-only document with a title. Draw it with
/// `Widget for &Pager` or as a [`Renderable`].
pub struct Pager {
    title: String,
    content: Vec<Box<dyn Renderable>>,
    scroll: usize,
    /// Content rows shown by the last paint — the page size.
    last_page: Cell<Option<usize>>,
    /// Largest offset at the last paint's width and height.
    last_max: Cell<Option<usize>>,
    hints: bool,
    glyphs: PagerGlyphs,
    theme: Theme,
    selection: Option<Selection>,
    selection_style: Style,
    /// The content area and offset of the last paint, for the mouse.
    last_view: Cell<Option<(Rect, usize)>>,
}

impl fmt::Debug for Pager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager")
            .field("title", &self.title)
            .field("items", &self.content.len())
            .field("scroll", &self.scroll)
            .finish_non_exhaustive()
    }
}

fn wrap_lines(lines: Vec<Line<'static>>) -> Vec<Box<dyn Renderable>> {
    lines
        .into_iter()
        .map(|l| Box::new(CachedRenderable::new(WrappedLine(l))) as Box<dyn Renderable>)
        .collect()
}

impl Pager {
    /// A pager over lines that wrap at the pager's width.
    pub fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self::with_renderables(title, wrap_lines(lines))
    }

    /// A pager over plain text, one line per `\n`.
    pub fn from_text(title: impl Into<String>, text: &str) -> Self {
        let lines = text
            .split('\n')
            .map(|l| Line::from(l.to_string()))
            .collect();
        Self::new(title, lines)
    }

    /// A pager over any stack of renderables (Codex's `with_renderables`):
    /// a highlighted block, a table, a diff. Wrap expensive ones in
    /// [`CachedRenderable`]; the pager asks every item its height each paint.
    pub fn with_renderables(title: impl Into<String>, content: Vec<Box<dyn Renderable>>) -> Self {
        Self {
            title: title.into(),
            content,
            scroll: 0,
            last_page: Cell::new(None),
            last_max: Cell::new(None),
            hints: true,
            glyphs: PagerGlyphs::default(),
            theme: Theme::default(),
            selection: None,
            selection_style: Style::new().add_modifier(Modifier::REVERSED),
            last_view: Cell::new(None),
        }
    }

    /// Replace the lines, keeping the scroll position (clamped at the next
    /// paint).
    pub fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.content = wrap_lines(lines);
        self.last_max.set(None);
    }

    /// Show the key-hint row under the bottom rule.
    pub fn hints(mut self, hints: bool) -> Self {
        self.hints = hints;
        self
    }

    pub fn glyphs(mut self, glyphs: PagerGlyphs) -> Self {
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

    /// How selected cells are drawn: patched over their own style. Default
    /// `REVERSED` (no colour, so it shows in mono too).
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = style;
        self
    }

    /// The current selection, if any.
    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    /// Replace the selection (select all, a search hit) or clear it.
    pub fn set_selection(&mut self, selection: Option<Selection>) {
        self.selection = selection;
    }

    /// Forget the selection; returns whether one was shown.
    pub fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some_and(|s| s.is_visible())
    }

    /// The content cell under screen cell `(col, row)` in the last paint,
    /// clamped into the content area, and whether it was inside.
    fn content_cell(&self, col: u16, row: u16) -> Option<((usize, u16), bool)> {
        let (view, offset) = self.last_view.get()?;
        if view.is_empty() {
            return None;
        }
        let inside = view.contains(Position::new(col, row));
        let c = col.clamp(view.x, view.right() - 1) - view.x;
        let r = row.clamp(view.y, view.bottom() - 1) - view.y;
        Some(((offset + r as usize, c), inside))
    }

    /// Mouse button down at screen cell `(col, row)`. On the content it
    /// anchors a new selection (nothing highlighted until the pointer
    /// moves); anywhere else it clears the old one. Returns whether to
    /// redraw.
    pub fn mouse_down(&mut self, col: u16, row: u16) -> bool {
        match self.content_cell(col, row) {
            Some(((r, c), true)) => {
                let was = self.clear_selection();
                self.selection = Some(Selection::anchor(r, c));
                was
            }
            _ => self.clear_selection(),
        }
    }

    /// The pointer moved with the button down. Past the content's top or
    /// bottom edge the pager scrolls ([`edge_scroll_rows`]) and the
    /// selection follows; repeat the last drag on a timer to keep scrolling
    /// while the pointer rests there. Returns whether to redraw.
    pub fn mouse_drag(&mut self, col: u16, row: u16) -> bool {
        let Some((view, offset)) = self.last_view.get() else {
            return false;
        };
        if view.is_empty()
            || !self
                .selection
                .is_some_and(|s| s.phase != SelectionPhase::Done)
        {
            return false;
        }
        let max = self.last_max.get().unwrap_or(0);
        let next = if row < view.y {
            offset.saturating_sub(edge_scroll_rows(view.y - row))
        } else if row >= view.bottom() {
            offset
                .saturating_add(edge_scroll_rows(row - view.bottom() + 1))
                .min(max)
        } else {
            offset
        };
        if next != offset {
            self.scroll = next;
            self.last_view.set(Some((view, next)));
        }
        let Some(((r, c), inside)) = self.content_cell(col, row) else {
            return false;
        };
        let Some(sel) = self.selection.as_mut() else {
            return false;
        };
        let before = *sel;
        sel.drag(r, c);
        // Off the content the pointer clamps; it still counts as a drag.
        if !inside && sel.phase == SelectionPhase::Anchored {
            sel.phase = SelectionPhase::Dragging;
        }
        *sel != before || next != offset
    }

    /// Button released. Returns `true` when a selection was made — copy it
    /// now ([`Pager::copy_sequence`]); a plain click leaves none.
    pub fn mouse_up(&mut self, col: u16, row: u16) -> bool {
        self.mouse_drag(col, row);
        let made = self.selection.as_mut().is_some_and(Selection::finish);
        if !made {
            self.selection = None;
        }
        made
    }

    /// Double-click: select the word under screen cell `(col, row)` (no
    /// spaces, brackets or quotes in it). Returns whether a word was
    /// selected.
    pub fn select_word(&mut self, col: u16, row: u16) -> bool {
        let Some(((r, c), true)) = self.content_cell(col, row) else {
            return false;
        };
        let Some(buf) = self.content_rows(r, r) else {
            return false;
        };
        let cells = row_cells(&buf, 0);
        let Some(at) = cells.iter().position(|(x, w, _)| c >= *x && c < x + w) else {
            return false;
        };
        if !is_word(&cells[at].2) {
            return false;
        }
        let first = (0..at)
            .rev()
            .take_while(|&i| is_word(&cells[i].2))
            .last()
            .unwrap_or(at);
        let last = (at + 1..cells.len())
            .take_while(|&i| is_word(&cells[i].2))
            .last()
            .unwrap_or(at);
        let (x0, _, _) = cells[first];
        let (x1, w1, _) = cells[last];
        self.selection = Some(Selection::range((r, x0), (r, x1 + w1 - 1)));
        true
    }

    /// Content rows `first..=last` drawn at the last paint's width.
    fn content_rows(&self, first: usize, last: usize) -> Option<Buffer> {
        let (view, _) = self.last_view.get()?;
        let h = u16::try_from(last.checked_sub(first)? + 1).unwrap_or(u16::MAX);
        let area = Rect::new(0, 0, view.width, h);
        let mut buf = Buffer::empty(area);
        self.render_content(area, &mut buf, first);
        Some(buf)
    }

    /// The selected text, read from the cells as drawn at the last paint's
    /// width: one line per row, trailing blanks trimmed, a wide glyph (CJK,
    /// emoji, a flag) whole even when the selection starts or ends on its
    /// second cell. `None` without a visible selection or a paint.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.selection.filter(|s| s.is_visible())?;
        let (view, _) = self.last_view.get()?;
        let ((sr, sc), (er, ec)) = sel.ordered();
        let total = self.content_height(view.width);
        if total == 0 || sr >= total {
            return Some(String::new());
        }
        // Rows past the end (the `~` rows) hold no text.
        let last = er.min(total - 1);
        let buf = self.content_rows(sr, last)?;
        let mut out: Vec<String> = Vec::with_capacity(last - sr + 1);
        for r in sr..=last {
            let c0 = if r == sr { sc } else { 0 };
            let c1 = if r == er { ec } else { u16::MAX };
            let line: String = row_cells(&buf, (r - sr) as u16)
                .into_iter()
                .filter(|(x, w, _)| *x <= c1 && x + w > c0)
                .map(|(_, _, sym)| sym)
                .collect();
            out.push(line.trim_end().to_string());
        }
        Some(out.join("\n"))
    }

    /// The OSC 52 escape that puts the selected text on the clipboard of
    /// the person's terminal ([`crate::osc52::sequence`]); write it to the
    /// terminal after `mouse_up` returns `true`.
    pub fn copy_sequence(&self, mux: osc52::Mux) -> Option<String> {
        self.selected_text()
            .filter(|t| !t.is_empty())
            .map(|t| osc52::sequence(&t, mux))
    }

    /// The selection's highlight rects in the last paint (at most three).
    pub fn selection_rects(&self) -> [Rect; 3] {
        match (self.selection, self.last_view.get()) {
            (Some(sel), Some((view, offset))) => sel.rects(view, offset),
            _ => [Rect::default(); 3],
        }
    }

    /// Start scrolled down by `rows` (clamped at the next paint).
    pub fn scroll_to(mut self, rows: usize) -> Self {
        self.scroll = rows;
        self
    }

    /// Rows scrolled past, as of the last paint.
    pub fn scroll_offset(&self) -> usize {
        self.offset()
    }

    /// Rows the content takes at `width`.
    pub fn content_height(&self, width: u16) -> usize {
        self.content
            .iter()
            .map(|c| c.desired_height(width) as usize)
            .sum()
    }

    fn offset(&self) -> usize {
        self.scroll.min(self.last_max.get().unwrap_or(usize::MAX))
    }

    /// Act on a key: ↑/k, ↓/j, PgUp/Shift-Space/Ctrl-B, PgDn/Space/Ctrl-F,
    /// Ctrl-U/Ctrl-D half a page, Home, End; q, Esc or Ctrl-C close.
    pub fn key(&mut self, key: KeyInput) -> Outcome {
        let ctrl = key.mods.ctrl && !key.mods.alt;
        let page = self.last_page.get().unwrap_or(1).max(1);
        let half = page.saturating_add(1) / 2;
        let cur = self.offset();
        let max = self.last_max.get().unwrap_or(usize::MAX);
        let next = match key.key {
            Key::Char('q') if !ctrl => return Outcome::Closed,
            Key::Char('c') if ctrl => return Outcome::Closed,
            Key::Esc => return Outcome::Closed,
            Key::Up => cur.saturating_sub(1),
            Key::Char('k') if !ctrl => cur.saturating_sub(1),
            Key::Down => cur.saturating_add(1),
            Key::Char('j') if !ctrl => cur.saturating_add(1),
            Key::PageUp => cur.saturating_sub(page),
            Key::Char('b') if ctrl => cur.saturating_sub(page),
            Key::Char(' ') if key.mods.shift => cur.saturating_sub(page),
            Key::PageDown => cur.saturating_add(page),
            Key::Char('f') if ctrl => cur.saturating_add(page),
            Key::Char(' ') if !ctrl => cur.saturating_add(page),
            Key::Char('u') if ctrl => cur.saturating_sub(half),
            Key::Char('d') if ctrl => cur.saturating_add(half),
            Key::Home => 0,
            Key::End => usize::MAX,
            _ => return Outcome::Ignored,
        }
        .min(max);
        if next == cur {
            return Outcome::Ignored;
        }
        self.scroll = next;
        Outcome::Scrolled
    }

    /// `/ Title / / / /`: the fill starts one cell after the title, so it
    /// never touches it whatever the title's width.
    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let t = &self.theme;
        let w = area.width as usize;
        let lead = self.glyphs.title_fill;
        let title_room = w.saturating_sub(text::width(lead));
        let title = text::truncate(&self.title, title_room).into_owned();
        let used = text::width(lead) + text::width(&title) + 1;
        let fill_w = text::width(lead).max(1);
        let fill = lead.repeat(w.saturating_sub(used) / fill_w + 1);
        let line = Line::from(vec![
            Span::styled(lead, t.dim()),
            Span::styled(title, t.text().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(
                text::take_cells(&fill, w.saturating_sub(used))
                    .0
                    .to_string(),
                t.dim(),
            ),
        ]);
        text::truncate_line(line, w).render(area, buf);
    }

    /// Paint the content scrolled by `offset`; returns the rows painted.
    fn render_content(&self, area: Rect, buf: &mut Buffer, offset: usize) -> u16 {
        buf.set_style(area, self.theme.text());
        let mut y = -(offset as isize);
        let mut drawn: u16 = 0;
        let view = area.height as isize;
        for item in &self.content {
            let top = y;
            let height = item.desired_height(area.width) as isize;
            y += height;
            let bottom = y;
            if bottom <= 0 {
                continue;
            }
            if top >= view {
                break;
            }
            if top < 0 {
                let skip = u16::try_from(-top).unwrap_or(u16::MAX);
                drawn = drawn.max(render_offset_content(area, buf, item.as_ref(), skip));
            } else {
                let top = top as u16;
                let h = (height as u16).min(area.height - top);
                item.render(Rect::new(area.x, area.y + top, area.width, h), buf);
                drawn = drawn.max(top + h);
            }
        }
        drawn
    }

    fn render_eof(&self, area: Rect, buf: &mut Buffer, drawn: u16) {
        for y in area.y + drawn..area.bottom() {
            Line::styled(self.glyphs.eof, self.theme.dim())
                .render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }

    fn render_bottom_bar(&self, area: Rect, buf: &mut Buffer, percent: u8) {
        let t = &self.theme;
        let rule_w = text::width(self.glyphs.rule).max(1);
        let rule = self.glyphs.rule.repeat(area.width as usize / rule_w);
        Line::styled(rule, t.border()).render(area, buf);
        let label = format!(" {percent}% ");
        let w = text::width(&label) as u16;
        if area.width > w {
            let x = area.right() - w - 1;
            Line::styled(label, t.secondary()).render(Rect::new(x, area.y, w, 1), buf);
        } else {
            let short = format!("{percent}%");
            Line::styled(
                text::truncate(&short, area.width as usize).into_owned(),
                t.secondary(),
            )
            .render(area, buf);
        }
    }

    fn hint_line(&self) -> Line<'static> {
        let t = &self.theme;
        let mut spans = Vec::new();
        for (i, (key, label)) in [
            ("q/esc", "close"),
            ("up/down", "scroll"),
            ("space/pgdn", "page"),
            ("home/end", "jump"),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(key, t.accent()));
            spans.push(Span::styled(format!(" {label}"), t.dim()));
        }
        Line::from(spans)
    }
}

/// Glyphs for [`notes_with`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotesGlyphs {
    /// Before each `- ` item.
    pub bullet: &'static str,
    /// Down the left of a fenced code block.
    pub gutter: &'static str,
}

impl Default for NotesGlyphs {
    /// `∙` (U+2219) and `▐` (U+2590): both East-Asian Narrow.
    fn default() -> Self {
        Self {
            bullet: "∙",
            gutter: "▐",
        }
    }
}

impl NotesGlyphs {
    /// herdr's `•` and `▏`. Both are East-Asian *Ambiguous*: two cells on a
    /// CJK terminal set to wide ambiguous glyphs. Opt in knowingly.
    pub const fn herdr() -> Self {
        Self {
            bullet: "•",
            gutter: "▏",
        }
    }
}

/// Release-notes markdown as lines for a [`Pager`] (or any paragraph):
/// `#`–`###` headings upper-cased in bold accent, `- ` and `* ` bullets
/// (indent kept), `` `code` `` in bold accent without the backticks (spaces
/// made non-breaking when it holds `=`, so `key = "value"` wraps whole),
/// fenced blocks verbatim behind a gutter, blank lines kept. Lines start one
/// cell in, as herdr draws them.
pub fn notes(md: &str, theme: &Theme) -> Vec<Line<'static>> {
    notes_with(md, theme, NotesGlyphs::default())
}

/// [`notes`] with other glyphs.
pub fn notes_with(md: &str, theme: &Theme, glyphs: NotesGlyphs) -> Vec<Line<'static>> {
    let text = theme.text();
    let code = theme.accent().add_modifier(Modifier::BOLD);
    let mut out = Vec::new();
    let mut fenced = false;
    for raw in md.lines() {
        let line = raw.trim_end();
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            let mut spans = vec![
                Span::styled(glyphs.gutter, theme.accent()),
                Span::styled(" ", text),
            ];
            if !line.is_empty() {
                spans.push(Span::styled(line.to_string(), text));
            }
            out.push(Line::from(spans));
            continue;
        }
        if line.is_empty() {
            out.push(Line::default());
            continue;
        }
        let body = line.trim_start();
        let indent = line.len() - body.len();
        let heading = body
            .strip_prefix("### ")
            .or_else(|| body.strip_prefix("## "))
            .or_else(|| body.strip_prefix("# "));
        if let Some(rest) = heading {
            let rest = rest.trim();
            out.push(if rest.is_empty() {
                Line::default()
            } else {
                Line::from(vec![
                    Span::raw(" "),
                    Span::styled(rest.to_uppercase(), code),
                ])
            });
            continue;
        }
        let mut spans = vec![Span::raw(" ".repeat(1 + indent))];
        let rest = match body.strip_prefix("- ").or_else(|| body.strip_prefix("* ")) {
            Some(rest) => {
                spans.push(Span::styled(glyphs.bullet, theme.accent()));
                spans.push(Span::raw(" "));
                rest
            }
            None => body,
        };
        spans.extend(inline_code(rest, text, code));
        out.push(Line::from(spans));
    }
    out
}

/// `` `code` `` runs styled without their backticks; an unclosed backtick
/// stays as written (herdr's `release_notes_inline_spans`).
fn inline_code(s: &str, text: Style, code: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('`') {
        let (before, after) = rest.split_at(start);
        if !before.is_empty() {
            spans.push(Span::styled(before.to_string(), text));
        }
        let after = &after[1..];
        let Some(end) = after.find('`') else {
            spans.push(Span::styled(format!("`{after}"), text));
            return spans;
        };
        let snippet = &after[..end];
        if !snippet.is_empty() {
            // Keep `key = "value"` together when the line wraps.
            let shown = if snippet.contains('=') {
                snippet.replace(' ', "\u{a0}")
            } else {
                snippet.to_string()
            };
            spans.push(Span::styled(shown, code));
        }
        rest = &after[end + 1..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), text));
    }
    spans
}

/// `scroll / max_scroll` as a percentage, or 100 when everything fits.
fn percent(offset: usize, total: usize, view: usize) -> u8 {
    let max_scroll = total.saturating_sub(view);
    if total == 0 || max_scroll == 0 {
        100
    } else {
        ((offset.min(max_scroll) as f32 / max_scroll as f32) * 100.0).round() as u8
    }
}

impl Renderable for Pager {
    fn desired_height(&self, width: u16) -> u16 {
        let content = u16::try_from(self.content_height(width)).unwrap_or(u16::MAX);
        content
            .saturating_add(2)
            .saturating_add(u16::from(self.hints))
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // Rows yield from the top of this list: hints, title, bottom rule.
        let h = area.height;
        let hints = self.hints && h >= 4;
        let header = h >= 3;
        let rule = h >= 2;
        let mut y = area.y;
        if header {
            self.render_header(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
        let content_h = h - u16::from(header) - u16::from(rule) - u16::from(hints);
        let content = Rect::new(area.x, y, area.width, content_h);
        let total = self.content_height(area.width);
        let view = content_h as usize;
        let max = total.saturating_sub(view);
        self.last_max.set(Some(max));
        self.last_page.set(Some(view));
        let offset = self.scroll.min(max);
        let drawn = self.render_content(content, buf, offset);
        self.render_eof(content, buf, drawn);
        self.last_view.set(Some((content, offset)));
        if let Some(sel) = self.selection {
            for r in sel.rects(content, offset) {
                if !r.is_empty() {
                    buf.set_style(r, self.selection_style);
                }
            }
        }
        y += content_h;
        if rule {
            self.render_bottom_bar(
                Rect::new(area.x, y, area.width, 1),
                buf,
                percent(offset, total, view),
            );
            y += 1;
        }
        if hints {
            Renderable::render(
                &self.hint_line(),
                Rect::new(area.x + 1, y, area.width - 1, 1),
                buf,
            );
        }
    }
}

impl Widget for &Pager {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Renderable::render(self, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::text::Text;

    fn draw(p: &Pager, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Renderable::render(p, area, &mut buf);
        snapshot::lines(&buf)
    }

    fn paragraph_block(label: &str, lines: usize) -> Box<dyn Renderable> {
        let text = Text::from(
            (0..lines)
                .map(|i| Line::from(format!("{label}{i}")))
                .collect::<Vec<_>>(),
        );
        Box::new(Paragraph::new(text)) as Box<dyn Renderable>
    }

    // Upstream: `static_overlay_snapshot_basic` (one hint row; Narrow rule).
    #[test]
    fn static_overlay_snapshot_basic() {
        let p = Pager::new(
            "S T A T I C",
            vec!["one".into(), "two".into(), "three".into()],
        );
        assert_eq!(
            draw(&p, 40, 9),
            vec![
                "/ S T A T I C / / / / / / / / / / / / /",
                "one",
                "two",
                "three",
                "~",
                "~",
                "~",
                "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌",
                " q/esc close  up/down scroll  space/pgd…",
            ]
        );
    }

    // Upstream: `static_overlay_wraps_long_lines`.
    #[test]
    fn static_overlay_wraps_long_lines() {
        let p = Pager::new(
            "S T A T I C",
            vec![
                "a very long line that should wrap when rendered within a narrow pager overlay width"
                    .into(),
            ],
        )
        .glyphs(PagerGlyphs::box_drawing());
        // Three content rows of four: at the top, 0%.
        assert_eq!(
            draw(&p, 24, 6),
            vec![
                "/ S T A T I C / / / / /",
                "a very long line that",
                "should wrap when",
                "rendered within a narrow",
                "─────────────────── 0% ─",
                " q/esc close  up/down s…",
            ]
        );
        assert_eq!(draw(&p, 24, 8)[4], "pager overlay width");
    }

    // Upstream: `pager_view_content_height_counts_renderables`.
    #[test]
    fn pager_view_content_height_counts_renderables() {
        let p =
            Pager::with_renderables("T", vec![paragraph_block("a", 2), paragraph_block("b", 3)]);
        assert_eq!(p.content_height(80), 5);
    }

    #[test]
    fn keys_scroll_page_and_clamp_at_the_ends() {
        let lines: Vec<Line<'static>> = (1..=50).map(|i| Line::from(format!("line {i}"))).collect();
        let mut p = Pager::new("log", lines);
        // 12 rows: title, 9 content rows, rule, hints.
        let _ = draw(&p, 30, 12);
        assert_eq!(
            p.key(Key::Up.into()),
            Outcome::Ignored,
            "already at the top"
        );
        assert_eq!(p.key(Key::Down.into()), Outcome::Scrolled);
        assert_eq!(p.scroll_offset(), 1);
        p.key(KeyInput::from(' '));
        assert_eq!(p.scroll_offset(), 10);
        p.key(KeyInput::ctrl(Key::Char('d')));
        assert_eq!(p.scroll_offset(), 15);
        p.key(Key::End.into());
        assert_eq!(p.scroll_offset(), 41);
        assert_eq!(p.key(Key::PageDown.into()), Outcome::Ignored);
        let lines = draw(&p, 30, 12);
        assert_eq!(lines[1], "line 42");
        assert_eq!(lines[9], "line 50");
        assert!(lines[10].ends_with("100% ╌"), "{lines:#?}");
        p.key(Key::Home.into());
        assert!(draw(&p, 30, 12)[10].ends_with(" 0% ╌"));
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        assert!(draw(&p, 30, 12)[10].ends_with(" 10% ╌"));
        for k in [
            KeyInput::from('q'),
            Key::Esc.into(),
            KeyInput::ctrl(Key::Char('c')),
        ] {
            assert_eq!(p.key(k), Outcome::Closed);
        }
    }

    #[test]
    fn wrapped_body_matches_one_wrapped_paragraph() {
        let lines: Vec<Line<'static>> = [
            "  - removed the sleep(50ms) and awaited the frame tick instead; the flake rate went from 1 in 40 to 0 in 500 runs",
            "",
            "word  double  spaces   triple    end",
            "    fn render(&self, area: Rect, buf: &mut Buffer) { let x = area.width.saturating_sub(1); }",
            "한국어 문장도 셀 폭으로 줄바꿈합니다 그리고 끝",
        ]
        .into_iter()
        .map(Line::from)
        .collect();
        for w in [7u16, 13, 24, 31, 57] {
            let p = Pager::new("t", lines.clone()).hints(false);
            let total = p.content_height(w);
            let area = Rect::new(0, 0, w, total as u16);
            let mut expected = Buffer::empty(area);
            Paragraph::new(Text::from(lines.clone()))
                .wrap(Wrap { trim: false })
                .render(area, &mut expected);
            let got = draw(&p, w, total as u16 + 2);
            assert_eq!(&got[1..=total], &snapshot::lines(&expected)[..], "w={w}");
        }
    }

    #[test]
    fn heights_are_cached_per_width() {
        let p = Pager::from_text("t", "alpha beta gamma\ndelta");
        assert_eq!(p.content_height(80), 2);
        assert_eq!(p.content_height(6), 4);
        assert_eq!(p.content_height(80), 2);
    }

    #[test]
    fn scrolling_through_a_partly_visible_line_paints_its_tail() {
        let p = Pager::new(
            "t",
            vec![Line::from("aaaa bbbb cccc dddd"), Line::from("next")],
        )
        .hints(false)
        .scroll_to(2);
        // At width 5 the first line is four rows; offset 2 starts at "cccc".
        // Three content rows: five rows in all, so the offset can reach 2.
        let lines = draw(&p, 5, 5);
        assert_eq!(&lines[1..4], &["cccc", "dddd", "next"]);
        assert_eq!(p.scroll_offset(), 2);
    }

    #[test]
    fn tiny_areas_keep_the_content_then_the_rule() {
        let p = Pager::from_text("Request body", "{}");
        assert_eq!(draw(&p, 20, 1), vec!["{}"]);
        assert_eq!(draw(&p, 20, 2)[1], "╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌");
        assert!(draw(&p, 20, 3)[0].starts_with("/ Request body"));
        assert_eq!(draw(&p, 4, 2)[1], "100%");
        assert_eq!(draw(&p, 3, 2)[1], "10…");
        let _ = draw(&p, 1, 1);
    }

    #[test]
    fn mono_rules_and_eof_carry_no_colour() {
        let p = Pager::from_text("t", "x").theme(Theme::mono());
        let area = Rect::new(0, 0, 20, 6);
        let mut buf = Buffer::empty(area);
        Renderable::render(&p, area, &mut buf);
        for c in buf.content() {
            assert!(matches!(c.fg, ratatui::style::Color::Reset), "{c:?}");
            assert!(matches!(c.bg, ratatui::style::Color::Reset), "{c:?}");
        }
        assert!(buf[(0, 2)].modifier.contains(Modifier::DIM), "~ is dim");
    }

    fn paint(p: &Pager, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Renderable::render(p, area, &mut buf);
        buf
    }

    fn numbered(n: usize) -> Pager {
        let lines: Vec<Line<'static>> = (0..n)
            .map(|i| Line::from(format!("line {i:02} {}", "x".repeat(20))))
            .collect();
        Pager::new("log", lines).hints(false)
    }

    fn make_sel(sr: usize, sc: u16, er: usize, ec: u16) -> Selection {
        let mut s = Selection::anchor(sr, sc);
        s.drag(er, ec);
        s
    }

    // Upstream: selection.rs `ordering_forward`, `ordering_backward`.
    #[test]
    fn ordering_is_reading_order() {
        assert_eq!(make_sel(2, 5, 4, 10).ordered(), ((2, 5), (4, 10)));
        assert_eq!(make_sel(4, 10, 2, 5).ordered(), ((2, 5), (4, 10)));
    }

    // Upstream: `single_line_contains`, `multi_line_contains`.
    #[test]
    fn contains_single_and_multi_line() {
        let sel = make_sel(2, 5, 2, 15);
        assert!(!sel.contains(2, 4));
        assert!(sel.contains(2, 5) && sel.contains(2, 10) && sel.contains(2, 15));
        assert!(!sel.contains(2, 16) && !sel.contains(1, 10) && !sel.contains(3, 10));
        let sel = make_sel(2, 5, 4, 10);
        assert!(!sel.contains(2, 4));
        assert!(sel.contains(2, 5) && sel.contains(2, 79));
        assert!(sel.contains(3, 0) && sel.contains(3, 79));
        assert!(sel.contains(4, 0) && sel.contains(4, 10));
        assert!(!sel.contains(4, 11));
    }

    // Upstream: `anchored_not_visible`, `click_without_drag`, `drag_then_finish`.
    #[test]
    fn a_click_is_not_a_selection_until_the_pointer_moves() {
        let mut sel = Selection::anchor(5, 10);
        assert!(!sel.is_visible() && !sel.contains(5, 10));
        assert!(sel.is_just_click());
        assert!(!sel.finish());
        let mut sel = Selection::anchor(5, 10);
        sel.drag(7, 20);
        assert_eq!(sel.phase(), SelectionPhase::Dragging);
        assert!(sel.is_visible() && !sel.is_just_click());
        assert!(sel.finish());
        assert_eq!(sel.phase(), SelectionPhase::Done);
    }

    // Upstream: `visible_rects_match_selection_cells_when_scrolled_and_reversed`.
    #[test]
    fn rects_match_selected_cells_when_scrolled_and_reversed() {
        let inner = Rect::new(10, 5, 8, 6);
        for (start, end) in [
            ((0, 2), (0, 5)),
            ((1, 3), (4, 2)),
            ((4, 2), (1, 3)),
            ((0, 0), (20, 7)),
            ((20, 0), (22, 7)),
        ] {
            let sel = Selection::range(start, end);
            for top in [0, 2, 5, 20] {
                let rects = sel.rects(inner, top);
                for row in 0..inner.height {
                    for col in 0..inner.width {
                        let p = Position::new(inner.x + col, inner.y + row);
                        assert_eq!(
                            rects.iter().any(|r| r.contains(p)),
                            sel.contains(top + row as usize, col),
                            "start={start:?} end={end:?} top={top} row={row} col={col}"
                        );
                    }
                    for r in rects.iter().filter(|r| !r.is_empty()) {
                        assert_eq!(r.intersection(inner), *r, "inside the view");
                    }
                }
            }
        }
    }

    // Upstream: `drag_uses_buffer_rows_when_scrolled` and
    // `contains_tracks_current_viewport_after_scroll`: content rows, so the
    // selection stays on its text while the pager scrolls.
    #[test]
    fn a_selection_stays_on_its_rows_while_scrolling() {
        let mut p = numbered(50).scroll_to(10);
        paint(&p, 40, 11); // title, 9 content rows (y 1..10), rule
        assert!(!p.mouse_down(10, 1));
        assert_eq!(p.selection().map(|s| s.ordered().0), Some((10, 10)));
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        paint(&p, 40, 11);
        assert!(p.mouse_drag(4, 2));
        assert_eq!(
            p.selection().map(|s| s.ordered()),
            Some(((10, 10), (13, 4)))
        );
        assert!(p.mouse_up(4, 2));
        let text = p.selected_text().expect("text");
        assert_eq!(text.lines().count(), 4);
        assert!(text.starts_with("xxxxx"), "{text}");
        // Scroll on: the highlight moves up with its rows (row 13 now at the
        // top), the text holds.
        p.key(Key::Down.into());
        let buf = paint(&p, 40, 11);
        assert_eq!(
            p.selection_rects(),
            [Rect::default(), Rect::default(), Rect::new(0, 1, 5, 1)]
        );
        assert!(buf[(4, 1)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(5, 1)].modifier.contains(Modifier::REVERSED));
        assert_eq!(p.selected_text().as_deref(), Some(text.as_str()));
    }

    // Upstream: `clamp_to_pane_bounds`; plus herdr's edge autoscroll.
    #[test]
    fn dragging_past_the_edges_clamps_and_scrolls() {
        assert_eq!(
            [
                edge_scroll_rows(0),
                edge_scroll_rows(1),
                edge_scroll_rows(2),
                edge_scroll_rows(9)
            ],
            [3, 3, 6, 15]
        );
        let mut p = numbered(50);
        paint(&p, 40, 11);
        p.mouse_down(5, 3);
        assert!(p.mouse_drag(200, 10), "one row below the content");
        assert_eq!(p.scroll_offset(), 3);
        assert_eq!(p.selection().map(|s| s.ordered().1), Some((3 + 8, 39)));
        paint(&p, 40, 11);
        assert!(p.mouse_drag(0, 0), "above the content");
        assert_eq!(p.scroll_offset(), 0);
        assert_eq!(p.selection().map(|s| s.ordered().0), Some((0, 0)));
        // A click off the content clears the selection; a click is not one.
        assert!(p.mouse_up(0, 0));
        assert!(p.mouse_down(0, 10), "the rule row clears it");
        assert!(p.selection().is_none());
        p.mouse_down(3, 2);
        assert!(!p.mouse_up(3, 2));
        assert!(p.selection().is_none());
        // Before any paint the mouse does nothing.
        let mut fresh = numbered(3);
        assert!(!fresh.mouse_down(1, 1) && !fresh.mouse_drag(2, 2) && !fresh.mouse_up(2, 2));
    }

    /// herdr H5: truncation split graphemes. Selection reads whole graphemes
    /// by cell: a wide glyph is taken whole from either of its cells.
    #[test]
    fn selected_text_is_read_by_cells_with_wide_glyphs_whole() {
        let mut p = Pager::new(
            "t",
            vec![
                Line::from("한국어 text   "),
                Line::from("🇰🇷 team · 👍🏽ok · 👩\u{200d}💻👩\u{200d}💻"),
                Line::from("e\u{301}tude"),
            ],
        )
        .hints(false);
        paint(&p, 40, 6);
        // From the second cell of 한 to the first cell of 국.
        p.set_selection(Some(Selection::range((0, 1), (0, 2))));
        assert_eq!(p.selected_text().as_deref(), Some("한국"));
        // Trailing blanks trimmed.
        p.set_selection(Some(Selection::range((0, 7), (0, 30))));
        assert_eq!(p.selected_text().as_deref(), Some("text"));
        p.set_selection(Some(Selection::range((1, 0), (1, 0))));
        assert_eq!(p.selected_text().as_deref(), Some("🇰🇷"));
        p.set_selection(Some(Selection::range((1, 1), (2, 0))));
        assert_eq!(
            p.selected_text().as_deref(),
            Some("🇰🇷 team · 👍🏽ok · 👩\u{200d}💻👩\u{200d}💻\ne\u{301}")
        );
        // Rows past the end hold nothing.
        p.set_selection(Some(Selection::range((2, 0), (9, 5))));
        assert_eq!(p.selected_text().as_deref(), Some("e\u{301}tude"));
        assert_eq!(
            p.copy_sequence(osc52::Mux::None),
            Some(osc52::sequence("e\u{301}tude", osc52::Mux::None))
        );
    }

    #[test]
    fn double_click_selects_a_word() {
        let mut p = Pager::new(
            "t",
            vec![Line::from(
                "dev1@2lab.ai failed (http://localhost:3456/llmux/refresh): refused",
            )],
        )
        .hints(false);
        paint(&p, 80, 4);
        assert!(p.select_word(4, 1));
        assert_eq!(p.selected_text().as_deref(), Some("dev1@2lab.ai"));
        assert_eq!(p.selection().map(|s| s.phase()), Some(SelectionPhase::Done));
        assert!(p.select_word(30, 1));
        assert_eq!(
            p.selected_text().as_deref(),
            Some("http://localhost:3456/llmux/refresh")
        );
        assert!(!p.select_word(12, 1), "a space");
        assert!(!p.select_word(5, 3), "off the content");
    }

    #[test]
    fn the_highlight_is_a_builder_style_and_mono_has_no_colour() {
        let mut p = numbered(5).theme(Theme::mono());
        paint(&p, 40, 8);
        p.set_selection(Some(Selection::range((1, 2), (2, 3))));
        let buf = paint(&p, 40, 8);
        for c in buf.content() {
            assert!(matches!(c.fg, ratatui::style::Color::Reset), "{c:?}");
            assert!(matches!(c.bg, ratatui::style::Color::Reset), "{c:?}");
        }
        assert!(buf[(2, 2)].modifier.contains(Modifier::REVERSED));
        assert!(
            buf[(39, 2)].modifier.contains(Modifier::REVERSED),
            "to the row end"
        );
        assert!(buf[(3, 3)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(4, 3)].modifier.contains(Modifier::REVERSED));
        let mut p = p.selection_style(Style::new().add_modifier(Modifier::UNDERLINED));
        p.set_selection(Some(Selection::range((1, 2), (1, 3))));
        let buf = paint(&p, 40, 8);
        assert!(buf[(2, 2)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buf[(2, 2)].modifier.contains(Modifier::REVERSED));
        assert!(p.clear_selection());
        assert_eq!(p.selection_rects(), [Rect::default(); 3]);
        // Tiny areas: no panic, rects inside.
        p.set_selection(Some(Selection::range((0, 0), (4, 39))));
        for (w, h) in [(1, 1), (2, 2), (5, 3), (12, 2)] {
            paint(&p, w, h);
            for r in p.selection_rects() {
                assert!(r.right() <= w && r.bottom() <= h, "{w}x{h} {r:?}");
            }
        }
    }

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    // Upstream: release_notes.rs `release_notes_inline_code_spans_are_styled_without_backticks`.
    #[test]
    fn notes_inline_code_is_styled_without_backticks() {
        let t = Theme::ansi();
        let lines = notes("- `herdr pane run ...` now works", &t);
        assert_eq!(lines.len(), 1);
        assert_eq!(line_text(&lines[0]), " ∙ herdr pane run ... now works");
        let code = &lines[0].spans[3];
        assert_eq!(code.content.as_ref(), "herdr pane run ...");
        assert_eq!(code.style, t.accent().add_modifier(Modifier::BOLD));
    }

    // Upstream: `release_notes_config_inline_code_uses_nonbreaking_spaces`.
    #[test]
    fn notes_config_code_uses_non_breaking_spaces() {
        let lines = notes("- After: `new_tab = \"prefix+c\"`", &Theme::ansi());
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0].spans[4].content.as_ref(),
            "new_tab\u{a0}=\u{a0}\"prefix+c\""
        );
        assert_eq!(
            line_text(&lines[0]).replace('\u{a0}', " "),
            " ∙ After: new_tab = \"prefix+c\""
        );
    }

    // Upstream: `release_notes_fenced_code_blocks_render_as_preformatted_lines`
    // and `release_notes_fenced_code_blocks_preserve_blank_lines`.
    #[test]
    fn notes_fenced_blocks_are_verbatim_behind_a_gutter() {
        let t = Theme::ansi();
        let lines = notes(
            "### Fixed\n```bash\njust check\n- not a bullet\n```\n- after",
            &t,
        );
        let text: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(
            text,
            [" FIXED", "▐ just check", "▐ - not a bullet", " ∙ after"]
        );
        assert_eq!(lines[1].spans[0].style, t.accent());
        let lines = notes("```\nfirst\n\nsecond\n```", &t);
        let text: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(text, ["▐ first", "▐ ", "▐ second"]);
    }

    #[test]
    fn notes_headings_bullets_and_glyphs() {
        let t = Theme::mono();
        let md = "## Added\n\nPlain `text`.\n  - nested\n* star\n# 한국어 제목\nan `unclosed tick";
        let text: Vec<String> = notes(md, &t).iter().map(line_text).collect();
        assert_eq!(
            text,
            [
                " ADDED",
                "",
                " Plain text.",
                "   ∙ nested",
                " ∙ star",
                " 한국어 제목",
                " an `unclosed tick"
            ]
        );
        // Mono: headings and code bold, nothing coloured.
        for l in notes(md, &t) {
            for sp in &l.spans {
                assert_eq!((sp.style.fg, sp.style.bg), (None, None), "{sp:?}");
            }
        }
        assert!(
            notes(md, &t)[0].spans[1]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
        let herdr: Vec<String> = notes_with("- a\n```\nb\n```", &t, NotesGlyphs::herdr())
            .iter()
            .map(line_text)
            .collect();
        assert_eq!(herdr, [" • a", "▏ b"]);
        for g in [NotesGlyphs::default().bullet, NotesGlyphs::default().gutter] {
            assert_eq!(unicode_width::UnicodeWidthStr::width_cjk(g), 1, "{g}");
        }
    }
}
