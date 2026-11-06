// Derived from herdrdev/herdr `src/client/shell/notifications.rs`
// (`render_mobile_notice_banner`, `render_notification_card`),
// `src/client/shell/notification_policy.rs` (`notification_duration`,
// `queue_visible_notification`, `promote_queued_notification`),
// `src/ui/status.rs` (`copy_feedback_rect`) and `src/ui.rs`
// (`copy_feedback_offset_for_toast`, `rectangles_overlap`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab:
// one card for every kind, each kind with its own glyph and word (herdr told
// kinds apart by the dot's colour alone); six positions, every rect clamped
// inside the bounds (herdr's copy toast left the buffer at small heights and
// panicked); widths in cells by grapheme, title and body cut with `…`; a
// multi-line body that says `+N more`; the one-row banner chosen by width or
// height; a stack of several toasts, each past the last, that counts what
// does not fit; a queue that shows several at once, replaces by key, counts
// what it drops and takes time as a `Duration` instead of `Instant::now()`.
// Colours from the `Theme`.

//! A message that arrives over the screen, says what kind it is, and goes
//! away on its own — positioned, stacked and queued.
//!
//! [`Toast`] is a small card anchored to a corner or the middle of an edge:
//! the kind's word in the top border (`┌ fail ──┐`), the kind's glyph and the
//! title (`✗ usage refresh: 3 of 5 failed`), then a body of one line per fact,
//! cut with `…` and ending in `+N more` when lines are dropped. On a narrow
//! area, or one too short for a card, the same toast is a one-row banner
//! (`✗ refresh failed · ai15: 401 +2 more`). [`ToastStack`] draws several at
//! one anchor, each past the one before, and says on the last card how many
//! did not fit and how many the queue dropped. [`ToastQueue`] decides which
//! toasts are up: first in, first out, a few at a time, each for its own time
//! to live; when too many wait, the oldest waiting one is dropped and counted.
//!
//! How it differs from [`crate::banner::Flash`]: a flash is one line about the
//! last action, in a row the host already keeps for it (the footer), and the
//! next flash replaces it. A toast floats over the content at a position,
//! carries several lines, and stacks with others; the queue makes a burst of
//! messages readable one after another instead of overwritten.
//!
//! Ported from herdr (Apache-2.0): the notification card and its clamp, the
//! narrow-screen banner, the copy toast's six positions and its overlap
//! offset, and the notification queue (cap 8, oldest dropped, a time to live
//! per kind, promoted when the one showing expires). The port fixes what the
//! audit found: the copy toast was not clamped and panicked on a 100×4
//! terminal (H1); the kind was told by the dot's colour only (H2); title and
//! body were clipped without `…` (H8); widths were in bytes (H6). No clock is
//! read: `now` is the host's `Duration` since it started, and the queue
//! promotes a waiting toast at the instant the slot freed — so what is up at
//! `now` does not depend on how often the host ticks.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::collections::VecDeque;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Widget};

use crate::banner::Outcome;
use crate::text;
use crate::theme::{Level, Status, Theme};

/// Widest card unless the host says otherwise.
pub const DEFAULT_MAX_WIDTH: u16 = 56;
/// Body lines a card shows before it says `+N more`.
pub const DEFAULT_MAX_BODY: usize = 4;
/// At or below this many columns a stack is drawn as banners — herdr's
/// narrow ("mobile") breakpoint.
pub const DEFAULT_NARROW_WIDTH: u16 = 64;
/// Narrowest card; below it the toast is a banner.
pub const MIN_CARD_WIDTH: u16 = 10;
/// Toasts waiting to be shown before the oldest is dropped (herdr's
/// `MAX_QUEUED_NOTIFICATIONS`).
pub const DEFAULT_CAP: usize = 8;
/// Toasts up at once.
pub const DEFAULT_MAX_VISIBLE: usize = 3;

/// Cells between the card's border and its text, and the body's indent
/// under the title (past the glyph).
const PAD: u16 = 1;
const INDENT: &str = "  ";
const SEP: &str = " · ";

/// What a toast is about. Each kind has its own glyph and word, so the kind
/// reads without colour; every glyph is one cell even on a terminal that
/// draws East-Asian Ambiguous glyphs wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastKind {
    /// Something to know: an update is ready, a setting changed.
    Info,
    /// Something under way: reconnecting, refreshing.
    Progress,
    /// Something waits for the person: an agent needs input.
    Attention,
    /// An action worked: copied, saved, finished.
    Ok,
    /// Worked, but not fully, or about to go wrong.
    Warn,
    /// An action failed.
    Fail,
}

impl ToastKind {
    pub const ALL: [ToastKind; 6] = [
        ToastKind::Info,
        ToastKind::Progress,
        ToastKind::Attention,
        ToastKind::Ok,
        ToastKind::Warn,
        ToastKind::Fail,
    ];

    /// `▸ ⋯ ◉ ✓ ! ✗` — all East-Asian Narrow (herdr's `●` is Ambiguous).
    pub const fn glyph(self) -> &'static str {
        match self {
            ToastKind::Info => "▸",
            ToastKind::Progress => "⋯",
            ToastKind::Attention => "◉",
            ToastKind::Ok => "✓",
            ToastKind::Warn => "!",
            ToastKind::Fail => "✗",
        }
    }

    /// The word in the card's top border.
    pub const fn word(self) -> &'static str {
        match self {
            ToastKind::Info => "note",
            ToastKind::Progress => "working",
            ToastKind::Attention => "waiting",
            ToastKind::Ok => "ok",
            ToastKind::Warn => "warn",
            ToastKind::Fail => "fail",
        }
    }

    /// How long a toast of this kind stays up: 8 s for what needs the person
    /// (herdr's attention time, and long enough to read several failures),
    /// 5 s for the rest (herdr's and llmux's status time).
    pub const fn ttl(self) -> Duration {
        match self {
            ToastKind::Attention | ToastKind::Fail => Duration::from_secs(8),
            _ => Duration::from_secs(5),
        }
    }

    /// The kind's tone. In mono: fail is reversed, warn and info bold.
    pub fn style(self, theme: &Theme) -> Style {
        match self {
            ToastKind::Info => theme.accent(),
            ToastKind::Progress => theme.status(Status::Running),
            ToastKind::Attention => theme.status(Status::Waiting),
            ToastKind::Ok => theme.status(Status::Done),
            ToastKind::Warn => theme.level(Level::Warn),
            ToastKind::Fail => theme.level(Level::Crit),
        }
    }
}

impl From<Outcome> for ToastKind {
    /// A [`crate::banner::Flash`] outcome as a toast kind.
    fn from(o: Outcome) -> ToastKind {
        match o {
            Outcome::Ok => ToastKind::Ok,
            Outcome::Warn => ToastKind::Warn,
            Outcome::Fail => ToastKind::Fail,
        }
    }
}

impl From<Status> for ToastKind {
    fn from(s: Status) -> ToastKind {
        match s {
            Status::Running => ToastKind::Progress,
            Status::Waiting => ToastKind::Attention,
            Status::Done => ToastKind::Ok,
            Status::Failed => ToastKind::Fail,
        }
    }
}

