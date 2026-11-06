// Derived from herdrdev/herdr `src/client/shell/agent_sidebar.rs`
// (`ordered_agent_pane_ids`, `render_agent_list`, `render_agent_row`),
// `src/client/shell.rs` (`status_icon`, `status_priority`, `status_text`,
// `status_color`), `src/client/shell/sidebar.rs` (`render_collapsed_sidebar`)
// and `src/client/shell/mobile.rs` (`render_agent_summary`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab:
// a distinct glyph per state, each one cell on a CJK terminal (herdr's
// default dots gave working, blocked and done the same `●`, told apart by
// colour, and `● ○ ◐ · ×` are East-Asian Ambiguous); the state's word on
// every row; a failed state; recency as a host `Duration` instead of a
// sequence number; rows that do not fit are counted (`▾ +N more`) instead of
// skipped; a scroll offset that follows the focus when it moves; hit rects
// per item; the rail and the tally measured in cells, the tally always ends
// in `…` when it drops a count. herdr's token configuration, workspace and
// pane lookups and its palette are left behind; colours from the `Theme`.

//! Who is doing what, and who needs the person first: a list of agents (or
//! accounts) with one glyph per state.
//!
//! Each [`AgentItem`] is a row `›◉ claude            waiting` — a focus
//! mark, the state's glyph, the label, the state's word at the right — and
//! any number of secondary lines under the label (`repo tui · tab 2`), so
//! rows differ in height. [`AgentList`] sorts them by priority (failed,
//! waiting, done, working, idle, unknown; within a state the most recent
//! change first), leaves gaps between them, scrolls, and says `▴ +N more` /
//! `▾ +N more` for what is out of view. The focused item is highlighted and
//! kept in view when the focus moves. [`AgentRail`] is the same list folded
//! to a 4–6 column sidebar: index and glyph. [`Tally`] is the list in one
//! line — `2 waiting · 1 done · 3 working …` — dropping whole counts and
//! ending in `…` when it does.
//!
//! Ported from herdr (Apache-2.0), the agents panel of its sidebar, the
//! collapsed rail and the narrow-screen tally. Its states map onto the
//! crate's [`Status`]: working → running, blocked → waiting, done → done;
//! idle and unknown have no status, and `Failed` is added for an agent (or an
//! account) that errored. The port fixes what the audit found: three states
//! shared one `●` and differed only in colour (H2); the glyphs were
//! East-Asian Ambiguous (H9); rows that did not fit were dropped without a
//! count (H8); header text was written one `char` per cell (H4).
//!
//! The working glyph turns with a frame tick the host passes in
//! ([`AgentList::frame`]); under reduced motion it holds still. Time (for
//! "most recent change") is the host's `Duration`, never read here.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::spinner::SpinnerKind;
use crate::text::{self, Align};
use crate::theme::{Status, Theme};

/// What an agent (or an account) is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AgentState {
    /// It errored or stopped; it needs the person.
    Failed,
    /// It waits for the person (herdr's blocked).
    Waiting,
    /// It finished; the result is ready to look at.
    Done,
    /// It is working.
    Working,
    /// Nothing to do, ready.
    Idle,
    /// No report.
    Unknown,
}

impl AgentState {
    /// Every state, in priority order.
    pub const ALL: [AgentState; 6] = [
        AgentState::Failed,
        AgentState::Waiting,
        AgentState::Done,
        AgentState::Working,
        AgentState::Idle,
        AgentState::Unknown,
    ];

    /// Higher sorts first: herdr's blocked > done > working > idle > unknown,
    /// with failed above them all.
    pub const fn priority(self) -> u8 {
        match self {
            AgentState::Failed => 5,
            AgentState::Waiting => 4,
            AgentState::Done => 3,
            AgentState::Working => 2,
            AgentState::Idle => 1,
            AgentState::Unknown => 0,
        }
    }

    pub const fn word(self) -> &'static str {
        match self {
            AgentState::Failed => "failed",
            AgentState::Waiting => "waiting",
            AgentState::Done => "done",
            AgentState::Working => "working",
            AgentState::Idle => "idle",
            AgentState::Unknown => "unknown",
        }
    }

    /// The glyph held still: `✗ ◉ ✓ ⠋ ◦ ?`, all one cell on a CJK terminal.
    pub const fn still(self) -> char {
        match self {
            AgentState::Failed => '✗',
            AgentState::Waiting => '◉',
            AgentState::Done => '✓',
            AgentState::Working => SpinnerKind::Braille.frames()[0],
            AgentState::Idle => '◦',
            AgentState::Unknown => '?',
        }
    }

    /// The glyph at frame tick `frame`: working turns (a braille spinner),
    /// the rest hold still.
    pub fn glyph(self, frame: usize) -> char {
        match self {
            AgentState::Working => SpinnerKind::Braille.glyph(frame),
            s => s.still(),
        }
    }

    /// The crate status this state draws in, if any.
    pub const fn status(self) -> Option<Status> {
        match self {
            AgentState::Failed => Some(Status::Failed),
            AgentState::Waiting => Some(Status::Waiting),
            AgentState::Done => Some(Status::Done),
            AgentState::Working => Some(Status::Running),
            AgentState::Idle | AgentState::Unknown => None,
        }
    }

    /// The state's tone. In mono, failed and waiting are bold, unknown dim.
    pub fn style(self, theme: &Theme) -> Style {
        match self {
            AgentState::Failed | AgentState::Waiting => self
                .status()
                .map_or_else(Style::new, |s| theme.status(s))
                .add_modifier(Modifier::BOLD),
            AgentState::Done | AgentState::Working => {
                self.status().map_or_else(Style::new, |s| theme.status(s))
            }
            AgentState::Idle => theme.secondary(),
            AgentState::Unknown => theme.dim(),
        }
    }
}

