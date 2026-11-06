// Derived from herdrdev/herdr `src/client/shell/tabs.rs` (`render_tab_bar`,
// `tab_drop_indicator_x`, `centered_tab_scroll`, `max_tab_scroll` and its test)
// and `src/client/shell/mouse.rs` (`tab_drop_index_at`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab: the
// `<` `>` scroll buttons, the trailing action, the centred reveal and the drop
// marker are opt-in parts of the llmux strip below, measured in cells over
// its labels and dividers instead of herdr's fixed-width padded tabs; the
// buttons say by weight (not colour) whether anything is hidden on their
// side; the drop marker is a Narrow glyph clamped inside the area and also
// shows for a drop index just past the visible window; the scroll position
// is passed in and read back (`Reveal`, `TabsLayout::hidden_before`) instead
// of living in shell state.

//! A one-row tab strip: which view you are on, and how to get to the others.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `draw_tabs`, `TABS`,
//! `TabHit`, `hit_test_tabs`, `tabs_room` and `draw_tabs_tail`. Every label's
//! rect comes back from the render, so a mouse click maps to the tab that was
//! actually drawn there; a right-aligned tail (llmux puts the header's `cfg`
//! path there) takes the room the labels leave and gives way first.
//!
//! Changes from llmux:
//! - widths are terminal cells, not `chars().count()`, so a Korean label gets
//!   a click rect as wide as it is drawn (llmux defect §9.1, `ui.rs:684`);
//! - key hints are kept when they fit, measured, instead of a fixed 140-column
//!   threshold; a key letter found in its label is underlined at no cost;
//! - when even the plain labels do not fit, the strip scrolls to keep the
//!   active tab on screen and counts the hidden ones (`‹2 … 3›`) instead of
//!   clipping them silently.
//!
//! Opt-in, from herdr's tab bar (`src/client/shell/tabs.rs`, Apache-2.0):
//! clickable ` < ` ` > ` scroll buttons in place of the counts
//! ([`Tabs::scroll_buttons`]), a trailing action such as ` + ` for a new tab
//! ([`Tabs::action`]), centring the active tab when it is revealed
//! ([`Reveal::Centre`]), and while a tab is dragged, a marker where it would
//! drop ([`Tabs::drop_marker`]) with the drop index under the pointer
//! ([`TabsLayout::drop_index`]).

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::Theme;

/// One tab: its label and, optionally, the key that opens it (`"a"`,
/// `"Esc"`, `"?"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab<'a> {
    pub label: Cow<'a, str>,
    pub key: Option<Cow<'a, str>>,
}

impl<'a> Tab<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>) -> Tab<'a> {
        Tab {
            label: label.into(),
            key: None,
        }
    }

    /// The key that opens this tab. A single character is matched against
    /// typed text; `Esc`, `Enter` and `Tab` name those keys.
    pub fn key(mut self, key: impl Into<Cow<'a, str>>) -> Tab<'a> {
        self.key = Some(key.into());
        self
    }
}

/// How a tab's key is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hints {
    /// The key letter is underlined inside the label (`a` in `accounts`);
    /// a key the label does not contain gets a `[Esc] ` prefix while it fits.
    #[default]
    Mnemonic,
    /// llmux's form: every keyed tab reads `[a] accounts` while it fits.
    Bracket,
    /// No hints.
    Off,
}

/// The rect one tab label was drawn in, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabHit {
    pub rect: Rect,
    pub index: usize,
}

impl TabHit {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row >= self.rect.y
            && row < self.rect.bottom()
            && col >= self.rect.x
            && col < self.rect.right()
    }
}

/// Which tab, if any, a click at `(col, row)` lands on.
pub fn hit_test(hits: &[TabHit], col: u16, row: u16) -> Option<usize> {
    hits.iter().find(|h| h.contains(col, row)).map(|h| h.index)
}

/// The marker drawn where a dragged tab would drop. U+2590 is East-Asian
/// Narrow, so it stays one cell on a CJK terminal, and it differs from the
/// `│` divider by shape as well as colour.
pub const DROP_MARKER: &str = "▐";

/// Cells of one scroll button (` < ` / ` > `).
const BUTTON: usize = 3;

/// The narrowest a tab may get before the buttons or the action give way
/// (herdr's `MIN_TAB_WIDTH`).
const MIN_TAB: usize = 8;

/// Which tab the strip starts at when the labels overflow the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reveal {
    /// As far left as keeps the active tab on screen (llmux's strip).
    #[default]
    Edge,
    /// The active tab as near the middle as the window allows — herdr's
    /// reveal after a tab switch.
    Centre,
    /// This tab first: the host's scroll position after a click on a scroll
    /// button. Clamped so the strip never scrolls past its last tab; the
    /// active tab may then be off screen.
    From(usize),
}

/// What a click on the strip lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabTarget {
    Tab(usize),
    /// The ` < ` button: show earlier tabs.
    ScrollLeft,
    /// The ` > ` button: show later tabs.
    ScrollRight,
    /// The trailing action (` + `).
    Action,
}

/// Where everything went in one render.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TabsLayout {
    /// One rect per drawn label, left to right.
    pub hits: Vec<TabHit>,
    /// The `[k] ` prefixes are shown.
    pub hinted: bool,
    /// Tabs scrolled off the left and right ends (`‹N` / `N›`). The first
    /// drawn tab is `hidden_before`: keep it and pass it back as
    /// [`Reveal::From`] to hold the scroll position.
    pub hidden_before: usize,
    pub hidden_after: usize,
    /// Cells left free at the right end of the row after the labels.
    pub room: usize,
    /// The tail was drawn.
    pub tail: bool,
    /// Every tab, drawn or not.
    pub total: usize,
    /// The largest useful first tab: past it the strip would only show
    /// fewer tabs (herdr's `max_tab_scroll`).
    pub max_start: usize,
    /// The ` < ` and ` > ` buttons, when [`Tabs::scroll_buttons`] is on and
    /// the labels overflow.
    pub scroll_left: Option<Rect>,
    pub scroll_right: Option<Rect>,
    /// The trailing action, when set and there is room for it.
    pub action: Option<Rect>,
    /// The column of the drop marker, when one was asked for.
    pub drop_marker: Option<u16>,
}

