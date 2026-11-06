// Derived from herdrdev/herdr `src/popup_size.rs` (`PopupSize`, `resolve`,
// `parse_cli`, `resolve_popup_geometry` and their tests), `src/ui/widgets.rs`
// (`centered_popup_rect`, `modal_stack_areas`, `action_button_width`,
// `close_button_rect`), `src/client/shell/overlays.rs` (the backdrop dim in
// `render_client_overlay`, `panel`, `popup_with_width_cap`, `button`, `row`,
// `render_release_notes_overlay`, `render_confirm_close_overlay`) and
// `src/ui/release_notes.rs` (`display_lines_scroll_metrics`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab: one
// `Modal` builder instead of an overlay function per product popup; each side
// is fixed (cells or percent, as herdr) or fits the content up to a cap; the
// popup is clamped to the area instead of disappearing below a minimum, and
// degrades row by row (gaps, subtitle, footer go first; the title, buttons and
// a body row stay); the stack's gaps are dropped before content; the body
// wraps, scrolls with the crate's scrollbar and says which rows are in view;
// the footer is a `KeyHints` line; buttons can take focus (Tab, ← →), are
// measured in cells (herdr counted chars), keep a glyph for focus, shrink with
// `…` instead of overrunning, and return hit rects clipped to what is drawn;
// a destructive tone colours the border and writes a word on it, so it is not
// colour alone; every rect is clamped inside the area and the buffer; keys
// and mouse events are handled by `ModalState`; styles come from the `Theme`.

//! A dialog over the screen: a title, a body that wraps and scrolls, a key
//! hint line and buttons — release notes, a destructive confirm, a notice.
//!
//! Ported from herdr (`src/popup_size.rs`, `src/ui/widgets.rs`,
//! `src/client/shell/overlays.rs`, Apache-2.0), whose release notes, rename
//! box, onboarding and close confirmation share one frame. It tells the person
//! that something needs an answer before they go on, what it is, and what
//! each answer does:
//!
//! ```text
//! ┌─ danger ─────────────────────────────────┐
//! │ Delete account ai3@iq.io?                │
//! │                                          │
//! │ Its API key and 30 days of usage are     │
//! │ removed. Requests fail over to the next  │
//! │ account in rotation.                     │
//! │                                          │
//! │      › n cancel ‹  [ y delete ]          │
//! └──────────────────────────────────────────┘
//! ```
//!
//! Sizing: each side is [`ModalSize::Fixed`] (herdr's [`PopupSize`] in cells
//! or percent) or [`ModalSize::Fit`] — as much as the content needs, up to a
//! cap — so a two-line notice gets a small box rather than 80 % of the
//! screen. Either way the popup is centred, raised to a minimum and clamped
//! to the area. [`Modal::render`] returns a [`ModalLayout`] with the rects a
//! host routes the mouse by; [`ModalState`] keeps the scroll position and
//! the focused button and turns keys and clicks into a [`ModalOutcome`].

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, StatefulWidget, Widget, Wrap};

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Status, Theme};
use crate::widgets::key_hints::KeyHints;
use crate::widgets::scrollbar::{ScrollPos, Scrollbar, ScrollbarDrag};

/// One side of a popup: cells, or a share of the area (herdr's
/// `PopupSize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopupSize {
    Cells(u16),
    /// 1–100; more counts as 100.
    Percent(u8),
}

impl PopupSize {
    /// Cells out of `available`: the cell count itself, or the share rounded
    /// down.
    pub const fn resolve(self, available: u16) -> u16 {
        match self {
            PopupSize::Cells(cells) => cells,
            PopupSize::Percent(p) => {
                let p = if p > 100 { 100 } else { p };
                ((available as u32 * p as u32) / 100) as u16
            }
        }
    }

    /// `"120"` (cells) or `"80%"` (1 % to 100 %), as herdr's
    /// `--width`/`--height` flags read it.
    pub fn parse(value: &str) -> Result<PopupSize, String> {
        const HINT: &str = "must be a number of cells or a percentage like 80%";
        if let Some(percent) = value.strip_suffix('%') {
            let percent = percent.parse::<u8>().map_err(|_| HINT.to_string())?;
            if !(1..=100).contains(&percent) {
                return Err("percentage must be between 1% and 100%".to_string());
            }
            return Ok(PopupSize::Percent(percent));
        }
        value
            .parse::<u16>()
            .map(PopupSize::Cells)
            .map_err(|_| HINT.to_string())
    }
}

/// The smallest popup herdr keeps: 6 × 4 cells, border included.
pub const MIN_SIZE: (u16, u16) = (6, 4);

/// herdr's `resolve_popup_geometry`, outer rect: each side resolved against
/// the area, raised to `min`, clamped to the area, centred. Never outside
/// `area`; empty only when `area` is (herdr returned `None` below 6 × 4 —
/// here the popup takes what there is).
pub fn popup_rect(area: Rect, width: PopupSize, height: PopupSize, min: (u16, u16)) -> Rect {
    let w = width.resolve(area.width).max(min.0).min(area.width);
    let h = height.resolve(area.height).max(min.1).min(area.height);
    centre(area, w, h)
}

fn centre(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// How one side of a [`Modal`] is sized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalSize {
    /// Exactly this (clamped to the area).
    Fixed(PopupSize),
    /// What the content needs — the widest line, the wrapped body — but no
    /// more than this.
    Fit(PopupSize),
}

/// The modal's tone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    #[default]
    Normal,
    /// A destructive confirm: the border in the failure style (bold in mono)
    /// and a word on it ([`Modal::tone_word`], default `danger`).
    Danger,
}

/// A button in the action row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button<'a> {
    pub label: Cow<'a, str>,
    /// Shown before the label. A single character also presses the button
    /// when typed.
    pub key: Option<Cow<'a, str>>,
    /// Drawn in the failure style: delete, revoke, reset.
    pub destructive: bool,
}

impl<'a> Button<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>) -> Button<'a> {
        Button {
            label: label.into(),
            key: None,
            destructive: false,
        }
    }

    pub fn key(mut self, key: impl Into<Cow<'a, str>>) -> Button<'a> {
        self.key = Some(key.into());
        self
    }

    pub fn destructive(mut self, destructive: bool) -> Button<'a> {
        self.destructive = destructive;
        self
    }

    fn typed_key(&self) -> Option<char> {
        let mut cs = self.key.as_deref()?.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        }
    }

    /// Cells as drawn: `[ key label ]`.
    pub fn width(&self) -> u16 {
        let key = self.key.as_deref().map_or(0, |k| text::width(k) + 1);
        u16::try_from(4 + key + text::width(&self.label)).unwrap_or(u16::MAX)
    }
}

/// Where a modal's parts went, for the mouse. Every rect is inside the
/// area the modal was drawn in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModalLayout {
    /// The whole popup, border included. Empty when nothing was drawn.
    pub outer: Rect,
    /// Where the body text is (without the scrollbar column).
    pub body: Rect,
    /// The scrollbar track, when the body overflows.
    pub track: Option<Rect>,
    /// The body's scroll position, for [`crate::scrollbar`]'s maths.
    pub pos: ScrollPos,
    /// One rect per drawn button and the button's index.
    pub buttons: Vec<(Rect, usize)>,
    /// The `esc close` button, when there was room for it.
    pub close: Option<Rect>,
}

impl ModalLayout {
    /// The button under `(col, row)`.
    pub fn button_at(&self, col: u16, row: u16) -> Option<usize> {
        let p = Position::new(col, row);
        self.buttons
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, i)| *i)
    }

    /// Whether `(col, row)` is on the popup.
    pub fn contains(&self, col: u16, row: u16) -> bool {
        self.outer.contains(Position::new(col, row))
    }
}

