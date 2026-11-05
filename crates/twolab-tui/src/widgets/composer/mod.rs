// Derived from vercel-labs/fx `src/ui/input/visual_layout.zig`,
// `src/ui/footer/input_presentation.zig` and `src/ui/input/shortcuts.zig` at
// commit 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025
// Vercel, Inc.): the composer's wrap (a word that fits the next row moves down
// whole, a word wider than a row breaks where it starts, spaces hang past the
// margin, a soft-wrap boundary belongs to the following row), the preferred
// column kept across vertical moves, the visible window that follows the
// caret, the two-cell prompt column with a marker for hidden rows, and the
// newline chords (Shift/Alt+Enter, Ctrl+J, `\` then Enter). Translated from
// Zig to Rust and re-expressed as ratatui text; rules checked against 2lab's
// xfx `src/tui/wrap.rs` (`composer`, `composer_point`) and
// `src/tui/editor.rs` (Apache-2.0) at 93f516b. Modified by 2lab.

//! What you will send next: a multi-line input that grows with its text.
//!
//! Ported from Vercel fx's composer (`src/ui/input/visual_layout.zig`,
//! Apache-2.0). The text soft-wraps by cells: a word that fits on a fresh row
//! moves down whole, a word wider than a row breaks where it starts, and the
//! spaces at a break hang past the margin so no row starts with one. A
//! prompt glyph marks the first row; the box grows up to
//! [`Composer::max_height`] rows and then scrolls so the caret's row stays in
//! view, with `⋮` in the prompt column on the side that has hidden rows.
//!
//! The caret is drawn as a reversed cell — the host keeps the terminal's own
//! cursor (or places it with [`Composer::cursor_position`]). Editing is by
//! grapheme cluster, so a Korean syllable or an emoji family is one step and
//! one Backspace. Enter submits ([`ComposerEvent::Submitted`]); Shift+Enter,
//! Alt+Enter, Ctrl+J, or `\` then Enter insert a line break. ↑↓ move by
//! visual row and remember the column they aimed for; Home and End go to the
//! start and end of the line (the text between line breaks), as readline does.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;
use unicode_segmentation::UnicodeSegmentation;

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::Theme;
use crate::widgets::choice_form::{next_grapheme, prev_grapheme, snap_to_grapheme};

/// Cells of the prompt column: the glyph and a space.
pub const PROMPT_WIDTH: u16 = 2;
/// Rows the composer grows to before it scrolls.
pub const DEFAULT_MAX_HEIGHT: u16 = 6;
/// The most text the composer holds; an insert past it is refused whole
/// (fx `paste_framing.zig`, 8 MiB).
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
/// Cells a pasted tab becomes.
const TAB_CELLS: usize = 4;

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerEvent {
    /// Not a key the composer uses (Esc, Tab, an arrow at the edge): the
    /// host may give it another meaning, such as history.
    Ignored,
    /// The caret moved.
    Moved,
    /// The text changed.
    Edited,
    /// Enter on text that is not blank: the text, taken out of the composer.
    Submitted(String),
}

/// The text, the caret, and the view's scroll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerState {
    text: String,
    /// A byte offset into `text`, always on a grapheme boundary.
    cursor: usize,
    /// The column vertical moves aim for, while a run of them lasts.
    sticky: Option<usize>,
    /// The first visible row, as last drawn.
    scroll: usize,
    /// Rows a page moves, as last drawn.
    page: usize,
}

impl Default for ComposerState {
    fn default() -> Self {
        ComposerState {
            text: String::new(),
            cursor: 0,
            sticky: None,
            scroll: 0,
            page: DEFAULT_MAX_HEIGHT as usize - 1,
        }
    }
}

/// `s` as composer text: CR LF and CR become LF, a tab becomes spaces, and
/// every other control character is dropped — so nothing in the draft is a
/// byte a terminal would obey.
fn clean(s: &str) -> Cow<'_, str> {
    if !s.chars().any(|c| c.is_control() && c != '\n') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' => out.push('\n'),
            '\t' => out.extend(std::iter::repeat_n(' ', TAB_CELLS)),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Cells of `s`, cluster by cluster — the same sum the wrap uses.