impl TabsLayout {
    /// What a click at `(col, row)` lands on: a tab, a scroll button or the
    /// action.
    pub fn target_at(&self, col: u16, row: u16) -> Option<TabTarget> {
        let p = Position::new(col, row);
        if let Some(i) = hit_test(&self.hits, col, row) {
            return Some(TabTarget::Tab(i));
        }
        [
            (self.scroll_left, TabTarget::ScrollLeft),
            (self.scroll_right, TabTarget::ScrollRight),
            (self.action, TabTarget::Action),
        ]
        .into_iter()
        .find(|(r, _)| r.is_some_and(|r| r.contains(p)))
        .map(|(_, t)| t)
    }

    /// Where a tab dragged to `(col, row)` would be inserted: `0..=total`,
    /// by the midpoint of the label under the pointer. `None` off the tab
    /// row or when no tab is drawn. Port of herdr's `tab_drop_index_at`.
    pub fn drop_index(&self, col: u16, row: u16) -> Option<usize> {
        let (first, last) = (self.hits.first()?, self.hits.last()?);
        if row != first.rect.y {
            return None;
        }
        let p = Position::new(col, row);
        if self.scroll_left.is_some_and(|r| r.contains(p)) {
            return Some(0);
        }
        if self.scroll_right.is_some_and(|r| r.contains(p)) {
            return Some(self.total);
        }
        if col <= first.rect.x {
            return Some(first.index);
        }
        if col >= last.rect.right() {
            return Some(last.index + 1);
        }
        for h in &self.hits {
            if col < h.rect.x + h.rect.width / 2 {
                return Some(h.index);
            }
            if col < h.rect.right() {
                return Some(h.index + 1);
            }
        }
        Some(last.index + 1)
    }
}

/// One label as drawn: an optional `[k] ` prefix, and the byte range of an
/// underlined key letter.
struct Piece<'s> {
    prefix: Option<String>,
    label: &'s str,
    mnemonic: Option<(usize, usize)>,
}

impl Piece<'_> {
    fn width(&self) -> usize {
        self.prefix.as_deref().map_or(0, text::width) + text::width(self.label)
    }
}

/// A tab strip. Draws one row.
#[derive(Debug, Clone)]
pub struct Tabs<'a> {
    tabs: Vec<Tab<'a>>,
    active: usize,
    theme: Theme,
    hints: Hints,
    divider: Cow<'a, str>,
    indent: u16,
    tail: Vec<Span<'a>>,
    reveal: Reveal,
    buttons: bool,
    action: Option<Cow<'a, str>>,
    drop: Option<usize>,
}

impl<'a> Tabs<'a> {
    pub fn new(tabs: impl IntoIterator<Item = Tab<'a>>) -> Tabs<'a> {
        Tabs {
            tabs: tabs.into_iter().collect(),
            active: 0,
            theme: Theme::default(),
            hints: Hints::default(),
            divider: Cow::Borrowed(" │ "),
            indent: 1,
            tail: Vec::new(),
            reveal: Reveal::Edge,
            buttons: false,
            action: None,
            drop: None,
        }
    }

    /// Which tab the strip starts at when the labels overflow. Default
    /// [`Reveal::Edge`].
    pub fn reveal(mut self, reveal: Reveal) -> Tabs<'a> {
        self.reveal = reveal;
        self
    }