/// What a key or a click did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModalOutcome {
    /// Nothing changed.
    Ignored,
    /// The body moved; redraw.
    Scrolled,
    /// Focus moved to this button; redraw.
    Focused(usize),
    /// This button was pressed (Enter on it, its key, a click).
    Pressed(usize),
    /// Esc, the close button, or Enter on a modal without buttons.
    Closed,
    /// A press outside the popup. herdr ignores it; a host may close.
    Outside,
}

/// The scroll position, the focused button, and what the last render drew.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModalState {
    /// Body rows scrolled past (clamped at each render).
    pub scroll: usize,
    /// The focused button (clamped at each render).
    pub focus: usize,
    /// Where the last render put things.
    pub layout: ModalLayout,
    keys: Vec<Option<char>>,
    drag: ScrollbarDrag,
}

impl ModalState {
    pub fn new() -> ModalState {
        ModalState::default()
    }

    /// Start with button `focus` focused — the safe answer of a destructive
    /// confirm, say.
    pub fn with_focus(focus: usize) -> ModalState {
        ModalState {
            focus,
            ..ModalState::default()
        }
    }

    /// Scroll the body to `rows` (clamped at the next render).
    pub fn scroll_to(&mut self, rows: usize) {
        self.scroll = rows;
    }

    fn set_scroll(&mut self, top: usize) -> ModalOutcome {
        let top = top.min(self.layout.pos.max);
        if top == self.scroll {
            return ModalOutcome::Ignored;
        }
        self.scroll = top;
        self.layout.pos = self.layout.pos.with_top(top);
        ModalOutcome::Scrolled
    }

    /// Act on a key, using what the last render drew (its buttons, page
    /// size and scroll range): ↑ ↓ PgUp PgDn Home End scroll the body; Tab /
    /// → and Shift-Tab / ← move between buttons; Enter presses the focused
    /// one (closes a modal without buttons); a button's one-character key
    /// presses it; Esc closes.
    pub fn key(&mut self, key: KeyInput) -> ModalOutcome {
        let n = self.keys.len();
        if let Some(top) = self.layout.pos.with_top(self.scroll).on_key(key) {
            return self.set_scroll(top);
        }
        let back =
            matches!(key.key, Key::BackTab | Key::Left) || (key.key == Key::Tab && key.mods.shift);
        match key.key {
            Key::Esc => ModalOutcome::Closed,
            Key::Enter if n == 0 => ModalOutcome::Closed,
            Key::Enter => ModalOutcome::Pressed(self.focus.min(n - 1)),
            Key::Tab | Key::BackTab | Key::Left | Key::Right if n > 0 => {
                let cur = self.focus.min(n - 1);
                self.focus = if back {
                    (cur + n - 1) % n
                } else {
                    (cur + 1) % n
                };
                ModalOutcome::Focused(self.focus)
            }
            _ => match key.typed() {
                Some(c) => match self.keys.iter().position(|k| *k == Some(c)) {
                    Some(i) => {
                        self.focus = i;
                        ModalOutcome::Pressed(i)
                    }
                    None => ModalOutcome::Ignored,
                },
                None => ModalOutcome::Ignored,
            },
        }
    }

    /// A mouse press at `(col, row)`: the close button closes, a button is
    /// focused and pressed, the scrollbar jumps or grabs its thumb.
    pub fn press(&mut self, col: u16, row: u16) -> ModalOutcome {
        let p = Position::new(col, row);
        if self.layout.close.is_some_and(|r| r.contains(p)) {
            return ModalOutcome::Closed;
        }
        if let Some(i) = self.layout.button_at(col, row) {
            self.focus = i;
            return ModalOutcome::Pressed(i);
        }
        if let Some(track) = self.layout.track {
            let pos = self.layout.pos.with_top(self.scroll);
            if let Some(top) = self.drag.press(pos, track, col, row) {
                return self.set_scroll(top);
            }
        }
        if self.layout.contains(col, row) {
            ModalOutcome::Ignored
        } else {
            ModalOutcome::Outside
        }
    }

    /// The pointer moved with the button held: drags the scrollbar thumb
    /// when a press grabbed it.
    pub fn drag(&mut self, _col: u16, row: u16) -> ModalOutcome {
        let Some(track) = self.layout.track else {
            return ModalOutcome::Ignored;
        };
        let pos = self.layout.pos.with_top(self.scroll);
        match self.drag.drag(pos, track, row) {
            Some(top) => self.set_scroll(top),
            None => ModalOutcome::Ignored,
        }
    }

    /// The mouse button went up.
    pub fn release(&mut self) {
        self.drag.release();
    }

    /// The wheel: `delta` rows down (negative: up).
    pub fn scroll_by(&mut self, delta: isize) -> ModalOutcome {
        let top = self.scroll.saturating_add_signed(delta);
        self.set_scroll(top)
    }
}

const CLOSE_KEY: &str = "esc";
const CLOSE_LABEL: &str = " close";
const CLOSE_W: u16 = 9;
const GAP: u16 = 1;

/// A centred dialog. Build it each frame, draw it over the whole screen
/// (or the part it belongs to) with [`Modal::render`].
#[derive(Debug, Clone)]
pub struct Modal<'a> {
    title: Cow<'a, str>,
    subtitle: Option<Cow<'a, str>>,
    body: Vec<Line<'a>>,
    footer: Option<KeyHints<'a>>,
    actions: Vec<Button<'a>>,
    width: ModalSize,
    height: ModalSize,
    min: (u16, u16),
    margin: (u16, u16),
    tone: Tone,
    tone_word: Cow<'a, str>,
    scrim: bool,
    close: bool,
    theme: Theme,
}

/// Rows of each part of the stack, after the area had its say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Rows {
    title: u16,
    subtitle: u16,
    gap_head: u16,
    body: u16,
    spacer: u16,
    gap_footer: u16,
    footer: u16,
    gap_actions: u16,
    actions: u16,
}

/// Grant `rows` to the parts in order of importance: the title, the
/// buttons, a body row, the footer, the subtitle, then the gaps; the body
/// takes what is left.
fn stack(rows: u16, sub: bool, body: bool, footer: bool, actions: bool) -> Rows {
    let mut left = rows;
    let mut take = |want: bool| -> u16 {
        if want && left > 0 {
            left -= 1;
            1
        } else {
            0
        }
    };
    let title = take(true);
    let actions = take(actions);
    let body_min = take(body);
    let footer = take(footer);
    let subtitle = take(sub);
    let gap_head = take(title > 0 && body_min + footer + actions > 0) * GAP;
    let gap_actions = take(actions > 0 && body_min + footer > 0) * GAP;
    let gap_footer = take(footer > 0 && body_min > 0) * GAP;
    let (body, spacer) = if body {
        (body_min + left, 0)
    } else {
        (0, left)
    };
    Rows {
        title,
        subtitle,
        gap_head,
        body,
        spacer,
        gap_footer,
        footer,
        gap_actions,
        actions,
    }
}

/// Everything a render needs, worked out once.
struct Plan {
    outer: Rect,
    border: bool,
    title: Option<Rect>,
    subtitle: Option<Rect>,
    text: Rect,
    track: Option<Rect>,
    footer: Option<Rect>,
    close: Option<Rect>,
    buttons: Vec<(Rect, usize)>,
    pos: ScrollPos,
    /// Body rows with no room at all (the stack gave the buttons the space).
    hidden: usize,
    lines: Vec<Line<'static>>,
}

