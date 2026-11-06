// Derived from herdrdev/herdr `src/ui/scrollbar.rs` (`scrollbar_thumb`,
// `scrollbar_thumb_grab_offset`, `scrollbar_offset_from_thumb_top`,
// `scrollbar_offset_from_row`, `scrollbar_offset_from_drag_row`,
// `render_scrollbar_buffer`, `render_pane_scrollbar_buffer`) and
// `src/client/shell/scroll.rs` (`list_scroll_metrics`,
// `list_scroll_start_to_reveal`, `render_list_scrollbar` and its test) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab:
// positions count from the top (`ScrollPos { top, max, viewport }`) instead of
// herdr's offset from the bottom, with conversions both ways; the maths round
// in integers (herdr went through `f32`); a press, drag and release are one
// `ScrollbarDrag` instead of a copy per overlay in the mouse handler; a track
// too short to move the thumb leaves the position alone (herdr jumped to the
// bottom); revealing a row is one linear pass (herdr re-measured the list once
// per step); drawing skips cells outside the buffer (herdr indexed them); the
// thumb differs from the track by glyph and by weight, focused or not, and both
// default glyphs are East-Asian Narrow (herdr's track `▕` is Ambiguous and its
// unfocused thumb was the track glyph); styles come from the `Theme`.

//! A one-column scrollbar you can click and drag, for anything longer than
//! its box: a log, a modal's body, a list whose rows differ in height.
//!
//! Ported from herdr (`src/ui/scrollbar.rs`, `src/client/shell/scroll.rs`,
//! Apache-2.0), where every pane, sidebar list, navigator and release-notes
//! popup carries one. It tells the person how much there is and where they
//! are in it — the thumb's length is the share in view, its place is the
//! place in the whole — and lets the mouse move there:
//!
//! ```text
//! 09:14:02 req_01JD8Q claude-opus-5-5  200 ▐
//! 09:14:05 req_01JD8R claude-sonnet-5  200 ▐
//! 09:14:07 req_01JD8S claude-opus-5-5  429 ╎
//! 09:14:09 req_01JD8T claude-haiku-5   200 ╎
//! ```
//!
//! The maths are free functions over a [`ScrollPos`] and the track's
//! [`Rect`], so the same numbers drive the drawing and the hit tests:
//! [`thumb`] (length and place), [`offset_at`] (a click on the track centres
//! the thumb there), [`grab`] and [`offset_dragged`] (a drag keeps the cell
//! you grabbed under the pointer), and [`list_pos`] / [`reveal`] for lists
//! whose rows differ in height and have gaps between groups — positions there
//! count rows of the list, not screen lines. [`ScrollbarDrag`] wraps press,
//! drag and release for a host's mouse handler; [`Scrollbar`] draws.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;

use crate::input::{Key, KeyInput};
use crate::theme::Theme;

/// Where a scrolled view is, in rows (screen rows, or list rows for
/// [`list_pos`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollPos {
    /// Rows scrolled past: the first row in view. At most `max`.
    pub top: usize,
    /// The largest `top`: rows in all minus rows in view. 0 when it all fits.
    pub max: usize,
    /// Rows in view.
    pub viewport: usize,
}

impl ScrollPos {
    /// A position; `top` is clamped to `max`.
    pub const fn new(top: usize, max: usize, viewport: usize) -> ScrollPos {
        ScrollPos {
            top: if top < max { top } else { max },
            max,
            viewport,
        }
    }

    /// `total` rows seen through `viewport` rows, scrolled down by `top`
    /// (clamped).
    pub const fn of(total: usize, viewport: usize, top: usize) -> ScrollPos {
        ScrollPos::new(top, total.saturating_sub(viewport), viewport)
    }

    /// From a scrollback position (herdr's and a terminal's convention):
    /// `offset_from_bottom` rows above the live end.
    pub const fn from_bottom(offset_from_bottom: usize, max: usize, viewport: usize) -> ScrollPos {
        ScrollPos::new(max.saturating_sub(offset_from_bottom), max, viewport)
    }

    /// Rows above the end: 0 at the bottom, `max` at the top.
    pub const fn offset_from_bottom(&self) -> usize {
        self.max.saturating_sub(self.top)
    }

    /// Rows in all.
    pub const fn total(&self) -> usize {
        self.max.saturating_add(self.viewport)
    }

    /// Whether anything is out of view. A scrollbar only shows then.
    pub const fn overflows(&self) -> bool {
        self.max > 0
    }