/// Where a toast sits in its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Anchor {
    TopLeft,
    TopCenter,
    #[default]
    TopRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 6] = [
        Anchor::TopLeft,
        Anchor::TopCenter,
        Anchor::TopRight,
        Anchor::BottomLeft,
        Anchor::BottomCenter,
        Anchor::BottomRight,
    ];

    pub const fn is_top(self) -> bool {
        matches!(self, Anchor::TopLeft | Anchor::TopCenter | Anchor::TopRight)
    }

    /// `w × h` placed at this anchor inside `region` (both already fit).
    fn place(self, region: Rect, w: u16, h: u16) -> Rect {
        let w = w.min(region.width);
        let h = h.min(region.height);
        let x = match self {
            Anchor::TopLeft | Anchor::BottomLeft => region.x,
            Anchor::TopCenter | Anchor::BottomCenter => region.x + (region.width - w) / 2,
            Anchor::TopRight | Anchor::BottomRight => region.right() - w,
        };
        let y = if self.is_top() {
            region.y
        } else {
            region.bottom() - h
        };
        Rect::new(x, y, w, h)
    }

    /// `bounds` less `offset` rows at the anchored edge.
    fn region(self, bounds: Rect, offset: u16) -> Rect {
        let off = offset.min(bounds.height);
        if self.is_top() {
            Rect {
                y: bounds.y + off,
                height: bounds.height - off,
                ..bounds
            }
        } else {
            Rect {
                height: bounds.height - off,
                ..bounds
            }
        }
    }
}

/// How a toast is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Form {
    /// A bordered card at the anchor.
    Card,
    /// One full-width row at the anchored edge.
    Banner,
}

/// One message: a kind, a title, and optional body lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    kind: ToastKind,
    title: String,
    body: Vec<String>,
    word: Option<String>,
    key: Option<String>,
    ttl: Duration,
    max_body: usize,
    max_width: u16,
    theme: Theme,
}

impl Toast {
    pub fn new(kind: ToastKind, title: impl Into<String>) -> Toast {
        Toast {
            kind,
            title: title.into(),
            body: Vec::new(),
            word: None,
            key: None,
            ttl: kind.ttl(),
            max_body: DEFAULT_MAX_BODY,
            max_width: DEFAULT_MAX_WIDTH,
            theme: Theme::default(),
        }
    }