fn owned(line: &Line<'_>) -> Line<'static> {
    let mut l = Line::from(
        line.spans
            .iter()
            .map(|s| Span::styled(s.content.to_string(), s.style))
            .collect::<Vec<_>>(),
    )
    .style(line.style);
    l.alignment = line.alignment;
    text::wrap_safe_line(l)
}

/// Rows `lines` take wrapped at `width` — ratatui's own count.
fn wrapped(lines: &[Line<'static>], width: u16) -> usize {
    if lines.is_empty() {
        return 0;
    }
    Paragraph::new(lines.to_vec())
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
}

/// The lines that fill `height` rows from wrapped row `top` at `width`, and
/// how many rows of the first one are above the view. Whole lines before
/// `top` are skipped by counting, so the scroll position stays a `usize`
/// (ratatui's `Paragraph::scroll` takes a `u16`); only a single line taller
/// than `u16::MAX` rows would be cut short.
fn from_row<'l>(
    lines: &'l [Line<'static>],
    width: u16,
    top: usize,
    height: u16,
) -> (&'l [Line<'static>], u16) {
    let rows = |l: &Line<'static>| wrapped(std::slice::from_ref(l), width);
    let mut start = 0;
    let mut first = lines.len();
    for (i, l) in lines.iter().enumerate() {
        let n = rows(l);
        if start + n > top {
            first = i;
            break;
        }
        start += n;
    }
    let skip = top - start;
    let need = skip.saturating_add(usize::from(height));
    let (mut end, mut got) = (first, 0);
    while end < lines.len() && got < need {
        got += rows(&lines[end]);
        end += 1;
    }
    (&lines[first..end], u16::try_from(skip).unwrap_or(u16::MAX))
}

fn rect_clamped(r: Rect, within: Rect) -> Rect {
    let r = r.intersection(within);
    if r.is_empty() { Rect::default() } else { r }
}

impl<'a> Modal<'a> {
    pub fn new(title: impl Into<Cow<'a, str>>) -> Modal<'a> {
        Modal {
            title: title.into(),
            subtitle: None,
            body: Vec::new(),
            footer: None,
            actions: Vec::new(),
            width: ModalSize::Fit(PopupSize::Percent(80)),
            height: ModalSize::Fit(PopupSize::Percent(80)),
            min: MIN_SIZE,
            margin: (2, 1),
            tone: Tone::Normal,
            tone_word: Cow::Borrowed("danger"),
            scrim: true,
            close: true,
            theme: Theme::default(),
        }
    }

    /// A line under the title, in the secondary colour.
    pub fn subtitle(mut self, subtitle: impl Into<Cow<'a, str>>) -> Modal<'a> {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// The body: lines that wrap at the popup's width and scroll when they
    /// do not fit.
    pub fn body<L: Into<Line<'a>>>(mut self, lines: impl IntoIterator<Item = L>) -> Modal<'a> {
        self.body = lines.into_iter().map(Into::into).collect();
        self
    }

    /// The body from plain text, one line per `\n`.
    pub fn text(self, text: &'a str) -> Modal<'a> {
        self.body(text.split('\n'))
    }

    /// A key-hint line at the bottom; it drops whole hints when narrow.
    pub fn footer(mut self, hints: KeyHints<'a>) -> Modal<'a> {
        self.footer = Some(hints);
        self
    }

    /// The button row, centred under everything else.
    pub fn actions(mut self, buttons: impl IntoIterator<Item = Button<'a>>) -> Modal<'a> {
        self.actions = buttons.into_iter().collect();
        self
    }

    /// Both sides fixed, in cells or percent (herdr's popup sizes).
    pub fn size(mut self, width: PopupSize, height: PopupSize) -> Modal<'a> {
        self.width = ModalSize::Fixed(width);
        self.height = ModalSize::Fixed(height);
        self
    }

    /// Both sides fit the content, up to `max_width` × `max_height`. The
    /// default, at 80 % × 80 %.
    pub fn fit_content(mut self, max_width: PopupSize, max_height: PopupSize) -> Modal<'a> {
        self.width = ModalSize::Fit(max_width);
        self.height = ModalSize::Fit(max_height);
        self
    }

    pub fn width(mut self, width: ModalSize) -> Modal<'a> {
        self.width = width;
        self
    }

    pub fn height(mut self, height: ModalSize) -> Modal<'a> {
        self.height = height;
        self
    }

    /// The smallest popup, border included, when the area allows. Default
    /// 6 × 4 (herdr's).
    pub fn min_size(mut self, width: u16, height: u16) -> Modal<'a> {
        self.min = (width, height);
        self
    }

    /// Cells kept free left and right, rows above and below, so the screen
    /// under the popup still shows. Default 2 and 1 (herdr's popups).
    pub fn margin(mut self, horizontal: u16, vertical: u16) -> Modal<'a> {
        self.margin = (horizontal, vertical);
        self
    }

