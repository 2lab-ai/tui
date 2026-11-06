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
//
// Derived from herdrdev/herdr `src/client/shell/text_editor.rs`
// (`TextEditor::insert`, `remove`, `handle_key`, `viewport`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0): the single-line
// field, the kill buffer and Ctrl+Y yank, `replace_on_type`, the readline
// keys Ctrl+B/F/D/H and Alt+B/F/D, and the horizontal window that keeps the
// caret in view. Modified by 2lab: the window moves only as far as it must
// and says what it hides (`‹` in the prompt column, `›` at the right edge)
// instead of clipping silently; a preset is drawn as a selection; words stay
// whitespace-separated as in the rest of the composer; a masked mode that
// never draws the secret.

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
//!
//! Ctrl+U, Ctrl+K and Ctrl+W (or Alt/Ctrl+Backspace, Alt+D) put what they cut
//! in the state's kill buffer and Ctrl+Y puts it back at the caret, as in
//! readline (herdr's `text_editor.rs`).
//!
//! # One line, a preset, a secret
//!
//! - [`ComposerState::single_line`] makes a field — a rename, a search, a
//!   filter. It never wraps: a window of the text follows the caret, moving
//!   only as far as it must, and `‹` in the prompt column or `›` at the right
//!   edge says there is more text that way. Line breaks in a paste become
//!   spaces; Enter submits and ↑↓ are left to the host.
//! - [`ComposerState::replace_on_type`] marks the text as a preset (a
//!   suggested branch name): it is drawn as a selection, the first typed key
//!   or paste replaces it whole, Backspace empties it, and a move keeps it.
//! - [`ComposerState::masked`] is for an API key or a password. The text is
//!   drawn as one [`Mask`] glyph per grapheme (or a fixed run that hides even
//!   the length), never as itself; [`ComposerState::text`], `take`, `Debug`
//!   and the rendered cells all keep it hidden. Enter answers
//!   [`ComposerEvent::SubmittedSecret`] and the host reads the text once with
//!   [`ComposerState::take_secret`].
//!
//! [`Composer::line`] gives the one-line view as a `Line` for a host that
//! places the field inside its own paragraph (a prompt in a footer).

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
    /// Enter on a masked composer that is not blank. The text stays in the
    /// state, hidden, until the host takes it with
    /// [`ComposerState::take_secret`] — so it never travels in an event a
    /// host might log.
    SubmittedSecret,
}

/// The glyph a masked composer draws its text as (`∙`, U+2219): one cell on
/// every terminal, CJK ones included (`•` is East-Asian Ambiguous, two cells
/// there).
pub const MASK_GLYPH: char = '\u{2219}';

/// How a masked composer draws its text: never as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Mask {
    glyph: char,
    /// `Some(n)`: always `n` glyphs, whatever the length.
    fixed: Option<u16>,
}

impl Default for Mask {
    fn default() -> Self {
        Mask::each(MASK_GLYPH)
    }
}

impl Mask {
    /// One `glyph` per grapheme typed: the length shows, the text does not.
    /// A glyph that is not one cell wide falls back to [`MASK_GLYPH`].
    pub fn each(glyph: char) -> Mask {
        Mask {
            glyph: Mask::one_cell(glyph),
            fixed: None,
        }
    }

    /// Always `count` glyphs (at least one) once anything is typed: not even
    /// the length shows. The caret stays at the end, since a caret moving
    /// through glyphs that do not move would hide where an edit lands.
    pub fn fixed(glyph: char, count: u16) -> Mask {
        Mask {
            glyph: Mask::one_cell(glyph),
            fixed: Some(count.max(1)),
        }
    }

    pub fn glyph(&self) -> char {
        self.glyph
    }

    fn one_cell(glyph: char) -> char {
        let mut b = [0u8; 4];
        if text::width(glyph.encode_utf8(&mut b)) == 1 {
            glyph
        } else {
            MASK_GLYPH
        }
    }
}

/// The text, the caret, and the view's scroll.
#[derive(Clone, PartialEq, Eq)]
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
    /// One-line mode: the first visible byte of the window, as last drawn.
    hscroll: usize,
    single_line: bool,
    /// The text is a preset the first typed key replaces.
    replace: bool,
    /// What the last Ctrl+U / Ctrl+K / Ctrl+W cut, for Ctrl+Y.
    killed: String,
    mask: Option<Mask>,
}

impl Default for ComposerState {
    fn default() -> Self {
        ComposerState {
            text: String::new(),
            cursor: 0,
            sticky: None,
            scroll: 0,
            page: DEFAULT_MAX_HEIGHT as usize - 1,
            hscroll: 0,
            single_line: false,
            replace: false,
            killed: String::new(),
            mask: None,
        }
    }
}

/// A masked state's text and kill buffer never reach a log.
impl std::fmt::Debug for ComposerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hidden = |s: &str| -> String {
            if self.mask.is_some() {
                "<masked>".to_string()
            } else {
                format!("{s:?}")
            }
        };
        let (text, killed) = (hidden(&self.text), hidden(&self.killed));
        f.debug_struct("ComposerState")
            .field("text", &format_args!("{text}"))
            .field("cursor", &self.cursor)
            .field("single_line", &self.single_line)
            .field("replace_on_type", &self.replace)
            .field("killed", &format_args!("{killed}"))
            .field("mask", &self.mask)
            .finish_non_exhaustive()
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