impl From<Status> for AgentState {
    fn from(s: Status) -> AgentState {
        match s {
            Status::Running => AgentState::Working,
            Status::Waiting => AgentState::Waiting,
            Status::Done => AgentState::Done,
            Status::Failed => AgentState::Failed,
        }
    }
}

/// One agent or account.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentItem {
    pub state: AgentState,
    pub label: String,
    /// What the state's word says instead (`needs input`, `cooling 1h12m`).
    pub word: Option<String>,
    /// Secondary lines under the label; unstyled spans are secondary text.
    pub lines: Vec<Line<'static>>,
    /// When the state last changed, on the host's clock. Within a state the
    /// most recent change sorts first.
    pub changed_at: Duration,
    pub focused: bool,
}

impl AgentItem {
    pub fn new(state: AgentState, label: impl Into<String>) -> AgentItem {
        AgentItem {
            state,
            label: label.into(),
            word: None,
            lines: Vec::new(),
            changed_at: Duration::ZERO,
            focused: false,
        }
    }

    pub fn word(mut self, word: impl Into<String>) -> AgentItem {
        self.word = Some(word.into());
        self
    }

    /// Add a secondary line.
    pub fn line(mut self, line: impl Into<Line<'static>>) -> AgentItem {
        self.lines.push(line.into());
        self
    }

    pub fn changed_at(mut self, at: Duration) -> AgentItem {
        self.changed_at = at;
        self
    }

    pub fn focused(mut self, focused: bool) -> AgentItem {
        self.focused = focused;
        self
    }

    /// Rows the item takes: the label row and its secondary lines.
    pub fn height(&self) -> u16 {
        u16::try_from(1 + self.lines.len()).unwrap_or(u16::MAX)
    }

    fn word_str(&self) -> &str {
        self.word.as_deref().unwrap_or(self.state.word())
    }
}

/// The order rows are shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Sort {
    /// Failed, waiting, done, working, idle, unknown; the most recent change
    /// first within a state; ties keep the order given.
    #[default]
    Priority,
    /// The order given.
    Given,
}

/// Indices into `items` in the order they are shown — herdr's
/// `ordered_agent_pane_ids`.
pub fn order(items: &[AgentItem], sort: Sort) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..items.len()).collect();
    if sort == Sort::Priority {
        idx.sort_by_key(|&i| {
            (
                std::cmp::Reverse(items[i].state.priority()),
                std::cmp::Reverse(items[i].changed_at),
            )
        });
    }
    idx
}

/// What a list or rail drew, for clicks: `(index into the items, rect)` per
/// item shown, and the `▴`/`▾` rows with how many items each stands for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentListLayout {
    pub items: Vec<(usize, Rect)>,
    pub above: usize,
    pub below: usize,
    pub more_above: Option<Rect>,
    pub more_below: Option<Rect>,
}

/// Scroll position and what the last frame drew. One state serves the list
/// and its rail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentListState {
    /// The first item shown, in display order.
    pub offset: usize,
    /// The focused item (index into the items) last brought into view, so
    /// the list follows the focus when it moves and otherwise stays where
    /// the wheel put it.
    revealed: Option<usize>,
    layout: AgentListLayout,
}

impl AgentListState {
    pub fn new() -> AgentListState {
        AgentListState::default()
    }

    /// Scroll by `delta` items (the mouse wheel); clamped when drawn.
    pub fn scroll_by(&mut self, delta: isize) {
        self.offset = self.offset.saturating_add_signed(delta);
    }

    pub fn layout(&self) -> &AgentListLayout {
        &self.layout
    }

    /// The item (index into the items) under `(col, row)`.
    pub fn hit_test(&self, col: u16, row: u16) -> Option<usize> {
        let p = Position::new(col, row);
        self.layout
            .items
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(i, _)| *i)
    }
}

/// Rows `heights` (display order) take from `o` in `h` rows: whether the
/// `▴` row is drawn, the items shown as `(position, y, rows)`, and the first
/// position not shown.
struct Lay {
    top: bool,
    avail: u16,
    shown: Vec<(usize, u16, u16)>,
    end: usize,
}

fn lay(heights: &[u16], gap: u16, h: u16, o: usize) -> Lay {
    let n = heights.len();
    let top = o > 0 && h >= 3;
    let avail = h - u16::from(top);
    let mut shown = Vec::new();
    // Sums in u32: a row count plus a gap can pass u16::MAX, and a
    // saturated sum would compare equal to a full screen and pass.
    let mut used = 0u32;
    let mut k = o;
    while k < n {
        let g = if k > o { u32::from(gap) } else { 0 };
        // Keep a row for `▾ +N more` while items remain after this one.
        let limit = if k + 1 < n && avail >= 2 {
            avail - 1
        } else {
            avail
        };
        let need = g + u32::from(heights[k]);
        if used + need > u32::from(limit) {
            break;
        }
        // Inside `limit`, so it fits a u16.
        shown.push((k, (used + g) as u16, heights[k]));
        used += need;
        k += 1;
    }
    if shown.is_empty() && o < n {
        // Taller than the room: its first rows.
        let rows = if o + 1 < n && avail >= 2 {
            avail - 1
        } else {
            avail
        };
        if rows > 0 {
            shown.push((o, 0, rows.min(heights[o])));
            k = o + 1;
        }
    }
    Lay {
        top,
        avail,
        shown,
        end: k,
    }
}

/// The largest useful offset: the first item from which the rest fit.
fn max_offset(heights: &[u16], gap: u16, h: u16) -> usize {
    let n = heights.len();
    if n == 0 {
        return 0;
    }
    let mut o = n - 1;
    // u32, as in `lay`: never a saturated sum that passes as a fit.
    let mut total = u32::from(heights[o]);
    while o > 0 {
        let c = o - 1;
        let add = u32::from(heights[c]) + u32::from(gap);
        let room = u32::from(h - u16::from(c > 0 && h >= 3));
        if total + add > room {
            break;
        }
        total += add;
        o = c;
    }
    o
}