    pub fn tone(mut self, tone: Tone) -> Modal<'a> {
        self.tone = tone;
        self
    }

    /// The word a [`Tone::Danger`] modal writes on its border. Default
    /// `danger`.
    pub fn tone_word(mut self, word: impl Into<Cow<'a, str>>) -> Modal<'a> {
        self.tone_word = word.into();
        self
    }

    /// Dim everything under the popup (the area it is drawn in). Default
    /// true. A weight, not a colour, so mono keeps it.
    pub fn scrim(mut self, scrim: bool) -> Modal<'a> {
        self.scrim = scrim;
        self
    }

    /// The `esc close` button in the title row. Default true.
    pub fn close_button(mut self, close: bool) -> Modal<'a> {
        self.close = close;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Modal<'a> {
        self.theme = theme;
        self
    }

    fn danger(&self) -> bool {
        self.tone == Tone::Danger
    }

    fn footer_line(&self, width: u16) -> Option<Line<'static>> {
        self.footer
            .as_ref()
            .map(|f| f.clone().theme(self.theme).line(width))
    }

    /// Cells the content wants across, without border and padding.
    fn content_width(&self) -> u16 {
        let close = if self.close { CLOSE_W as usize + 2 } else { 0 };
        let title = text::width(&self.title) + close;
        let tone = if self.danger() {
            text::width(&self.tone_word) + 2
        } else {
            0
        };
        let subtitle = self.subtitle.as_deref().map_or(0, text::width);
        let body = self.body.iter().map(text::line_width).max().unwrap_or(0);
        let footer = self
            .footer_line(u16::MAX)
            .map_or(0, |l| text::line_width(&l));
        let n = self.actions.len();
        let actions = self
            .actions
            .iter()
            .map(|b| usize::from(b.width()))
            .sum::<usize>()
            + 2 * n.saturating_sub(1);
        let w = [title, tone, subtitle, body, footer, actions]
            .into_iter()
            .max()
            .unwrap_or(0);
        u16::try_from(w).unwrap_or(u16::MAX)
    }

    /// Rows the whole popup wants at outer width `w`.
    fn fit_height(&self, w: u16, lines: &[Line<'static>]) -> u16 {
        let border = w >= 4;
        let inner_w = if border { w - 2 } else { w };
        let pad = u16::from(inner_w >= 8);
        let content_w = inner_w - 2 * pad;
        let body = u16::try_from(wrapped(lines, content_w)).unwrap_or(u16::MAX);
        // The header, then each part present after a gap.
        let mut h: u16 = 1 + u16::from(self.subtitle.is_some());
        for (present, rows) in [
            (body > 0, body),
            (self.footer.is_some(), 1),
            (!self.actions.is_empty(), 1),
        ] {
            if present {
                h = h.saturating_add(GAP).saturating_add(rows);
            }
        }
        h.saturating_add(2 * u16::from(border))
    }

    fn outer_size(&self, area: Rect, lines: &[Line<'static>]) -> (u16, u16) {
        let cap_w = area.width.saturating_sub(self.margin.0.saturating_mul(2));
        let cap_h = area.height.saturating_sub(self.margin.1.saturating_mul(2));
        let w = match self.width {
            ModalSize::Fixed(s) => s.resolve(area.width),
            ModalSize::Fit(max) => self
                .content_width()
                .saturating_add(4)
                .min(max.resolve(area.width)),
        };
        let w = w.min(cap_w).max(self.min.0).min(area.width);
        let h = match self.height {
            ModalSize::Fixed(s) => s.resolve(area.height),
            ModalSize::Fit(max) => self.fit_height(w, lines).min(max.resolve(area.height)),
        };
        let h = h.min(cap_h).max(self.min.1).min(area.height);
        (w, h)
    }

    /// The popup's outer rect in `area`: what it would cover.
    pub fn outer(&self, area: Rect) -> Rect {
        let lines: Vec<Line<'static>> = self.body.iter().map(owned).collect();
        let (w, h) = self.outer_size(area, &lines);
        centre(area, w, h)
    }

    fn button_rects(&self, row: Rect) -> Vec<(Rect, usize)> {
        let n = self.actions.len();
        if n == 0 || row.is_empty() {
            return Vec::new();
        }
        let full: Vec<u16> = self.actions.iter().map(Button::width).collect();
        let sum = |ws: &[u16]| ws.iter().map(|w| u32::from(*w)).sum::<u32>();
        let gaps = |g: u16| u32::from(g) * (n as u32 - 1);
        let mut gap = 2u16;
        if sum(&full) + gaps(gap) > u32::from(row.width) {
            gap = 1;
        }
        let mut ws = full.clone();
        if sum(&ws) + gaps(gap) > u32::from(row.width) {
            let room = u32::from(row.width).saturating_sub(gaps(gap));
            let share = u16::try_from(room / n as u32).unwrap_or(u16::MAX).max(1);
            ws.iter_mut().for_each(|w| *w = (*w).min(share));
        }
        let total = sum(&ws) + gaps(gap);
        let lead = u32::from(row.width).saturating_sub(total) / 2;
        let mut x = u32::from(row.x) + lead;
        let mut out = Vec::new();
        for (i, w) in ws.into_iter().enumerate() {
            if x >= u32::from(row.right()) {
                break;
            }
            let x16 = x as u16;
            let w = w.min(row.right() - x16);
            out.push((Rect::new(x16, row.y, w, 1), i));
            x += u32::from(w) + u32::from(gap);
        }
        out
    }

    fn plan(&self, area: Rect, scroll: usize) -> Plan {
        let lines: Vec<Line<'static>> = self.body.iter().map(owned).collect();
        let (w, h) = self.outer_size(area, &lines);
        let outer = centre(area, w, h);
        let empty = Plan {
            outer,
            border: false,
            title: None,
            subtitle: None,
            text: Rect::default(),
            track: None,
            footer: None,
            close: None,
            buttons: Vec::new(),
            pos: ScrollPos::default(),
            hidden: 0,
            lines: Vec::new(),
        };
        if outer.is_empty() {
            return empty;
        }
        let border = outer.width >= 4 && outer.height >= 3;
        let inner = if border {
            outer.inner(Margin::new(1, 1))
        } else {
            outer
        };
        let pad = u16::from(inner.width >= 8);
        let content = Rect::new(inner.x + pad, inner.y, inner.width - 2 * pad, inner.height);
        let has_body = !lines.is_empty();
        let rows = stack(
            content.height,
            self.subtitle.is_some(),
            has_body,
            self.footer.is_some(),
            !self.actions.is_empty(),
        );
        let row = |y: u16, h: u16| (h > 0).then(|| Rect::new(content.x, y, content.width, h));
        let mut y = content.y;
        let title = row(y, rows.title);
        y += rows.title;
        let subtitle = row(y, rows.subtitle);
        y += rows.subtitle + rows.gap_head;
        let body = row(y, rows.body);
        y += rows.body + rows.spacer + rows.gap_footer;
        let footer = row(y, rows.footer);
        y += rows.footer + rows.gap_actions;
        let actions = row(y, rows.actions);

        let (text, track, pos) = match body {
            Some(body) => {
                let full = wrapped(&lines, body.width);
                let view = usize::from(body.height);
                if full <= view {
                    (body, None, ScrollPos::of(full, view, scroll))
                } else if pad == 1 {
                    // The right padding column holds the track: the text
                    // keeps its width, so nothing re-wraps.
                    let track = Rect::new(inner.right() - 1, body.y, 1, body.height);
                    (body, Some(track), ScrollPos::of(full, view, scroll))
                } else if body.width >= 2 {
                    let text = Rect {
                        width: body.width - 1,
                        ..body
                    };
                    let total = wrapped(&lines, text.width);
                    let track = Rect::new(body.right() - 1, body.y, 1, body.height);
                    (text, Some(track), ScrollPos::of(total, view, scroll))
                } else {
                    (body, None, ScrollPos::of(full, view, scroll))
                }
            }
            None => (Rect::default(), None, ScrollPos::default()),
        };

        let close = match title {
            Some(t) if self.close && t.width >= CLOSE_W + 6 => {
                Some(Rect::new(t.right() - CLOSE_W, t.y, CLOSE_W, 1))
            }
            _ => None,
        };
        let buttons = actions.map_or_else(Vec::new, |r| self.button_rects(r));
        let hidden = if has_body && body.is_none() {
            wrapped(&lines, content.width)
        } else {
            0
        };
        Plan {
            outer,
            border,
            title,
            subtitle,
            text,
            track,
            footer,
            close,
            buttons,
            pos,
            hidden,
            lines,
        }
    }

    /// Where everything would go in `area` with `state`'s scroll position,
    /// without drawing — for a host that routes input before it draws.
    pub fn layout(&self, area: Rect, state: &ModalState) -> ModalLayout {
        let p = self.plan(area, state.scroll);
        ModalLayout {
            outer: p.outer,
            body: p.text,
            track: p.track,
            pos: p.pos,
            buttons: p.buttons,
            close: p.close,
        }
    }

    fn border_style(&self) -> Style {
        let t = &self.theme;
        match (self.tone, t.is_mono()) {
            (Tone::Danger, _) => t.status(Status::Failed).add_modifier(Modifier::BOLD),
            (Tone::Normal, true) => Style::new(),
            (Tone::Normal, false) => t.accent(),
        }
    }

    fn draw_title(&self, buf: &mut Buffer, p: &Plan) {
        let t = &self.theme;
        let Some(r) = p.title else { return };
        let room = match p.close {
            Some(_) => r.width - CLOSE_W - 2,
            None => r.width,
        };
        let mut spans = Vec::new();
        if self.danger() && !p.border {
            // No border to write the word on: it leads the title.
            spans.push(Span::styled(
                format!("{}: ", self.tone_word),
                t.status(Status::Failed).add_modifier(Modifier::BOLD),
            ));
        }
        let title_style = if self.danger() {
            t.status(Status::Failed).add_modifier(Modifier::BOLD)
        } else {
            t.text().add_modifier(Modifier::BOLD)
        };
        spans.push(Span::styled(self.title.to_string(), title_style));
        let line = text::truncate_line(Line::from(spans), usize::from(room));
        buf.set_line(r.x, r.y, &line, room);
        if let Some(c) = p.close {
            let line = Line::from(vec![
                Span::styled(CLOSE_KEY, t.accent().add_modifier(Modifier::BOLD)),
                Span::styled(CLOSE_LABEL, t.secondary()),
            ]);
            buf.set_line(c.x, c.y, &line, c.width);
        }
    }

    fn draw_border(&self, buf: &mut Buffer, p: &Plan) {
        if !p.border {
            return;
        }
        let t = &self.theme;
        let style = self.border_style();
        Block::bordered().border_style(style).render(p.outer, buf);
        let room = usize::from(p.outer.width.saturating_sub(4));
        if self.danger() && room > 0 {
            let word = text::truncate(&format!(" {} ", self.tone_word), room).into_owned();
            let line = Line::styled(word, style);
            buf.set_line(p.outer.x + 2, p.outer.y, &line, room as u16);
        }
        let label = if p.pos.overflows() {
            let last = (p.pos.top + p.pos.viewport).min(p.pos.total());
            Some(format!(" {}-{last} of {} ", p.pos.top + 1, p.pos.total()))
        } else if p.hidden > 0 {
            // The body got no row at all: say how much is not shown.
            Some(format!(" +{} more ", p.hidden))
        } else {
            None
        };
        if let Some(label) = label {
            let w = text::width(&label);
            if w <= room {
                let x = p.outer.right() - 2 - w as u16;
                let y = p.outer.bottom() - 1;
                buf.set_line(x, y, &Line::styled(label, t.secondary()), w as u16);
            }
        }
    }

    fn button_line(&self, b: &Button, focused: bool, w: u16) -> Line<'static> {
        let t = &self.theme;
        let fail = t.status(Status::Failed);
        let (edge, key, label, open, close) = if focused {
            let mut s = t.selected().add_modifier(Modifier::BOLD);
            if b.destructive {
                s = s.patch(fail);
            }
            (s, s, s, "›", "‹")
        } else {
            let label = if b.destructive { fail } else { t.text() };
            (
                t.dim(),
                t.accent().add_modifier(Modifier::BOLD),
                label,
                "[",
                "]",
            )
        };
        let mut inner = vec![Span::styled(" ", label)];
        if let Some(k) = b.key.as_deref() {
            inner.push(Span::styled(k.to_string(), key));
            inner.push(Span::styled(" ", label));
        }
        inner.push(Span::styled(b.label.to_string(), label));
        inner.push(Span::styled(" ", label));
        if w < 3 {
            let mut all = vec![Span::styled(open, edge)];
            all.extend(inner);
            return text::truncate_line(Line::from(all), usize::from(w));
        }
        let inner = text::truncate_line(Line::from(inner), usize::from(w - 2));
        let mut spans = vec![Span::styled(open, edge)];
        spans.extend(inner.spans);
        spans.push(Span::styled(close, edge));
        Line::from(spans)
    }

    /// Draw into `area` (clamped to the buffer) and return where things
    /// went. The scroll position and focus in `state` are clamped, and the
    /// layout is kept in `state.layout` for [`ModalState`]'s key and mouse
    /// handling.
    pub fn render(&self, area: Rect, buf: &mut Buffer, state: &mut ModalState) -> ModalLayout {
        let area = rect_clamped(area, buf.area);
        let p = self.plan(area, state.scroll);
        state.scroll = p.pos.top;
        state.focus = state.focus.min(self.actions.len().saturating_sub(1));
        state.keys = self.actions.iter().map(Button::typed_key).collect();
        let layout = ModalLayout {
            outer: p.outer,
            body: p.text,
            track: p.track,
            pos: p.pos,
            buttons: p.buttons.clone(),
            close: p.close,
        };
        state.layout = layout.clone();
        if self.scrim {
            for y in area.top()..area.bottom() {
                for x in area.left()..area.right() {
                    buf[(x, y)].modifier.insert(Modifier::DIM);
                }
            }
        }
        if p.outer.is_empty() {
            return layout;
        }
        let t = &self.theme;
        Clear.render(p.outer, buf);
        buf.set_style(p.outer, Style::new().bg(t.surface_raised));
        self.draw_border(buf, &p);
        self.draw_title(buf, &p);
        if let Some(r) = p.subtitle {
            let s = self.subtitle.as_deref().unwrap_or("");
            let line = Line::styled(
                text::truncate(s, usize::from(r.width)).into_owned(),
                t.secondary(),
            );
            buf.set_line(r.x, r.y, &line, r.width);
        }
        if !p.text.is_empty() {
            let (shown, skip) = from_row(&p.lines, p.text.width, p.pos.top, p.text.height);
            Paragraph::new(shown.to_vec())
                .wrap(Wrap { trim: false })
                .scroll((skip, 0))
                .render(p.text, buf);
        }
        if let Some(track) = p.track {
            Scrollbar::new(p.pos).theme(*t).render(track, buf);
        }
        if let Some(r) = p.footer
            && let Some(line) = self.footer_line(r.width)
        {
            buf.set_line(r.x, r.y, &line, r.width);
        }
        for (r, i) in &p.buttons {
            let line = self.button_line(&self.actions[*i], *i == state.focus, r.width);
            buf.set_line(r.x, r.y, &line, r.width);
        }
        layout
    }
}