/// `s` as one-line text (herdr `TextEditor::insert`): CR LF, CR, LF and a
/// tab become one space each — or, in a secret, nothing — and every other
/// control character is dropped.
fn clean_line(s: &str, secret: bool) -> Cow<'_, str> {
    if !s.chars().any(char::is_control) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' | '\t' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                if !secret {
                    out.push(' ');
                }
            }
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

/// What the one-line view shows of a text: bytes `start..end`, whether text
/// is hidden on either side, and the caret's cell in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Strip {
    start: usize,
    end: usize,
    left: bool,
    right: bool,
    caret: usize,
}

/// The one-line window over `text` at `cols` cells (herdr
/// `TextEditor::viewport`, made stable): it moves from `prev` only as far as
/// it must to keep the caret's cell in view, and when the text ends short of
/// the edge it pulls back to show more on the left. `cell` measures a
/// grapheme; the grapheme under the caret takes at least one cell. Linear in
/// the window, not the text.
fn window(
    text: &str,
    cursor: usize,
    prev: usize,
    cols: usize,
    cell: &impl Fn(&str) -> usize,
) -> (usize, usize) {
    let cols = cols.max(1);
    let under = text[cursor..]
        .graphemes(true)
        .next()
        .map_or(1, |g| cell(g).max(1));
    // The earliest start that still shows the caret's cell.
    let (mut earliest, mut used) = (cursor, under);
    for (i, g) in text[..cursor].grapheme_indices(true).rev() {
        let c = cell(g);
        if used + c > cols {
            break;
        }
        used += c;
        earliest = i;
    }
    let mut start = snap_to_grapheme(text, prev).clamp(earliest, cursor);
    // Pull back when what follows the start, plus a cell for a caret past
    // the end, fits — that cell counted wherever the caret is, so stepping
    // left off the end does not shift the text.
    let at = |i: usize, g: &str| if i == cursor { cell(g).max(1) } else { cell(g) };
    let mut tail = 1;
    let mut fits = true;
    for (i, g) in text[start..].grapheme_indices(true) {
        tail += at(start + i, g);
        if tail > cols {
            fits = false;
            break;
        }
    }
    if fits {
        for (i, g) in text[..start].grapheme_indices(true).rev() {
            let c = cell(g);
            if tail + c > cols {
                break;
            }
            tail += c;
            start = i;
        }
    }
    let (mut end, mut used) = (start, 0);
    for (i, g) in text[start..].grapheme_indices(true) {
        let c = at(start + i, g);
        if used + c > cols {
            break;
        }
        used += c;
        end = start + i + g.len();
    }
    (start, end)
}

/// [`window`], keeping the last cell for `›` when text is hidden on the
/// right.
fn strip(
    text: &str,
    cursor: usize,
    prev: usize,
    cols: usize,
    cell: &impl Fn(&str) -> usize,
) -> Strip {
    let (mut start, mut end) = window(text, cursor, prev, cols, cell);
    if end < text.len() && cols >= 2 {
        (start, end) = window(text, cursor, prev, cols - 1, cell);
    }
    let caret = text[start..cursor].graphemes(true).map(cell).sum();
    Strip {
        start,
        end,
        left: start > 0,
        right: end < text.len(),
        caret,
    }
}

/// A grapheme as the one-line view draws it: a line break as a space.
fn line_cell(g: &str) -> usize {
    if g == "\n" || g == "\r\n" {
        1
    } else {
        text::grapheme_width(g)
    }
}