/// Where the window sits: the offset clamped so nothing is blank at the
/// end, moved to show `keep` (a focus that just moved).
fn window(heights: &[u16], gap: u16, h: u16, offset: usize, keep: Option<usize>) -> (usize, Lay) {
    let mut o = offset.min(max_offset(heights, gap, h));
    if let Some(f) = keep {
        if f < o {
            o = f;
        }
        while o < f && lay(heights, gap, h, o).end <= f {
            o += 1;
        }
    }
    (o, lay(heights, gap, h, o))
}

/// Agents (or accounts) as rows: a state glyph, the label, the state's word
/// and secondary lines, in priority order, scrolling with `+N more`.
#[derive(Debug, Clone)]
pub struct AgentList<'a> {
    items: &'a [AgentItem],
    sort: Sort,
    gap: u16,
    frame: usize,
    words: bool,
    empty: String,
    theme: Theme,
}

impl<'a> AgentList<'a> {
    pub fn new(items: &'a [AgentItem]) -> AgentList<'a> {
        AgentList {
            items,
            sort: Sort::Priority,
            gap: 0,
            frame: 0,
            words: true,
            empty: "no agents".into(),
            theme: Theme::default(),
        }
    }

    pub fn sort(mut self, sort: Sort) -> Self {
        self.sort = sort;
        self
    }

    /// Blank rows between items (herdr's `row_gap`; default 0).
    pub fn gap(mut self, rows: u16) -> Self {
        self.gap = rows;
        self
    }

    /// The shared frame tick (`spinner::tick`); the working glyph turns with
    /// it unless the theme asks for reduced motion.
    pub fn frame(mut self, tick: usize) -> Self {
        self.frame = tick;
        self
    }

    /// Show the state's word at the right of each row (default on).
    pub fn words(mut self, on: bool) -> Self {
        self.words = on;
        self
    }

    /// What an empty list says (default `no agents`).
    pub fn empty(mut self, message: impl Into<String>) -> Self {
        self.empty = message.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Indices into the items in the order shown.
    pub fn order(&self) -> Vec<usize> {
        order(self.items, self.sort)
    }

    /// The item `delta` rows from the focused one in the order shown,
    /// clamped at the ends — for ↑/↓ in a host that keeps the focus. The
    /// first item when nothing is focused.
    pub fn next_focus(&self, delta: isize) -> Option<usize> {
        let ord = self.order();
        let at = ord.iter().position(|&i| self.items[i].focused);
        let pos = match at {
            Some(p) => p
                .saturating_add_signed(delta)
                .min(ord.len().checked_sub(1)?),
            None => 0,
        };
        ord.get(pos).copied()
    }

    /// The same items folded to a narrow rail.
    pub fn rail(&self) -> AgentRail<'a> {
        AgentRail {
            items: self.items,
            sort: self.sort,
            frame: self.frame,
            theme: self.theme,
        }
    }

    /// The items as a one-line tally.
    pub fn tally(&self) -> Tally {
        Tally::of(self.items).theme(self.theme)
    }

    fn glyph(&self, state: AgentState) -> char {
        if self.theme.reduced_motion() {
            state.still()
        } else {
            state.glyph(self.frame)
        }
    }

    /// The label row in exactly `w` cells.
    fn head(&self, item: &AgentItem, w: usize) -> Line<'static> {
        let t = &self.theme;
        let mark = if item.focused { "›" } else { " " };
        let tone = item.state.style(t);
        let word = item.word_str();
        let word_w = text::width(word);
        let label_w = text::width(&item.label);
        let show_word = self.words && w >= 3 + label_w.min(6) + 1 + word_w;
        let room = w.saturating_sub(3 + if show_word { word_w + 1 } else { 0 });
        let mut spans = vec![
            Span::styled(mark, t.accent()),
            Span::styled(self.glyph(item.state).to_string(), tone),
            Span::raw(" "),
            Span::styled(
                text::pad(&item.label, room, Align::Left),
                t.text().add_modifier(Modifier::BOLD),
            ),
        ];
        if show_word {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(word.to_string(), tone));
        }
        text::truncate_line(Line::from(spans), w)
    }

    fn draw_item(&self, item: &AgentItem, r: Rect, buf: &mut Buffer) {
        let t = &self.theme;
        if item.focused {
            buf.set_style(r, t.selected());
        }
        let w = usize::from(r.width);
        buf.set_line(r.x, r.y, &self.head(item, w), r.width);
        for (n, line) in item
            .lines
            .iter()
            .take(usize::from(r.height.saturating_sub(1)))
            .enumerate()
        {
            let y = r.y + 1 + n as u16;
            if r.width <= 3 {
                break;
            }
            let mut l = text::truncate_line(line.clone(), w - 3);
            l.style = t.secondary().patch(l.style);
            buf.set_line(r.x + 3, y, &l, r.width - 3);
        }
    }
}

/// `▴ +3 more`, cut to `w`.
fn marker(glyph: &str, n: usize, w: usize, sep: &str) -> String {
    text::truncate(
        &format!(
            "{glyph}{sep}+{n}{}",
            if sep.is_empty() { "" } else { " more" }
        ),
        w,
    )
    .into_owned()
}

/// Write ` +N` at the right of row `y` — hidden items with no row of their
/// own to say so.
fn unsaid(n: usize, area: Rect, y: u16, t: &Theme, buf: &mut Buffer) {
    let s = format!(" +{n}");
    let w = text::width(&s) as u16;
    if n == 0 || area.width < w + 2 {
        return;
    }
    buf.set_stringn(area.right() - w, y, s, usize::from(w), t.dim());
}