    /// The same view moved to `top` (clamped).
    pub const fn with_top(self, top: usize) -> ScrollPos {
        ScrollPos::new(top, self.max, self.viewport)
    }

    /// The `top` a scroll key asks for: ↑/↓ one row, PgUp/PgDn one viewport
    /// (at least one row), Home/End the ends. `None` for any other key.
    pub fn on_key(&self, key: KeyInput) -> Option<usize> {
        let page = self.viewport.max(1);
        let top = match key.key {
            Key::Up => self.top.saturating_sub(1),
            Key::Down => self.top.saturating_add(1),
            Key::PageUp => self.top.saturating_sub(page),
            Key::PageDown => self.top.saturating_add(page),
            Key::Home => 0,
            Key::End => self.max,
            _ => return None,
        };
        Some(top.min(self.max))
    }
}

/// Where the thumb is on a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Thumb {
    /// The thumb's first row, in buffer coordinates (`track.y` at the top).
    pub top: u16,
    /// Rows the thumb covers, at least 1.
    pub len: u16,
}

impl Thumb {
    /// Whether `row` is on the thumb.
    pub const fn contains(&self, row: u16) -> bool {
        row >= self.top && row - self.top < self.len
    }
}

/// `n / d` rounded half up, in integers. `d > 0`.
fn round_div(n: u128, d: u128) -> u128 {
    (2 * n + d) / (2 * d)
}

/// The thumb for `pos` on `track` (its rows; the column does not matter),
/// or `None` when nothing overflows or the track has no rows.
///
/// Length: the share in view, `round(viewport × track / total)`, at least 1.
/// Place: how far down, `round(top × (track − len) / max)`.
pub fn thumb(pos: ScrollPos, track: Rect) -> Option<Thumb> {
    if pos.max == 0 || track.height == 0 {
        return None;
    }
    let h = u128::from(track.height);
    let total = pos.total() as u128;
    let len = round_div(pos.viewport as u128 * h, total).clamp(1, h);
    let room = h - len;
    let top = if room == 0 {
        0
    } else {
        round_div(pos.top.min(pos.max) as u128 * room, pos.max as u128).min(room)
    };
    Some(Thumb {
        top: track.y.saturating_add(top as u16),
        len: len as u16,
    })
}

/// How far below the thumb's first row `row` is, when `row` is on the thumb:
/// what a drag must keep under the pointer. `None` off the thumb.
pub fn grab(pos: ScrollPos, track: Rect, row: u16) -> Option<u16> {
    let t = thumb(pos, track)?;
    t.contains(row).then(|| row - t.top)
}

/// The `top` that puts the thumb's first row `thumb_row` rows down the track.
fn offset_from_thumb_top(pos: ScrollPos, track: Rect, thumb_row: usize) -> usize {
    if pos.max == 0 {
        return 0;
    }
    let Some(t) = thumb(pos, track) else {
        return pos.top;
    };
    let room = usize::from(track.height - t.len);
    if room == 0 {
        // The thumb fills the track: it cannot say where to go.
        return pos.top;
    }
    let row = thumb_row.min(room) as u128;
    round_div(row * pos.max as u128, room as u128).min(pos.max as u128) as usize
}

/// The track row under `row`, clamped onto the track.
fn track_row(track: Rect, row: u16) -> usize {
    let last = track.y.saturating_add(track.height.saturating_sub(1));
    usize::from(row.clamp(track.y, last) - track.y)
}

/// The `top` for a click on the track at `row`: the thumb centres on that
/// row (a row past either end counts as the end). Unchanged when nothing
/// overflows.
pub fn offset_at(pos: ScrollPos, track: Rect, row: u16) -> usize {
    let Some(t) = thumb(pos, track) else {
        return pos.top;
    };
    let wanted = track_row(track, row).saturating_sub(usize::from(t.len / 2));
    offset_from_thumb_top(pos, track, wanted)
}

/// The `top` while dragging: the thumb row grabbed at [`grab`] stays under
/// the pointer at `row` (clamped onto the track, so dragging past the end
/// reaches the end).
pub fn offset_dragged(pos: ScrollPos, track: Rect, row: u16, grab: u16) -> usize {
    if track.height == 0 {
        return pos.top;
    }
    let wanted = track_row(track, row).saturating_sub(usize::from(grab));
    offset_from_thumb_top(pos, track, wanted)
}

/// The rightmost column of `area`: where [`Scrollbar`] draws, and the rect
/// to test clicks against.
pub fn track(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(area.width.saturating_sub(1)),
        width: area.width.min(1),
        ..area
    }
}