/// `s` as the one-line view draws it: masked, one `glyph` per grapheme;
/// otherwise with line breaks as spaces.
fn shown(s: &str, mask: Option<char>) -> String {
    match mask {
        Some(glyph) => s.graphemes(true).map(|_| glyph).collect(),
        None => s
            .graphemes(true)
            .map(|g| if g == "\n" || g == "\r\n" { " " } else { g })
            .collect(),
    }
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

    /// One line: no wrap, a window that follows the caret, line breaks in a
    /// paste become spaces, and Enter submits. Line breaks already in the
    /// text become spaces too.
    pub fn single_line(mut self) -> ComposerState {
        self.single_line = true;
        let t = clean_line(&self.text, self.mask.is_some()).into_owned();
        self.text = t;
        self.cursor = self.cursor.min(self.text.len());
        self.cursor = snap_to_grapheme(&self.text, self.cursor);
        self
    }

    /// The text is a preset (a suggested name): drawn as a selection, it is
    /// replaced whole by the first key typed or text pasted, emptied by
    /// Backspace, and kept by a move.
    pub fn replace_on_type(mut self, on: bool) -> ComposerState {
        self.replace = on && !self.text.is_empty();
        self
    }

    /// A secret — an API key, a password. One line; the text is drawn as
    /// `mask` glyphs and never as itself, line breaks and tabs in a paste
    /// are dropped, and only [`ComposerState::take_secret`] gives it back.
    pub fn masked(mut self, mask: Mask) -> ComposerState {
        self.mask = Some(mask);
        self.single_line()
    }

    /// The text — empty while masked (use [`ComposerState::is_empty`] and
    /// [`ComposerState::take_secret`]).
    pub fn text(&self) -> &str {
        if self.mask.is_some() { "" } else { &self.text }
    }

    /// The caret's byte offset into the text.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn is_single_line(&self) -> bool {
        self.single_line || self.mask.is_some()
    }

    pub fn is_masked(&self) -> bool {
        self.mask.is_some()
    }

    /// The text is still a preset the next typed key replaces.
    pub fn is_preset(&self) -> bool {
        self.replace
    }

    /// The one place a masked composer's text comes out: taken, leaving the
    /// composer and its kill buffer empty. Call it on
    /// [`ComposerEvent::SubmittedSecret`]. Works unmasked too.
    pub fn take_secret(&mut self) -> String {
        self.killed.clear();
        let t = std::mem::take(&mut self.text);
        self.clear();
        t
    }

    fn clean<'s>(&self, s: &'s str) -> Cow<'s, str> {
        if self.is_single_line() {
            clean_line(s, self.mask.is_some())
        } else {
            clean(s)
        }
    }

    /// Replace the text (cleaned as a paste is), the caret at its end. It is
    /// not a preset any more.
    pub fn set_text(&mut self, text: &str) {
        let mut t = self.clean(text).into_owned();
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
        self.hscroll = 0;
        self.replace = false;
    }

    /// Empty the composer.
    pub fn clear(&mut self) {
        self.set_text("");
    }

    /// Take the text out, leaving the composer empty. A masked composer
    /// gives nothing here and keeps its text: take it with
    /// [`ComposerState::take_secret`].
    pub fn take(&mut self) -> String {
        if self.mask.is_some() {
            return String::new();
        }
        let t = std::mem::take(&mut self.text);
        self.clear();
        t
    }

    /// Insert `s` at the caret — typing or a paste. Line breaks are kept
    /// (CR LF and CR become LF; one line: a space each), tabs become spaces,
    /// other control characters are dropped. A preset is replaced whole.
    /// Refused whole past [`MAX_BYTES`].
    pub fn insert_str(&mut self, s: &str) -> bool {
        let s = self.clean(s);
        if s.is_empty() {
            return false;
        }
        let kept = if self.replace { 0 } else { self.text.len() };
        if kept + s.len() > MAX_BYTES {
            return false;
        }
        if std::mem::take(&mut self.replace) {
            self.text.clear();
            self.cursor = 0;
            self.hscroll = 0;
        }
        self.text.insert_str(self.cursor, &s);
        self.cursor = snap_to_grapheme(&self.text, self.cursor + s.len());
        self.sticky = None;
        true
    }

    /// Typed text: `Edited`, or `Moved` when it replaced a preset with the
    /// same text (herdr reports that as no change).
    fn type_str(&mut self, s: &str) -> ComposerEvent {
        let same = self.replace && *self.clean(s) == *self.text;
        if !self.insert_str(s) {
            ComposerEvent::Ignored
        } else if same {
            ComposerEvent::Moved
        } else {
            ComposerEvent::Edited
        }
    }

    fn delete(&mut self, from: usize, to: usize) -> ComposerEvent {
        let was = std::mem::take(&mut self.replace);
        if from >= to {
            return if was {
                ComposerEvent::Moved
            } else {
                ComposerEvent::Ignored
            };
        }
        self.text.replace_range(from..to, "");
        self.cursor = snap_to_grapheme(&self.text, from);
        self.sticky = None;
        ComposerEvent::Edited
    }

    /// Delete `from..to` into the kill buffer (herdr `remove(.., kill)`).
    fn kill(&mut self, from: usize, to: usize) -> ComposerEvent {
        if from < to {
            self.killed = self.text[from..to].to_owned();
        }
        self.delete(from, to)
    }

    /// Ctrl+Y: what the last kill cut, at the caret.
    fn yank(&mut self) -> ComposerEvent {
        if self.killed.is_empty() {
            return ComposerEvent::Ignored;
        }
        let k = std::mem::take(&mut self.killed);
        let ev = self.type_str(&k);
        self.killed = k;
        ev
    }

    /// Backspace: one grapheme, or the whole preset.
    fn backspace(&mut self) -> ComposerEvent {
        if self.replace {
            let had = !self.text.is_empty();
            self.clear();
            return if had {
                ComposerEvent::Edited
            } else {
                ComposerEvent::Moved
            };
        }
        self.delete(prev_grapheme(&self.text, self.cursor), self.cursor)
    }

    fn move_to(&mut self, at: usize) -> ComposerEvent {
        self.sticky = None;
        let was = std::mem::take(&mut self.replace);
        if at == self.cursor && !was {
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

    /// A fixed mask keeps the caret at the end.
    fn pinned(&self) -> bool {
        self.mask.is_some_and(|m| m.fixed.is_some())
    }

    /// The one-line view at `cols` cells, from the last drawn window.
    fn strip(&self, cols: usize) -> Strip {
        match self.mask {
            Some(Mask {
                glyph,
                fixed: Some(n),
            }) => {
                let t = shown_fixed(glyph, n, self.text.is_empty());
                strip(&t, t.len(), 0, cols, &line_cell)
            }
            Some(_) => strip(&self.text, self.cursor, self.hscroll, cols, &|_| 1),
            None => strip(&self.text, self.cursor, self.hscroll, cols, &line_cell),
        }
    }

    /// Where the caret is drawn at an area `width` wide: its row and column
    /// in the text (not the window). One line: row 0 and the cells before
    /// the caret as drawn.
    pub fn caret(&self, width: u16) -> (usize, usize) {
        if self.is_single_line() {
            let col = match self.mask {
                Some(Mask { fixed: Some(n), .. }) if !self.text.is_empty() => usize::from(n),
                Some(_) => self.text[..self.cursor].graphemes(true).count(),
                None => self.text[..self.cursor]
                    .graphemes(true)
                    .map(line_cell)
                    .sum(),
            };
            return (0, col);
        }
        let cols = text_cols(width);
        let rows = layout(&self.text, cols);
        let (r, c, _) = display_point(&self.text, &rows, self.cursor, cols);
        (r, c)
    }

    /// Rows the text takes at an area `width` wide (one for an empty text,
    /// and always one on a single line).
    pub fn rows(&self, width: u16) -> usize {
        if self.is_single_line() {
            return 1;
        }
        let cols = text_cols(width);
        let rows = layout(&self.text, cols);
        let (_, _, extra) = display_point(&self.text, &rows, self.cursor, cols);
        rows.len() + usize::from(extra)
    }

    /// `count` visual rows up or down, aiming at the remembered column; at
    /// the first or the last row the caret goes to the start or the end of
    /// the text, and only then is the key [`ComposerEvent::Ignored`].
    fn vertical(&mut self, down: bool, count: usize, width: u16) -> ComposerEvent {
        if self.is_single_line() {
            return ComposerEvent::Ignored;
        }
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
        self.replace = false;
        ComposerEvent::Moved
    }

    /// Apply a key. `width` is the width of the area the composer is drawn
    /// in (the same one given to render): ↑↓ move by the rows it wraps to.
    pub fn key(&mut self, key: KeyInput, width: u16) -> ComposerEvent {
        self.cursor = snap_to_grapheme(&self.text, self.cursor);
        if !self.pinned() {
            return self.apply(key, width);
        }
        self.cursor = self.text.len();
        let was = self.replace;
        let ev = self.apply(key, width);
        self.cursor = self.text.len();
        if ev == ComposerEvent::Moved && was == self.replace {
            return ComposerEvent::Ignored;
        }
        ev
    }

    fn apply(&mut self, key: KeyInput, width: u16) -> ComposerEvent {
        let m = key.mods;
        let one_line = self.is_single_line();
        if m.ctrl {
            return match key.key {
                Key::Char('j') if one_line => ComposerEvent::Ignored,
                Key::Char('j') => self.newline(),
                Key::Char('a') => self.move_to(self.line_start()),
                Key::Char('e') => self.move_to(self.line_end()),
                Key::Char('b') => self.move_to(prev_grapheme(&self.text, self.cursor)),
                Key::Char('f') => self.move_to(next_grapheme(&self.text, self.cursor)),
                Key::Char('h') => self.backspace(),
                Key::Char('d') => self.delete(self.cursor, next_grapheme(&self.text, self.cursor)),
                Key::Char('u') => self.kill(self.line_start(), self.cursor),
                Key::Char('k') => self.kill(self.cursor, self.line_end()),
                Key::Char('w') | Key::Backspace => self.kill(self.word_left(), self.cursor),
                Key::Char('y') => self.yank(),
                Key::Left => self.move_to(self.word_left()),
                Key::Right => self.move_to(self.word_right()),
                Key::Home => self.move_to(0),
                Key::End => self.move_to(self.text.len()),
                _ => ComposerEvent::Ignored,
            };
        }
        match key.key {
            Key::Enter if m.shift || m.alt => {
                if one_line {
                    ComposerEvent::Ignored
                } else {
                    self.newline()
                }
            }
            Key::Enter => {
                // fx: a backslash before the caret turns Enter into a newline.
                if !one_line && self.text[..self.cursor].ends_with('\\') {
                    self.text.replace_range(self.cursor - 1..self.cursor, "\n");
                    self.sticky = None;
                    self.replace = false;
                    return ComposerEvent::Edited;
                }
                if self.text.trim().is_empty() {
                    return ComposerEvent::Ignored;
                }
                if self.mask.is_some() {
                    self.replace = false;
                    return ComposerEvent::SubmittedSecret;
                }
                ComposerEvent::Submitted(self.take())
            }
            Key::Char('b') if m.alt => self.move_to(self.word_left()),
            Key::Char('f') if m.alt => self.move_to(self.word_right()),
            Key::Char('d') if m.alt => self.kill(self.cursor, self.word_right()),
            Key::Char(c) if !m.alt => {
                let mut b = [0u8; 4];
                self.type_str(c.encode_utf8(&mut b))
            }
            Key::Backspace if m.alt => self.kill(self.word_left(), self.cursor),
            Key::Backspace => self.backspace(),
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

/// What a fixed mask draws: `n` glyphs, or nothing while empty.
fn shown_fixed(glyph: char, n: u16, empty: bool) -> String {
    if empty {
        String::new()
    } else {
        std::iter::repeat_n(glyph, usize::from(n)).collect()
    }
}

/// A growing multi-line input with a prompt glyph and a drawn caret.
#[derive(Debug, Clone)]
pub struct Composer<'a> {
    prompt: Cow<'a, str>,
    placeholder: Cow<'a, str>,
    max_height: u16,
    selection: Option<Style>,
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
            selection: None,
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

    /// How a preset ([`ComposerState::replace_on_type`]) is drawn. Default
    /// `theme.selected()`; a host that knows the terminal's background
    /// passes its selection colour.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection = Some(style);
        self
    }

    fn body_style(&self, state: &ComposerState) -> Style {
        if state.replace {
            self.selection.unwrap_or_else(|| self.theme.selected())
        } else {
            self.theme.text()
        }
    }

    /// Rows to give it at `width`: its rows, up to the max height (one for
    /// a single line).
    pub fn height(&self, state: &ComposerState, width: u16) -> u16 {
        (state.rows(width).min(self.max_height as usize) as u16).max(1)
    }

    /// Where the terminal cursor would go, for a host that shows its own
    /// instead of the drawn caret. `None` when the area is empty.
    pub fn cursor_position(&self, state: &ComposerState, area: Rect) -> Option<Position> {
        if area.is_empty() {
            return None;
        }
        if state.is_single_line() {
            let caret = if state.text.is_empty() {
                0
            } else {
                state.strip(text_cols(area.width)).caret
            };
            let x = area.x + PROMPT_WIDTH + caret as u16;
            return (x < area.right()).then_some(Position { x, y: area.y });
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

    /// The prompt, then the placeholder behind the caret.
    fn empty_line(&self, width: u16) -> Line<'static> {
        let t = &self.theme;
        let caret = Style::new().add_modifier(Modifier::REVERSED);
        let mut spans = vec![self.gutter(&self.prompt, t.accent())];
        let (first, rest) = match self.placeholder.graphemes(true).next() {
            Some(g) => (g.to_string(), self.placeholder[g.len()..].to_string()),
            None => (" ".to_string(), String::new()),
        };
        spans.push(Span::styled(first, t.dim().patch(caret)));
        spans.push(Span::styled(rest, t.dim()));
        text::truncate_line(Line::from(spans), width as usize)
    }

    /// The one-line view as a line of at most `width` cells: the prompt (or
    /// `‹` when text is hidden on the left), the window of text around the
    /// caret, the caret as a reversed cell, and `›` at the right edge when
    /// text is hidden there. A masked state shows only its mask glyphs. For
    /// a host that places the field inside its own paragraph; like render,
    /// it moves the window. A multi-line state is shown on one line, its
    /// line breaks as spaces.
    pub fn line(&self, state: &mut ComposerState, width: u16) -> Line<'static> {
        if width == 0 {
            return Line::default();
        }
        state.cursor = snap_to_grapheme(&state.text, state.cursor);
        if state.pinned() {
            state.cursor = state.text.len();
        }
        if state.text.is_empty() {
            state.hscroll = 0;
            return self.empty_line(width);
        }
        let t = &self.theme;
        let caret = Style::new().add_modifier(Modifier::REVERSED);
        let body = self.body_style(state);
        let cols = text_cols(width);
        let st = state.strip(cols);
        // What is drawn: a fixed mask's own glyphs, or the text (masked
        // per grapheme, or as it is).
        let fixed;
        let (src, cursor, glyph) = match state.mask {
            Some(Mask {
                glyph,
                fixed: Some(n),
            }) => {
                fixed = shown_fixed(glyph, n, false);
                (fixed.as_str(), fixed.len(), None)
            }
            Some(m) => {
                state.hscroll = st.start;
                (state.text.as_str(), state.cursor, Some(m.glyph))
            }
            None => {
                state.hscroll = st.start;
                (state.text.as_str(), state.cursor, None)
            }
        };
        let mut spans = vec![if st.left {
            self.gutter("\u{2039}", t.dim())
        } else {
            self.gutter(&self.prompt, t.accent())
        }];
        spans.push(Span::styled(shown(&src[st.start..cursor], glyph), body));
        let mut used = st.caret;
        if cursor < st.end {
            let g = src[cursor..].graphemes(true).next().unwrap_or("");
            let mut under = shown(g, glyph);
            if text::width(&under) == 0 {
                under = " ".to_string();
            }
            used += text::width(&under);
            spans.push(Span::styled(under, body.patch(caret)));
            let after = cursor + g.len();
            if after < st.end {
                let rest = shown(&src[after..st.end], glyph);
                used += text::width(&rest);
                spans.push(Span::styled(rest, body));
            }
        } else if used < cols {
            used += 1;
            spans.push(Span::styled(" ", t.text().patch(caret)));
        }
        if st.right {
            let pad = cols.saturating_sub(used + 1);
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.push(Span::styled("\u{203a}", t.dim()));
        }
        text::truncate_line(Line::from(spans), width as usize)
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

        if state.is_single_line() {
            state.scroll = 0;
            let line = self.line(state, area.width);
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        }
        if state.text.is_empty() {
            state.scroll = 0;
            let line = self.empty_line(area.width);
            buf.set_line(area.x, area.y, &line, area.width);
            return;
        }
        let body = self.body_style(state);

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
                let (text, _) = text::take_cells(&state.text[row.start..row.body_end], cols);
                spans.push(Span::styled(text.to_string(), body));
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

    // ---- herdr `text_editor.rs` tests, ported -------------------------

    fn line_state(text: &str) -> ComposerState {
        ComposerState::with_text(text).single_line()
    }

    fn alt(k: Key) -> KeyInput {
        KeyInput {
            key: k,
            mods: Mods {
                alt: true,
                ..Mods::default()
            },
        }
    }

    fn on_boundary(st: &ComposerState) -> bool {
        st.cursor == st.text.len() || st.text.grapheme_indices(true).any(|(i, _)| i == st.cursor)
    }

    /// herdr `every_binding_edits_at_the_cursor`.
    #[test]
    fn every_binding_edits_at_the_cursor() {
        let ctrl = KeyInput::ctrl;
        for (key, text, cursor, killed) in [
            (Key::Left.into(), "one two", 3, ""),
            (ctrl(Key::Char('b')), "one two", 3, ""),
            (Key::Right.into(), "one two", 5, ""),
            (ctrl(Key::Char('f')), "one two", 5, ""),
            (Key::Home.into(), "one two", 0, ""),
            (ctrl(Key::Char('a')), "one two", 0, ""),
            (Key::End.into(), "one two", 7, ""),
            (ctrl(Key::Char('e')), "one two", 7, ""),
            (Key::Backspace.into(), "onetwo", 3, ""),
            (ctrl(Key::Char('h')), "onetwo", 3, ""),
            (Key::Delete.into(), "one wo", 4, ""),
            (ctrl(Key::Char('d')), "one wo", 4, ""),
            (alt(Key::Char('b')), "one two", 0, ""),
            (alt(Key::Char('f')), "one two", 7, ""),
            (ctrl(Key::Char('u')), "two", 0, "one "),
            (ctrl(Key::Char('k')), "one ", 4, "two"),
            (ctrl(Key::Char('w')), "two", 0, "one "),
            (alt(Key::Backspace), "two", 0, "one "),
            (ctrl(Key::Backspace), "two", 0, "one "),
            (alt(Key::Char('d')), "one ", 4, "two"),
            (ctrl(Key::Char('y')), "one two", 4, ""),
            (KeyInput::from('X'), "one Xtwo", 5, ""),
        ] {
            for mut st in [line_state("one two"), ComposerState::with_text("one two")] {
                st.cursor = 4;
                let ev = st.key(key, 40);
                assert!(on_boundary(&st), "{key:?}");
                assert_eq!(
                    (st.text(), st.cursor(), st.killed.as_str()),
                    (text, cursor, killed),
                    "{key:?}"
                );
                assert_eq!(ev == ComposerEvent::Edited, text != "one two", "{key:?}");
                let mut empty = line_state("");
                empty.key(key, 40);
                assert!(on_boundary(&empty));
            }
        }
    }

    /// herdr `suggestions_movement_kills_and_yank`.
    #[test]
    fn a_preset_is_replaced_by_typing_and_kept_by_a_move() {
        for (k, expected) in [
            (Key::Left, "defaulxt"),
            (Key::Right, "defaultx"),
            (Key::Home, "xdefault"),
            (Key::End, "defaultx"),
        ] {
            let mut st = line_state("default").replace_on_type(true);
            assert_ne!(st.key(k.into(), 40), ComposerEvent::Ignored, "{k:?}");
            assert!(!st.is_preset());
            st.key('x'.into(), 40);
            assert_eq!(st.text(), expected, "{k:?}");
        }
        let mut st = line_state("default").replace_on_type(true);
        assert!(st.insert_str("new"));
        assert_eq!(st.text(), "new");
        // Typing the preset's own text changes nothing, typing other text does.
        for (typed, ev) in [('x', ComposerEvent::Moved), ('y', ComposerEvent::Edited)] {
            let mut st = line_state("x").replace_on_type(true);
            assert_eq!(st.key(typed.into(), 40), ev);
            assert_eq!(st.text(), typed.to_string());
            assert!(!st.is_preset());
        }
        let mut st = line_state("default").replace_on_type(true);
        st.key(Key::Backspace.into(), 40);
        assert!(st.is_empty());
        let mut st = line_state("one two").replace_on_type(true);
        st.key(KeyInput::ctrl(Key::Char('w')), 40);
        assert_eq!(st.text(), "one ");
        assert!(!st.is_preset());
        st.key(KeyInput::ctrl(Key::Char('k')), 40);
        st.key(Key::Backspace.into(), 40);
        st.key(KeyInput::ctrl(Key::Char('y')), 40);
        st.key(KeyInput::ctrl(Key::Char('y')), 40);
        assert_eq!(st.text(), "onetwotwo");
        st.key(KeyInput::ctrl(Key::Char('u')), 40);
        assert_eq!(st.killed, "onetwotwo");
        let mut other = line_state("");
        assert_eq!(
            other.key(KeyInput::ctrl(Key::Char('y')), 40),
            ComposerEvent::Ignored
        );
        assert!(other.is_empty());
        // An empty yank leaves a preset alone; a paste replaces it.
        let mut st = line_state("main").replace_on_type(true);
        st.key(KeyInput::ctrl(Key::Char('y')), 40);
        assert!(st.is_preset());
        st.insert_str("dev\n");
        assert_eq!(st.text(), "dev ");
    }

    /// herdr `unicode_graphemes_and_boundary_changing_edits`.
    #[test]
    fn edits_that_join_clusters_keep_the_caret_on_a_boundary() {
        let mut st = line_state("e\u{301}中👩\u{200d}💻");
        for expected in ["e\u{301}中", "e\u{301}", ""] {
            st.key(Key::Backspace.into(), 40);
            assert_eq!(st.text(), expected);
        }
        let mut st = line_state("👩💻");
        st.key(Key::Left.into(), 40);
        st.insert_str("\u{200d}");
        assert_eq!(st.cursor(), st.text().len());
        st.key(Key::Backspace.into(), 40);
        assert!(st.is_empty());
        let mut st = line_state("\u{301}x");
        st.key(Key::Home.into(), 40);
        st.insert_str("e");
        assert_eq!(st.cursor(), "e\u{301}".len());
        st.key(Key::Delete.into(), 40);
        assert_eq!(st.text(), "e\u{301}");
    }

    /// herdr `insertion_normalizes_controls_and_respects_host_text`.
    #[test]
    fn a_one_line_paste_turns_breaks_and_tabs_into_spaces() {
        let mut st = line_state("ab");
        st.key(Key::Left.into(), 40);
        st.insert_str("中\r\n\r\n\t\x00\x1b\u{7f}e\u{301}");
        assert_eq!(st.text(), "a中   e\u{301}b");
        let st = ComposerState::with_text("one\ntwo").single_line();
        assert_eq!(st.text(), "one two", "a multi-line text made one line");
    }

    /// herdr `viewport_and_render_are_pure_and_grapheme_safe`, with the
    /// window kept where it was and `‹ ›` where text is hidden.
    #[test]
    fn the_window_keeps_the_caret_in_view_at_every_width() {
        let c = Composer::new().theme(Theme::mono());
        for text in [
            "abcdefghijklmnopqrstuvwxyz",
            "e\u{301}中👩\u{200d}💻xyz",
            "\u{301}abc",
        ] {
            let mut st = line_state(text);
            let offsets: Vec<usize> = text
                .grapheme_indices(true)
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            for &cursor in &offsets {
                for cols in [0u16, 1, 2, 3, 8, 80] {
                    st.cursor = cursor;
                    let width = cols + PROMPT_WIDTH;
                    let (t0, c0) = (st.text.clone(), st.cursor);
                    let line = c.line(&mut st, width);
                    assert!(
                        text::line_width(&line) <= usize::from(width),
                        "{text:?} {cols}"
                    );
                    assert_eq!((st.text.clone(), st.cursor), (t0, c0), "pure");
                    assert_eq!(c.line(&mut st, width), line, "idempotent");
                    let buf = render(&c, &mut st, width, 1);
                    let caret: Vec<u16> = (0..width)
                        .filter(|&x| buf[(x, 0)].modifier.contains(Modifier::REVERSED))
                        .collect();
                    assert!(!caret.is_empty() || cols == 0, "{text:?} {cursor} {cols}");
                    if let Some(pos) = c.cursor_position(&st, buf.area) {
                        assert_eq!(Some(&pos.x), caret.first(), "{text:?} {cursor} {cols}");
                    }
                }
            }
        }
        // herdr pins `("def", 3)` at the end of `abcdef` in four cells.
        let mut st = line_state("abcdef");
        let row = |st: &mut ComposerState| snapshot::lines(&render(&c, st, 6, 1))[0].clone();
        assert_eq!(row(&mut st), "\u{2039} def");
        assert_eq!(st.strip(4).caret, 3);
        // Stepping left from the end moves the caret, not the text.
        st.key(Key::Left.into(), 6);
        assert_eq!(row(&mut st), "\u{2039} def");
        assert_eq!(st.strip(4).caret, 2);
        st.key(Key::Left.into(), 6);
        st.key(Key::Left.into(), 6);
        assert_eq!(row(&mut st), "\u{2039} def", "the caret on `d`");
        // Past the left edge the window follows by one grapheme.
        st.key(Key::Left.into(), 6);
        assert_eq!(row(&mut st), "\u{2039} cdef");
        st.key(Key::Home.into(), 6);
        assert_eq!(
            row(&mut st),
            "\u{276f} abc\u{203a}",
            "`›`: more on the right"
        );
        // Deleting at the end pulls hidden text back in.
        let mut st = line_state("abcdefgh");
        row(&mut st);
        for _ in 0..3 {
            st.key(Key::Backspace.into(), 6);
        }
        assert_eq!(row(&mut st), "\u{2039} cde");
    }

    #[test]
    fn one_line_leaves_up_down_and_newline_chords_to_the_host() {
        let mut st = line_state("draft");
        for k in [Key::Up, Key::Down, Key::PageUp, Key::PageDown] {
            assert_eq!(st.key(k.into(), 20), ComposerEvent::Ignored, "{k:?}");
        }
        assert_eq!(st.key(shift(Key::Enter), 20), ComposerEvent::Ignored);
        assert_eq!(
            st.key(KeyInput::ctrl(Key::Char('j')), 20),
            ComposerEvent::Ignored
        );
        typed(&mut st, "\\", 20);
        assert_eq!(
            st.key(Key::Enter.into(), 20),
            ComposerEvent::Submitted("draft\\".into())
        );
        let c = Composer::new().max_height(4);
        assert_eq!(c.height(&line_state("a b c d e f g h"), 5), 1);
    }

    #[test]
    fn a_preset_is_drawn_as_a_selection() {
        let t = Theme::dark();
        let c = Composer::new().theme(t);
        let mut st = line_state("main").replace_on_type(true);
        let buf = render(&c, &mut st, 20, 1);
        assert_eq!(snapshot::lines(&buf)[0], "\u{276f} main");
        for x in 2..6 {
            assert_eq!(buf[(x, 0)].bg, t.surface_raised, "{x}");
        }
        assert!(buf[(6, 0)].modifier.contains(Modifier::REVERSED), "caret");
        typed(&mut st, "d", 20);
        let buf = render(&c, &mut st, 20, 1);
        assert_eq!(snapshot::lines(&buf)[0], "\u{276f} d");
        assert_ne!(buf[(2, 0)].bg, t.surface_raised);
        // A host colour wins over the theme's.
        let sel = Style::new().add_modifier(Modifier::UNDERLINED);
        let mut st = line_state("main").replace_on_type(true);
        let buf = render(&c.clone().selection_style(sel), &mut st, 20, 1);
        assert!(buf[(2, 0)].modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn multi_line_kills_fill_the_buffer_and_ctrl_y_puts_it_back() {
        let mut st = ComposerState::with_text("first line\nsecond line");
        st.key(KeyInput::ctrl(Key::Char('u')), 40);
        assert_eq!(st.text(), "first line\n");
        st.key(KeyInput::ctrl(Key::Home), 40);
        assert_eq!(
            st.key(KeyInput::ctrl(Key::Char('y')), 40),
            ComposerEvent::Edited
        );
        assert_eq!(st.text(), "second linefirst line\n");
    }

    // ---- masked --------------------------------------------------------

    /// A secret whose characters appear nowhere in the composer's own chrome.
    const SECRET: &str = "sk-ant-api03-한글-👩\u{200d}💻-ÄQZ_9";

    fn assert_no_secret_in(s: &str, what: &str) {
        for c in s.chars() {
            assert!(
                !SECRET.contains(c) || c == ' ',
                "{what}: secret character {c:?} in {s:?}"
            );
        }
    }

    #[test]
    fn a_masked_composer_never_draws_the_secret() {
        for mask in [Mask::default(), Mask::fixed(MASK_GLYPH, 8), Mask::each('*')] {
            for theme in [Theme::ansi(), Theme::dark(), Theme::mono()] {
                let c = Composer::new().placeholder("paste a key").theme(theme);
                let mut st = ComposerState::with_text(SECRET).masked(mask);
                for w in 1..48u16 {
                    for k in [Key::End, Key::Left, Key::Left, Key::Home, Key::Right] {
                        st.key(k.into(), w);
                        let buf = render(&c, &mut st, w, 1);
                        for cell in buf.content() {
                            assert_no_secret_in(cell.symbol(), "cell");
                        }
                        assert_no_secret_in(&snapshot::text(&buf), "snapshot");
                        for span in c.line(&mut st, w).spans {
                            assert_no_secret_in(&span.content, "line");
                        }
                    }
                }
                assert_eq!(st.text(), "");
                assert_eq!(st.take(), "", "take gives nothing");
                assert!(!st.is_empty(), "and keeps the text");
                // Neither the text nor what a kill cut shows in Debug.
                st.key(KeyInput::ctrl(Key::Char('w')), 40);
                let dbg = format!("{st:?}");
                for part in SECRET.split('-').filter(|p| p.len() > 2) {
                    assert!(!dbg.contains(part), "debug shows {part:?}: {dbg}");
                }
            }
        }
        // One glyph per grapheme: the family emoji is one.
        let c = Composer::new().theme(Theme::mono());
        let mut st = ComposerState::with_text("a👩\u{200d}💻b").masked(Mask::default());
        let row = snapshot::lines(&render(&c, &mut st, 20, 1))[0].clone();
        assert_eq!(row, "\u{276f} \u{2219}\u{2219}\u{2219}");
        // A fixed mask hides the length and keeps the caret at the end.
        let mut st = ComposerState::with_text("ab").masked(Mask::fixed('*', 6));
        assert_eq!(
            snapshot::lines(&render(&c, &mut st, 20, 1))[0],
            "\u{276f} ******"
        );
        assert_eq!(st.key(Key::Home.into(), 20), ComposerEvent::Ignored);
        assert_eq!(st.cursor(), 2);
        st.key(Key::Backspace.into(), 20);
        st.key('z'.into(), 20);
        assert_eq!(st.take_secret(), "az");
        // A glyph wider than a cell falls back to the default.
        assert_eq!(Mask::each('가').glyph(), MASK_GLYPH);
    }

    #[test]
    fn enter_on_a_masked_composer_hands_the_secret_over_only_on_request() {
        let mut st = ComposerState::new().masked(Mask::default());
        assert_eq!(st.key(Key::Enter.into(), 20), ComposerEvent::Ignored);
        // A key copied with its line break: the break is dropped, not a space.
        st.insert_str("sk-123\r\n");
        typed(&mut st, "4", 20);
        assert_eq!(
            st.key(Key::Enter.into(), 20),
            ComposerEvent::SubmittedSecret
        );
        assert_eq!(st.text(), "");
        st.key(KeyInput::ctrl(Key::Char('w')), 20);
        st.key(KeyInput::ctrl(Key::Char('y')), 20);
        assert_eq!(st.take_secret(), "sk-1234");
        assert!(st.is_empty() && st.killed.is_empty(), "and the kill buffer");
        assert_eq!(st.take_secret(), "");
    }

    #[test]
    fn line_is_the_one_line_view_for_a_host_paragraph() {
        let c = Composer::new().prompt("").theme(Theme::mono());
        let mut st = line_state("hello");
        let line = c.line(&mut st, 10);
        let s: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(s, "  hello ");
        let caret = line.spans.last().unwrap();
        assert!(caret.style.add_modifier.contains(Modifier::REVERSED));
        // A multi-line state on one line: its breaks are spaces.
        let mut st = ComposerState::with_text("a\nb");
        let s: String = c
            .line(&mut st, 10)
            .spans
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(s, "  a b ");
        assert_eq!(c.line(&mut st, 0), Line::default());
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