impl StatefulWidget for &AgentList<'_> {
    type State = AgentListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut AgentListState) {
        state.layout = AgentListLayout::default();
        let area = area.intersection(buf.area);
        let t = &self.theme;
        if area.is_empty() {
            return;
        }
        let ord = self.order();
        if ord.is_empty() {
            state.offset = 0;
            let msg = text::truncate(&self.empty, usize::from(area.width));
            buf.set_stringn(area.x, area.y, msg, usize::from(area.width), t.dim());
            return;
        }
        let heights: Vec<u16> = ord.iter().map(|&i| self.items[i].height()).collect();
        let focus = ord.iter().position(|&i| self.items[i].focused);
        let focus_item = focus.map(|p| ord[p]);
        let keep = focus.filter(|_| focus_item != state.revealed);
        let (o, l) = window(&heights, self.gap, area.height, state.offset, keep);
        state.offset = o;
        state.revealed = focus_item;
        let body_y = area.y + u16::from(l.top);
        let w = usize::from(area.width);
        let mut layout = AgentListLayout {
            above: o,
            below: ord.len() - l.end,
            ..AgentListLayout::default()
        };
        if l.top {
            let r = Rect::new(area.x, area.y, area.width, 1);
            buf.set_stringn(r.x, r.y, marker(" ▴", o, w, " "), w, t.dim());
            layout.more_above = Some(r);
        }
        let mut last_y = area.y;
        for &(k, y, rows) in &l.shown {
            let r = Rect::new(area.x, body_y + y, area.width, rows);
            self.draw_item(&self.items[ord[k]], r, buf);
            layout.items.push((ord[k], r));
            last_y = r.bottom() - 1;
        }
        let bottom = layout.below > 0 && l.avail >= 2;
        if bottom {
            let r = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            buf.set_stringn(r.x, r.y, marker(" ▾", layout.below, w, " "), w, t.dim());
            layout.more_below = Some(r);
        }
        let hidden = if l.top { 0 } else { o } + if bottom { 0 } else { layout.below };
        unsaid(hidden, area, last_y, t, buf);
        state.layout = layout;
    }
}

/// [`AgentList`] folded to a rail of 4–6 columns: each row is the item's
/// position and its glyph (`12 ◉`), the focused row highlighted; `▴+N` and
/// `▾+N` for what is out of view.
#[derive(Debug, Clone)]
pub struct AgentRail<'a> {
    items: &'a [AgentItem],
    sort: Sort,
    frame: usize,
    theme: Theme,
}

impl<'a> AgentRail<'a> {
    pub fn new(items: &'a [AgentItem]) -> AgentRail<'a> {
        AgentList::new(items).rail()
    }

    pub fn sort(mut self, sort: Sort) -> Self {
        self.sort = sort;
        self
    }

    pub fn frame(mut self, tick: usize) -> Self {
        self.frame = tick;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// One row in `w` cells: the 1-based position right-aligned, a space,
    /// the glyph.
    fn row(&self, pos: usize, item: &AgentItem, w: usize) -> Line<'static> {
        let t = &self.theme;
        let glyph = if t.reduced_motion() {
            item.state.still()
        } else {
            item.state.glyph(self.frame)
        };
        let (num_w, gap) = match w {
            0 | 1 => (0, ""),
            2 => (1, ""),
            _ => (w - 2, " "),
        };
        let num = text::pad(&(pos + 1).to_string(), num_w, Align::Right);
        let num_style = if item.focused { t.text() } else { t.dim() };
        Line::from(vec![
            Span::styled(num, num_style),
            Span::raw(gap),
            Span::styled(glyph.to_string(), item.state.style(t)),
        ])
    }
}

impl StatefulWidget for &AgentRail<'_> {
    type State = AgentListState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut AgentListState) {
        state.layout = AgentListLayout::default();
        let area = area.intersection(buf.area);
        let t = &self.theme;
        if area.is_empty() {
            return;
        }
        let ord = order(self.items, self.sort);
        if ord.is_empty() {
            state.offset = 0;
            buf.set_stringn(area.x, area.y, "—", usize::from(area.width), t.dim());
            return;
        }
        let heights = vec![1u16; ord.len()];
        let focus = ord.iter().position(|&i| self.items[i].focused);
        let focus_item = focus.map(|p| ord[p]);
        let keep = focus.filter(|_| focus_item != state.revealed);
        let (o, l) = window(&heights, 0, area.height, state.offset, keep);
        state.offset = o;
        state.revealed = focus_item;
        let w = usize::from(area.width);
        let mut layout = AgentListLayout {
            above: o,
            below: ord.len() - l.end,
            ..AgentListLayout::default()
        };
        let body_y = area.y + u16::from(l.top);
        if l.top {
            let r = Rect::new(area.x, area.y, area.width, 1);
            buf.set_stringn(r.x, r.y, marker("▴", o, w, ""), w, t.dim());
            layout.more_above = Some(r);
        }
        let mut last_y = area.y;
        for &(k, y, _) in &l.shown {
            let r = Rect::new(area.x, body_y + y, area.width, 1);
            let item = &self.items[ord[k]];
            if item.focused {
                buf.set_style(r, t.selected());
            }
            buf.set_line(r.x, r.y, &self.row(k, item, w), r.width);
            layout.items.push((ord[k], r));
            last_y = r.y;
        }
        let bottom = layout.below > 0 && l.avail >= 2;
        if bottom {
            let r = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            buf.set_stringn(r.x, r.y, marker("▾", layout.below, w, ""), w, t.dim());
            layout.more_below = Some(r);
        }
        let hidden = if l.top { 0 } else { o } + if bottom { 0 } else { layout.below };
        // A rail is too narrow for ` +N` beside a row; the last row says it.
        if hidden > 0 && w >= 2 {
            let s = text::truncate(&format!("+{hidden}"), w).into_owned();
            buf.set_stringn(area.x, last_y, text::pad(&s, w, Align::Right), w, t.dim());
        }
        state.layout = layout;
    }
}