    /// When the labels overflow, clickable ` < ` and ` > ` buttons at the
    /// ends of the row instead of the `‹N` / `N›` counts. A button is dim
    /// when nothing is hidden on its side. Below 14 cells (plus the action)
    /// the counts come back. Default off.
    pub fn scroll_buttons(mut self, on: bool) -> Tabs<'a> {
        self.buttons = on;
        self
    }

    /// A trailing action with its own hit rect, drawn ` label ` after the
    /// last tab (at the right end when the labels overflow): `"+"` for a new
    /// tab. Given way when there is no room beside an 8-cell tab.
    pub fn action(mut self, label: impl Into<Cow<'a, str>>) -> Tabs<'a> {
        self.action = Some(label.into());
        self
    }

    /// While a tab is dragged: draw [`DROP_MARKER`] where it would be
    /// inserted (`0..=len`, from [`TabsLayout::drop_index`]). An index
    /// outside the visible window marks the nearer end.
    pub fn drop_marker(mut self, index: Option<usize>) -> Tabs<'a> {
        self.drop = index;
        self
    }

    /// Index of the view you are on (clamped to the tabs given).
    pub fn active(mut self, index: usize) -> Tabs<'a> {
        self.active = index;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Tabs<'a> {
        self.theme = theme;
        self
    }

    pub fn hints(mut self, hints: Hints) -> Tabs<'a> {
        self.hints = hints;
        self
    }

    /// Between two labels. Default `" │ "`.
    pub fn divider(mut self, divider: impl Into<Cow<'a, str>>) -> Tabs<'a> {
        self.divider = divider.into();
        self
    }

    /// Blank cells before the first label. Default 1.
    pub fn indent(mut self, cells: u16) -> Tabs<'a> {
        self.indent = cells;
        self
    }

    /// Spans right-aligned at the end of the row, drawn only when they fit
    /// the room the labels leave (with a one-cell gap).
    pub fn tail(mut self, spans: impl IntoIterator<Item = Span<'a>>) -> Tabs<'a> {
        self.tail = spans.into_iter().collect();
        self
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn active_index(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }

    /// The tab a key selects: ←/→ (and Tab/Shift-Tab) step with wrap-around,
    /// Home/End jump to the ends, and a tab's own key opens it.
    pub fn on_key(&self, key: KeyInput) -> Option<usize> {
        let n = self.tabs.len();
        if n == 0 {
            return None;
        }
        let at = self.active_index();
        if let Some(i) = self.tabs.iter().position(|t| {
            t.key
                .as_deref()
                .is_some_and(|k| key_matches(k, key) && !key.mods.ctrl && !key.mods.alt)
        }) {
            return Some(i);
        }
        match key.key {
            Key::Left | Key::BackTab => Some((at + n - 1) % n),
            Key::Right | Key::Tab => Some((at + 1) % n),
            Key::Home => Some(0),
            Key::End => Some(n - 1),
            _ => None,
        }
    }

    fn piece(&self, i: usize, full: bool) -> Piece<'_> {
        let tab = &self.tabs[i];
        let label = tab.label.as_ref();
        let key = match (self.hints, tab.key.as_deref()) {
            (Hints::Off, _) | (_, None) => {
                return Piece {
                    prefix: None,
                    label,
                    mnemonic: None,
                };
            }
            (_, Some(k)) => k,
        };
        if self.hints == Hints::Mnemonic {
            let mut cs = key.chars();
            if let (Some(c), None) = (cs.next(), cs.next())
                && let Some(at) = label.find(c)
            {
                return Piece {
                    prefix: None,
                    label,
                    mnemonic: Some((at, at + c.len_utf8())),
                };
            }
        }
        Piece {
            prefix: full.then(|| format!("[{key}] ")),
            label,
            mnemonic: None,
        }
    }

    /// Where the labels, hints and tail go at `area`, without drawing.
    pub fn layout(&self, area: Rect) -> TabsLayout {
        self.place(area, None)
    }

    /// Draw and return where everything went (the hit rects included).
    pub fn render_layout(&self, area: Rect, buf: &mut Buffer) -> TabsLayout {
        self.place(area, Some(buf))
    }

    /// Draw and return one rect per drawn label.
    pub fn render_hits(&self, area: Rect, buf: &mut Buffer) -> Vec<TabHit> {
        self.place(area, Some(buf)).hits
    }

    /// The whole strip, unclipped, as a line (for hosts that place it in
    /// their own paragraph; no hit rects).
    pub fn line(&self) -> Line<'static> {
        let mut out = Vec::new();
        let full = self.layout(Rect::new(0, 0, u16::MAX, 1)).hinted;
        out.push(Span::raw(" ".repeat(self.indent as usize)));
        for i in 0..self.tabs.len() {
            if i > 0 {
                out.push(Span::styled(self.divider.to_string(), self.theme.dim()));
            }
            out.extend(self.piece_spans(i, full, usize::MAX));
        }
        Line::from(out)
    }

    fn label_style(&self, i: usize) -> Style {
        let t = &self.theme;
        if i == self.active_index() {
            t.selected().patch(t.accent()).add_modifier(Modifier::BOLD)
        } else {
            t.accent()
        }
    }

    /// The spans of one label, clipped to `max` cells.
    fn piece_spans(&self, i: usize, full: bool, max: usize) -> Vec<Span<'static>> {
        let p = self.piece(i, full);
        let style = self.label_style(i);
        let mut spans = Vec::with_capacity(4);
        if let Some(prefix) = p.prefix {
            spans.push(Span::styled(prefix, style));
        }
        match p.mnemonic {
            Some((a, b)) => {
                if a > 0 {
                    spans.push(Span::styled(p.label[..a].to_string(), style));
                }
                spans.push(Span::styled(
                    p.label[a..b].to_string(),
                    style.add_modifier(Modifier::UNDERLINED),
                ));
                if b < p.label.len() {
                    spans.push(Span::styled(p.label[b..].to_string(), style));
                }
            }
            None => spans.push(Span::styled(p.label.to_string(), style)),
        }
        text::truncate_line(Line::from(spans), max).spans
    }

    /// Cells the action takes (` + `), or 0 without one.
    fn action_width(&self) -> usize {
        self.action.as_deref().map_or(0, |a| text::width(a) + 2)
    }

    /// Which form fits in `width` cells with `indent` before the labels.
    fn fit(&self, width: usize, indent: usize) -> Fit {
        let n = self.tabs.len();
        let div = text::width(&self.divider);
        let action_w = self.action_width();
        // The action keeps room for itself and an 8-cell tab beside it.
        let action = action_w > 0 && width >= indent + action_w + MIN_TAB;
        let action_w = if action { action_w } else { 0 };
        let avail = width - action_w;
        let strip = |full: bool| {
            indent + (0..n).map(|i| self.piece(i, full).width()).sum::<usize>() + div * (n - 1)
        };
        let has_prefix = (0..n).any(|i| self.piece(i, true).prefix.is_some());
        let whole = |hinted| Fit {
            hinted,
            start: 0,
            end: n,
            markers: false,
            buttons: false,
            action,
            max_start: 0,
        };
        if has_prefix && strip(true) <= avail {
            return whole(true);
        }
        if strip(false) <= avail {
            return whole(false);
        }
        // Scroll: a window of plain labels, with the hidden tabs counted at
        // each end — or, with buttons, ` < ` and ` > ` at the ends of the row.
        let buttons = self.buttons && width >= MIN_TAB + 2 * BUTTON + action_w;
        let (lead, avail) = if buttons {
            (0, width - 2 * BUTTON - action_w)
        } else {
            (indent, avail)
        };
        let w: Vec<usize> = (0..n).map(|i| self.piece(i, false).width()).collect();
        let before = |s: usize| {
            if !buttons && s > 0 {
                text::width(&marker_before(s))
            } else {
                0
            }
        };
        let cost = |s: usize, e: usize| {
            let after = if !buttons && e < n {
                text::width(&marker_after(n - e))
            } else {
                0
            };
            lead + before(s) + w[s..e].iter().sum::<usize>() + div * (e - s - 1) + after
        };
        let fits = |s: usize, e: usize| cost(s, e) <= avail;
        let max_start = max_start_by(n, cost, avail);
        let at = self.active_index();
        let edge = (0..=at).find(|&s| fits(s, at + 1));
        let start = match self.reveal {
            Reveal::Edge => edge,
            // herdr's `centered_tab_scroll`: the start that puts the middle
            // of the active label nearest the middle of the room (the later
            // start on a tie), never past the largest useful start.
            Reveal::Centre => (0..=at)
                .rev()
                .filter(|&s| fits(s, at + 1))
                .min_by_key(|&s| {
                    let left = lead + before(s) + w[s..at].iter().sum::<usize>() + div * (at - s);
                    (2 * left + w[at]).abs_diff(avail)
                })
                .map(|s| s.min(max_start))
                .or(edge),
            Reveal::From(first) => Some(first.min(max_start)),
        };
        let fit = |start: usize, end: usize, markers: bool| Fit {
            hinted: false,
            start,
            end,
            markers,
            buttons,
            action,
            max_start,
        };
        match start {
            Some(start) if fits(start, start + 1) => fit(
                start,
                (start + 1..=n)
                    .filter(|&e| fits(start, e))
                    .max()
                    .unwrap_or(start + 1),
                !buttons,
            ),
            // Not even one label with its counts fits: that label alone,
            // clipped with `…` (the counts stay in the layout).
            Some(start) => fit(start, start + 1, false),
            None => fit(at, at + 1, false),
        }
    }

    fn place(&self, area: Rect, mut buf: Option<&mut Buffer>) -> TabsLayout {
        let n = self.tabs.len();
        let mut out = TabsLayout {
            room: area.width as usize,
            total: n,
            ..TabsLayout::default()
        };
        if area.is_empty() {
            return out;
        }
        let y = area.y;
        let right = area.right();
        // The indent gives way before the labels do.
        let indent = if area.width > self.indent {
            self.indent
        } else {
            0
        };
        let mut x = area.x + indent;
        let t = &self.theme;
        let put =
            |x: &mut u16, limit: u16, spans: &[Span<'static>], buf: &mut Option<&mut Buffer>| {
                for s in spans {
                    let w = (text::width(&s.content) as u16).min(limit.saturating_sub(*x));
                    if let Some(b) = buf.as_deref_mut() {
                        b.set_span(*x, y, s, w);
                    }
                    *x += w;
                }
            };
        if n > 0 {
            let Fit {
                hinted: full,
                start,
                end,
                markers,
                buttons,
                action,
                max_start,
            } = self.fit(area.width as usize, indent as usize);
            out.hinted = full;
            out.hidden_before = start;
            out.hidden_after = n - end;
            out.max_start = max_start;
            let action_w = if action {
                self.action_width() as u16
            } else {
                0
            };
            // Labels (and counts) end here.
            let mut limit = right - action_w;
            if buttons {
                let left = Rect::new(area.x, y, BUTTON as u16, 1);
                let right_b = Rect::new(limit - BUTTON as u16, y, BUTTON as u16, 1);
                for (rect, glyph, live) in [(left, " < ", start > 0), (right_b, " > ", end < n)] {
                    // Live says something is hidden on that side; dim says
                    // nothing is — by weight, so mono tells them apart.
                    let style = if live {
                        t.secondary().add_modifier(Modifier::BOLD)
                    } else {
                        t.dim()
                    };
                    if let Some(b) = buf.as_deref_mut() {
                        b.set_stringn(rect.x, y, glyph, BUTTON, style);
                    }
                }
                out.scroll_left = Some(left);
                out.scroll_right = Some(right_b);
                x = left.right();
                limit = right_b.x;
            }
            if markers && start > 0 {
                put(
                    &mut x,
                    limit,
                    &[Span::styled(marker_before(start), t.dim())],
                    &mut buf,
                );
            }
            let after = (markers && end < n).then(|| marker_after(n - end));
            let reserve = after.as_deref().map_or(0, text::width);
            for i in start..end {
                if i > start {
                    put(
                        &mut x,
                        limit,
                        &[Span::styled(self.divider.to_string(), t.dim())],
                        &mut buf,
                    );
                }
                let max = limit.saturating_sub(x) as usize;
                let max = if i + 1 == end {
                    max.saturating_sub(reserve).max(1).min(max)
                } else {
                    max
                };
                let spans = self.piece_spans(i, full, max);
                let x0 = x;
                put(&mut x, limit, &spans, &mut buf);
                if x > x0 {
                    out.hits.push(TabHit {
                        rect: Rect::new(x0, y, x - x0, 1),
                        index: i,
                    });
                }
            }
            if let Some(m) = after {
                put(&mut x, limit, &[Span::styled(m, t.dim())], &mut buf);
            }
            if buttons {
                x = right - action_w;
            }
            if action && let Some(label) = self.action.as_deref() {
                let rect = Rect::new(x, y, action_w.min(right - x), 1);
                if let Some(b) = buf.as_deref_mut() {
                    b.set_stringn(
                        rect.x,
                        y,
                        format!(" {label} "),
                        rect.width as usize,
                        t.secondary(),
                    );
                }
                if !rect.is_empty() {
                    out.action = Some(rect);
                }
                x = rect.right();
            }
            if let Some(d) = self.drop {
                out.drop_marker = self.drop_x(d, &out.hits, area);
                if let (Some(mx), Some(b)) = (out.drop_marker, buf.as_deref_mut()) {
                    b.set_stringn(
                        mx,
                        y,
                        DROP_MARKER,
                        1,
                        t.accent().add_modifier(Modifier::BOLD),
                    );
                }
            }
        }
        out.room = (right - x) as usize;
        let tail_w = text::spans_width(&self.tail);
        if tail_w > 0 && tail_w < out.room {
            out.tail = true;
            if let Some(b) = buf {
                b.set_line(
                    right - tail_w as u16,
                    y,
                    &Line::from(self.tail.clone()),
                    tail_w as u16,
                );
            }
        }
        out
    }

    /// The column of the drop marker for insert index `d`: on the divider
    /// before tab `d`, or just outside the first or last drawn label (herdr's
    /// `tab_drop_indicator_x`). Always inside `area`.
    fn drop_x(&self, d: usize, hits: &[TabHit], area: Rect) -> Option<u16> {
        let (first, last) = (hits.first()?, hits.last()?);
        let x = if d <= first.index {
            first.rect.x.saturating_sub(1)
        } else if d > last.index {
            last.rect.right()
        } else {
            let div = text::width(&self.divider) as u16;
            hits.iter()
                .find(|h| h.index == d)
                .map_or(last.rect.right(), |h| {
                    h.rect.x.saturating_sub(div.div_ceil(2).max(1))
                })
        };
        Some(x.clamp(area.x, area.right() - 1))
    }
}