/// `area` split into what is left for content and the track, when `pos`
/// overflows and there is room for both (two columns or more).
pub fn split(area: Rect, pos: ScrollPos) -> (Rect, Option<Rect>) {
    if !pos.overflows() || area.width < 2 {
        return (area, None);
    }
    let content = Rect {
        width: area.width - 1,
        ..area
    };
    (content, Some(track(area)))
}

/// Clamped height of list row `i`: at least 1, at most the body.
fn row_height(heights: &[u16], i: usize, body: u16) -> u16 {
    heights[i].max(1).min(body)
}

fn gap_after(gaps: &[u16], i: usize) -> u16 {
    gaps.get(i).copied().unwrap_or(0)
}

/// The position of a list whose rows differ in height, shown in `body_h`
/// screen rows from row `start`. Positions count list rows: `top` is the
/// first row shown (clamped so the last page is full), `max` the last such
/// start, `viewport` the rows that fit whole from `top`.
///
/// `gaps_after[i]` blank lines follow row `i` (missing entries are 0); the
/// caller decides where gaps go, so a group's last row can have one and its
/// children none. A row taller than the body counts as the body's height.
pub fn list_pos(heights: &[u16], gaps_after: &[u16], body_h: u16, start: usize) -> ScrollPos {
    if heights.is_empty() || body_h == 0 {
        return ScrollPos::default();
    }
    let body = u32::from(body_h);
    let mut used = 0u32;
    let mut max_start = heights.len();
    for i in (0..heights.len()).rev() {
        let need = u32::from(row_height(heights, i, body_h)) + u32::from(gap_after(gaps_after, i));
        if used + need > body {
            break;
        }
        used += need;
        max_start = i;
    }
    let max_start = max_start.min(heights.len() - 1);
    let start = start.min(max_start);

    let mut viewport = 0usize;
    let mut used = 0u32;
    for i in start..heights.len() {
        let h = u32::from(row_height(heights, i, body_h));
        if used + h > body {
            break;
        }
        used += h;
        viewport += 1;
        let gap = u32::from(gap_after(gaps_after, i));
        if used + gap > body {
            break;
        }
        used += gap;
    }
    ScrollPos {
        top: start,
        max: max_start,
        viewport,
    }
}

/// The first row to show so that row `target` is in view whole, moving as
/// little as possible from `start`: up to `target` when it is above, down
/// just far enough when it is below. A target past the end scrolls to the
/// end.
pub fn reveal(
    heights: &[u16],
    gaps_after: &[u16],
    body_h: u16,
    start: usize,
    target: usize,
) -> usize {
    let pos = list_pos(heights, gaps_after, body_h, start);
    let mut s = pos.top;
    if target < s {
        return target;
    }
    if target >= heights.len() {
        return pos.max;
    }
    // Screen rows from row `s` through the whole of row `target`. The target
    // is in view when they fit; each step down drops row `s` and its gap.
    let body = u64::from(body_h);
    let mut span: u64 = (s..target)
        .map(|i| u64::from(row_height(heights, i, body_h)) + u64::from(gap_after(gaps_after, i)))
        .sum::<u64>()
        + u64::from(row_height(heights, target, body_h));
    while span > body && s < pos.max {
        span -= u64::from(row_height(heights, s, body_h)) + u64::from(gap_after(gaps_after, s));
        s += 1;
    }
    s
}

/// A press on the track, then drags, then a release: what herdr's mouse
/// handler repeats for every scrollable surface, in one place.
///
/// A press on the thumb grabs it where it was pressed and leaves the view
/// alone; a press elsewhere on the track jumps so the thumb centres on the
/// pointer — and grabs it there, so the same press can keep dragging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ScrollbarDrag {
    grab: Option<u16>,
}

impl ScrollbarDrag {
    pub const fn new() -> ScrollbarDrag {
        ScrollbarDrag { grab: None }
    }

    /// A press at `(col, row)`. `Some(top)` when it landed on a track that
    /// has a thumb — the new position (unchanged on the thumb itself);
    /// `None` when it missed, so the host can treat it as a click on
    /// whatever else is there.
    pub fn press(&mut self, pos: ScrollPos, track: Rect, col: u16, row: u16) -> Option<usize> {
        self.grab = None;
        if !track.contains(Position::new(col, row)) {
            return None;
        }
        let t = thumb(pos, track)?;
        if t.contains(row) {
            self.grab = Some(row - t.top);
            return Some(pos.top);
        }
        let top = offset_at(pos, track, row);
        self.grab = grab(pos.with_top(top), track, row);
        Some(top)
    }