/// The list in one line: `2 waiting · 1 done · 3 working …` — counts in
/// priority order, zeros left out, the first (most urgent) in its tone. When
/// the width runs out, whole counts drop and the line ends in `…` (herdr's
/// narrow-screen summary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    counts: [usize; 6],
    words: [String; 6],
    glyphs: bool,
    empty: String,
    theme: Theme,
}

const TALLY_SEP: &str = " · ";

impl Tally {
    pub fn new(states: impl IntoIterator<Item = AgentState>) -> Tally {
        let mut counts = [0usize; 6];
        for s in states {
            counts[Tally::slot(s)] += 1;
        }
        Tally {
            counts,
            words: AgentState::ALL.map(|s| s.word().to_string()),
            glyphs: false,
            empty: "no agents".into(),
            theme: Theme::default(),
        }
    }

    pub fn of(items: &[AgentItem]) -> Tally {
        Tally::new(items.iter().map(|i| i.state))
    }

    fn slot(s: AgentState) -> usize {
        AgentState::ALL.iter().position(|x| *x == s).unwrap_or(0)
    }

    /// The word a state is counted under (`ok` for idle accounts).
    pub fn word(mut self, state: AgentState, word: impl Into<String>) -> Self {
        self.words[Tally::slot(state)] = word.into();
        self
    }

    /// Lead each count with the state's glyph (default off).
    pub fn glyphs(mut self, on: bool) -> Self {
        self.glyphs = on;
        self
    }

    /// What the line says with nothing to count (default `no agents`).
    pub fn empty(mut self, message: impl Into<String>) -> Self {
        self.empty = message.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn count(&self, state: AgentState) -> usize {
        self.counts[Tally::slot(state)]
    }

    /// The tally in at most `width` cells.
    pub fn line(&self, width: u16) -> Line<'static> {
        let t = &self.theme;
        let w = usize::from(width);
        let entries: Vec<(AgentState, String)> = AgentState::ALL
            .iter()
            .zip(self.counts.iter().zip(self.words.iter()))
            .filter(|(_, (n, _))| **n > 0)
            .map(|(s, (n, word))| {
                let text = if self.glyphs {
                    format!("{} {n} {word}", s.still())
                } else {
                    format!("{n} {word}")
                };
                (*s, text)
            })
            .collect();
        if entries.is_empty() {
            return Line::from(Span::styled(
                text::truncate(&self.empty, w).into_owned(),
                t.dim(),
            ));
        }
        let sep_w = text::width(TALLY_SEP);
        let mut spans = Vec::new();
        let mut used = 0usize;
        for (j, (state, s)) in entries.iter().enumerate() {
            let style = if j == 0 {
                state.style(t).add_modifier(Modifier::BOLD)
            } else {
                t.secondary()
            };
            let sep = if j == 0 { 0 } else { sep_w };
            let need = sep + text::width(s);
            let more_after = j + 1 < entries.len();
            // Leave room for ` …` while counts remain after this one.
            let reserve = if more_after { 2 } else { 0 };
            if used + need + reserve <= w {
                if j > 0 {
                    spans.push(Span::styled(TALLY_SEP, t.dim()));
                }
                spans.push(Span::styled(s.clone(), style));
                used += need;
                continue;
            }
            if j == 0 {
                // Not even the first count fits whole: cut it, always with `…`.
                let cut = if w == 0 {
                    String::new()
                } else if more_after || text::width(s) > w {
                    let (p, _) = text::take_cells(s, w - 1);
                    format!("{p}{}", text::ELLIPSIS)
                } else {
                    s.clone()
                };
                spans.push(Span::styled(cut, style));
            } else {
                spans.push(Span::styled(" …", t.dim()));
            }
            break;
        }
        Line::from(spans)
    }
}

impl Widget for &Tally {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        buf.set_line(area.x, area.y, &self.line(area.width), area.width);
    }
}

impl Widget for Tally {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::theme::Motion;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthChar;

    const S: fn(u64) -> Duration = Duration::from_secs;

    fn flat(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn render_list(list: &AgentList, state: &mut AgentListState, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        list.render(buf.area, &mut buf, state);
        buf
    }

    /// herdr `agent_sidebar_honors_priority_symbols_tokens_and_stable_hits`:
    /// the blocked agent (newer change) is listed and hit first, with its
    /// state label and token line.
    #[test]
    fn blocked_sorts_before_done_with_its_label_and_tokens() {
        let items = [
            AgentItem::new(AgentState::Done, "pi one")
                .line("review complete")
                .changed_at(S(10))
                .focused(true),
            AgentItem::new(AgentState::Waiting, "pi two")
                .word("needs input")
                .line("waiting for Can")
                .changed_at(S(20)),
        ];
        let list = AgentList::new(&items).theme(Theme::ansi());
        assert_eq!(list.order(), vec![1, 0]);
        let mut state = AgentListState::new();
        let buf = render_list(&list, &mut state, 40, 10);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[0], " ◉ pi two                    needs input");
        assert_eq!(lines[1], "   waiting for Can");
        assert_eq!(lines[2], "›✓ pi one                           done");
        assert_eq!(lines[3], "   review complete");
        assert_eq!(state.layout().items[0], (1, Rect::new(0, 0, 40, 2)));
        assert_eq!(state.hit_test(5, 1), Some(1));
        assert_eq!(state.hit_test(5, 3), Some(0));
        // Short: the first hit is still the blocked one; scrolling shows the other.
        let mut state = AgentListState::new();
        render_list(&list, &mut state, 40, 2);
        assert_eq!(
            state.layout().items.first().map(|h| h.0),
            Some(0),
            "focus kept in view"
        );
        let mut state = AgentListState::new();
        let unfocused = [items[0].clone().focused(false), items[1].clone()];
        let list = AgentList::new(&unfocused);
        render_list(&list, &mut state, 40, 2);
        assert_eq!(state.layout().items.first().map(|h| h.0), Some(1));
        state.scroll_by(1);
        render_list(&list, &mut state, 40, 2);
        assert_eq!(state.layout().items.first().map(|h| h.0), Some(0));
    }