/// The form [`Tabs::fit`] picked: hints or not, the window of tabs shown,
/// whether the hidden counts or the scroll buttons are drawn, and whether
/// the action has room.
struct Fit {
    hinted: bool,
    start: usize,
    end: usize,
    markers: bool,
    buttons: bool,
    action: bool,
    max_start: usize,
}

/// The largest useful first tab of `n`: the start of the longest run that
/// shows through the last tab, or the last tab alone when even it does not
/// fit (it must stay reachable). Port of herdr's `max_tab_scroll` over any
/// cost of a window `[s, e)`.
fn max_start_by(n: usize, cost: impl Fn(usize, usize) -> usize, avail: usize) -> usize {
    (0..n)
        .find(|&s| cost(s, n) <= avail)
        .unwrap_or(n.saturating_sub(1))
}

fn marker_before(hidden: usize) -> String {
    format!("‹{hidden} ")
}

fn marker_after(hidden: usize) -> String {
    format!(" {hidden}›")
}

fn key_matches(spec: &str, key: KeyInput) -> bool {
    let mut cs = spec.chars();
    if let (Some(c), None) = (cs.next(), cs.next()) {
        return key.key == Key::Char(c);
    }
    match spec {
        "Esc" => key.key == Key::Esc,
        "Enter" => key.key == Key::Enter,
        "Tab" => key.key == Key::Tab,
        _ => false,
    }
}