    /// The pointer moved to `row` with the button held: the new `top`, or
    /// `None` when no drag is under way. The column is ignored, so drifting
    /// sideways off the track keeps dragging.
    pub fn drag(&self, pos: ScrollPos, track: Rect, row: u16) -> Option<usize> {
        self.grab.map(|g| offset_dragged(pos, track, row, g))
    }

    /// The button went up.
    pub fn release(&mut self) {
        self.grab = None;
    }

    pub const fn is_dragging(&self) -> bool {
        self.grab.is_some()
    }

    /// The row offset into the thumb being held.
    pub const fn grab_offset(&self) -> Option<u16> {
        self.grab
    }
}

/// The track and thumb glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollbarGlyphs {
    pub track: &'static str,
    pub thumb: &'static str,
}

impl Default for ScrollbarGlyphs {
    /// Track `╎` (U+254E), thumb `▐` (U+2590): both East-Asian Narrow, so
    /// one cell on a CJK terminal too.
    fn default() -> Self {
        ScrollbarGlyphs {
            track: "╎",
            thumb: "▐",
        }
    }
}

impl ScrollbarGlyphs {
    /// `|` and `#`.
    pub const fn ascii() -> Self {
        ScrollbarGlyphs {
            track: "|",
            thumb: "#",
        }
    }

    /// herdr's own: track `▕` (U+2595), thumb `▐`. The track glyph is
    /// East-Asian *Ambiguous* — two cells on a CJK terminal set to wide
    /// ambiguous glyphs. Opt in knowingly.
    pub const fn herdr() -> Self {
        ScrollbarGlyphs {
            track: "▕",
            thumb: "▐",
        }
    }
}

/// A vertical scrollbar drawn in the rightmost column of its area. Draws
/// nothing when nothing overflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    pos: ScrollPos,
    glyphs: ScrollbarGlyphs,
    focused: bool,
    theme: Theme,
}

impl Scrollbar {
    pub fn new(pos: ScrollPos) -> Scrollbar {
        Scrollbar {
            pos,
            glyphs: ScrollbarGlyphs::default(),
            focused: true,
            theme: Theme::default(),
        }
    }

    pub fn glyphs(mut self, glyphs: ScrollbarGlyphs) -> Scrollbar {
        self.glyphs = glyphs;
        self
    }