    /// herdr `aggregate_priority_uses_client_observed_recency_across_machines`:
    /// within a state the most recent change leads; a working agent leads idle
    /// ones; going back to idle keeps the newer change first.
    #[test]
    fn recency_breaks_ties_within_a_state() {
        let local = |s, at| AgentItem::new(s, "local agent").changed_at(S(at));
        let remote = |s, at| AgentItem::new(s, "remote agent").changed_at(S(at));
        let names = |items: &[AgentItem]| {
            order(items, Sort::Priority)
                .into_iter()
                .map(|i| items[i].label.clone())
                .collect::<Vec<_>>()
        };
        let items = [remote(AgentState::Idle, 1), local(AgentState::Idle, 2)];
        assert_eq!(names(&items), ["local agent", "remote agent"]);
        let items = [local(AgentState::Idle, 2), remote(AgentState::Working, 2)];
        assert_eq!(names(&items), ["remote agent", "local agent"]);
        let items = [local(AgentState::Idle, 2), remote(AgentState::Idle, 3)];
        assert_eq!(names(&items), ["remote agent", "local agent"]);
        // The full order, and ties keep the order given.
        let all: Vec<AgentItem> = [
            AgentState::Unknown,
            AgentState::Idle,
            AgentState::Working,
            AgentState::Done,
            AgentState::Waiting,
            AgentState::Failed,
            AgentState::Idle,
        ]
        .iter()
        .enumerate()
        .map(|(n, s)| AgentItem::new(*s, format!("a{n}")))
        .collect();
        assert_eq!(names(&all), ["a5", "a4", "a3", "a2", "a1", "a6", "a0"]);
        assert_eq!(order(&all, Sort::Given), (0..7).collect::<Vec<_>>());
    }