    pub fn info(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Info, title)
    }

    pub fn progress(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Progress, title)
    }

    pub fn attention(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Attention, title)
    }

    pub fn ok(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Ok, title)
    }

    pub fn warn(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Warn, title)
    }

    pub fn fail(title: impl Into<String>) -> Toast {
        Toast::new(ToastKind::Fail, title)
    }

    /// Body text; each `\n` starts a new line. Adds to what is there.
    pub fn body(mut self, body: impl AsRef<str>) -> Toast {
        self.body
            .extend(body.as_ref().lines().map(|l| l.trim_end().to_string()));
        while self.body.last().is_some_and(String::is_empty) {
            self.body.pop();
        }
        self
    }

    /// Body lines, one fact each (a failure per account). Adds to what is
    /// there.
    pub fn lines<I, S>(mut self, lines: I) -> Toast
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.body.extend(lines.into_iter().map(Into::into));
        self
    }

    /// The word in the border instead of the kind's (`copied`, `update`).
    pub fn word(mut self, word: impl Into<String>) -> Toast {
        self.word = Some(word.into());
        self
    }

    /// A toast with the same key replaces this one in a [`ToastQueue`]
    /// instead of queueing behind it (a newer refresh result).
    pub fn key(mut self, key: impl Into<String>) -> Toast {
        self.key = Some(key.into());
        self
    }

    /// How long it stays up once shown (default by kind: 8 s for attention
    /// and fail, else 5 s). `Duration::MAX` keeps it until dismissed.
    pub fn ttl(mut self, ttl: Duration) -> Toast {
        self.ttl = ttl;
        self
    }

    /// Body lines shown before `+N more` (at least 1; default 4).
    pub fn max_body(mut self, lines: usize) -> Toast {
        self.max_body = lines.max(1);
        self
    }

    /// Widest card in cells (default 56); never wider than its bounds.
    pub fn max_width(mut self, width: u16) -> Toast {
        self.max_width = width.max(MIN_CARD_WIDTH);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Toast {
        self.theme = theme;
        self
    }

    pub fn kind_of(&self) -> ToastKind {
        self.kind
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn body_lines(&self) -> &[String] {
        &self.body
    }

    pub fn key_of(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn ttl_of(&self) -> Duration {
        self.ttl
    }

    fn word_str(&self) -> &str {
        self.word.as_deref().unwrap_or(self.kind.word())
    }

    /// Cells the card asks for: the widest of the title (after the glyph),
    /// the body lines (indented), `+N more` and the border word, plus the
    /// borders and padding — herdr's `widest + 6`, measured in cells.
    fn natural_width(&self) -> u16 {
        let title = 2 + text::width(&self.title);
        let body = self
            .body
            .iter()
            .map(|l| INDENT.len() + text::width(l))
            .max()
            .unwrap_or(0);
        let more = if self.body.len() > self.max_body {
            INDENT.len() + more_text(self.body.len()).len()
        } else {
            0
        };
        let word = text::width(self.word_str());
        let content = title.max(body).max(more).max(word);
        let w = content + 2 + 2 * usize::from(PAD);
        u16::try_from(w)
            .unwrap_or(u16::MAX)
            .min(self.max_width)
            .max(MIN_CARD_WIDTH)
    }

    /// The card's size when drawn alone in `bounds`.
    pub fn size(&self, bounds: Rect) -> (u16, u16) {
        let w = self.natural_width().min(bounds.width);
        match self.plan_card(bounds, Anchor::TopLeft, w) {
            Some(p) => (p.rect.width, p.rect.height),
            None => (bounds.width, bounds.height.min(1)),
        }
    }

    /// A card of width `w` in `region`, the body cut to the rows there are.
    fn plan_card(&self, region: Rect, anchor: Anchor, w: u16) -> Option<Plan> {
        let w = w.min(region.width);
        if region.height < 3 || w < MIN_CARD_WIDTH {
            return None;
        }
        let cap = self.max_body.min(usize::from(region.height - 3));
        let n = self.body.len();
        let (rows, more) = if n <= cap {
            (n, 0)
        } else if cap == 0 {
            (0, n)
        } else {
            (cap - 1, n - (cap - 1))
        };
        let more_row = usize::from(more > 0 && cap > 0);
        let h = 3 + rows + more_row;
        Some(Plan {
            rect: anchor.place(region, w, h as u16),
            form: Form::Card,
            rows,
            more,
        })
    }

    fn plan_banner(region: Rect, anchor: Anchor) -> Option<Plan> {
        if region.is_empty() {
            return None;
        }
        let y = if anchor.is_top() {
            region.y
        } else {
            region.bottom() - 1
        };
        Some(Plan {
            rect: Rect::new(region.x, y, region.width, 1),
            form: Form::Banner,
            rows: 0,
            more: 0,
        })
    }

    /// The toast as one row in at most `width` cells: ` ✗ title · first body
    /// line +N more`. When it does not fit, the body is cut first, then the
    /// title, always with `…`. For a host that shows it in its own status
    /// row.
    pub fn banner_line(&self, width: u16) -> Line<'static> {
        self.banner_line_with(width, &self.theme)
    }

    fn banner_line_with(&self, width: u16, t: &Theme) -> Line<'static> {
        let w = usize::from(width);
        let mut head = Vec::with_capacity(4);
        if w >= 8 {
            head.push(Span::raw(" "));
        }
        head.push(Span::styled(
            self.kind.glyph(),
            self.kind.style(t).add_modifier(Modifier::BOLD),
        ));
        head.push(Span::raw(" "));
        head.push(Span::styled(
            self.title.clone(),
            t.text().add_modifier(Modifier::BOLD),
        ));
        let first = self.body.first().filter(|l| !l.is_empty());
        let tail = (self.body.len() > 1)
            .then(|| Span::styled(format!(" {}", more_text(self.body.len() - 1)), t.dim()));
        let head_w = text::spans_width(&head);
        let tail_w = tail.as_ref().map_or(0, |s| text::width(&s.content));
        let sep_w = text::width(SEP);
        let full = head_w + first.map_or(0, |b| sep_w + text::width(b)) + tail_w;
        let mut spans = head;
        match first {
            Some(b) if full <= w => {
                spans.push(Span::styled(SEP, t.dim()));
                spans.push(Span::styled(b.clone(), t.secondary()));
                spans.extend(tail);
            }
            // Cut the body, keep `+N more`.
            Some(b) if head_w + sep_w + 2 + tail_w <= w => {
                let room = w - head_w - sep_w - tail_w;
                spans.push(Span::styled(SEP, t.dim()));
                spans.push(Span::styled(
                    text::truncate(b, room).into_owned(),
                    t.secondary(),
                ));
                spans.extend(tail);
            }
            // No room for any body: say how many lines are hidden.
            Some(_) if head_w + 1 + more_text(self.body.len()).len() <= w => {
                spans.push(Span::styled(
                    format!(" {}", more_text(self.body.len())),
                    t.dim(),
                ));
            }
            Some(b) => {
                spans.push(Span::styled(SEP, t.dim()));
                spans.push(Span::styled(b.clone(), t.secondary()));
            }
            None => spans.extend(tail),
        }
        text::truncate_line(Line::from(spans), w)
    }

    /// Draw alone at `anchor`, `offset` rows in from the anchored edge (a
    /// header row above, a status row below), as a card or — on a narrow or
    /// short area — a banner. Always inside `bounds` and the buffer; when the
    /// offset leaves no room, the toast covers the offset rows rather than
    /// vanish. Returns the rect drawn (a click target).
    pub fn render_at(&self, bounds: Rect, anchor: Anchor, offset: u16, buf: &mut Buffer) -> Rect {
        ToastStack::new([self])
            .anchor(anchor)
            .offset(offset)
            .theme(self.theme)
            .render(bounds, buf)
            .toasts
            .first()
            .map_or_else(Rect::default, |(_, r)| *r)
    }

    /// The smallest offset, at least `base`, at which this card at `anchor`
    /// overlaps none of `taken` (other toasts, a menu) — herdr's copy-toast
    /// offset, for any number of rects. `None` when no offset clears them
    /// inside `bounds`.
    pub fn offset_clear_of(
        &self,
        bounds: Rect,
        anchor: Anchor,
        base: u16,
        taken: &[Rect],
    ) -> Option<u16> {
        offset_clear_of(bounds, anchor, self.size(bounds), base, taken)
    }

    fn draw(&self, plan: Plan, note: Option<&Note>, t: &Theme, buf: &mut Buffer) {
        let r = plan.rect.intersection(buf.area);
        if r.is_empty() {
            return;
        }
        Clear.render(r, buf);
        buf.set_style(r, Style::new().bg(t.surface_raised));
        match plan.form {
            Form::Banner => self.draw_banner(r, note, t, buf),
            Form::Card => self.draw_card(r, plan, note, t, buf),
        }
    }

    fn draw_banner(&self, r: Rect, note: Option<&Note>, t: &Theme, buf: &mut Buffer) {
        let w = usize::from(r.width);
        // The whole note beside at least a glyph and a few cells of title,
        // else its short form, so the count is not lost on a narrow row.
        let note = note.and_then(|n| {
            [format!(" {} ", n.long), format!(" {}", n.short)]
                .into_iter()
                .find(|s| text::width(s) + 4 <= w)
        });
        let note_w = note.as_deref().map_or(0, text::width);
        let line = self.banner_line_with((w - note_w) as u16, t);
        buf.set_line(r.x, r.y, &line, r.width);
        if let Some(n) = note {
            let x = r.right() - note_w as u16;
            buf.set_stringn(x, r.y, n, note_w, t.dim());
        }
    }

    fn draw_card(&self, r: Rect, plan: Plan, note: Option<&Note>, t: &Theme, buf: &mut Buffer) {
        let tone = self.kind.style(t);
        let border = if t.is_mono() {
            t.border()
        } else {
            tone.remove_modifier(Modifier::REVERSED)
        };
        let title_room = usize::from(r.width.saturating_sub(4));
        let mut block = Block::bordered().border_style(border);
        if title_room > 0 {
            let word = text::truncate(self.word_str(), title_room);
            block = block.title(Span::styled(
                format!(" {word} "),
                tone.add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(n) = note.filter(|_| title_room > 0) {
            let n = if text::width(&n.long) <= title_room {
                n.long.as_str()
            } else {
                n.short.as_str()
            };
            let n = text::truncate(n, title_room);
            block = block
                .title_bottom(Line::from(Span::styled(format!(" {n} "), t.dim())).right_aligned());
        }
        let inner = block.inner(r);
        block.render(r, buf);
        let content = Rect {
            x: inner.x + PAD,
            width: inner.width.saturating_sub(2 * PAD),
            ..inner
        };
        if content.is_empty() {
            return;
        }
        let cw = usize::from(content.width);
        // Title row; when no body row fits, it says how many lines it hides.
        let suffix = (plan.more > 0 && plan.rows == 0 && plan.rect.height == 3)
            .then(|| {
                [
                    format!(" {}", more_text(plan.more)),
                    format!(" +{}", plan.more),
                ]
                .into_iter()
                .find(|s| 2 + 1 + text::width(s) <= cw)
            })
            .flatten();
        let suffix_w = suffix.as_deref().map_or(0, text::width);
        let title_w = cw.saturating_sub(2 + suffix_w);
        let mut spans = vec![
            Span::styled(self.kind.glyph(), tone.add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(
                text::truncate(&self.title, title_w).into_owned(),
                t.text().add_modifier(Modifier::BOLD),
            ),
        ];
        if let Some(s) = suffix {
            spans.push(Span::styled(s, t.dim()));
        }
        let line = text::truncate_line(Line::from(spans), cw);
        buf.set_line(content.x, content.y, &line, content.width);
        let mut y = content.y + 1;
        let body_w = cw.saturating_sub(INDENT.len());
        for l in self.body.iter().take(plan.rows) {
            if y >= content.bottom() {
                return;
            }
            let line = Line::from(vec![
                Span::raw(INDENT),
                Span::styled(text::truncate(l, body_w).into_owned(), t.secondary()),
            ]);
            buf.set_line(content.x, y, &line, content.width);
            y += 1;
        }
        if plan.more > 0 && y < content.bottom() {
            let line = Line::from(vec![
                Span::raw(INDENT),
                Span::styled(
                    text::truncate(&more_text(plan.more), body_w).into_owned(),
                    t.dim(),
                ),
            ]);
            buf.set_line(content.x, y, &line, content.width);
        }
    }
}

impl Widget for &Toast {
    /// Draws at the default anchor (top right) with no offset.
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_at(area, Anchor::default(), 0, buf);
    }
}

impl Widget for Toast {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

fn more_text(n: usize) -> String {
    format!("+{n} more")
}

/// What the last toast of a stack says about the others: `+2 more · 1
/// dropped`, or `+3` where that does not fit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Note {
    long: String,
    short: String,
}

/// The smallest offset, at least `base`, at which a `size` rect at `anchor`
/// in `bounds` overlaps none of `taken`; `None` when none clears them inside
/// `bounds`. herdr moved its copy toast past one notification card
/// (`copy_feedback_offset_for_toast`); this steps past any number.
pub fn offset_clear_of(
    bounds: Rect,
    anchor: Anchor,
    size: (u16, u16),
    base: u16,
    taken: &[Rect],
) -> Option<u16> {
    let (w, h) = (size.0.min(bounds.width), size.1.max(1));
    let mut off = base;
    loop {
        let region = anchor.region(bounds, off);
        if region.height < h || region.width == 0 {
            return None;
        }
        let rect = anchor.place(region, w, h);
        let Some(hit) = taken.iter().find(|t| t.intersects(rect)) else {
            return Some(off);
        };
        let next = if anchor.is_top() {
            hit.bottom().saturating_sub(bounds.y)
        } else {
            bounds.bottom().saturating_sub(hit.y)
        };
        if next <= off {
            return None;
        }
        off = next;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    rect: Rect,
    form: Form,
    /// Body lines shown.
    rows: usize,
    /// Body lines dropped (said as `+N more`).
    more: usize,
}

/// Where a [`ToastStack`] drew: one rect per toast drawn, as `(index into
/// the toasts given, rect)` — click targets that never point at a toast that
/// is not on screen.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StackLayout {
    pub toasts: Vec<(usize, Rect)>,
    /// Cards or banners.
    pub form: Option<Form>,
    /// Toasts not drawn: those that did not fit plus the queued ones.
    pub hidden: usize,
}

impl StackLayout {
    /// The toast under `(col, row)`.
    pub fn hit_test(&self, col: u16, row: u16) -> Option<usize> {
        let p = ratatui::layout::Position::new(col, row);
        self.toasts
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(i, _)| *i)
    }
}

/// Several toasts at one anchor, the first nearest the anchored edge and each
/// next one past it, so none overlap. Cards in a stack share one width. What
/// does not fit is not drawn but counted on the last card's bottom border
/// (`+2 more · 1 dropped`), with the toasts still queued and those the queue
/// dropped.
#[derive(Debug, Clone)]
pub struct ToastStack<'a> {
    toasts: Vec<&'a Toast>,
    anchor: Anchor,
    offset: u16,
    gap: u16,
    narrow: u16,
    queued: usize,
    dropped: usize,
    theme: Theme,
}

impl<'a> ToastStack<'a> {
    pub fn new(toasts: impl IntoIterator<Item = &'a Toast>) -> ToastStack<'a> {
        ToastStack {
            toasts: toasts.into_iter().collect(),
            anchor: Anchor::default(),
            offset: 0,
            gap: 0,
            narrow: DEFAULT_NARROW_WIDTH,
            queued: 0,
            dropped: 0,
            theme: Theme::default(),
        }
    }

    pub fn anchor(mut self, anchor: Anchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Rows kept clear at the anchored edge (a header, a status row).
    pub fn offset(mut self, rows: u16) -> Self {
        self.offset = rows;
        self
    }

    /// Blank rows between two toasts (default 0).
    pub fn gap(mut self, rows: u16) -> Self {
        self.gap = rows;
        self
    }

    /// At or below this many columns the stack is banners (default 64,
    /// herdr's narrow layout); 0 keeps cards at any width that fits one.
    pub fn narrow_width(mut self, cols: u16) -> Self {
        self.narrow = cols;
        self
    }

    /// Toasts waiting in a queue, counted in `+N more`.
    pub fn queued(mut self, n: usize) -> Self {
        self.queued = n;
        self
    }

    /// Toasts a queue dropped, said as `N dropped`.
    pub fn dropped(mut self, n: usize) -> Self {
        self.dropped = n;
        self
    }

    /// Draws every toast in the stack (their own themes are not used).
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    fn plan(&self, bounds: Rect) -> (Vec<(usize, Plan)>, Option<Form>) {
        let mut out = Vec::new();
        if bounds.is_empty() || self.toasts.is_empty() {
            return (out, None);
        }
        let mut region = self.anchor.region(bounds, self.offset);
        if region.is_empty() {
            // No row past the offset: cover it rather than lose the message.
            region = bounds;
        }
        // Wide enough for the note too, at the most it could ever count.
        let note_w = self
            .note(self.toasts.len() + self.queued)
            .map_or(0, |n| text::width(&n.long) + 4);
        let width = self
            .toasts
            .iter()
            .map(|t| t.natural_width())
            .max()
            .unwrap_or(MIN_CARD_WIDTH)
            .max(
                u16::try_from(note_w)
                    .unwrap_or(u16::MAX)
                    .min(DEFAULT_MAX_WIDTH),
            )
            .min(region.width);
        let narrow = self.narrow > 0 && bounds.width <= self.narrow;
        let form = if narrow || region.height < 3 || width < MIN_CARD_WIDTH {
            Form::Banner
        } else {
            Form::Card
        };
        for (i, t) in self.toasts.iter().enumerate() {
            let plan = match form {
                Form::Card => t.plan_card(region, self.anchor, width),
                Form::Banner => Toast::plan_banner(region, self.anchor),
            };
            let Some(plan) = plan else { break };
            out.push((i, plan));
            let used = if self.anchor.is_top() {
                plan.rect.bottom() - region.y
            } else {
                region.bottom() - plan.rect.y
            };
            let used = used.saturating_add(self.gap).min(region.height);
            region = if self.anchor.is_top() {
                Rect {
                    y: region.y + used,
                    height: region.height - used,
                    ..region
                }
            } else {
                Rect {
                    height: region.height - used,
                    ..region
                }
            };
        }
        (out, Some(form))
    }

    /// Where each toast would go, without drawing.
    pub fn layout(&self, bounds: Rect) -> StackLayout {
        let (plans, form) = self.plan(bounds);
        StackLayout {
            hidden: self.toasts.len() - plans.len() + self.queued,
            toasts: plans.into_iter().map(|(i, p)| (i, p.rect)).collect(),
            form,
        }
    }

    /// The note on the last toast: `+2 more · 1 dropped`.
    fn note(&self, hidden: usize) -> Option<Note> {
        let mut parts = Vec::new();
        if hidden > 0 {
            parts.push(more_text(hidden));
        }
        if self.dropped > 0 {
            parts.push(format!("{} dropped", self.dropped));
        }
        let long = parts.join(SEP);
        let short = if hidden > 0 {
            format!("+{hidden}")
        } else {
            long.clone()
        };
        (!parts.is_empty()).then_some(Note { long, short })
    }

    /// Draw the stack inside `bounds` (and the buffer) and return where each
    /// toast went.
    pub fn render(&self, bounds: Rect, buf: &mut Buffer) -> StackLayout {
        let bounds = bounds.intersection(buf.area);
        let (plans, form) = self.plan(bounds);
        let hidden = self.toasts.len() - plans.len() + self.queued;
        let note = self.note(hidden);
        let last = plans.len().saturating_sub(1);
        for (n, (i, plan)) in plans.iter().enumerate() {
            let note = if n == last { note.as_ref() } else { None };
            self.toasts[*i].draw(*plan, note, &self.theme, buf);
        }
        StackLayout {
            toasts: plans.into_iter().map(|(i, p)| (i, p.rect)).collect(),
            form,
            hidden,
        }
    }
}

/// `frame.render_widget(&stack, area)`. Only by reference, so that
/// `stack.render(area, buf)` is always the method that returns the layout.
impl Widget for &ToastStack<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        ToastStack::render(self, area, buf);
    }
}

/// Which toast a [`ToastQueue`] handed out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToastId(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Up {
    id: ToastId,
    toast: Toast,
    since: Duration,
}