fn cells(s: &str) -> usize {
    s.graphemes(true).map(text::width).sum()
}

/// One visual row: the text's `start..body_end` is shown, `end` is where the
/// next row starts (past the line break of a hard row), `width` what the body
/// measures (wider than the row when spaces hang).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VRow {
    start: usize,
    body_end: usize,
    end: usize,
    width: usize,
}

/// The cells of the word that starts at `index`, up to a space or a line
/// break, and no further than one past `cap` (fx `measureWordWidth`).
fn word_width(text: &str, index: usize, cap: usize) -> usize {
    let mut w = 0;
    for g in text[index..].graphemes(true) {
        if g == " " || g == "\n" || g == "\r\n" {
            break;
        }
        w += text::width(g);
        if w > cap {
            break;
        }
    }
    w
}

/// The composer's rows at `cols` cells (fx `visual_layout.zig`; xfx
/// `wrap::composer`).
fn layout(text: &str, cols: usize) -> Vec<VRow> {
    let available = cols.max(1);
    let mut rows = Vec::new();
    let (mut start, mut used, mut index) = (0usize, 0usize, 0usize);
    let mut at_word_start = true;
    let mut force_wrap = false;
    while index < text.len() {
        let cluster = text[index..]
            .graphemes(true)
            .next()
            .unwrap_or(&text[index..]);
        let end = index + cluster.len();
        if cluster == "\n" || cluster == "\r\n" {
            rows.push(VRow {
                start,
                body_end: index,
                end,
                width: used,
            });
            (start, index, used) = (end, end, 0);
            at_word_start = true;
            force_wrap = false;
            continue;
        }
        let w = text::width(cluster);
        let space = cluster == " ";
        let word_moves =
            !space && at_word_start && used > 0 && !force_wrap && used < available && {
                let word = word_width(text, index, available);
                word > available - used && word <= available
            };
        let crosses =
            w > 0 && (force_wrap || (used > 0 && (used >= available || used + w > available)));
        if !space && (word_moves || crosses) {
            rows.push(VRow {
                start,
                body_end: index,
                end: index,
                width: used,
            });
            start = index;
            used = 0;
            at_word_start = true;
            force_wrap = false;
            continue;
        }
        if used == 0 && w > available {
            force_wrap = true;
        }
        used += w;
        at_word_start = space;
        index = end;
    }
    rows.push(VRow {
        start,
        body_end: text.len(),
        end: text.len(),
        width: used,
    });
    rows
}

/// Which row a byte offset is on and the cells to its left: an offset on a
/// soft-wrap boundary belongs to the following row (fx `pendingResolvesAt`).
fn point(text: &str, rows: &[VRow], cursor: usize) -> (usize, usize) {
    for (i, r) in rows.iter().enumerate() {
        let last = i + 1 == rows.len();
        if cursor >= r.end && !last {
            continue;
        }
        if cursor >= r.body_end {
            return (i, r.width);
        }
        return (i, cells(&text[r.start..cursor.max(r.start)]));
    }
    (0, 0)
}

/// Where the caret is drawn: its row and column, a column at or past the
/// margin moved onto a fresh row after the last one (fx `lineCount`'s extra
/// row) or onto the last cell of an inner row whose spaces hang.
fn display_point(text: &str, rows: &[VRow], cursor: usize, cols: usize) -> (usize, usize, bool) {
    let (row, col) = point(text, rows, cursor);
    if col < cols {
        (row, col, false)
    } else if row + 1 == rows.len() {
        (row + 1, 0, true)
    } else {
        (row, cols.saturating_sub(1), false)
    }
}

/// The offset on `row` nearest the column `target` without passing it; a
/// soft row cannot end on its own end, which is the next row's start (xfx
/// `Editor::offset_on`).
fn offset_on(text: &str, row: &VRow, last: bool, target: usize) -> usize {
    let soft = row.body_end == row.end && !last;
    let (mut best, mut used, mut at) = (row.start, 0usize, row.start);
    while at < row.body_end && used <= target {
        best = at;
        let g = text[at..row.body_end].graphemes(true).next().unwrap_or("");
        used += text::width(g);
        at += g.len().max(1);
    }
    if !soft && at >= row.body_end && used <= target {
        best = row.body_end;
    }
    best
}