impl StatefulWidget for &Modal<'_> {
    type State = ModalState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ModalState) {
        Modal::render(self, area, buf, state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::widgets::key_hints::KeyHint;
    use ratatui::style::Color;

    fn draw(m: &Modal, state: &mut ModalState, w: u16, h: u16) -> (Buffer, ModalLayout) {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        let layout = m.render(area, &mut buf, state);
        (buf, layout)
    }

    fn row_text(buf: &Buffer, r: Rect) -> String {
        (r.x..r.right())
            .map(|x| buf[(x, r.y)].symbol().to_string())
            .collect()
    }

    // ---- herdr's popup_size.rs tests ----

    // Upstream: `parses_cells_and_percent`.
    #[test]
    fn parses_cells_and_percent() {
        assert_eq!(PopupSize::parse("120"), Ok(PopupSize::Cells(120)));
        assert_eq!(PopupSize::parse("80%"), Ok(PopupSize::Percent(80)));
        assert_eq!(PopupSize::Percent(80).resolve(100), 80);
    }

    // Upstream: `rejects_invalid_percent`.
    #[test]
    fn rejects_invalid_percent() {
        assert!(PopupSize::parse("0%").is_err());
        assert!(PopupSize::parse("101%").is_err());
        assert!(PopupSize::parse("%").is_err());
        assert!(PopupSize::parse("-3").is_err());
        assert!(PopupSize::parse("80 %").is_err());
    }

    // Upstream: `resolves_requested_outer_size_and_inner_terminal_area` (outer).
    #[test]
    fn resolves_requested_outer_size() {
        let r = popup_rect(
            Rect::new(0, 0, 100, 30),
            PopupSize::Percent(80),
            PopupSize::Percent(40),
            MIN_SIZE,
        );
        assert_eq!(r, Rect::new(10, 9, 80, 12));
    }

    // Upstream: `allows_full_terminal_outer_size`.
    #[test]
    fn allows_full_terminal_outer_size() {
        let r = popup_rect(
            Rect::new(4, 2, 100, 30),
            PopupSize::Percent(100),
            PopupSize::Percent(100),
            MIN_SIZE,
        );
        assert_eq!(r, Rect::new(4, 2, 100, 30));
    }

    // Upstream: `enforces_runtime_minimum_terminal_width`; herdr's `None` for
    // a 5-cell area is a 5-cell popup here.
    #[test]
    fn enforces_minimum_and_clamps_instead_of_vanishing() {
        let r = popup_rect(
            Rect::new(0, 0, 80, 24),
            PopupSize::Cells(4),
            PopupSize::Percent(50),
            MIN_SIZE,
        );
        assert_eq!(r.width, 6);
        let tiny = popup_rect(
            Rect::new(0, 0, 5, 24),
            PopupSize::Percent(50),
            PopupSize::Percent(50),
            MIN_SIZE,
        );
        assert_eq!((tiny.x, tiny.width), (0, 5));
        assert_eq!(PopupSize::Percent(250).resolve(40), 40);
    }

    /// herdr's release-notes frame: 80 × 24 cells in a 106 × 30 screen,
    /// header of 2 rows, the body, a footer, gaps of 1
    /// (`centered_popup_rect` + `modal_stack_areas(inner, 2, 1, 0, 1)`).
    #[test]
    fn release_notes_frame_matches_herdrs_geometry_and_stack() {
        let body: Vec<Line> = (0..60).map(|i| Line::from(format!("- line {i}"))).collect();
        let m = Modal::new("v0.8.3")
            .subtitle("what's new in this release")
            .body(body)
            .footer(KeyHints::new([KeyHint::new("esc", "close")]))
            .size(PopupSize::Cells(80), PopupSize::Cells(24))
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 106, 30);
        // centered_popup_rect(106×30, 80, 24): width ≤ 106-4, height ≤ 30-2.
        assert_eq!(l.outer, Rect::new(13, 3, 80, 24));
        // inner (14, 4, 78, 22): header rows 4-5, gap 6, content 7..24,
        // gap 24, footer 25.
        assert_eq!((l.body.y, l.body.height), (7, 17));
        assert_eq!(buf[(15, 4)].symbol(), "v");
        assert!(buf[(15, 4)].modifier.contains(Modifier::BOLD));
        assert_eq!(
            row_text(&buf, Rect::new(15, 5, 26, 1)),
            "what's new in this release"
        );
        assert_eq!(row_text(&buf, Rect::new(15, 25, 9, 1)), "esc close");
        // The close button sits at the header's right edge.
        assert_eq!(l.close, Some(Rect::new(91 - CLOSE_W, 4, CLOSE_W, 1)));
        assert_eq!(row_text(&buf, l.close.unwrap()), "esc close");
        // 60 lines in 17 rows: the track sits in the right padding column.
        assert_eq!(l.track, Some(Rect::new(91, 7, 1, 17)));
        assert_eq!(l.pos, ScrollPos::of(60, 17, 0));
        assert_eq!(row_text(&buf, Rect::new(78, 26, 14, 1)), "─ 1-17 of 60 ─");
    }

    #[test]
    fn a_two_line_notice_fits_its_content_not_eighty_percent() {
        let m = Modal::new("Copied")
            .body([
                "sk-ant-…4f2a is on the clipboard.",
                "Paste it into the client's config.",
            ])
            .close_button(false)
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 100, 30);
        // widest line 34 + padding 2 + border 2; title, gap, 2 body rows + border.
        assert_eq!((l.outer.width, l.outer.height), (38, 6));
        assert_eq!(l.outer, Rect::new(31, 12, 38, 6));
        assert_eq!(l.track, None);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[13].trim(), "│ Copied                             │");
        assert_eq!(lines[15].trim(), "│ sk-ant-…4f2a is on the clipboard.  │");
    }

    #[test]
    fn fit_is_capped_and_then_the_body_scrolls() {
        let text = "llmux rotates to the next account when the five-hour or seven-day window of the current one is spent.";
        let body: Vec<&str> = std::iter::repeat_n(text, 12).collect();
        let m = Modal::new("Rotation")
            .body(body)
            .fit_content(PopupSize::Percent(50), PopupSize::Percent(50))
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        let (_, l) = draw(&m, &mut st, 80, 30);
        assert_eq!((l.outer.width, l.outer.height), (40, 15));
        assert!(l.track.is_some());
        assert!(l.pos.overflows());
        // Width fits the content when it is narrower than the cap.
        let small = Modal::new("Rotation")
            .text("short")
            .close_button(false)
            .fit_content(PopupSize::Percent(50), PopupSize::Percent(50));
        assert_eq!(small.outer(Rect::new(0, 0, 80, 30)).width, 12);
    }

    /// herdr's help overlay estimated wrapped rows (H7) and scrolled past
    /// the end or short of it; the body here is measured by ratatui's own
    /// wrapper, so End lands exactly on the last row.
    #[test]
    fn end_reaches_the_last_wrapped_row_exactly() {
        let body: Vec<Line> = [
            " h / j / k / l / left / right move focus",
            "  - removed the sleep and awaited the frame tick instead; the flake rate went to zero",
            "한국어 문장도 셀 폭으로 줄바꿈합니다 그리고 끝",
            "word  double  spaces   triple    the end.",
        ]
        .into_iter()
        .cycle()
        .take(12)
        .map(Line::from)
        .collect();
        for w in [24u16, 25, 28, 31] {
            let m = Modal::new("help")
                .body(body.clone())
                .size(PopupSize::Cells(w), PopupSize::Cells(10))
                .close_button(false);
            let mut st = ModalState::new();
            let _ = draw(&m, &mut st, 40, 12);
            st.key(Key::End.into());
            let (buf, l) = draw(&m, &mut st, 40, 12);
            let expected = Paragraph::new(body.clone())
                .wrap(Wrap { trim: false })
                .line_count(l.body.width);
            assert_eq!(l.pos.total(), expected, "w={w}");
            let last = Rect::new(l.body.x, l.body.bottom() - 1, l.body.width, 1);
            assert!(row_text(&buf, last).trim_end().ends_with("end."), "w={w}");
        }
    }

    /// Review #3: 70,000 numbered lines scrolled to 69,990. The footer said
    /// 69991-69996 while the body (scrolled by a u16) showed line-65535.
    #[test]
    fn a_body_past_u16_max_rows_shows_the_rows_the_footer_names() {
        let body: Vec<Line> = (0..70_000)
            .map(|i| Line::from(format!("line-{i}")))
            .collect();
        let m = Modal::new("log")
            .body(body)
            .size(PopupSize::Cells(40), PopupSize::Cells(10))
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        st.scroll_to(69_990);
        let (buf, l) = draw(&m, &mut st, 60, 20);
        assert_eq!(st.scroll, 69_990);
        let first = Rect::new(l.body.x, l.body.y, l.body.width, 1);
        assert_eq!(row_text(&buf, first).trim_end(), "line-69990");
        let last = (69_990 + l.pos.viewport).min(70_000);
        let label = format!("69991-{last} of 70000");
        let bottom = Rect::new(l.outer.x, l.outer.bottom() - 1, l.outer.width, 1);
        assert!(row_text(&buf, bottom).contains(&label), "{label}");
        let end = Rect::new(l.body.x, l.body.y + (last - 69_991) as u16, l.body.width, 1);
        assert_eq!(row_text(&buf, end).trim_end(), format!("line-{}", last - 1));
        // Scrolled into a line that wraps: its tail rows, not its first.
        let body = vec![
            Line::from("a"),
            Line::from("bbbb cccc dddd eeee"),
            Line::from("z"),
        ];
        let m = Modal::new("w")
            .body(body)
            .size(PopupSize::Cells(10), PopupSize::Cells(6))
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        st.scroll_to(2);
        let (buf, l) = draw(&m, &mut st, 20, 10);
        let rows: Vec<String> = (0..l.body.height)
            .map(|y| row_text(&buf, Rect::new(l.body.x, l.body.y + y, l.body.width, 1)))
            .map(|r| r.trim_end().to_string())
            .collect();
        let full: Vec<String> = {
            let all = Paragraph::new(vec![
                Line::from("a"),
                Line::from("bbbb cccc dddd eeee"),
                Line::from("z"),
            ])
            .wrap(Wrap { trim: false });
            let area = Rect::new(0, 0, l.body.width, 20);
            let mut b = Buffer::empty(area);
            all.render(area, &mut b);
            (0..20)
                .map(|y| {
                    row_text(&b, Rect::new(0, y, l.body.width, 1))
                        .trim_end()
                        .to_string()
                })
                .collect()
        };
        let top = st.scroll;
        assert!(top > 0);
        assert_eq!(rows[..], full[top..top + rows.len()]);
    }

    #[test]
    fn keys_scroll_the_body_and_clamp() {
        let body: Vec<Line> = (1..=40).map(|i| Line::from(format!("note {i}"))).collect();
        let m = Modal::new("notes")
            .body(body)
            .size(PopupSize::Cells(40), PopupSize::Cells(14))
            .theme(Theme::ansi());
        let mut st = ModalState::new();
        let _ = draw(&m, &mut st, 60, 20);
        let view = st.layout.pos.viewport;
        assert_eq!(st.key(Key::Up.into()), ModalOutcome::Ignored);
        assert_eq!(st.key(Key::Down.into()), ModalOutcome::Scrolled);
        assert_eq!(st.scroll, 1);
        st.key(Key::PageDown.into());
        assert_eq!(st.scroll, 1 + view);
        assert_eq!(st.key(Key::End.into()), ModalOutcome::Scrolled);
        assert_eq!(st.scroll, 40 - view);
        assert_eq!(st.key(Key::Down.into()), ModalOutcome::Ignored);
        let (buf, l) = draw(&m, &mut st, 60, 20);
        let last = Rect::new(l.body.x, l.body.bottom() - 1, 7, 1);
        assert_eq!(row_text(&buf, last), "note 40");
        st.key(Key::Home.into());
        assert_eq!(st.scroll, 0);
        st.scroll_to(999);
        let _ = draw(&m, &mut st, 60, 20);
        assert_eq!(st.scroll, 40 - view, "clamped at render");
        assert_eq!(st.scroll_by(-3), ModalOutcome::Scrolled);
        assert_eq!(st.scroll, 40 - view - 3);
        // Enter on a modal without buttons closes it; so does Esc.
        assert_eq!(st.key(Key::Enter.into()), ModalOutcome::Closed);
        assert_eq!(st.key(Key::Esc.into()), ModalOutcome::Closed);
    }

    fn confirm(theme: Theme) -> Modal<'static> {
        Modal::new("Delete account ai3@iq.io?")
            .text("Its API key and 30 days of usage are removed.")
            .actions([
                Button::new("cancel").key("n"),
                Button::new("delete").key("y").destructive(true),
            ])
            .tone(Tone::Danger)
            .close_button(false)
            .theme(theme)
    }

    #[test]
    fn buttons_take_focus_and_report_presses() {
        let m = confirm(Theme::ansi());
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 80, 20);
        assert_eq!(l.buttons.len(), 2);
        let (cancel, _) = l.buttons[0];
        let (delete, _) = l.buttons[1];
        assert_eq!(row_text(&buf, cancel), "› n cancel ‹");
        assert_eq!(row_text(&buf, delete), "[ y delete ]");
        assert_eq!(delete.x - cancel.right(), 2, "gap of 2");
        assert_eq!(st.key(Key::Tab.into()), ModalOutcome::Focused(1));
        assert_eq!(st.key(Key::Tab.into()), ModalOutcome::Focused(0));
        assert_eq!(st.key(Key::BackTab.into()), ModalOutcome::Focused(1));
        assert_eq!(st.key(Key::Left.into()), ModalOutcome::Focused(0));
        assert_eq!(st.key(Key::Enter.into()), ModalOutcome::Pressed(0));
        assert_eq!(st.key(KeyInput::from('y')), ModalOutcome::Pressed(1));
        assert_eq!(st.focus, 1);
        assert_eq!(st.key(KeyInput::from('x')), ModalOutcome::Ignored);
        assert_eq!(st.key(Key::Esc.into()), ModalOutcome::Closed);
        let (buf, _) = draw(&m, &mut st, 80, 20);
        assert_eq!(row_text(&buf, delete), "› y delete ‹");
    }

    #[test]
    fn mouse_presses_buttons_close_and_outside() {
        let m = confirm(Theme::ansi()).close_button(true);
        let mut st = ModalState::new();
        let (_, l) = draw(&m, &mut st, 80, 20);
        let (delete, i) = l.buttons[1];
        assert_eq!(st.press(delete.x + 3, delete.y), ModalOutcome::Pressed(i));
        assert_eq!(st.focus, 1);
        let close = l.close.expect("close button");
        assert_eq!(st.press(close.x, close.y), ModalOutcome::Closed);
        assert_eq!(st.press(0, 0), ModalOutcome::Outside);
        assert_eq!(
            st.press(l.outer.x + 1, l.outer.y + 1),
            ModalOutcome::Ignored
        );
    }

    #[test]
    fn the_scrollbar_jumps_and_drags_from_the_mouse() {
        let body: Vec<Line> = (1..=50).map(|i| Line::from(format!("row {i}"))).collect();
        let m = Modal::new("log")
            .body(body)
            .size(PopupSize::Cells(30), PopupSize::Cells(12));
        let mut st = ModalState::new();
        let (_, l) = draw(&m, &mut st, 40, 20);
        let track = l.track.expect("track");
        assert_eq!(
            st.press(track.x, track.bottom() - 1),
            ModalOutcome::Scrolled
        );
        assert_eq!(st.scroll, l.pos.max);
        st.release();
        let (_, l) = draw(&m, &mut st, 40, 20);
        let thumb = crate::widgets::scrollbar::thumb(l.pos, track).expect("thumb");
        assert_eq!(
            st.press(track.x, thumb.top),
            ModalOutcome::Ignored,
            "grabbed"
        );
        assert_eq!(st.drag(track.x + 5, track.y), ModalOutcome::Scrolled);
        assert_eq!(st.scroll, 0);
        st.release();
        assert_eq!(st.drag(track.x, track.bottom()), ModalOutcome::Ignored);
    }

    #[test]
    fn danger_tone_is_a_colour_and_a_word() {
        let m = confirm(Theme::ansi());
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 80, 20);
        let o = l.outer;
        assert_eq!(buf[(o.x, o.y)].symbol(), "┌");
        assert_eq!(buf[(o.x, o.y)].fg, Theme::ansi().fail);
        assert_eq!(row_text(&buf, Rect::new(o.x + 2, o.y, 8, 1)), " danger ");
        // Mono: bold border and the same word.
        let m = confirm(Theme::mono());
        let (buf, _) = draw(&m, &mut ModalState::new(), 80, 20);
        assert!(buf[(o.x, o.y + 1)].modifier.contains(Modifier::BOLD));
        assert_eq!(row_text(&buf, Rect::new(o.x + 2, o.y, 8, 1)), " danger ");
        // Normal tone in mono: a plain border, not bold.
        let n = Modal::new("ok").text("fine").theme(Theme::mono());
        let (buf, l) = draw(&n, &mut ModalState::new(), 40, 10);
        assert!(
            !buf[(l.outer.x, l.outer.y + 1)]
                .modifier
                .contains(Modifier::BOLD)
        );
        // Without room for a border the word leads the title.
        let (buf, _) = draw(&confirm(Theme::mono()), &mut ModalState::new(), 30, 2);
        assert!(
            snapshot::text(&buf).contains("danger: Delete"),
            "{}",
            snapshot::text(&buf)
        );
    }

    #[test]
    fn mono_tells_focused_and_unfocused_buttons_apart() {
        let m = confirm(Theme::mono());
        let mut st = ModalState::with_focus(1);
        let (buf, l) = draw(&m, &mut st, 80, 20);
        let (cancel, _) = l.buttons[0];
        let (delete, _) = l.buttons[1];
        for x in delete.x..delete.right() {
            assert!(buf[(x, delete.y)].modifier.contains(Modifier::REVERSED));
        }
        for x in cancel.x..cancel.right() {
            assert!(!buf[(x, cancel.y)].modifier.contains(Modifier::REVERSED));
        }
        assert_eq!(buf[(delete.x, delete.y)].symbol(), "›");
        assert_eq!(buf[(cancel.x, cancel.y)].symbol(), "[");
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{c:?}");
        }
    }

    #[test]
    fn scrim_dims_what_is_under_and_not_the_popup() {
        let m = Modal::new("hi").text("there").theme(Theme::mono());
        let area = Rect::new(0, 0, 30, 10);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "dashboard", Style::new());
        let l = m.render(area, &mut buf, &mut ModalState::new());
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        assert_eq!(buf[(0, 0)].symbol(), "d", "content stays");
        let inside = &buf[(l.outer.x + 2, l.outer.y + 1)];
        assert!(!inside.modifier.contains(Modifier::DIM));
        let mut plain = Buffer::empty(area);
        m.clone()
            .scrim(false)
            .render(area, &mut plain, &mut ModalState::new());
        assert!(!plain[(0, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn widths_are_cells_for_titles_and_buttons() {
        let m = Modal::new("릴리스 노트")
            .text("한국어 본문도 셀 폭으로 줄바꿈합니다")
            .actions([Button::new("확인").key("y")])
            .close_button(false);
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 60, 12);
        let (b, _) = l.buttons[0];
        // `› y 확인 ‹`: 2 + 2 + 4 + 2 cells (herdr counted chars: 8).
        assert_eq!(b.width, 10);
        assert_eq!(Button::new("확인").key("y").width(), 10);
        assert_eq!(l.outer.width, 4 + 36);
        assert!(snapshot::text(&buf).contains("릴리스 노트"));
    }

    #[test]
    fn narrow_rows_shrink_buttons_with_an_ellipsis_and_footers_drop_hints() {
        let m = Modal::new("Reset limits?")
            .text("All accounts")
            .footer(KeyHints::new([
                KeyHint::new("tab", "switch"),
                KeyHint::new("enter", "press"),
                KeyHint::new("esc", "cancel"),
            ]))
            .actions([
                Button::new("keep limits").key("n"),
                Button::new("reset every account")
                    .key("y")
                    .destructive(true),
            ])
            .close_button(false)
            .size(PopupSize::Cells(24), PopupSize::Cells(12));
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 40, 14);
        assert_eq!(l.buttons.len(), 2);
        for (r, _) in &l.buttons {
            assert!(r.right() < l.outer.right(), "inside the border");
            let t = row_text(&buf, *r);
            assert!(t.contains('…'), "{t}");
        }
        let text = snapshot::text(&buf);
        assert!(text.contains("? more") || text.contains('…'), "{text}");
    }

    #[test]
    fn rows_give_way_in_order_and_keep_title_and_buttons() {
        let m = confirm(Theme::ansi())
            .subtitle("ai3@iq.io · Max plan")
            .footer(KeyHints::new([KeyHint::new("esc", "cancel")]));
        // Border + 3 inner rows: title, buttons, one body row; the
        // subtitle, footer and gaps gave way.
        let mut st = ModalState::new();
        let (buf, l) = draw(&m, &mut st, 60, 7);
        let lines = snapshot::lines(&buf);
        assert_eq!(l.outer.height, 5);
        assert!(lines[2].contains("Delete account"), "{lines:#?}");
        assert!(lines[3].contains("Its API key"), "{lines:#?}");
        assert!(lines[4].contains("› n cancel ‹"), "{lines:#?}");
        assert_eq!(l.buttons.len(), 2);
        assert!(l.track.is_some(), "the second body row is a scroll away");
        // Two inner rows: title and buttons; the body is counted on the
        // border instead of vanishing.
        let (buf, l) = draw(&m, &mut st, 60, 5);
        let lines = snapshot::lines(&buf);
        assert!(lines[1].contains("Delete account"), "{lines:#?}");
        assert!(lines[2].contains("delete"), "{lines:#?}");
        // 80% of 60 columns caps the width at 48: the body is two rows.
        assert_eq!(l.outer.width, 48);
        assert!(lines[3].contains(" +2 more "), "{lines:#?}");
        assert_eq!(l.body, Rect::default());
        // A long body squeezed to one row scrolls, and says so.
        let long = m.clone().text("one\ntwo\nthree");
        let (buf, l) = draw(&long, &mut st, 60, 7);
        assert!(l.track.is_some());
        assert!(
            snapshot::text(&buf).contains(" 1-1 of 3 "),
            "{}",
            snapshot::text(&buf)
        );
    }

    /// Fill with a sentinel, draw in a sub-area, and check nothing outside
    /// it changed.
    #[test]
    fn tiny_sizes_never_panic_and_never_draw_outside_the_area() {
        let full = Modal::new("v0.9.5 — what's new")
            .subtitle("release notes")
            .body((0..30).map(|i| Line::from(format!("- change number {i} in this release"))))
            .footer(KeyHints::new([KeyHint::new("esc", "close")]))
            .actions([Button::new("ok").key("y"), Button::new("later").key("n")])
            .tone(Tone::Danger);
        let fit = Modal::new("x").text("y");
        let buf_area = Rect::new(0, 0, 20, 14);
        for m in [&full, &fit] {
            for w in 0..=14u16 {
                for h in 0..=10u16 {
                    let area = Rect::new(3, 2, w, h);
                    let mut buf = Buffer::empty(buf_area);
                    for c in buf.content.iter_mut() {
                        c.set_symbol("Z");
                    }
                    let mut st = ModalState::new();
                    let l = m.render(area, &mut buf, &mut st);
                    for y in 0..buf_area.height {
                        for x in 0..buf_area.width {
                            if !area.contains(Position::new(x, y)) {
                                let c = &buf[(x, y)];
                                assert_eq!(c.symbol(), "Z", "({x},{y}) in {w}x{h}");
                                assert_eq!(c.modifier, Modifier::empty());
                            }
                        }
                    }
                    let inside = |r: Rect| r.is_empty() || area.intersection(r) == r;
                    assert!(inside(l.outer) && inside(l.body), "{l:?} in {area:?}");
                    assert!(l.track.is_none_or(inside));
                    assert!(l.close.is_none_or(inside));
                    assert!(l.buttons.iter().all(|(r, _)| inside(*r)));
                    for k in [Key::Down, Key::End, Key::Tab, Key::Enter] {
                        let _ = st.key(k.into());
                    }
                    let _ = st.press(4, 3);
                    let _ = st.drag(4, 9);
                }
            }
        }
        // An area hanging off the buffer is clamped to it.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        let l = full.render(Rect::new(5, 2, 50, 50), &mut buf, &mut ModalState::new());
        assert!(Rect::new(0, 0, 10, 5).intersection(l.outer) == l.outer);
    }

    #[test]
    fn layout_without_drawing_matches_the_render() {
        let m = confirm(Theme::dark());
        let st = ModalState::new();
        let l = m.layout(Rect::new(0, 0, 70, 20), &st);
        let mut st2 = ModalState::new();
        let (_, drawn) = draw(&m, &mut st2, 70, 20);
        assert_eq!(l, drawn);
        assert_eq!(st2.layout, drawn);
    }
}