impl Up {
    fn until(&self) -> Duration {
        self.since.saturating_add(self.toast.ttl)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Waiting {
    id: ToastId,
    toast: Toast,
    pushed: Duration,
}

/// Which toasts are up: first in, first out, at most `max_visible` at once,
/// each for its own time to live from the moment it is shown. When more
/// than `cap` wait, the oldest waiting one is dropped and counted; the count
/// is shown on the stack until the queue is empty again.
///
/// Time is the host's `Duration` since it started (`now`), never read here.
/// A waiting toast is shown at the instant a slot freed, not when `tick`
/// happens to run, so the queue at `now` is the same however often the host
/// ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToastQueue {
    up: Vec<Up>,
    waiting: VecDeque<Waiting>,
    cap: usize,
    max_visible: usize,
    dropped: usize,
    next: u64,
}

impl Default for ToastQueue {
    fn default() -> Self {
        ToastQueue::new()
    }
}

impl ToastQueue {
    /// Cap 8 waiting, 3 up at once.
    pub fn new() -> ToastQueue {
        ToastQueue {
            up: Vec::new(),
            waiting: VecDeque::new(),
            cap: DEFAULT_CAP,
            max_visible: DEFAULT_MAX_VISIBLE,
            dropped: 0,
            next: 0,
        }
    }