/// The first row a window of `limit` rows shows, moved as little as it must
/// to keep the caret's row in view.
fn view_start(scroll: usize, caret_row: usize, total: usize, limit: usize) -> usize {
    let limit = limit.max(1);
    if total <= limit {
        return 0;
    }
    let mut start = scroll;
    if caret_row < start {
        start = caret_row;
    } else if caret_row >= start + limit {
        start = caret_row + 1 - limit;
    }
    start.min(total - limit)
}

fn text_cols(width: u16) -> usize {
    (width.saturating_sub(PROMPT_WIDTH) as usize).max(1)
}

impl ComposerState {
    pub fn new() -> ComposerState {
        ComposerState::default()
    }

    /// Holding `text`, the caret at its end.
    pub fn with_text(text: &str) -> ComposerState {
        let mut s = ComposerState::default();
        s.set_text(text);
        s
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret's byte offset into [`ComposerState::text`].
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Replace the text (cleaned as a paste is), the caret at its end.
    pub fn set_text(&mut self, text: &str) {
        let mut t = clean(text).into_owned();
        if t.len() > MAX_BYTES {
            let mut cut = MAX_BYTES;
            while !t.is_char_boundary(cut) {
                cut -= 1;
            }
            t.truncate(cut);
        }
        self.text = t;
        self.cursor = self.text.len();
        self.sticky = None;
        self.scroll = 0;
    }

    /// Empty the composer.
    pub fn clear(&mut self) {
        self.set_text("");
    }

    /// Take the text out, leaving the composer empty.
    pub fn take(&mut self) -> String {
        let t = std::mem::take(&mut self.text);
        self.clear();
        t
    }

    /// Insert `s` at the caret — typing or a paste. Line breaks are kept
    /// (CR LF and CR become LF), tabs become spaces, other control characters
    /// are dropped. Refused whole past [`MAX_BYTES`].
    pub fn insert_str(&mut self, s: &str) -> bool {
        let s = clean(s);
        if s.is_empty() || self.text.len() + s.len() > MAX_BYTES {
            return false;
        }
        self.text.insert_str(self.cursor, &s);
        self.cursor = snap_to_grapheme(&self.text, self.cursor + s.len());
        self.sticky = None;
        true
    }

    fn delete(&mut self, from: usize, to: usize) -> ComposerEvent {
        if from >= to {
            return ComposerEvent::Ignored;
        }
        self.text.replace_range(from..to, "");
        self.cursor = snap_to_grapheme(&self.text, from);
        self.sticky = None;
        ComposerEvent::Edited
    }

    fn move_to(&mut self, at: usize) -> ComposerEvent {
        self.sticky = None;
        if at == self.cursor {
            return ComposerEvent::Ignored;
        }
        self.cursor = at;
        ComposerEvent::Moved
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }

    /// Back over spaces, then over the word before them.
    fn word_left(&self) -> usize {
        let mut at = self.cursor;
        while at > 0 && self.text[..at].ends_with(char::is_whitespace) {
            at = prev_grapheme(&self.text, at);
        }
        while at > 0 && !self.text[..at].ends_with(char::is_whitespace) {
            at = prev_grapheme(&self.text, at);
        }
        at
    }

    /// Over the word ahead, then over the spaces after it.
    fn word_right(&self) -> usize {
        let mut at = self.cursor;
        let n = self.text.len();
        while at < n && self.text[at..].starts_with(char::is_whitespace) {
            at = next_grapheme(&self.text, at);
        }
        while at < n && !self.text[at..].starts_with(char::is_whitespace) {
            at = next_grapheme(&self.text, at);
        }
        at
    }

    /// Where the caret is drawn at an area `width` wide: its row and column
    /// in the text (not the window).
    pub fn caret(&self, width: u16) -> (usize, usize) {
        let cols = text_cols(width);
        let rows = layout(&self.text, cols);
        let (r, c, _) = display_point(&self.text, &rows, self.cursor, cols);
        (r, c)
    }

    /// Rows the text takes at an area `width` wide (one for an empty text).
    pub fn rows(&self, width: u16) -> usize {
        let cols = text_cols(width);
        let rows = layout(&self.text, cols);
        let (_, _, extra) = display_point(&self.text, &rows, self.cursor, cols);
        rows.len() + usize::from(extra)
    }

    /// `count` visual rows up or down, aiming at the remembered column; at
    /// the first or the last row the caret goes to the start or the end of
    /// the text, and only then is the key [`ComposerEvent::Ignored`].
    fn vertical(&mut self, down: bool, count: usize, width: u16) -> ComposerEvent {
        let cols = text_cols(width);
        let rows = layout(&self.text, cols);
        let (row, col, _) = display_point(&self.text, &rows, self.cursor, cols);
        let target = self.sticky.unwrap_or(col);
        let wanted = if down {
            (row + count).min(rows.len() - 1)
        } else {
            row.saturating_sub(count)
        };
        let edge = if down { self.text.len() } else { 0 };
        if (down && row + 1 >= rows.len()) || (!down && row == 0) || wanted == row {
            return self.move_to(edge);
        }
        let last = wanted + 1 == rows.len();
        self.cursor = offset_on(&self.text, &rows[wanted], last, target);
        self.sticky = Some(target);
        ComposerEvent::Moved
    }

    /// Apply a key. `width` is the width of the area the composer is drawn
    /// in (the same one given to render): ↑↓ move by the rows it wraps to.
    pub fn key(&mut self, key: KeyInput, width: u16) -> ComposerEvent {
        self.cursor = snap_to_grapheme(&self.text, self.cursor);
        let m = key.mods;
        if m.ctrl {
            return match key.key {
                Key::Char('j') => self.newline(),
                Key::Char('a') => self.move_to(self.line_start()),
                Key::Char('e') => self.move_to(self.line_end()),
                Key::Char('u') => self.delete(self.line_start(), self.cursor),
                Key::Char('k') => self.delete(self.cursor, self.line_end()),
                Key::Char('w') | Key::Backspace => self.delete(self.word_left(), self.cursor),
                Key::Left => self.move_to(self.word_left()),
                Key::Right => self.move_to(self.word_right()),
                Key::Home => self.move_to(0),
                Key::End => self.move_to(self.text.len()),
                _ => ComposerEvent::Ignored,
            };
        }
        match key.key {
            Key::Enter if m.shift || m.alt => self.newline(),
            Key::Enter => {
                // fx: a backslash before the caret turns Enter into a newline.
                if self.text[..self.cursor].ends_with('\\') {
                    self.text.replace_range(self.cursor - 1..self.cursor, "\n");
                    self.sticky = None;
                    return ComposerEvent::Edited;
                }
                if self.text.trim().is_empty() {
                    return ComposerEvent::Ignored;
                }
                ComposerEvent::Submitted(self.take())
            }
            Key::Char(c) if !m.alt => {
                let mut b = [0u8; 4];
                if self.insert_str(c.encode_utf8(&mut b)) {
                    ComposerEvent::Edited
                } else {
                    ComposerEvent::Ignored
                }
            }
            Key::Backspace if m.alt => self.delete(self.word_left(), self.cursor),
            Key::Backspace => self.delete(prev_grapheme(&self.text, self.cursor), self.cursor),
            Key::Delete => self.delete(self.cursor, next_grapheme(&self.text, self.cursor)),
            Key::Left if m.alt => self.move_to(self.word_left()),
            Key::Right if m.alt => self.move_to(self.word_right()),
            Key::Left => self.move_to(prev_grapheme(&self.text, self.cursor)),
            Key::Right => self.move_to(next_grapheme(&self.text, self.cursor)),
            Key::Home => self.move_to(self.line_start()),
            Key::End => self.move_to(self.line_end()),
            Key::Up => self.vertical(false, 1, width),
            Key::Down => self.vertical(true, 1, width),
            Key::PageUp => self.vertical(false, self.page.max(1), width),
            Key::PageDown => self.vertical(true, self.page.max(1), width),
            _ => ComposerEvent::Ignored,
        }
    }

    fn newline(&mut self) -> ComposerEvent {
        if self.insert_str("\n") {
            ComposerEvent::Edited
        } else {
            ComposerEvent::Ignored
        }
    }
}

/// A growing multi-line input with a prompt glyph and a drawn caret.
#[derive(Debug, Clone)]
pub struct Composer<'a> {
    prompt: Cow<'a, str>,
    placeholder: Cow<'a, str>,
    max_height: u16,
    theme: Theme,
}