impl Widget for Tabs<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_hits(area, buf);
    }
}

impl Widget for &Tabs<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_hits(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    /// llmux's `TABS`, with the keys `draw_tabs` hard-codes.
    fn llmux_tabs(keys: bool, only_dashboard: bool) -> Vec<Tab<'static>> {
        [
            ("dashboard", "Esc"),
            ("accounts", "a"),
            ("stats", "g"),
            ("usage", "U"),
            ("keys", "K"),
            ("perf", "p"),
            ("logs", "l"),
            ("sessions", "s"),
            ("misc", "?"),
            ("config", "c"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, key))| {
            let tab = Tab::new(label);
            if keys && (!only_dashboard || i == 0) {
                tab.key(key)
            } else {
                tab
            }
        })
        .collect()
    }

    fn render(tabs: &Tabs, width: u16) -> (Buffer, TabsLayout) {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
        let layout = tabs.render_layout(Rect::new(0, 1, width, 1), &mut buf);
        (buf, layout)
    }

    /// Port of llmux `assert_c2a_tab_strip` (`ui.rs:15806`): the row text,
    /// one hit per tab at the expected cells, the active style only on the
    /// active label, nothing on the rows around it.
    fn assert_strip(tabs: &Tabs, width: u16, row: &str, expected: &[(&str, u16)]) {
        let (buf, layout) = render(tabs, width);
        assert_eq!(snapshot::lines(&buf)[1], row);
        assert_eq!(layout.hits.len(), expected.len(), "one hit per tab");
        for (hit, (label, x)) in layout.hits.iter().zip(expected) {
            let w = text::width(label) as u16;
            assert_eq!(hit.rect, Rect::new(*x, 1, w, 1), "{label}");
            for col in hit.rect.x..hit.rect.right() {
                assert_eq!(hit_test(&layout.hits, col, 1), Some(hit.index), "{label}");
                let cell = &buf[(col, 1)];
                assert_eq!(cell.fg, Color::Cyan, "{label}");
                assert_eq!(
                    cell.modifier.contains(Modifier::REVERSED | Modifier::BOLD),
                    hit.index == tabs.active_index(),
                    "{label} active style"
                );
            }
        }
        for col in 0..width {
            if !layout.hits.iter().any(|h| h.contains(col, 1)) {
                assert_eq!(hit_test(&layout.hits, col, 1), None, "gap at {col}");
            }
            for row in [0, 2] {
                assert_eq!(buf[(col, row)].symbol(), " ", "one-row strip");
                assert_eq!(hit_test(&layout.hits, col, row), None);
            }
        }
    }

    const PLAIN: &[(&str, u16)] = &[
        ("dashboard", 1),
        ("accounts", 13),
        ("stats", 24),
        ("usage", 32),
        ("keys", 40),
        ("perf", 47),
        ("logs", 54),
        ("sessions", 61),
        ("misc", 72),
        ("config", 79),
    ];

    #[test]
    fn plain_labels_keep_llmux_hit_targets() {
        for active in 0..10 {
            let tabs = Tabs::new(llmux_tabs(false, false))
                .active(active)
                .theme(Theme::ansi());
            assert_strip(
                &tabs,
                139,
                " dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
                PLAIN,
            );
        }
    }

    #[test]
    fn bracket_hints_show_when_they_fit_with_full_hit_targets() {
        let tabs = Tabs::new(llmux_tabs(true, false))
            .hints(Hints::Bracket)
            .theme(Theme::ansi());
        let row = " [Esc] dashboard │ [a] accounts │ [g] stats │ [U] usage │ [K] keys │ [p] perf │ [l] logs │ [s] sessions │ [?] misc │ [c] config";
        let hits = &[
            ("[Esc] dashboard", 1),
            ("[a] accounts", 19),
            ("[g] stats", 34),
            ("[U] usage", 46),
            ("[K] keys", 58),
            ("[p] perf", 69),
            ("[l] logs", 80),
            ("[s] sessions", 91),
            ("[?] misc", 106),
            ("[c] config", 117),
        ];
        assert_strip(&tabs, 140, row, hits);
        // Measured, not a 140-column threshold: the hinted strip is 127
        // cells, so it holds down to 127 and gives way at 126.
        assert_strip(&tabs, 127, row, hits);
        assert_strip(
            &tabs,
            126,
            " dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
            PLAIN,
        );
    }

    #[test]
    fn overlay_shows_only_the_dashboard_return_hint() {
        for active in 1..10 {
            let tabs = Tabs::new(llmux_tabs(true, true))
                .active(active)
                .hints(Hints::Bracket)
                .theme(Theme::ansi());
            assert_strip(
                &tabs,
                140,
                " [Esc] dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
                &[
                    ("[Esc] dashboard", 1),
                    ("accounts", 19),
                    ("stats", 30),
                    ("usage", 38),
                    ("keys", 46),
                    ("perf", 53),
                    ("logs", 60),
                    ("sessions", 67),
                    ("misc", 78),
                    ("config", 85),
                ],
            );
        }
    }

    /// llmux measured `chars().count()`: `계정` got a 2-cell rect for a
    /// 4-cell label. Rects are cells here.
    #[test]
    fn korean_labels_get_rects_in_cells() {
        let tabs = Tabs::new(["대시보드", "계정", "통계", "로그"].map(Tab::new))
            .active(1)
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 40);
        assert_eq!(snapshot::lines(&buf)[1], " 대시보드 │ 계정 │ 통계 │ 로그");
        let rects: Vec<(u16, u16)> = layout
            .hits
            .iter()
            .map(|h| (h.rect.x, h.rect.width))
            .collect();
        assert_eq!(rects, vec![(1, 8), (12, 4), (19, 4), (26, 4)]);
        // The last cell of a wide label still maps to it.
        assert_eq!(hit_test(&layout.hits, 15, 1), Some(1));
        assert_eq!(hit_test(&layout.hits, 16, 1), None);
    }

    #[test]
    fn mnemonic_underlines_the_key_letter_at_no_cost() {
        let tabs = Tabs::new([
            Tab::new("dashboard").key("Esc"),
            Tab::new("accounts").key("a"),
            Tab::new("logs").key("l"),
        ])
        .theme(Theme::mono());
        let (buf, layout) = render(&tabs, 60);
        assert_eq!(
            snapshot::lines(&buf)[1],
            " [Esc] dashboard │ accounts │ logs"
        );
        assert!(layout.hinted);
        let a = layout.hits[1].rect.x;
        assert!(buf[(a, 1)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buf[(a + 1, 1)].modifier.contains(Modifier::UNDERLINED));
        // Narrower: the bracket goes, the underline stays.
        let (buf, layout) = render(&tabs, 30);
        assert_eq!(snapshot::lines(&buf)[1], " dashboard │ accounts │ logs");
        assert!(!layout.hinted);
        assert!(
            buf[(layout.hits[2].rect.x, 1)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn tail_takes_the_room_and_gives_way_first() {
        let tabs = Tabs::new(["dashboard", "accounts", "stats"].map(Tab::new))
            .tail([Span::raw(" cfg ~/.config/llmux.json ")])
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 70);
        let row = &snapshot::lines(&buf)[1];
        assert!(row.ends_with(" cfg ~/.config/llmux.json"), "{row}");
        assert!(layout.tail);
        assert_eq!(layout.room, 70 - 29);
        // 29 cells of tabs + 1 gap + 26 of tail = 56: at 55 the tail goes,
        // the tabs stay whole.
        let (buf, layout) = render(&tabs, 56);
        assert!(layout.tail, "{:?}", snapshot::lines(&buf)[1]);
        let (buf, layout) = render(&tabs, 55);
        assert!(!layout.tail);
        assert_eq!(snapshot::lines(&buf)[1], " dashboard │ accounts │ stats");
        assert_eq!(layout.hits.len(), 3);
    }

    #[test]
    fn too_narrow_scrolls_to_the_active_tab_and_counts_the_rest() {
        let tabs = Tabs::new(llmux_tabs(false, false))
            .active(5)
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 30);
        let row = &snapshot::lines(&buf)[1];
        assert!(layout.hits.iter().any(|h| h.index == 5), "{row}");
        assert!(layout.hidden_before > 0 && layout.hidden_after > 0, "{row}");
        assert!(row.contains(&format!("‹{}", layout.hidden_before)), "{row}");
        assert!(row.ends_with(&format!("{}›", layout.hidden_after)), "{row}");
        assert_eq!(
            layout.hidden_before + layout.hits.len() + layout.hidden_after,
            10
        );
        for h in &layout.hits {
            assert!(h.rect.right() <= 30);
        }
        // Every width down to one cell: no panic, the active tab drawn.
        for w in 1..30 {
            let (_, layout) = render(&tabs, w);
            assert!(layout.hits.iter().all(|h| h.rect.right() <= w), "{w}");
            assert!(layout.hits.iter().any(|h| h.index == 5), "{w}");
        }
    }

    #[test]
    fn keys_step_wrap_and_open_by_shortcut() {
        let tabs = Tabs::new(llmux_tabs(true, false)).active(0);
        assert_eq!(tabs.on_key(Key::Right.into()), Some(1));
        assert_eq!(tabs.on_key(Key::Left.into()), Some(9));
        assert_eq!(tabs.on_key(Key::End.into()), Some(9));
        assert_eq!(tabs.on_key('l'.into()), Some(6));
        assert_eq!(tabs.on_key('?'.into()), Some(8));
        assert_eq!(tabs.on_key(Key::Esc.into()), Some(0));
        assert_eq!(tabs.on_key(KeyInput::ctrl(Key::Char('l'))), None);
        assert_eq!(tabs.on_key('x'.into()), None);
    }

    #[test]
    fn mono_marks_the_active_tab_by_weight() {
        let tabs = Tabs::new(["dashboard", "accounts"].map(Tab::new))
            .active(1)
            .theme(Theme::mono());
        let (buf, layout) = render(&tabs, 40);
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        let x = layout.hits[1].rect.x;
        assert!(buf[(x, 1)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(1, 1)].modifier.contains(Modifier::REVERSED));
    }

    /// herdr's workspace tabs: more than a narrow row holds.
    fn herdr_tabs() -> Vec<Tab<'static>> {
        [
            "main", "claude", "codex", "logs", "review", "deploy", "notes", "db",
        ]
        .map(Tab::new)
        .to_vec()
    }

    fn strip(active: usize) -> Tabs<'static> {
        Tabs::new(herdr_tabs())
            .active(active)
            .scroll_buttons(true)
            .action("+")
            .theme(Theme::ansi())
    }

    // Upstream: tabs.rs `trailing_scroll_limit_accounts_for_full_widths_and_separators`.
    #[test]
    fn trailing_scroll_limit_accounts_for_full_widths_and_separators() {
        for (widths, available, expected) in [
            (&[][..], 0, 0),
            (&[8, 13][..], 0, 1),
            (&[8, 13][..], 1, 1),
            (&[8, 13][..], 12, 1),
            (&[8, 13][..], 21, 1),
            (&[8, 13][..], 22, 0),
            (&[8, 13][..], 30, 0),
            (&[8, u16::MAX as usize][..], u16::MAX as usize, 1),
        ] {
            let cost = |s: usize, e: usize| widths[s..e].iter().sum::<usize>() + (e - s - 1);
            assert_eq!(
                max_start_by(widths.len(), cost, available),
                expected,
                "widths={widths:?}, available={available}"
            );
        }
    }

    #[test]
    fn scroll_buttons_and_action_sit_at_the_ends_with_their_own_rects() {
        let tabs = strip(0);
        let (buf, layout) = render(&tabs, 40);
        // 40 − 2 buttons − the action leaves 31 cells: four labels fit.
        assert_eq!(
            snapshot::lines(&buf)[1],
            " < main │ claude │ codex │ logs    >  +"
        );
        assert_eq!(layout.scroll_left, Some(Rect::new(0, 1, 3, 1)));
        assert_eq!(layout.scroll_right, Some(Rect::new(34, 1, 3, 1)));
        assert_eq!(layout.action, Some(Rect::new(37, 1, 3, 1)));
        assert_eq!((layout.hidden_before, layout.hidden_after), (0, 4));
        assert_eq!(layout.hits[0].rect, Rect::new(3, 1, 4, 1));
        assert_eq!(layout.target_at(1, 1), Some(TabTarget::ScrollLeft));
        assert_eq!(layout.target_at(36, 1), Some(TabTarget::ScrollRight));
        assert_eq!(layout.target_at(38, 1), Some(TabTarget::Action));
        assert_eq!(layout.target_at(4, 1), Some(TabTarget::Tab(0)));
        assert_eq!(layout.target_at(32, 1), None, "the gap before >");
        assert_eq!(layout.target_at(38, 0), None);
        // Nothing hidden to the left: ` < ` is dim; ` > ` is live (bold).
        assert_eq!(buf[(1, 1)].fg, Theme::ansi().muted);
        assert!(!buf[(1, 1)].modifier.contains(Modifier::BOLD));
        assert!(buf[(35, 1)].modifier.contains(Modifier::BOLD));
        // Mono tells them apart by weight alone.
        let (buf, _) = render(&strip(0).theme(Theme::mono()), 40);
        assert!(buf[(1, 1)].modifier.contains(Modifier::DIM));
        assert!(buf[(35, 1)].modifier.contains(Modifier::BOLD));
        // No counts while the buttons are there.
        assert!(!snapshot::lines(&buf)[1].contains('›'));
    }

    #[test]
    fn buttons_hold_their_columns_while_the_strip_scrolls() {
        let mut first = 0;
        let mut seen = Vec::new();
        loop {
            let tabs = strip(0).reveal(Reveal::From(first));
            let (buf, layout) = render(&tabs, 40);
            assert_eq!(layout.scroll_right, Some(Rect::new(34, 1, 3, 1)));
            assert_eq!(layout.hits[0].index, layout.hidden_before);
            seen.push(layout.hidden_before);
            // Live on the left once anything is hidden there.
            assert_eq!(
                buf[(1, 1)].modifier.contains(Modifier::BOLD),
                layout.hidden_before > 0
            );
            if layout.hidden_before == layout.max_start {
                // At the end: everything through `db` is drawn, `>` is dim.
                assert_eq!(layout.hidden_after, 0);
                assert_eq!(buf[(35, 1)].fg, Theme::ansi().muted);
                break;
            }
            first = layout.hidden_before + 1;
        }
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
        // Past the end is clamped to the last useful start.
        let (_, layout) = render(&strip(0).reveal(Reveal::From(99)), 40);
        assert_eq!(layout.hidden_before, layout.max_start);
    }

    #[test]
    fn centre_reveal_puts_the_active_tab_mid_strip() {
        let (_, edge) = render(&strip(5), 40);
        let (_, centre) = render(&strip(5).reveal(Reveal::Centre), 40);
        let mid = |l: &TabsLayout| {
            let h = l.hits.iter().find(|h| h.index == 5).expect("active drawn");
            h.rect.x + h.rect.width / 2
        };
        // Edge leaves the active tab at the right end; centre near 20.
        assert!(mid(&edge) > 25, "{edge:?}");
        assert!(mid(&centre).abs_diff(20) <= 6, "{centre:?}");
        // Near the end, centring is clamped to the last useful start.
        let (_, last) = render(&strip(7).reveal(Reveal::Centre), 40);
        assert_eq!(last.hidden_before, last.max_start);
        assert_eq!(last.hidden_after, 0);
        // The llmux counter strip centres too.
        let tabs = Tabs::new(llmux_tabs(false, false))
            .active(5)
            .reveal(Reveal::Centre)
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 40);
        let row = &snapshot::lines(&buf)[1];
        assert!(layout.hidden_before > 0 && layout.hidden_after > 0, "{row}");
        assert!(row.contains('‹') && row.ends_with('›'), "{row}");
    }

    #[test]
    fn action_follows_the_last_tab_when_everything_fits() {
        let tabs = Tabs::new(["main", "claude"].map(Tab::new))
            .action("+")
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 40);
        assert_eq!(snapshot::lines(&buf)[1], " main │ claude +");
        assert_eq!(layout.action, Some(Rect::new(14, 1, 3, 1)));
        assert_eq!(layout.target_at(15, 1), Some(TabTarget::Action));
        assert_eq!(layout.scroll_left, None);
        // Buttons are only for overflow.
        let (_, layout) = render(&tabs.clone().scroll_buttons(true), 40);
        assert_eq!((layout.scroll_left, layout.scroll_right), (None, None));
        // Too narrow for the action beside an 8-cell tab: it gives way.
        let (_, layout) = render(&tabs, 10);
        assert_eq!(layout.action, None);
    }

    // Port of the cases herdr's `tab_drop_index_at` handles: buttons, the
    // ends of the drawn labels, label midpoints, the dividers between them.
    #[test]
    fn drop_index_splits_each_label_at_its_midpoint() {
        let (_, layout) = render(&strip(0).reveal(Reveal::From(2)), 40);
        // " < codex │ logs │ review │ deploy    >  +"
        let at = |c| layout.drop_index(c, 1);
        let codex = layout.hits[0].rect;
        assert_eq!(layout.hits[0].index, 2);
        assert_eq!(at(1), Some(0), "on < : the very start");
        assert_eq!(at(35), Some(8), "on > : the very end");
        assert_eq!(at(codex.x), Some(2), "first cell of the first label");
        assert_eq!(at(codex.x + 1), Some(2), "left half");
        assert_eq!(at(codex.x + 3), Some(3), "right half");
        assert_eq!(at(codex.right() + 1), Some(3), "the divider after codex");
        let logs = layout.hits[1].rect;
        assert_eq!(at(logs.x + 1), Some(3));
        assert_eq!(at(logs.x + 2), Some(4));
        let last = layout.hits.last().expect("drawn");
        assert_eq!(
            at(last.rect.right()),
            Some(last.index + 1),
            "past the last label"
        );
        assert_eq!(layout.drop_index(10, 0), None, "off the row");
        assert_eq!(TabsLayout::default().drop_index(0, 0), None);
    }

    #[test]
    fn drop_marker_sits_on_the_divider_and_stays_inside() {
        let tabs = Tabs::new(["main", "claude", "codex"].map(Tab::new)).theme(Theme::mono());
        for (index, row, x) in [
            (0, "▐main │ claude │ codex", 0),
            (1, " main ▐ claude │ codex", 6),
            (2, " main │ claude ▐ codex", 15),
            (3, " main │ claude │ codex▐", 22),
            (9, " main │ claude │ codex▐", 22),
        ] {
            let (buf, layout) = render(&tabs.clone().drop_marker(Some(index)), 40);
            assert_eq!(layout.drop_marker, Some(x), "{index}");
            assert_eq!(snapshot::lines(&buf)[1], row, "{index}");
            // Shape and weight, not colour: mono keeps it apart from `│`.
            assert!(buf[(x, 1)].modifier.contains(Modifier::BOLD));
        }
        assert_eq!(unicode_width::UnicodeWidthStr::width_cjk(DROP_MARKER), 1);
        // A full row: the marker after the last label is clamped inside.
        let (_, layout) = render(&tabs.clone().drop_marker(Some(3)), 22);
        assert_eq!(layout.drop_marker, Some(21));
    }

    /// herdr H1: rects must never leave the area, at any width.
    #[test]
    fn every_rect_stays_inside_the_row_at_every_width() {
        for active in [0, 4, 7] {
            for reveal in [Reveal::Edge, Reveal::Centre, Reveal::From(3)] {
                for w in 1..=70u16 {
                    let tabs = strip(active).reveal(reveal).drop_marker(Some(active + 1));
                    let (_, layout) = render(&tabs, w);
                    let row = Rect::new(0, 1, w, 1);
                    let rects = layout
                        .hits
                        .iter()
                        .map(|h| h.rect)
                        .chain(layout.scroll_left)
                        .chain(layout.scroll_right)
                        .chain(layout.action);
                    for r in rects {
                        assert_eq!(r.intersection(row), r, "w={w} {layout:?}");
                    }
                    if let Some(x) = layout.drop_marker {
                        assert!(x < w, "w={w}");
                    }
                    assert!(
                        layout.hidden_before + layout.hits.len() + layout.hidden_after
                            <= layout.total
                    );
                }
            }
        }
    }
}