    /// Toasts that may wait before the oldest is dropped (default 8; 0
    /// drops whatever cannot be shown at once).
    pub fn cap(mut self, cap: usize) -> Self {
        self.cap = cap;
        self
    }

    /// Toasts up at once (at least 1; default 3). herdr showed one.
    pub fn max_visible(mut self, n: usize) -> Self {
        self.max_visible = n.max(1);
        self
    }

    /// Add a toast at `now`. It is shown at once if a slot is free, else it
    /// waits. A toast with the same [`Toast::key`] as one up replaces it in
    /// place (its time starts again); one waiting is replaced, not counted
    /// as dropped.
    pub fn push(&mut self, toast: Toast, now: Duration) -> ToastId {
        self.tick(now);
        let id = ToastId(self.next);
        self.next += 1;
        if let Some(key) = toast.key.clone() {
            self.waiting
                .retain(|w| w.toast.key.as_deref() != Some(key.as_str()));
            if let Some(u) = self
                .up
                .iter_mut()
                .find(|u| u.toast.key.as_deref() == Some(key.as_str()))
            {
                *u = Up {
                    id,
                    toast,
                    since: now,
                };
                return id;
            }
        }
        if self.up.len() < self.max_visible {
            self.up.push(Up {
                id,
                toast,
                since: now,
            });
            return id;
        }
        if self.cap == 0 {
            self.dropped += 1;
            return id;
        }
        if self.waiting.len() >= self.cap {
            self.waiting.pop_front();
            self.dropped += 1;
        }
        self.waiting.push_back(Waiting {
            id,
            toast,
            pushed: now,
        });
        id
    }

    /// Retire every toast whose time is up by `now`, showing waiting ones in
    /// their place from the instant each slot freed. Returns whether
    /// anything changed (redraw).
    pub fn tick(&mut self, now: Duration) -> bool {
        let mut changed = false;
        while let Some((i, end)) = self
            .up
            .iter()
            .enumerate()
            .map(|(i, u)| (i, u.until()))
            .filter(|(_, end)| *end <= now)
            .min_by_key(|(_, end)| *end)
        {
            self.up.remove(i);
            self.promote(end);
            changed = true;
        }
        self.settle() || changed
    }

    fn promote(&mut self, at: Duration) {
        while self.up.len() < self.max_visible {
            let Some(w) = self.waiting.pop_front() else {
                break;
            };
            self.up.push(Up {
                id: w.id,
                toast: w.toast,
                since: at.max(w.pushed),
            });
        }
    }

    /// Forget the drop count once nothing is up or waiting.
    fn settle(&mut self) -> bool {
        if self.up.is_empty() && self.waiting.is_empty() && self.dropped > 0 {
            self.dropped = 0;
            return true;
        }
        false
    }

    /// Take a toast down (clicked, or acted on) and show the next.
    pub fn dismiss(&mut self, id: ToastId, now: Duration) -> bool {
        let before = self.up.len() + self.waiting.len();
        self.up.retain(|u| u.id != id);
        self.waiting.retain(|w| w.id != id);
        let gone = before != self.up.len() + self.waiting.len();
        if gone {
            self.promote(now);
            self.settle();
        }
        gone
    }

    /// Drop everything, the drop count included.
    pub fn clear(&mut self) {
        self.up.clear();
        self.waiting.clear();
        self.dropped = 0;
    }

    /// The toasts up at `now`, first shown first. Call [`ToastQueue::tick`]
    /// first so waiting toasts move up.
    pub fn visible(&self, now: Duration) -> Vec<&Toast> {
        self.entries(now).map(|(_, t, _)| t).collect()
    }

    /// The toasts up at `now` with their ids and the time each has left.
    pub fn entries(&self, now: Duration) -> impl Iterator<Item = (ToastId, &Toast, Duration)> {
        self.up
            .iter()
            .filter(move |u| u.until() > now)
            .map(move |u| (u.id, &u.toast, u.until().saturating_sub(now)))
    }

    /// Toasts waiting for a slot.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Toasts dropped since the queue was last empty.
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    pub fn is_empty(&self) -> bool {
        self.up.is_empty() && self.waiting.is_empty()
    }