impl Default for Composer<'_> {
    fn default() -> Self {
        Composer::new()
    }
}

impl<'a> Composer<'a> {
    pub fn new() -> Composer<'a> {
        Composer {
            prompt: Cow::Borrowed("\u{276f}"),
            placeholder: Cow::Borrowed(""),
            max_height: DEFAULT_MAX_HEIGHT,
            theme: Theme::default(),
        }
    }

    /// The glyph on the first row (one cell; default `❯`).
    pub fn prompt(mut self, p: impl Into<Cow<'a, str>>) -> Self {
        self.prompt = p.into();
        self
    }

    /// Dim text shown while the composer is empty.
    pub fn placeholder(mut self, p: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = p.into();
        self
    }

    /// Rows it grows to before it scrolls (at least 1).
    pub fn max_height(mut self, rows: u16) -> Self {
        self.max_height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Rows to give it at `width`: its rows, up to the max height.
    pub fn height(&self, state: &ComposerState, width: u16) -> u16 {
        (state.rows(width).min(self.max_height as usize) as u16).max(1)
    }

    /// Where the terminal cursor would go, for a host that shows its own
    /// instead of the drawn caret. `None` when the area is empty.
    pub fn cursor_position(&self, state: &ComposerState, area: Rect) -> Option<Position> {
        if area.is_empty() {
            return None;
        }
        let limit = area.height.min(self.max_height) as usize;
        let (row, col) = state.caret(area.width);
        let start = view_start(state.scroll, row, state.rows(area.width), limit);
        let x = area.x + PROMPT_WIDTH + col as u16;
        let y = area.y + (row - start) as u16;
        (x < area.right() && y < area.bottom()).then_some(Position { x, y })
    }

    fn gutter(&self, glyph: &str, style: Style) -> Span<'static> {
        let g = text::truncate(glyph, 1).into_owned();
        Span::styled(
            text::pad(&g, PROMPT_WIDTH as usize, text::Align::Left),
            style,
        )
    }
}