    /// Whether the surface it scrolls has focus: the thumb is in the accent
    /// colour then, the secondary colour otherwise. Default true.
    pub fn focused(mut self, focused: bool) -> Scrollbar {
        self.focused = focused;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Scrollbar {
        self.theme = theme;
        self
    }

    pub fn track_style(&self) -> Style {
        self.theme.dim()
    }

    /// Bold in mono, focused or not, so the thumb never reads as track.
    pub fn thumb_style(&self) -> Style {
        let t = &self.theme;
        if t.is_mono() {
            Style::new().add_modifier(Modifier::BOLD)
        } else if self.focused {
            t.accent()
        } else {
            t.secondary()
        }
    }
}

impl Widget for Scrollbar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

impl Widget for &Scrollbar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let track = track(area);
        let Some(t) = thumb(self.pos, track) else {
            return;
        };
        let (track_style, thumb_style) = (self.track_style(), self.thumb_style());
        for y in track.y..track.bottom() {
            let on_thumb = t.contains(y);
            if let Some(cell) = buf.cell_mut((track.x, y)) {
                if on_thumb {
                    cell.set_symbol(self.glyphs.thumb).set_style(thumb_style);
                } else {
                    cell.set_symbol(self.glyphs.track).set_style(track_style);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    // ---- herdr's maths, verbatim, as the reference (offset from bottom) ----

    #[derive(Clone, Copy)]
    struct M {
        offset_from_bottom: usize,
        max_offset_from_bottom: usize,
        viewport_rows: usize,
    }

    fn h_thumb(m: M, track: Rect) -> Option<(u16, u16)> {
        if m.max_offset_from_bottom == 0 || track.height == 0 {
            return None;
        }
        let track_height = track.height as usize;
        let total_rows = m.max_offset_from_bottom + m.viewport_rows;
        let thumb_len = ((m.viewport_rows * track_height) as f32 / total_rows as f32)
            .round()
            .max(1.0)
            .min(track_height as f32) as usize;
        let max_thumb_top = track_height.saturating_sub(thumb_len);
        let scrolled_from_top = m
            .max_offset_from_bottom
            .saturating_sub(m.offset_from_bottom);
        let thumb_top = if max_thumb_top == 0 {
            0
        } else {
            ((scrolled_from_top * max_thumb_top) as f32 / m.max_offset_from_bottom as f32)
                .round()
                .clamp(0.0, max_thumb_top as f32) as usize
        };
        Some((track.y + thumb_top as u16, thumb_len as u16))
    }

    fn h_from_thumb_top(m: M, track: Rect, thumb_top: usize) -> usize {
        if m.max_offset_from_bottom == 0 {
            return 0;
        }
        let thumb_len = h_thumb(m, track).map(|t| t.1 as usize).unwrap_or(1);
        let max_thumb_top = track.height as usize - thumb_len.min(track.height as usize);
        if max_thumb_top == 0 {
            return 0;
        }
        let desired_top = thumb_top.min(max_thumb_top);
        let scrolled_from_top = ((desired_top * m.max_offset_from_bottom) as f32
            / max_thumb_top as f32)
            .round() as usize;
        m.max_offset_from_bottom.saturating_sub(scrolled_from_top)
    }

    fn h_from_row(m: M, track: Rect, row: u16) -> usize {
        let Some((top, len)) = h_thumb(m, track) else {
            return 0;
        };
        let _ = top;
        let clamped = row.clamp(track.y, track.y + track.height.saturating_sub(1));
        let row_offset = clamped.saturating_sub(track.y) as usize;
        h_from_thumb_top(m, track, row_offset.saturating_sub(len as usize / 2))
    }

    fn h_from_drag(m: M, track: Rect, row: u16, grab: u16) -> usize {
        let clamped = row.clamp(track.y, track.y + track.height.saturating_sub(1));
        let row_offset = clamped.saturating_sub(track.y) as usize;
        h_from_thumb_top(m, track, row_offset.saturating_sub(grab as usize))
    }

    fn h_list(heights: &[u16], gaps: &[u16], body: u16, req: usize) -> M {
        if heights.is_empty() || body == 0 {
            return M {
                offset_from_bottom: 0,
                max_offset_from_bottom: 0,
                viewport_rows: 0,
            };
        }
        let mut used = 0u16;
        let mut max_start = heights.len();
        for index in (0..heights.len()).rev() {
            let height = heights[index].max(1).min(body);
            let gap = gaps.get(index).copied().unwrap_or(0);
            if used.saturating_add(height).saturating_add(gap) > body {
                break;
            }
            used = used.saturating_add(height).saturating_add(gap);
            max_start = index;
        }
        max_start = max_start.min(heights.len().saturating_sub(1));
        let start = req.min(max_start);
        let mut viewport_rows = 0usize;
        let mut used = 0u16;
        for (index, row_height) in heights.iter().enumerate().skip(start) {
            let height = (*row_height).max(1).min(body);
            if used.saturating_add(height) > body {
                break;
            }
            used = used.saturating_add(height);
            viewport_rows += 1;
            let gap = gaps.get(index).copied().unwrap_or(0);
            if used.saturating_add(gap) > body {
                break;
            }
            used = used.saturating_add(gap);
        }
        M {
            offset_from_bottom: max_start.saturating_sub(start),
            max_offset_from_bottom: max_start,
            viewport_rows,
        }
    }

    fn h_reveal(heights: &[u16], gaps: &[u16], body: u16, req: usize, target: usize) -> usize {
        let mut m = h_list(heights, gaps, body, req);
        let mut start = m
            .max_offset_from_bottom
            .saturating_sub(m.offset_from_bottom);
        if target < start {
            return target;
        }
        while target >= start.saturating_add(m.viewport_rows) && start < m.max_offset_from_bottom {
            start = start.saturating_add(1);
            m = h_list(heights, gaps, body, start);
        }
        start
    }

    fn to_m(p: ScrollPos) -> M {
        M {
            offset_from_bottom: p.offset_from_bottom(),
            max_offset_from_bottom: p.max,
            viewport_rows: p.viewport,
        }
    }

    // ---- herdr's own tests ----

    // Upstream: scroll.rs `list_metrics_preserve_variable_rows_and_caller_owned_gap_policy`.
    #[test]
    fn list_metrics_preserve_variable_rows_and_caller_owned_gap_policy() {
        let top = list_pos(&[1, 3, 2], &[1, 1, 0], 5, 0);
        assert_eq!(top.max, 2);
        assert_eq!(top.offset_from_bottom(), 2);
        assert_eq!(top.top, 0);
        assert_eq!(top.viewport, 2);

        let bottom = list_pos(&[1, 3, 2], &[1, 1, 0], 5, usize::MAX);
        assert_eq!(bottom.offset_from_bottom(), 0);
        assert_eq!(bottom.viewport, 1);

        let parent_child = list_pos(&[2, 2, 2], &[0, 1, 0], 5, 0);
        assert_eq!(parent_child.max, 1);
        assert_eq!(parent_child.viewport, 2);
    }

    // Upstream: tests/copy.rs navigator scrollbar (track jump, grab, drag past the end)
    // and tests/popup_focus_projection.rs sidebar scrollbars (thumb drag to the end).
    #[test]
    fn track_jumps_grab_holds_and_drag_past_the_end_reaches_it() {
        let track = Rect::new(40, 3, 1, 12);
        // 60 rows through 12: a proportional thumb longer than one row.
        let pos = ScrollPos::of(60, 12, 0);
        let t = thumb(pos, track).expect("thumb");
        assert_eq!((t.top, t.len), (3, 2));
        assert!(t.len > 1);
        // A press on the last track row jumps to the end.
        assert_eq!(offset_at(pos, track, track.bottom() - 1), pos.max);
        // A press on the first row jumps back to the top.
        let end = pos.with_top(pos.max);
        assert_eq!(offset_at(end, track, track.y), 0);
        // Grab the thumb by its last row: the grab offset is len - 1, and
        // dragging to the same row does not move the view.
        let g = t.len - 1;
        assert_eq!(grab(pos, track, t.top + g), Some(g));
        assert_eq!(offset_dragged(pos, track, t.top + g, g), 0);
        // Dragging far past the bottom reaches the end.
        assert_eq!(offset_dragged(pos, track, track.bottom() + 5, g), pos.max);
        // Dragging from a top grab to the last row also reaches the end.
        assert_eq!(offset_dragged(pos, track, track.bottom() - 1, 0), pos.max);
    }

    #[test]
    fn maths_match_herdr_over_small_inputs() {
        for h in 0..=9u16 {
            let track = Rect::new(7, 2, 1, h);
            for max in 0..=30usize {
                for viewport in 0..=10usize {
                    for top in 0..=max {
                        let pos = ScrollPos::new(top, max, viewport);
                        let m = to_m(pos);
                        let ours = thumb(pos, track).map(|t| (t.top, t.len));
                        assert_eq!(ours, h_thumb(m, track), "thumb {pos:?} h={h}");
                        let Some((_, len)) = ours else { continue };
                        for row in 0..=(h + 4) {
                            let full = len == h;
                            // herdr jumps to the bottom when the thumb fills
                            // the track; the port keeps the view.
                            let expect_click = if full {
                                top
                            } else {
                                max - h_from_row(m, track, row)
                            };
                            assert_eq!(
                                offset_at(pos, track, row),
                                expect_click,
                                "{pos:?} h={h} r={row}"
                            );
                            for g in 0..len {
                                let expect = if full {
                                    top
                                } else {
                                    max - h_from_drag(m, track, row, g)
                                };
                                assert_eq!(offset_dragged(pos, track, row, g), expect);
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn list_pos_and_reveal_match_herdr_exhaustively() {
        let hs = [1u16, 2, 4];
        let gs = [0u16, 1];
        for len in 0..=4usize {
            let mut heights = vec![0u16; len];
            let mut gaps = vec![0u16; len];
            for hc in 0..hs.len().pow(len as u32) {
                let mut c = hc;
                for x in heights.iter_mut() {
                    *x = hs[c % hs.len()];
                    c /= hs.len();
                }
                for gc in 0..gs.len().pow(len as u32) {
                    let mut c = gc;
                    for x in gaps.iter_mut() {
                        *x = gs[c % gs.len()];
                        c /= gs.len();
                    }
                    for body in 0..=6u16 {
                        for start in 0..=5usize {
                            let ours = list_pos(&heights, &gaps, body, start);
                            let m = h_list(&heights, &gaps, body, start);
                            assert_eq!(
                                (ours.offset_from_bottom(), ours.max, ours.viewport),
                                (
                                    m.offset_from_bottom,
                                    m.max_offset_from_bottom,
                                    m.viewport_rows
                                ),
                                "{heights:?} {gaps:?} {body} {start}"
                            );
                            for target in 0..=5usize {
                                assert_eq!(
                                    reveal(&heights, &gaps, body, start, target),
                                    h_reveal(&heights, &gaps, body, start, target),
                                    "{heights:?} {gaps:?} body={body} start={start} target={target}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- what the port changes ----

    #[test]
    fn a_track_the_thumb_fills_keeps_the_view() {
        // herdr: offset_from_bottom 0, i.e. a jump to the bottom.
        let track = Rect::new(0, 0, 1, 1);
        let pos = ScrollPos::new(4, 9, 3);
        assert_eq!(thumb(pos, track).map(|t| t.len), Some(1));
        assert_eq!(offset_at(pos, track, 0), 4);
        assert_eq!(offset_dragged(pos, track, 0, 0), 4);
        // A zero-row track has no thumb: nothing moves either.
        let none = Rect::new(0, 0, 1, 0);
        assert_eq!(offset_at(pos, none, 0), 4);
        assert_eq!(offset_dragged(pos, none, 3, 0), 4);
    }

    #[test]
    fn reveal_is_linear_on_a_long_list() {
        // herdr re-measured the whole list per step: O(n²) here.
        let heights: Vec<u16> = (0..200_000).map(|i| 1 + (i % 3) as u16).collect();
        let gaps: Vec<u16> = (0..200_000).map(|i| u16::from(i % 5 == 4)).collect();
        let s = reveal(&heights, &gaps, 30, 0, 199_990);
        let pos = list_pos(&heights, &gaps, 30, s);
        assert!(s <= 199_990 && 199_990 < s + pos.viewport, "{s} {pos:?}");
        assert_eq!(
            reveal(&heights, &gaps, 30, s, 199_990),
            s,
            "already in view"
        );
        assert_eq!(
            reveal(&heights, &gaps, 30, s, 7),
            7,
            "above: it becomes the first"
        );
    }

    #[test]
    fn positions_convert_from_and_to_the_bottom() {
        let p = ScrollPos::from_bottom(3, 10, 5);
        assert_eq!((p.top, p.offset_from_bottom(), p.total()), (7, 3, 15));
        assert_eq!(ScrollPos::from_bottom(99, 10, 5).top, 0);
        assert_eq!(ScrollPos::new(99, 10, 5).top, 10);
        assert_eq!(ScrollPos::of(4, 10, 3), ScrollPos::new(0, 0, 10));
        assert!(!ScrollPos::of(4, 10, 3).overflows());
    }

    #[test]
    fn keys_move_by_row_page_and_to_the_ends() {
        let p = ScrollPos::of(100, 20, 10);
        assert_eq!(p.on_key(Key::Up.into()), Some(9));
        assert_eq!(p.on_key(Key::Down.into()), Some(11));
        assert_eq!(p.on_key(Key::PageUp.into()), Some(0));
        assert_eq!(p.on_key(Key::PageDown.into()), Some(30));
        assert_eq!(p.on_key(Key::Home.into()), Some(0));
        assert_eq!(p.on_key(Key::End.into()), Some(80));
        assert_eq!(p.with_top(80).on_key(Key::Down.into()), Some(80));
        assert_eq!(p.on_key(KeyInput::from('j')), None);
        // An empty viewport still pages by one.
        assert_eq!(
            ScrollPos::new(3, 9, 0).on_key(Key::PageDown.into()),
            Some(4)
        );
    }

    #[test]
    fn drag_helper_grabs_jumps_and_releases() {
        let track = Rect::new(20, 0, 1, 10);
        let pos = ScrollPos::of(100, 10, 0);
        let mut d = ScrollbarDrag::new();
        // Off the track: not ours.
        assert_eq!(d.press(pos, track, 19, 0), None);
        assert!(!d.is_dragging());
        // On the thumb (row 0, one row long): grabbed, view unchanged.
        assert_eq!(d.press(pos, track, 20, 0), Some(0));
        assert_eq!(d.grab_offset(), Some(0));
        let to = d.drag(pos, track, 5).expect("dragging");
        assert_eq!(to, offset_dragged(pos, track, 5, 0));
        // Sideways drift keeps dragging; release ends it.
        d.release();
        assert_eq!(d.drag(pos, track, 9), None);
        // A press on the track below the thumb jumps there and grabs, so
        // the same press can drag on.
        let top = d.press(pos, track, 20, 9).expect("on track");
        assert_eq!(top, pos.max);
        assert!(d.is_dragging());
        assert_eq!(d.drag(pos.with_top(top), track, 0), Some(0));
        // Nothing overflows: the track is not a target.
        let fits = ScrollPos::of(5, 10, 0);
        assert_eq!(d.press(fits, track, 20, 3), None);
        assert!(!d.is_dragging());
    }

    fn draw(sb: &Scrollbar, area: Rect, buf_area: Rect) -> Buffer {
        let mut buf = Buffer::empty(buf_area);
        sb.render(area, &mut buf);
        buf
    }

    #[test]
    fn draws_track_and_thumb_in_the_rightmost_column() {
        let pos = ScrollPos::of(40, 10, 15);
        let sb = Scrollbar::new(pos).theme(Theme::ansi());
        let buf = draw(&sb, Rect::new(0, 0, 4, 10), Rect::new(0, 0, 4, 10));
        let col: Vec<String> = snapshot::lines(&buf)
            .iter()
            .map(|l| l.chars().last().map(String::from).unwrap_or_default())
            .collect();
        // thumb: len round(10*10/40)=3, top round(15*7/30)=4 (3.5 rounds up).
        assert_eq!(
            col,
            ["╎", "╎", "╎", "╎", "▐", "▐", "▐", "╎", "╎", "╎"],
            "{col:?}"
        );
        assert_eq!(buf[(3, 0)].fg, Theme::ansi().muted);
        assert_eq!(buf[(3, 4)].fg, Theme::ansi().accent);
        let unfocused = draw(
            &sb.focused(false),
            Rect::new(0, 0, 4, 10),
            Rect::new(0, 0, 4, 10),
        );
        assert_eq!(unfocused[(3, 4)].fg, Theme::ansi().text_secondary);
        assert_eq!(unfocused[(3, 4)].symbol(), "▐", "same glyph focused or not");
    }

    #[test]
    fn nothing_is_drawn_when_it_all_fits() {
        let sb = Scrollbar::new(ScrollPos::of(5, 10, 0));
        let buf = draw(&sb, Rect::new(0, 0, 3, 10), Rect::new(0, 0, 3, 10));
        assert!(snapshot::text(&buf).trim().is_empty());
    }

    #[test]
    fn mono_tells_thumb_from_track_by_glyph_and_weight() {
        for focused in [true, false] {
            let sb = Scrollbar::new(ScrollPos::of(30, 10, 0))
                .focused(focused)
                .theme(Theme::mono());
            let buf = draw(&sb, Rect::new(0, 0, 1, 10), Rect::new(0, 0, 1, 10));
            let thumb = &buf[(0, 0)];
            let track = &buf[(0, 9)];
            assert_ne!(thumb.symbol(), track.symbol());
            assert!(thumb.modifier.contains(Modifier::BOLD));
            assert!(track.modifier.contains(Modifier::DIM));
            assert!(!thumb.modifier.contains(Modifier::DIM));
            for c in buf.content() {
                assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
            }
        }
    }

    #[test]
    fn default_glyphs_are_one_cell_on_cjk_terminals() {
        for g in [ScrollbarGlyphs::default(), ScrollbarGlyphs::ascii()] {
            assert_eq!(g.track.width_cjk(), 1, "{g:?}");
            assert_eq!(g.thumb.width_cjk(), 1, "{g:?}");
        }
        // herdr's track is Ambiguous: that is why it is opt-in.
        assert_eq!(ScrollbarGlyphs::herdr().track.width_cjk(), 2);
    }

    #[test]
    fn tiny_and_out_of_buffer_areas_never_panic_or_spill() {
        let pos = ScrollPos::of(50, 3, 7);
        for w in 0..=3u16 {
            for h in 0..=4u16 {
                let buf_area = Rect::new(2, 1, w, h);
                // An area hanging off the buffer on every side.
                for area in [
                    buf_area,
                    Rect::new(0, 0, w + 4, h + 4),
                    Rect::new(2 + w, 1 + h, 3, 3),
                ] {
                    let mut buf = Buffer::empty(buf_area);
                    Scrollbar::new(pos).render(area, &mut buf);
                    assert_eq!(buf.area, buf_area);
                }
            }
        }
        // Track helpers on a zero-width area.
        let z = Rect::new(5, 5, 0, 4);
        assert!(track(z).is_empty());
        assert_eq!(split(z, pos), (z, None));
        let (content, t) = split(Rect::new(0, 0, 10, 4), pos);
        assert_eq!((content.width, t), (9, Some(Rect::new(9, 0, 1, 4))));
    }

    #[test]
    fn huge_positions_do_not_overflow() {
        let track = Rect::new(0, 0, 1, u16::MAX);
        let pos = ScrollPos::new(usize::MAX / 3, usize::MAX - 10, 9);
        let t = thumb(pos, track).expect("thumb");
        assert_eq!(t.len, 1);
        let top = offset_at(pos, track, u16::MAX);
        assert_eq!(top, pos.max);
        assert_eq!(offset_dragged(pos, track, 0, 0), 0);
    }
}