    #[test]
    fn every_state_has_its_own_narrow_glyph_and_word() {
        let mut seen = std::collections::HashSet::new();
        let mut words = std::collections::HashSet::new();
        for s in AgentState::ALL {
            assert_eq!(s.still().width_cjk(), Some(1), "{s:?}");
            assert!(seen.insert(s.still()), "{s:?} shares a glyph");
            assert!(words.insert(s.word()), "{s:?}");
        }
        for f in 0..20 {
            let g = AgentState::Working.glyph(f);
            assert_eq!(g.width_cjk(), Some(1));
            for s in AgentState::ALL
                .iter()
                .filter(|s| **s != AgentState::Working)
            {
                assert_ne!(g, s.still(), "frame {f} looks like {s:?}");
            }
        }
        // herdr's dots fail this: `● ○ ◐ · ×` are East-Asian Ambiguous.
        for c in ['●', '○', '◐', '·', '×'] {
            assert_eq!(c.width_cjk(), Some(2), "{c:?}");
        }
        // Mono tells every state apart, by glyph and by word.
        let items: Vec<AgentItem> = AgentState::ALL
            .iter()
            .map(|s| AgentItem::new(*s, "agent"))
            .collect();
        let list = AgentList::new(&items).theme(Theme::mono());
        let buf = render_list(&list, &mut AgentListState::new(), 30, 6);
        let rows = snapshot::lines(&buf);
        let set: std::collections::HashSet<&String> = rows.iter().collect();
        assert_eq!(set.len(), 6, "{rows:?}");
        for (row, s) in rows.iter().zip(AgentState::ALL) {
            assert!(row.contains(s.still()) && row.ends_with(s.word()), "{row}");
        }
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
        }
    }

    #[test]
    fn working_turns_with_the_frame_and_holds_under_reduced_motion() {
        let items = [AgentItem::new(AgentState::Working, "claude")];
        let at = |f, t: Theme| {
            render_list(
                &AgentList::new(&items).frame(f).theme(t),
                &mut AgentListState::new(),
                20,
                1,
            )[(1, 0)]
                .symbol()
                .to_string()
        };
        assert_ne!(at(0, Theme::ansi()), at(1, Theme::ansi()));
        let r = Theme::ansi().with_motion(Motion::Reduced);
        assert_eq!(at(0, r), at(7, r));
    }

    #[test]
    fn hidden_rows_are_counted_and_the_focus_is_followed() {
        let items: Vec<AgentItem> = (0..29)
            .map(|i| {
                AgentItem::new(AgentState::Idle, format!("ai{i}@iq.io"))
                    .word("ok")
                    .line("claude · 62% left")
            })
            .collect();
        let list = AgentList::new(&items)
            .sort(Sort::Given)
            .theme(Theme::ansi());
        let mut state = AgentListState::new();
        let buf = render_list(&list, &mut state, 32, 10);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[9], " ▾ +25 more");
        assert_eq!(state.layout().items.len(), 4);
        assert_eq!(state.layout().below, 25);
        // Scrolled to the middle: both ends say what they hide.
        state.scroll_by(10);
        let lines = snapshot::lines(&render_list(&list, &mut state, 32, 10));
        assert_eq!(lines[0], " ▴ +10 more");
        assert_eq!(lines[9], " ▾ +15 more");
        // Past the end: clamped so the last item sits at the bottom.
        state.scroll_by(100);
        let lines = snapshot::lines(&render_list(&list, &mut state, 32, 10));
        assert!(lines[8].contains("claude"), "{lines:?}");
        assert_eq!(state.layout().below, 0);
        assert_eq!(state.layout().items.last().map(|h| h.0), Some(28));
        // The focus moves: the list follows; the wheel then wins.
        let mut items = items;
        items[3].focused = true;
        let list = AgentList::new(&items)
            .sort(Sort::Given)
            .theme(Theme::ansi());
        render_list(&list, &mut state, 32, 10);
        assert!(state.layout().items.iter().any(|h| h.0 == 3));
        state.scroll_by(8);
        render_list(&list, &mut state, 32, 10);
        assert!(!state.layout().items.iter().any(|h| h.0 == 3));
        assert_eq!(list.next_focus(1), Some(4));
        assert_eq!(list.next_focus(-10), Some(0));
        assert_eq!(list.next_focus(100), Some(28));
        items[3].focused = false;
        items[20].focused = true;
        let list = AgentList::new(&items)
            .sort(Sort::Given)
            .theme(Theme::ansi());
        render_list(&list, &mut state, 32, 10);
        assert!(state.layout().items.iter().any(|h| h.0 == 20));
    }

    /// Review #7: two items in 65,535 rows with gap(u16::MAX). The
    /// saturating sum equalled the room, passed the check, and the plain
    /// `+=` after it overflowed.
    #[test]
    fn a_huge_gap_and_area_do_not_overflow_the_sums() {
        let items = [
            AgentItem::new(AgentState::Working, "claude"),
            AgentItem::new(AgentState::Idle, "codex"),
        ];
        let list = AgentList::new(&items)
            .sort(Sort::Given)
            .gap(u16::MAX)
            .theme(Theme::ansi());
        let mut state = AgentListState::new();
        let buf = render_list(&list, &mut state, 12, u16::MAX);
        assert_eq!(state.layout().items.len(), 1, "the gap leaves no room");
        assert!(snapshot::text(&buf).contains("+1 more"));
        for (heights, gap, h) in [
            (&[1u16, 1][..], u16::MAX, u16::MAX),
            (&[u16::MAX, u16::MAX][..], u16::MAX, u16::MAX),
            (&[u16::MAX, 1, 1][..], 0, u16::MAX),
            (&[1, u16::MAX - 1][..], 0, u16::MAX),
        ] {
            let o = max_offset(heights, gap, h);
            for from in 0..heights.len() {
                let l = lay(heights, gap, h, from);
                for &(_, y, rows) in &l.shown {
                    assert!(u32::from(y) + u32::from(rows) <= u32::from(l.avail));
                }
            }
            assert!(o < heights.len());
        }
        assert_eq!(max_offset(&[1, u16::MAX - 1], 0, u16::MAX), 0, "they fit");
        assert_eq!(lay(&[1, u16::MAX - 1], 0, u16::MAX, 0).shown.len(), 2);
    }

    #[test]
    fn gaps_and_variable_heights() {
        let items = [
            AgentItem::new(AgentState::Working, "claude")
                .line("repo tui · tab 2")
                .line("editing src/lib.rs"),
            AgentItem::new(AgentState::Idle, "codex"),
            AgentItem::new(AgentState::Done, "pi").line("review complete"),
        ];
        let list = AgentList::new(&items)
            .sort(Sort::Given)
            .gap(1)
            .theme(Theme::ansi());
        let mut state = AgentListState::new();
        let buf = render_list(&list, &mut state, 30, 9);
        let rects: Vec<Rect> = state.layout().items.iter().map(|h| h.1).collect();
        assert_eq!(
            rects,
            [
                Rect::new(0, 0, 30, 3),
                Rect::new(0, 4, 30, 1),
                Rect::new(0, 6, 30, 2)
            ]
        );
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[2], "   editing src/lib.rs");
        assert_eq!(lines[3], "");
    }

    #[test]
    fn narrow_rows_drop_the_word_then_cut_the_label() {
        let items = [
            AgentItem::new(AgentState::Waiting, "claude-opus 한국어 에이전트").word("needs input"),
        ];
        let list = AgentList::new(&items).theme(Theme::ansi());
        let row =
            |w| snapshot::lines(&render_list(&list, &mut AgentListState::new(), w, 1))[0].clone();
        assert_eq!(row(48), " ◉ claude-opus 한국어 에이전트       needs input");
        assert_eq!(row(30), " ◉ claude-opus 한… needs input");
        assert_eq!(row(16), " ◉ claude-opus …");
        assert_eq!(row(3), " ◉");
        for w in 0..60u16 {
            let l = list.head(&items[0], usize::from(w));
            assert!(text::line_width(&l) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn rail_is_index_and_glyph_and_counts_what_it_hides() {
        let mut items: Vec<AgentItem> = (0..12)
            .map(|i| {
                let s = [AgentState::Idle, AgentState::Working, AgentState::Failed][i % 3];
                AgentItem::new(s, format!("a{i}"))
            })
            .collect();
        items[4].focused = true;
        let rail = AgentRail::new(&items)
            .sort(Sort::Given)
            .theme(Theme::mono());
        let mut state = AgentListState::new();
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 6));
        (&rail).render(buf.area, &mut buf, &mut state);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[0], " 1 ◦");
        assert_eq!(lines[1], " 2 ⠋");
        assert_eq!(lines[2], " 3 ✗");
        assert_eq!(lines[5], "▾+7");
        assert!(
            buf[(0, 4)].modifier.contains(Modifier::REVERSED),
            "focused row"
        );
        assert_eq!(state.hit_test(1, 4), Some(4));
        // 5 and 6 columns; three-digit positions are cut, not overflowed.
        let many: Vec<AgentItem> = (0..120)
            .map(|i| AgentItem::new(AgentState::Idle, i.to_string()))
            .collect();
        let rail = AgentRail::new(&many).sort(Sort::Given);
        let mut state = AgentListState {
            offset: 200,
            ..AgentListState::new()
        };
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 5));
        (&rail).render(buf.area, &mut buf, &mut state);
        let lines = snapshot::lines(&buf);
        assert_eq!(lines[0], "▴+1…");
        assert_eq!(lines[4], "1… ◦");
        for w in [5u16, 6] {
            let mut buf = Buffer::empty(Rect::new(0, 0, w, 3));
            (&rail).render(buf.area, &mut buf, &mut AgentListState::new());
            assert_eq!(
                snapshot::lines(&buf)[0].len(),
                usize::from(w) + 2,
                "◦ is 3 bytes"
            );
        }
    }

    /// herdr's `render_agent_summary`: counts in priority order, zeros left
    /// out, whole counts dropped with `…`, the first in its tone.
    #[test]
    fn tally_drops_whole_counts_and_ends_in_an_ellipsis() {
        use AgentState::*;
        let states = [
            Waiting, Waiting, Done, Working, Working, Working, Idle, Idle,
        ];
        let t = Tally::new(states).theme(Theme::ansi());
        assert_eq!(flat(&t.line(80)), "2 waiting · 1 done · 3 working · 2 idle");
        assert_eq!(flat(&t.line(36)), "2 waiting · 1 done · 3 working …");
        assert_eq!(flat(&t.line(31)), "2 waiting · 1 done …");
        assert_eq!(flat(&t.line(20)), "2 waiting · 1 done …");
        assert_eq!(flat(&t.line(19)), "2 waiting …");
        assert_eq!(flat(&t.line(11)), "2 waiting …");
        assert_eq!(flat(&t.line(10)), "2 waiting…");
        assert_eq!(flat(&t.line(5)), "2 wa…");
        for w in 0..60u16 {
            let l = t.line(w);
            assert!(text::line_width(&l) <= usize::from(w), "{w}");
            // The whole tally is 39 cells; anything narrower says it cut.
            if w < 39 {
                assert!(flat(&l).ends_with('…') || w == 0, "{w}: {}", flat(&l));
            }
        }
        let line = t.line(80);
        assert_eq!(line.spans[0].style.fg, Some(ratatui::style::Color::Yellow));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[2].style.fg, Some(ratatui::style::Color::Gray));
        // Empty; words and glyphs for llmux accounts.
        assert_eq!(flat(&Tally::new([]).line(20)), "no agents");
        let accounts = Tally::new([Failed, Waiting, Waiting, Idle, Idle, Idle])
            .word(Waiting, "cooling")
            .word(Idle, "ok")
            .glyphs(true);
        assert_eq!(
            flat(&accounts.line(60)),
            "✗ 1 failed · ◉ 2 cooling · ◦ 3 ok"
        );
        assert_eq!(accounts.count(Idle), 3);
    }

    #[test]
    fn tiny_sizes_never_panic_or_draw_outside() {
        let items: Vec<AgentItem> = AgentState::ALL
            .iter()
            .cycle()
            .take(9)
            .enumerate()
            .map(|(n, s)| {
                AgentItem::new(*s, format!("agent {n} 한국어"))
                    .line("repo · tab · pane")
                    .focused(n == 7)
            })
            .collect();
        let frame = Rect::new(0, 0, 12, 9);
        for w in 0..=10u16 {
            for h in 0..=7u16 {
                for gap in [0, 1, 3] {
                    for offset in [0, 3, 50] {
                        let area = Rect::new(1, 1, w, h);
                        let mut buf = Buffer::empty(frame);
                        for c in buf.content.iter_mut() {
                            c.set_symbol("X");
                        }
                        let mut state = AgentListState {
                            offset,
                            ..AgentListState::new()
                        };
                        let list = AgentList::new(&items).gap(gap);
                        (&list).render(area, &mut buf, &mut state);
                        let mut rail_state = AgentListState {
                            offset,
                            ..AgentListState::new()
                        };
                        (&list.rail()).render(area, &mut buf, &mut rail_state);
                        let t = list.tally();
                        (&t).render(area, &mut buf);
                        for y in 0..9 {
                            for x in 0..12 {
                                if !area.contains(Position::new(x, y)) {
                                    assert_eq!(buf[(x, y)].symbol(), "X", "{w}×{h} at {x},{y}");
                                }
                            }
                        }
                        for (_, r) in &state.layout().items {
                            assert!(
                                r.y >= area.y && r.bottom() <= area.bottom(),
                                "{r:?} {area:?}"
                            );
                        }
                        if !area.is_empty() {
                            let l = state.layout();
                            assert!(!l.items.is_empty(), "{w}×{h} gap {gap} off {offset}");
                            assert_eq!(l.items.len() + l.above + l.below, items.len());
                        }
                    }
                }
            }
        }
        // Empty list says so.
        let mut buf = Buffer::empty(Rect::new(0, 0, 20, 2));
        (&AgentList::new(&[]).empty("no accounts")).render(
            buf.area,
            &mut buf,
            &mut AgentListState::new(),
        );
        assert_eq!(snapshot::lines(&buf)[0], "no accounts");
    }

    #[test]
    fn focus_is_highlighted_not_only_coloured() {
        let items = [
            AgentItem::new(AgentState::Idle, "codex").line("repo llmux"),
            AgentItem::new(AgentState::Idle, "claude")
                .line("repo tui")
                .focused(true),
        ];
        for theme in [Theme::mono(), Theme::ansi(), Theme::dark()] {
            let list = AgentList::new(&items).sort(Sort::Given).theme(theme);
            let buf = render_list(&list, &mut AgentListState::new(), 24, 4);
            let lines = snapshot::lines(&buf);
            assert!(lines[2].starts_with('›'), "{lines:?}");
            assert!(lines[0].starts_with(' '));
            // The whole item, both rows, carries the selection style.
            let sel = theme.selected();
            for y in [2, 3] {
                let c = &buf[(10, y)];
                assert!(
                    c.modifier.contains(sel.add_modifier) || c.bg == sel.bg.unwrap_or(Color::Reset)
                );
            }
        }
    }
}