impl StatefulWidget for Composer<'_> {
    type State = ComposerState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ComposerState) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl StatefulWidget for &Composer<'_> {
    type State = ComposerState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ComposerState) {
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        let caret = Style::new().add_modifier(Modifier::REVERSED);
        let cols = text_cols(area.width);
        let limit = area.height.min(self.max_height) as usize;
        let text_x = area.x + PROMPT_WIDTH.min(area.width);
        state.cursor = snap_to_grapheme(&state.text, state.cursor);
        state.page = limit.saturating_sub(1).max(1);

        if state.text.is_empty() {
            state.scroll = 0;
            let mut spans = vec![self.gutter(&self.prompt, t.accent())];
            let (first, rest) = match self.placeholder.graphemes(true).next() {
                Some(g) => (g.to_string(), self.placeholder[g.len()..].to_string()),
                None => (" ".to_string(), String::new()),
            };
            spans.push(Span::styled(first, t.dim().patch(caret)));
            spans.push(Span::styled(rest, t.dim()));
            let line = text::truncate_line(Line::from(spans), area.width as usize);
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        }

        let rows = layout(&state.text, cols);
        let (crow, ccol, extra) = display_point(&state.text, &rows, state.cursor, cols);
        let total = rows.len() + usize::from(extra);
        let start = view_start(state.scroll, crow, total, limit);
        state.scroll = start;
        let end = (start + limit).min(total);

        for (dy, r) in (start..end).enumerate() {
            let y = area.y + dy as u16;
            // `⋮` on the side that has hidden rows.
            let gutter = if (r == start && start > 0) || (r + 1 == end && end < total) {
                self.gutter("\u{22ee}", t.dim())
            } else if r == 0 {
                self.gutter(&self.prompt, t.accent())
            } else {
                self.gutter("", Style::new())
            };
            let mut spans = vec![gutter];
            if let Some(row) = rows.get(r) {
                // Hanging spaces past the margin are clipped; they are spaces.
                let (body, _) = text::take_cells(&state.text[row.start..row.body_end], cols);
                spans.push(Span::styled(body.to_string(), t.text()));
            }
            let line = text::truncate_line(Line::from(spans), area.width as usize);
            buf.set_line(area.x, y, &line, area.width);
            if r == crow {
                let under = state.text[state.cursor..]
                    .graphemes(true)
                    .next()
                    .filter(|g| *g != "\n" && *g != "\r\n" && !extra)
                    .map_or(1, |g| text::width(g).max(1));
                let x = text_x + ccol as u16;
                if x < area.right() {
                    let w = (under as u16).min(area.right() - x);
                    buf.set_style(Rect::new(x, y, w, 1), caret);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Mods;
    use crate::snapshot;
    use ratatui::style::Color;

    fn texts<'t>(text: &'t str, rows: &[VRow]) -> Vec<&'t str> {
        rows.iter().map(|r| &text[r.start..r.body_end]).collect()
    }

    fn shift(k: Key) -> KeyInput {
        KeyInput {
            key: k,
            mods: Mods {
                shift: true,
                ..Mods::default()
            },
        }
    }

    fn typed(st: &mut ComposerState, s: &str, w: u16) {
        for c in s.chars() {
            st.key(c.into(), w);
        }
    }

    fn render(c: &Composer, st: &mut ComposerState, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        StatefulWidget::render(c, buf.area, &mut buf, st);
        buf
    }

    // fx visual_layout.zig cases, via xfx wrap.rs tests (text width only).
    #[test]
    fn words_that_fit_the_next_row_move_down_whole() {
        let t = "hello brave world";
        let rows = layout(t, 10);
        assert_eq!(texts(t, &rows), ["hello ", "brave ", "world"]);
        assert_eq!(point(t, &rows, "hello ".len()), (1, 0));
        let t = "aaaa bbbbbb cc";
        assert_eq!(texts(t, &layout(t, 10)), ["aaaa ", "bbbbbb cc"]);
    }

    #[test]
    fn a_word_wider_than_a_row_breaks_where_it_starts() {
        let t = "ab cdefghijklm";
        let rows = layout(t, 10);
        assert_eq!(texts(t, &rows), ["ab cdefghi", "jklm"]);
        assert_eq!(rows[0].width, 10);
    }

    #[test]
    fn spaces_hang_at_the_margin() {
        for t in ["abcdefghij kl", "abcdefghij   kl"] {
            assert_eq!(texts(t, &layout(t, 10))[1], "kl", "{t:?}");
        }
    }

    #[test]
    fn a_soft_wrap_boundary_belongs_to_the_following_row() {
        let t = "abcd";
        let rows = layout(t, 3);
        assert_eq!(texts(t, &rows), ["abc", "d"]);
        assert_eq!(point(t, &rows, 3), (1, 0));
        let t = "a\u{754c}";
        let rows = layout(t, 2);
        assert_eq!(texts(t, &rows), ["a", "\u{754c}"]);
        assert_eq!(point(t, &rows, 1), (1, 0));
    }

    #[test]
    fn a_caret_at_the_end_of_a_full_last_row_starts_a_fresh_row() {
        let mut st = ComposerState::with_text("abcdefgh");
        // 10 wide: 8 cells of text.
        assert_eq!(st.caret(10), (1, 0));
        assert_eq!(st.rows(10), 2);
        st.key(Key::Left.into(), 10);
        assert_eq!(st.caret(10), (0, 7));
        assert_eq!(st.rows(10), 1);
    }

    #[test]
    fn enter_submits_and_shift_alt_ctrl_j_and_backslash_insert_a_newline() {
        let mut st = ComposerState::new();
        assert_eq!(
            st.key(Key::Enter.into(), 40),
            ComposerEvent::Ignored,
            "blank"
        );
        typed(&mut st, "one", 40);
        assert_eq!(st.key(shift(Key::Enter), 40), ComposerEvent::Edited);
        typed(&mut st, "two", 40);
        let alt = KeyInput {
            key: Key::Enter,
            mods: Mods {
                alt: true,
                ..Mods::default()
            },
        };
        st.key(alt, 40);
        st.key(KeyInput::ctrl(Key::Char('j')), 40);
        typed(&mut st, "three\\", 40);
        assert_eq!(st.key(Key::Enter.into(), 40), ComposerEvent::Edited);
        typed(&mut st, "four", 40);
        assert_eq!(
            st.key(Key::Enter.into(), 40),
            ComposerEvent::Submitted("one\ntwo\n\nthree\nfour".into())
        );
        assert!(st.is_empty(), "submitting takes the text out");
    }

    #[test]
    fn editing_is_by_grapheme_for_korean_and_emoji() {
        let mut st = ComposerState::new();
        typed(&mut st, "안녕 👩\u{200d}💻 e\u{301}", 40);
        st.key(Key::Backspace.into(), 40);
        assert_eq!(st.text(), "안녕 👩\u{200d}💻 ", "é is one cluster");
        st.key(Key::Left.into(), 40);
        st.key(Key::Backspace.into(), 40);
        assert_eq!(st.text(), "안녕  ", "the coder is one cluster");
        st.key(Key::Home.into(), 40);
        st.key(Key::Delete.into(), 40);
        assert_eq!(st.text(), "녕  ");
        st.key(Key::Right.into(), 40);
        typed(&mut st, "하세요", 40);
        assert_eq!(st.text(), "녕하세요  ");
        // A joiner typed between two emoji makes one cluster; the caret is
        // never left inside it.
        let mut st = ComposerState::with_text("👨👩");
        st.key(Key::Left.into(), 40);
        st.key('\u{200d}'.into(), 40);
        assert_eq!(st.text(), "👨\u{200d}👩");
        assert_eq!(st.cursor(), st.text().len());
    }

    #[test]
    fn up_and_down_move_by_visual_row_and_keep_their_column() {
        // 12 wide is 10 cells of text: "abcdefgh ", "ij", "xy", "longer ", "line".
        let mut st = ComposerState::with_text("abcdefgh ij\nxy\nlonger line");
        assert_eq!(st.rows(12), 5);
        assert_eq!(st.caret(12), (4, 4), "after `line`");
        st.key(Key::Up.into(), 12);
        assert_eq!(st.caret(12), (3, 4), "into `longer `");
        st.key(Key::Up.into(), 12);
        assert_eq!(st.caret(12), (2, 2), "the short row ends the move");
        st.key(Key::Up.into(), 12);
        assert_eq!(st.caret(12), (1, 2));
        st.key(Key::Up.into(), 12);
        assert_eq!(st.caret(12), (0, 4), "and the column is remembered");
        assert_eq!(st.key(Key::Up.into(), 12), ComposerEvent::Moved);
        assert_eq!(st.cursor(), 0, "Up on the first row goes to the start");
        assert_eq!(
            st.key(Key::Up.into(), 12),
            ComposerEvent::Ignored,
            "then it is the host's"
        );
        st.key(Key::Down.into(), 12);
        assert_eq!(st.caret(12), (1, 0));
        st.key(Key::End.into(), 12);
        assert_eq!(
            st.cursor(),
            "abcdefgh ij".len(),
            "End goes to the end of the line"
        );
        st.key(Key::Home.into(), 12);
        assert_eq!(st.cursor(), 0, "Home to its start");
    }

    #[test]
    fn word_and_line_kills() {
        let mut st = ComposerState::with_text("git commit -m wip");
        st.key(KeyInput::ctrl(Key::Char('w')), 40);
        assert_eq!(st.text(), "git commit -m ");
        st.key(KeyInput::ctrl(Key::Left), 40);
        st.key(KeyInput::ctrl(Key::Char('k')), 40);
        assert_eq!(st.text(), "git commit ");
        st.key(KeyInput::ctrl(Key::Char('u')), 40);
        assert_eq!(st.text(), "");
    }

    #[test]
    fn a_paste_keeps_line_breaks_and_drops_controls() {
        let mut st = ComposerState::new();
        assert!(st.insert_str("a\r\nb\rc\td\u{1b}[2Je"));
        assert_eq!(st.text(), "a\nb\nc    d[2Je");
        assert!(!st.insert_str(""));
    }

    #[test]
    fn it_draws_a_prompt_the_text_and_a_reversed_caret() {
        let c = Composer::new().theme(Theme::mono());
        let mut st = ComposerState::with_text("hello world");
        st.key(Key::Home.into(), 20);
        let buf = render(&c, &mut st, 20, 3);
        assert_eq!(snapshot::lines(&buf), ["\u{276f} hello world", "", ""]);
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(3, 0)].modifier.contains(Modifier::REVERSED));
        assert!(
            buf[(0, 0)].modifier.contains(Modifier::BOLD),
            "the prompt is the accent"
        );
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        assert_eq!(
            c.cursor_position(&st, buf.area),
            Some(Position { x: 2, y: 0 })
        );
    }

    #[test]
    fn a_wide_grapheme_under_the_caret_is_reversed_whole() {
        let c = Composer::new();
        let mut st = ComposerState::with_text("가나");
        st.key(Key::Home.into(), 20);
        let buf = render(&c, &mut st, 20, 1);
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(3, 0)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(4, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn empty_shows_the_placeholder_behind_the_caret() {
        let c = Composer::new()
            .placeholder("Ask anything")
            .theme(Theme::dark());
        let mut st = ComposerState::new();
        let buf = render(&c, &mut st, 30, 2);
        assert_eq!(snapshot::lines(&buf)[0], "\u{276f} Ask anything");
        assert!(buf[(2, 0)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buf[(5, 0)].fg, Theme::dark().muted);
        assert_eq!(c.height(&st, 30), 1);
    }

    #[test]
    fn it_grows_to_its_max_height_then_follows_the_caret() {
        let c = Composer::new().max_height(3);
        let mut st = ComposerState::with_text("one\ntwo\nthree\nfour\nfive");
        assert_eq!(c.height(&st, 20), 3);
        let rows = snapshot::lines(&render(&c, &mut st, 20, 3));
        assert_eq!(rows, ["\u{22ee} three", "  four", "  five"]);
        for _ in 0..4 {
            st.key(Key::Up.into(), 20);
        }
        let rows = snapshot::lines(&render(&c, &mut st, 20, 3));
        assert_eq!(rows, ["\u{276f} one", "  two", "\u{22ee} three"]);
        // Moving inside the window does not scroll it.
        st.key(Key::Down.into(), 20);
        let rows = snapshot::lines(&render(&c, &mut st, 20, 3));
        assert_eq!(rows[0], "\u{276f} one");
        st.key(Key::PageDown.into(), 20);
        let rows = snapshot::lines(&render(&c, &mut st, 20, 3));
        assert_eq!(rows, ["\u{22ee} two", "  three", "\u{22ee} four"]);
    }

    #[test]
    fn a_long_paste_wraps_by_cells_and_never_draws_past_the_edge() {
        let c = Composer::new().max_height(4);
        let mut st = ComposerState::new();
        st.insert_str(&"한국어 텍스트와 emoji 🎉 섞인 긴 붙여넣기 ".repeat(8));
        let buf = render(&c, &mut st, 24, 4);
        for row in snapshot::lines(&buf) {
            assert!(text::width(&row) <= 24, "{row:?}");
        }
        assert!(snapshot::lines(&buf)[0].starts_with('\u{22ee}'));
        let pos = c.cursor_position(&st, buf.area).expect("on screen");
        assert_eq!(pos.y, 3, "the caret's row is the last one shown");
    }

    #[test]
    fn tiny_areas_do_not_panic() {
        let c = Composer::new().placeholder("type here");
        for text in [
            "",
            "abc",
            "한글 텍스트\n둘째 줄",
            "👩\u{200d}💻👩\u{200d}💻",
        ] {
            let mut st = ComposerState::with_text(text);
            for (w, h) in [(1, 1), (2, 1), (3, 2), (12, 2)] {
                render(&c, &mut st, w, h);
                st.key(Key::Up.into(), w);
                st.key(Key::Down.into(), w);
                st.key(Key::PageUp.into(), w);
            }
        }
    }
}