    /// When the next toast goes down — the host's next wake-up for a
    /// redraw. `None` when nothing is up or every toast stays.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.up
            .iter()
            .map(Up::until)
            .filter(|d| *d != Duration::MAX)
            .min()
    }

    /// The toasts up at `now` as a stack that also counts the waiting and
    /// the dropped ones. Set its anchor and theme, then render it.
    pub fn stack(&self, now: Duration) -> ToastStack<'_> {
        ToastStack::new(self.entries(now).map(|(_, t, _)| t))
            .queued(self.waiting.len())
            .dropped(self.dropped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthStr;

    const S: fn(u64) -> Duration = Duration::from_secs;

    fn filled(area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        for c in buf.content.iter_mut() {
            c.set_symbol("X");
        }
        buf
    }

    fn inside(r: Rect, b: Rect) -> bool {
        r.is_empty()
            || (r.x >= b.x && r.y >= b.y && r.right() <= b.right() && r.bottom() <= b.bottom())
    }

    /// Every cell outside `bounds` still holds the `X` it started with.
    fn untouched_outside(buf: &Buffer, bounds: Rect) -> bool {
        let a = buf.area;
        (a.top()..a.bottom()).all(|y| {
            (a.left()..a.right()).all(|x| {
                bounds.contains(ratatui::layout::Position::new(x, y)) || buf[(x, y)].symbol() == "X"
            })
        })
    }

    fn refresh_failed() -> Toast {
        Toast::fail("usage refresh: 3 of 5 failed").lines([
            "ai15@insightquest.io  401 token expired",
            "ai7@insightquest.io   timeout after 10s",
            "codex:icedac@gmail.com  connection refused",
        ])
    }

    /// herdr H1: a 100×4 terminal with a diagnostic row on top (offset 1)
    /// and a top copy toast panicked with `index outside of buffer`. Here it
    /// stays inside, in every top position and in herdr's exact pane rect.
    #[test]
    fn herdr_h1_short_area_with_offset_stays_inside() {
        let toast = Toast::ok("copied to clipboard");
        for anchor in [Anchor::TopLeft, Anchor::TopCenter, Anchor::TopRight] {
            let full = Rect::new(0, 0, 100, 4);
            let mut buf = filled(full);
            let r = toast.render_at(full, anchor, 1, &mut buf);
            assert!(!r.is_empty() && inside(r, full), "{anchor:?} {r:?}");
            assert!(r.y >= 1, "keeps the diagnostic row: {r:?}");
            // herdr's pane surface under a tab bar: 26,1 74×3 in a 100×4 frame.
            let pane = Rect::new(26, 1, 74, 3);
            let mut buf = filled(full);
            let r = toast.render_at(pane, anchor, 1, &mut buf);
            assert!(!r.is_empty() && inside(r, pane), "{anchor:?} {r:?}");
            assert!(untouched_outside(&buf, pane));
            assert!(snapshot::text(&buf).contains("copied to clipboard"));
            // 100×8 with a 4-row card already up (offset 5).
            let tall = Rect::new(0, 0, 100, 8);
            let mut buf = filled(tall);
            let r = toast.render_at(tall, anchor, 5, &mut buf);
            assert!(!r.is_empty() && inside(r, tall), "{anchor:?} {r:?}");
        }
    }

    /// herdr `notification_rect_stays_inside_short_nonzero_area`.
    #[test]
    fn rect_stays_inside_short_nonzero_area() {
        for height in [1, 2] {
            let area = Rect::new(3, 4, 8, height);
            for anchor in [Anchor::TopRight, Anchor::BottomRight] {
                let mut buf = filled(Rect::new(0, 0, 20, 10));
                let r = Toast::info("notice").render_at(area, anchor, 1, &mut buf);
                assert!(r.y >= area.y, "{r:?}");
                assert!(r.bottom() <= area.bottom(), "{r:?}");
                assert!(untouched_outside(&buf, area));
            }
        }
    }

    /// herdr `copy_feedback_rect_uses_configured_position`.
    #[test]
    fn centre_positions_sit_on_their_edge() {
        let area = Rect::new(10, 20, 100, 40);
        let t = Toast::ok("copied to clipboard");
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 70));
        let top = t.render_at(area, Anchor::TopCenter, 0, &mut buf);
        assert_eq!(top.y, area.y);
        assert_eq!(top.x, area.x + (area.width - top.width) / 2);
        let bottom = t.render_at(area, Anchor::BottomCenter, 0, &mut buf);
        assert_eq!(bottom.bottom(), area.bottom());
        assert_eq!(bottom.x, area.x + (area.width - bottom.width) / 2);
        let right = t.render_at(area, Anchor::BottomRight, 2, &mut buf);
        assert_eq!(
            (right.right(), right.bottom()),
            (area.right(), area.bottom() - 2)
        );
        // herdr's width: the widest of title and body plus 6, in cells.
        assert_eq!(top.width, "copied to clipboard".len() as u16 + 6);
        assert_eq!(top.height, 3);
    }

    /// herdr `mobile_notification_is_a_bottom_banner_with_released_title`:
    /// narrow, it is one full-width row above the warning row.
    #[test]
    fn narrow_area_gets_a_bottom_banner() {
        let area = Rect::new(0, 0, 44, 20);
        let mut buf = filled(area);
        let t = Toast::attention("pi waiting").body("workspace · tab 1");
        let r = t.render_at(area, Anchor::BottomCenter, 1, &mut buf);
        assert_eq!(r, Rect::new(0, 18, 44, 1));
        let row = &snapshot::lines(&buf)[18];
        assert!(row.contains("◉ pi waiting · workspace · tab 1"), "{row}");
        assert!((0..44).all(|x| buf[(x, 18)].symbol() != "X"));
        // Top anchors put the banner on the first row past the offset.
        let mut buf = filled(area);
        let r = t.render_at(area, Anchor::TopLeft, 2, &mut buf);
        assert_eq!(r, Rect::new(0, 2, 44, 1));
    }

    #[test]
    fn card_says_its_kind_in_a_glyph_and_a_word() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 8));
        let area = buf.area;
        refresh_failed()
            .theme(Theme::mono())
            .render_at(area, Anchor::TopRight, 0, &mut buf);
        let lines = snapshot::lines(&buf);
        assert!(lines[0].contains("┌ fail ─"), "{lines:?}");
        assert!(
            lines[1].contains("│ ✗ usage refresh: 3 of 5 failed"),
            "{lines:?}"
        );
        assert!(
            lines[2].contains("│   ai15@insightquest.io  401 token expired"),
            "{lines:?}"
        );
        assert!(lines[4].contains("codex:icedac@gmail.com  connection refused"));
        assert!(lines[5].contains("└───"), "{lines:?}");
        // Right-anchored: the border ends in the last column.
        assert!(lines[0].ends_with('┐'), "{lines:?}");
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
        }
    }

    #[test]
    fn kinds_have_distinct_narrow_glyphs_and_words() {
        let mut glyphs = std::collections::HashSet::new();
        let mut words = std::collections::HashSet::new();
        for k in ToastKind::ALL {
            assert_eq!(k.glyph().width_cjk(), 1, "{k:?} {}", k.glyph());
            assert_eq!(text::width(k.glyph()), 1);
            assert!(glyphs.insert(k.glyph()), "{k:?}");
            assert!(words.insert(k.word()), "{k:?}");
        }
        // Mono: every kind draws a different first row.
        let rows: std::collections::HashSet<String> = ToastKind::ALL
            .iter()
            .map(|k| {
                let mut buf = Buffer::empty(Rect::new(0, 0, 70, 3));
                let area = buf.area;
                Toast::new(*k, "same title").theme(Theme::mono()).render_at(
                    area,
                    Anchor::TopLeft,
                    0,
                    &mut buf,
                );
                snapshot::text(&buf)
            })
            .collect();
        assert_eq!(rows.len(), ToastKind::ALL.len());
        // Fail stands out by weight in mono.
        let s = ToastKind::Fail.style(&Theme::mono());
        assert!(s.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn long_bodies_say_how_many_lines_are_dropped() {
        let t = Toast::fail("refresh: 6 of 29 failed")
            .lines((1..=6).map(|i| format!("ai{i}@iq.io  401")))
            .theme(Theme::ansi());
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 12));
        let area = buf.area;
        let r = t.render_at(area, Anchor::TopLeft, 0, &mut buf);
        let lines = snapshot::lines(&buf);
        // Four body rows: three lines and `+3 more`.
        assert_eq!(r.height, 2 + 1 + 4);
        assert!(lines[4].contains("ai3@iq.io"), "{lines:?}");
        assert!(lines[5].contains("+3 more"), "{lines:?}");
        // A short area keeps fewer lines and counts the rest.
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 5));
        let area = buf.area;
        t.render_at(area, Anchor::TopLeft, 0, &mut buf);
        let lines = snapshot::lines(&buf);
        assert!(lines[2].contains("ai1@iq.io"), "{lines:?}");
        assert!(lines[3].contains("+5 more"), "{lines:?}");
        // Three rows: the title row says it.
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 3));
        let area = buf.area;
        t.render_at(area, Anchor::TopLeft, 0, &mut buf);
        assert!(
            snapshot::lines(&buf)[1].contains("+6 more"),
            "{:?}",
            snapshot::lines(&buf)
        );
        // The banner says it too.
        let line = t.banner_line(60);
        let s: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(s, " ✗ refresh: 6 of 29 failed · ai1@iq.io  401 +5 more");
        let narrow: String = t
            .banner_line(40)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(narrow.ends_with("… +5 more"), "{narrow}");
        assert_eq!(text::width(&narrow), 40);
        // Narrower: the body goes, its count stays.
        let narrower: String = t
            .banner_line(34)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(narrower, " ✗ refresh: 6 of 29 failed +6 more");
    }

    #[test]
    fn titles_and_bodies_are_cut_by_grapheme_with_an_ellipsis() {
        let t = Toast::warn("한국어 제목이 아주 길어서 잘립니다 👩‍💻👩‍💻👩‍💻")
            .body("🇰🇷 team 👍🏽 ok — a very long line that cannot fit in the card")
            .max_width(24)
            .theme(Theme::ansi());
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 5));
        let area = buf.area;
        let r = t.render_at(area, Anchor::TopLeft, 0, &mut buf);
        assert_eq!(r.width, 24);
        let lines = snapshot::lines(&buf);
        assert!(
            lines[1].contains('…') && lines[2].contains('…'),
            "{lines:?}"
        );
        for l in &lines {
            assert!(UnicodeWidthStr::width(l.as_str()) <= 24, "{l}");
            assert!(!l.contains('\u{200d}') || l.contains("👩‍💻"), "{l}");
        }
        for w in 0..60u16 {
            assert!(text::line_width(&t.banner_line(w)) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn a_stack_does_not_overlap_and_counts_what_does_not_fit() {
        let a = Toast::attention("claude needs attention").body("repo · tab 2");
        let b = Toast::ok("copied to clipboard");
        let c = refresh_failed();
        let toasts = [a, b, c];
        for anchor in Anchor::ALL {
            let mut buf = Buffer::empty(Rect::new(0, 0, 90, 30));
            let area = buf.area;
            let l = ToastStack::new(&toasts)
                .anchor(anchor)
                .offset(1)
                .gap(1)
                .theme(Theme::ansi())
                .render(area, &mut buf);
            assert_eq!(l.toasts.len(), 3, "{anchor:?}");
            assert_eq!(l.hidden, 0);
            for (n, (_, r)) in l.toasts.iter().enumerate() {
                assert!(inside(*r, area));
                for (_, s) in &l.toasts[n + 1..] {
                    assert!(!r.intersects(*s), "{anchor:?} {r:?} {s:?}");
                }
            }
            // One width for the whole stack.
            assert!(l.toasts.iter().all(|(_, r)| r.width == l.toasts[0].1.width));
            // The first is nearest the anchored edge.
            let ys: Vec<u16> = l.toasts.iter().map(|(_, r)| r.y).collect();
            if anchor.is_top() {
                assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
            } else {
                assert!(ys.windows(2).all(|w| w[0] > w[1]), "{ys:?}");
            }
        }
        // Nine rows: two fit, the third and two queued are counted.
        let mut buf = Buffer::empty(Rect::new(0, 0, 90, 9));
        let area = buf.area;
        let l = ToastStack::new(&toasts)
            .queued(2)
            .dropped(1)
            .theme(Theme::mono())
            .render(area, &mut buf);
        assert_eq!(l.toasts.len(), 2);
        assert_eq!(l.hidden, 3);
        let text = snapshot::text(&buf);
        assert!(text.contains(" +3 more · 1 dropped ┘"), "{text}");
        assert_eq!(l.hit_test(89, 0), Some(0));
    }

    #[test]
    fn offset_clear_of_steps_past_every_taken_rect() {
        let bounds = Rect::new(0, 0, 80, 20);
        let t = Toast::ok("copied to clipboard");
        let card = Rect::new(50, 0, 30, 4);
        let flag = Rect::new(60, 4, 20, 1);
        // herdr: past the notification card.
        assert_eq!(
            t.offset_clear_of(bounds, Anchor::TopRight, 0, &[card]),
            Some(4)
        );
        assert_eq!(
            t.offset_clear_of(bounds, Anchor::TopRight, 0, &[card, flag]),
            Some(5)
        );
        // Nothing in the way, or on the other side: the base stays.
        assert_eq!(
            t.offset_clear_of(bounds, Anchor::TopLeft, 1, &[card]),
            Some(1)
        );
        assert_eq!(
            t.offset_clear_of(bounds, Anchor::BottomRight, 0, &[card]),
            Some(0)
        );
        // No room left.
        let wall = Rect::new(0, 0, 80, 19);
        assert_eq!(
            t.offset_clear_of(bounds, Anchor::TopRight, 0, &[wall]),
            None
        );
    }

    /// herdr `concurrent_endpoint_notifications_are_queued_in_arrival_order`.
    #[test]
    fn queue_shows_in_arrival_order() {
        let mut q = ToastQueue::new().max_visible(1);
        let now = S(100);
        q.push(Toast::info("first"), now);
        q.push(Toast::info("second"), now);
        assert_eq!(q.visible(now)[0].title(), "first");
        assert_eq!(q.waiting(), 1);
        assert!(q.tick(now + S(5)));
        assert_eq!(q.visible(now + S(5))[0].title(), "second");
        assert_eq!(q.waiting(), 0);
    }

    /// herdr's cap: the oldest waiting toast goes, and the queue says so.
    #[test]
    fn a_full_queue_drops_the_oldest_waiting_and_counts_it() {
        let mut q = ToastQueue::new().max_visible(1).cap(2);
        for (i, t) in ["a", "b", "c", "d", "e"].iter().enumerate() {
            q.push(Toast::info(*t), Duration::from_millis(i as u64));
        }
        assert_eq!(q.visible(S(0))[0].title(), "a");
        assert_eq!(q.waiting(), 2);
        assert_eq!(q.dropped(), 2, "b and c were dropped");
        let mut buf = Buffer::empty(Rect::new(0, 0, 80, 6));
        let area = buf.area;
        q.stack(S(0)).render(area, &mut buf);
        assert!(snapshot::text(&buf).contains("+2 more · 2 dropped"));
        q.tick(S(5));
        assert_eq!(q.visible(S(5))[0].title(), "d");
        q.tick(S(60));
        assert!(q.is_empty());
        assert_eq!(q.dropped(), 0, "the count goes once the queue is empty");
        // Cap 0: what cannot be shown at once is dropped.
        let mut q = ToastQueue::new().max_visible(1).cap(0);
        q.push(Toast::info("a"), S(0));
        q.push(Toast::info("b"), S(0));
        assert_eq!((q.waiting(), q.dropped()), (0, 1));
    }

    #[test]
    fn ttl_is_per_toast_and_ticks_do_not_change_the_outcome() {
        let script = || {
            vec![
                (S(0), Toast::ok("copied").ttl(S(2))),
                (S(0), Toast::fail("refresh failed")),
                (S(1), Toast::attention("claude waiting")),
                (S(3), Toast::info("update ready").ttl(S(3))),
            ]
        };
        let titles = |q: &ToastQueue, now| {
            q.visible(now)
                .iter()
                .map(|t| t.title().to_string())
                .collect::<Vec<_>>()
        };
        for now_ms in (0..20_000).step_by(250) {
            let now = Duration::from_millis(now_ms);
            // Ticked only at pushes and at `now` …
            let mut coarse = ToastQueue::new().max_visible(2);
            // … and ticked every 10 ms.
            let mut fine = ToastQueue::new().max_visible(2);
            let mut t = Duration::ZERO;
            for (at, toast) in script() {
                if at > now {
                    break;
                }
                while t < at {
                    fine.tick(t);
                    t += Duration::from_millis(10);
                }
                coarse.push(toast.clone(), at);
                fine.push(toast, at);
            }
            while t < now {
                fine.tick(t);
                t += Duration::from_millis(10);
            }
            coarse.tick(now);
            fine.tick(now);
            assert_eq!(titles(&coarse, now), titles(&fine, now), "{now:?}");
        }
        let mut q = ToastQueue::new().max_visible(2);
        for (at, toast) in script() {
            q.push(toast, at);
        }
        q.tick(S(3));
        // `copied` left at 2 s; `claude waiting` moved up at 2 s, not 3 s.
        assert_eq!(titles(&q, S(3)), ["refresh failed", "claude waiting"]);
        let left: Vec<Duration> = q.entries(S(3)).map(|(_, _, l)| l).collect();
        assert_eq!(left, [S(5), S(7)]);
        assert_eq!(q.next_deadline(), Some(S(8)));
        q.tick(S(8));
        assert_eq!(titles(&q, S(8)), ["claude waiting", "update ready"]);
    }

    #[test]
    fn same_key_replaces_in_place_and_dismiss_moves_the_next_up() {
        let mut q = ToastQueue::new().max_visible(1);
        q.push(
            Toast::progress("refreshing 5 accounts").key("refresh"),
            S(0),
        );
        let waiting = q.push(Toast::info("update ready"), S(0));
        let id = q.push(Toast::ok("refreshed 5 accounts").key("refresh"), S(2));
        assert_eq!(q.visible(S(2))[0].title(), "refreshed 5 accounts");
        assert_eq!(q.entries(S(2)).next().map(|e| e.2), Some(S(5)));
        assert_eq!((q.waiting(), q.dropped()), (1, 0));
        assert!(q.dismiss(id, S(3)));
        assert_eq!(q.visible(S(3))[0].title(), "update ready");
        assert!(q.dismiss(waiting, S(3)));
        assert!(q.is_empty());
        assert!(!q.dismiss(id, S(3)));
    }

    #[test]
    fn tiny_sizes_never_panic_or_draw_outside() {
        let toasts = [
            refresh_failed(),
            Toast::attention("claude needs attention").body("repo · tab 2"),
            Toast::ok("copied"),
        ];
        let frame = Rect::new(0, 0, 16, 9);
        for w in 0..=14u16 {
            for h in 0..=7u16 {
                let bounds = Rect::new(1, 1, w, h);
                for anchor in Anchor::ALL {
                    for offset in [0, 1, 3, 8] {
                        for narrow in [0, DEFAULT_NARROW_WIDTH] {
                            let mut buf = filled(frame);
                            let l = ToastStack::new(&toasts)
                                .anchor(anchor)
                                .offset(offset)
                                .narrow_width(narrow)
                                .gap(1)
                                .render(bounds, &mut buf);
                            assert!(untouched_outside(&buf, bounds), "{w}×{h} {anchor:?}");
                            for (_, r) in &l.toasts {
                                assert!(inside(*r, bounds), "{r:?} in {bounds:?}");
                            }
                            if !bounds.is_empty() {
                                assert!(!l.toasts.is_empty(), "{w}×{h} {anchor:?} {offset}");
                            }
                        }
                    }
                }
            }
        }
        // Bounds past the buffer are clipped to it.
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        let r = Toast::ok("x").render_at(Rect::new(5, 1, 50, 50), Anchor::BottomRight, 0, &mut buf);
        assert!(inside(r, buf.area), "{r:?}");
    }

    #[test]
    fn mono_stack_has_no_colour_and_kinds_from_outcomes_and_status() {
        assert_eq!(ToastKind::from(Outcome::Fail), ToastKind::Fail);
        assert_eq!(ToastKind::from(Status::Waiting), ToastKind::Attention);
        let toasts: Vec<Toast> = ToastKind::ALL
            .iter()
            .map(|k| Toast::new(*k, k.word()))
            .collect();
        for theme in [Theme::mono(), Theme::ansi(), Theme::dark()] {
            let mut buf = Buffer::empty(Rect::new(0, 0, 80, 30));
            let area = buf.area;
            ToastStack::new(&toasts).theme(theme).render(area, &mut buf);
            if theme.is_mono() {
                for c in buf.content() {
                    assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{c:?}");
                }
            }
        }
    }
}
