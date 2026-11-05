// Derived from OpenAI Codex (Apache-2.0), commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72:
//   codex-rs/tui/src/status_indicator_widget.rs         the status row, fmt_elapsed_compact
//   codex-rs/tui/src/status_indicator_widget/timer.rs   the pause-aware timer
// Modified by 2lab: no app-event sender, frame requester or `Instant` — the
// host passes elapsed time and a paused flag (or a `WorkTimer` read at its own
// clock); narrow glyphs by default (braille spinner, ` - `, `↳`) with codex's
// `•` / `·` / `└` behind an opt-in; paused, done and failed phases; the theme
// supplies every colour.

//! What the agent is doing now, and for how long:
//! `⠋ Working (12s - esc to interrupt)`.
//!
//! Ported from OpenAI Codex `codex-rs/tui/src/status_indicator_widget.rs`
//! (Apache-2.0) — the row above the composer while a turn runs — with its
//! pause-aware timer. Web sibling: `<ui-working>`.
//!
//! One row: a braille spinner, the header as a [`Shimmer`], then the elapsed
//! time and the key that interrupts, then an optional inline note. Beneath it,
//! an optional detail wrapped by words in cells, at most three lines, the
//! last ending in `…` when more was cut. A row that does not fit is clipped
//! with `…` by cells.
//!
//! The host owns time: it passes `elapsed` (or a [`WorkTimer`] and its clock
//! reading) and `t` for the animation. A paused turn — waiting on the person
//! for an approval — keeps its clock still and says `paused`.
//!
//! Glyphs: codex's `•`, `·` and `└` are East-Asian *Ambiguous* (two cells on
//! many CJK terminals), so the default set is ` - ` and `↳` (Narrow), with
//! `✓` / `✗` for done and failed (Narrow). [`WorkingGlyphs::Unicode`] opts in
//! to codex's look.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::shimmer::Shimmer;
use crate::spinner::{Spinner, SpinnerKind};
use crate::text::{self, ELLIPSIS};
use crate::theme::{Status, Theme};

/// Upstream's default cap on detail lines.
pub const DETAIL_MAX_LINES: usize = 3;

/// Cells a too-long header keeps before the row is clipped at its end.
const HEADER_FLOOR: usize = 10;

/// `0s`, `59s`, `1m 00s`, `59m 59s`, `1h 00m 00s`, `25h 02m 03s`.
pub fn fmt_elapsed_compact(secs: u64) -> String {
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        return format!("{}m {:02}s", secs / 60, secs % 60);
    }
    format!(
        "{}h {:02}m {:02}s",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// The widest [`fmt_elapsed_compact`] form in the same unit as `secs` — the
/// width reserved so text after the clock does not shift every tick.
fn elapsed_slot(secs: u64) -> usize {
    if secs < 60 {
        3 // 59s
    } else if secs < 3600 {
        7 // 59m 59s
    } else {
        (secs / 3600).to_string().len() + 9 // Nh 59m 59s
    }
}

/// A turn's clock that stops while the turn waits on the person.
///
/// `now` is the host's monotonic clock as a [`Duration`] from any fixed
/// origin (`start.elapsed()`); the library never reads a clock itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkTimer {
    elapsed_running: Duration,
    last_resume_at: Duration,
    paused: bool,
}

impl WorkTimer {
    /// A running timer started at `now`.
    pub const fn started_at(now: Duration) -> WorkTimer {
        WorkTimer {
            elapsed_running: Duration::ZERO,
            last_resume_at: now,
            paused: false,
        }
    }

    /// Start counting again from `elapsed`, keeping the paused state (a turn
    /// can start while an approval is still open).
    pub fn reset(&mut self, elapsed: Duration, now: Duration) {
        self.elapsed_running = elapsed;
        self.last_resume_at = now;
    }

    pub fn pause_at(&mut self, now: Duration) {
        if !self.paused {
            self.elapsed_running += now.saturating_sub(self.last_resume_at);
            self.paused = true;
        }
    }

    pub fn resume_at(&mut self, now: Duration) {
        if self.paused {
            self.last_resume_at = now;
            self.paused = false;
        }
    }

    /// Time spent running, not counting pauses.
    pub fn elapsed_at(&self, now: Duration) -> Duration {
        self.elapsed_running
            + if self.paused {
                Duration::ZERO
            } else {
                now.saturating_sub(self.last_resume_at)
            }
    }

    pub const fn is_paused(&self) -> bool {
        self.paused
    }
}

/// Where the turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Phase {
    /// Working: spinner, shimmering header, clock running.
    #[default]
    Running,
    /// Waiting on the person (an approval): a slow drift, the clock stopped.
    Paused,
    /// Finished: `✓ Done in 4m 12s`.
    Done,
    /// Stopped with an error: `✗ Failed after 38s`.
    Failed,
}

/// Separator and detail glyphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum WorkingGlyphs {
    /// East-Asian Narrow only: ` - ` and `↳`. Safe on CJK terminals.
    #[default]
    Narrow,
    /// Codex's look: ` • `, ` · ` and `└` — East-Asian Ambiguous, two cells
    /// on many CJK terminals. Opt in where the host knows the terminal.
    Unicode,
}

impl WorkingGlyphs {
    const fn hint_sep(self) -> &'static str {
        match self {
            WorkingGlyphs::Narrow => " - ",
            WorkingGlyphs::Unicode => " • ",
        }
    }

    const fn note_sep(self) -> &'static str {
        match self {
            WorkingGlyphs::Narrow => " - ",
            WorkingGlyphs::Unicode => " · ",
        }
    }

    const fn detail_prefix(self) -> &'static str {
        match self {
            WorkingGlyphs::Narrow => "  ↳ ",
            WorkingGlyphs::Unicode => "  └ ",
        }
    }
}

/// The status row of a working agent.
#[derive(Debug, Clone, PartialEq)]
pub struct Working<'a> {
    header: Cow<'a, str>,
    phase: Phase,
    elapsed: Duration,
    header_age: Option<Duration>,
    interrupt: Option<Cow<'a, str>>,
    inline: Option<Cow<'a, str>>,
    detail: Option<Cow<'a, str>>,
    detail_max_lines: usize,
    glyphs: WorkingGlyphs,
}

impl<'a> Working<'a> {
    /// A running row with header `header` ("Working", "Thinking", "Reading
    /// files") and `esc` as the interrupt key.
    pub fn new(header: impl Into<Cow<'a, str>>) -> Working<'a> {
        Working {
            header: header.into(),
            phase: Phase::Running,
            elapsed: Duration::ZERO,
            header_age: None,
            interrupt: Some(Cow::Borrowed("esc")),
            inline: None,
            detail: None,
            detail_max_lines: DETAIL_MAX_LINES,
            glyphs: WorkingGlyphs::Narrow,
        }
    }

    /// Time the turn has run, pauses excluded (whole seconds are shown).
    pub fn elapsed(mut self, elapsed: Duration) -> Working<'a> {
        self.elapsed = elapsed;
        self
    }

    pub fn phase(mut self, phase: Phase) -> Working<'a> {
        self.phase = phase;
        self
    }

    /// `true` waits on the person ([`Phase::Paused`]); `false` resumes a
    /// paused row. Done and failed rows stay as they are.
    pub fn paused(mut self, paused: bool) -> Working<'a> {
        match (paused, self.phase) {
            (true, Phase::Running) => self.phase = Phase::Paused,
            (false, Phase::Paused) => self.phase = Phase::Running,
            _ => {}
        }
        self
    }

    /// Elapsed time and paused state from a [`WorkTimer`] read at `now`.
    pub fn timer(self, timer: &WorkTimer, now: Duration) -> Working<'a> {
        self.elapsed(timer.elapsed_at(now))
            .paused(timer.is_paused())
    }

    /// Time since the header text last changed; the shimmer restarts from
    /// it. Defaults to the frame time `t`.
    pub fn header_age(mut self, age: Duration) -> Working<'a> {
        self.header_age = Some(age);
        self
    }

    /// The key that interrupts (`esc`, `ctrl+c`), shown while running or
    /// paused.
    pub fn interrupt(mut self, keys: impl Into<Cow<'a, str>>) -> Working<'a> {
        self.interrupt = Some(keys.into());
        self
    }

    /// No interrupt hint: `(12s)` alone.
    pub fn no_interrupt(mut self) -> Working<'a> {
        self.interrupt = None;
        self
    }

    /// Short context after the clock (`2 background tasks`). Empty or blank
    /// text shows nothing.
    pub fn inline(mut self, note: impl Into<Cow<'a, str>>) -> Working<'a> {
        let note = note.into();
        self.inline = (!note.trim().is_empty()).then_some(note);
        self
    }

    /// What it is doing in detail, beneath the row.
    pub fn detail(mut self, detail: impl Into<Cow<'a, str>>) -> Working<'a> {
        let detail = detail.into();
        self.detail = (!detail.trim().is_empty()).then_some(detail);
        self
    }

    /// Most detail lines (at least one); the last ends in `…` when more was
    /// cut. Default 3.
    pub fn detail_lines(mut self, max: usize) -> Working<'a> {
        self.detail_max_lines = max.max(1);
        self
    }

    pub fn glyphs(mut self, glyphs: WorkingGlyphs) -> Working<'a> {
        self.glyphs = glyphs;
        self
    }

    /// Rows the widget takes at `width`.
    pub fn height(&self, width: u16) -> u16 {
        (1 + self.detail_rows(width).len()) as u16
    }

    /// The status row and the detail lines at frame time `t`, each fitted
    /// to `width` cells.
    pub fn lines_at(&self, t: Duration, theme: &Theme, width: u16) -> Vec<Line<'static>> {
        let width_cells = width as usize;
        let mut row = self.row(t, theme, &self.header);
        let over = text::line_width(&row).saturating_sub(width_cells);
        if over > 0 {
            // A long header gives way first (down to a few words), so the
            // clock — how long — stays on screen.
            let h = text::width(&self.header);
            let keep = h.saturating_sub(over).max(h.min(HEADER_FLOOR));
            if keep < h {
                row = self.row(t, theme, &text::truncate(&self.header, keep));
            }
        }
        let mut out = vec![text::truncate_line(row, width_cells)];
        let prefix = self.glyphs.detail_prefix();
        let indent = " ".repeat(text::width(prefix));
        for (i, row) in self.detail_rows(width).into_iter().enumerate() {
            let lead = if i == 0 {
                prefix.to_string()
            } else {
                indent.clone()
            };
            let line = Line::from(vec![
                Span::styled(lead, theme.dim()),
                Span::styled(row, theme.dim()),
            ]);
            out.push(text::truncate_line(line, width_cells));
        }
        out
    }

    /// A widget for one frame.
    pub fn at(self, t: Duration, theme: Theme) -> WorkingFrame<'a> {
        WorkingFrame {
            working: self,
            t,
            theme,
        }
    }

    fn row(&self, t: Duration, theme: &Theme, header: &str) -> Line<'static> {
        let secs = self.elapsed.as_secs();
        let clock = fmt_elapsed_compact(secs);
        let dim = theme.dim();
        let key = theme.text().add_modifier(Modifier::BOLD);
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(10);
        // Width of the clock group as drawn, and as wide as it gets in this
        // unit: text after it is padded to the latter.
        let clock_pad = elapsed_slot(secs).saturating_sub(text::width(&clock));
        match self.phase {
            Phase::Running | Phase::Paused => {
                let running = self.phase == Phase::Running;
                let (kind, status) = if running {
                    (SpinnerKind::Braille, Status::Running)
                } else {
                    (SpinnerKind::Drift, Status::Waiting)
                };
                spans.push(
                    Spinner::new(kind)
                        .style(theme.status(status))
                        .span_at(t, theme),
                );
                spans.push(Span::raw(" "));
                if running {
                    spans.extend(
                        Shimmer::new(header)
                            .style(theme.text())
                            .spans_at(self.header_age.unwrap_or(t), theme),
                    );
                } else {
                    spans.push(Span::styled(
                        header.to_string(),
                        theme.status(Status::Waiting),
                    ));
                }
                let paused = if running { "" } else { " paused" };
                match &self.interrupt {
                    Some(k) => {
                        spans.push(Span::styled(
                            format!(" ({clock}{paused}{}", self.glyphs.hint_sep()),
                            dim,
                        ));
                        spans.push(Span::styled(k.to_string(), key));
                        spans.push(Span::styled(" to interrupt)", dim));
                    }
                    None => spans.push(Span::styled(format!(" ({clock}{paused})"), dim)),
                }
            }
            Phase::Done | Phase::Failed => {
                let (glyph, status, word) = if self.phase == Phase::Done {
                    ("✓", Status::Done, "in")
                } else {
                    ("✗", Status::Failed, "after")
                };
                spans.push(Span::styled(glyph, theme.status(status)));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    header.to_string(),
                    theme.text().add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(format!(" {word} {clock}"), theme.secondary()));
            }
        }
        if let Some(note) = &self.inline {
            // Only a live clock ticks; a closed turn needs no reserve.
            let live = matches!(self.phase, Phase::Running | Phase::Paused);
            if live && clock_pad > 0 {
                spans.push(Span::raw(" ".repeat(clock_pad)));
            }
            spans.push(Span::styled(self.glyphs.note_sep(), dim));
            spans.push(Span::styled(note.trim().to_string(), dim));
        }
        Line::from(spans)
    }

    /// The detail wrapped by words to the space after the prefix, capped at
    /// `detail_max_lines`, the last kept line ending in `…` when cut.
    fn detail_rows(&self, width: u16) -> Vec<String> {
        let Some(detail) = self.detail.as_deref() else {
            return Vec::new();
        };
        if width == 0 {
            return Vec::new();
        }
        let content = (width as usize)
            .saturating_sub(text::width(self.glyphs.detail_prefix()))
            .max(1);
        let mut rows = wrap(detail.trim(), content);
        if rows.len() > self.detail_max_lines {
            rows.truncate(self.detail_max_lines);
            if let Some(last) = rows.last_mut() {
                let (kept, _) = text::take_cells(last, content.saturating_sub(1));
                *last = format!("{kept}{ELLIPSIS}");
            }
        }
        rows
    }
}

/// Greedy word wrap by cells; a word wider than a row is split by grapheme.
/// Shared with the status card's value column.
pub(crate) fn wrap(s: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for para in s.split('\n') {
        let start = rows.len();
        let mut row = String::new();
        let mut used = 0usize;
        for word in para.split_whitespace() {
            let w = text::width(word);
            let gap = usize::from(used > 0);
            if used + gap + w <= width {
                if gap == 1 {
                    row.push(' ');
                }
                row.push_str(word);
                used += gap + w;
                continue;
            }
            if used > 0 {
                rows.push(std::mem::take(&mut row));
            }
            let mut rest = word;
            while text::width(rest) > width {
                let (head, _) = text::take_cells(rest, width);
                // A grapheme wider than the row still gets a row of its own.
                let head = if head.is_empty() {
                    rest.graphemes(true).next().unwrap_or(rest)
                } else {
                    head
                };
                rows.push(head.to_string());
                rest = &rest[head.len()..];
            }
            row.push_str(rest);
            used = text::width(rest);
        }
        // A blank paragraph keeps its (empty) row; a split word leaves none.
        if !row.is_empty() || rows.len() == start {
            rows.push(row);
        }
    }
    rows
}

/// A [`Working`] row at one moment, ready to draw.
#[derive(Debug, Clone)]
pub struct WorkingFrame<'a> {
    working: Working<'a>,
    t: Duration,
    theme: Theme,
}

impl Widget for WorkingFrame<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut lines = self.working.lines_at(self.t, &self.theme, area.width);
        let rows = area.height as usize;
        if lines.len() > rows {
            // Never cut silently: the last row that fits says more follows.
            lines.truncate(rows);
            if let Some(last) = lines.pop() {
                lines.push(ends_in_ellipsis(last, area.width as usize));
            }
        }
        for (i, line) in lines.iter().enumerate() {
            buf.set_line(area.x, area.y + i as u16, line, area.width);
        }
    }
}

/// `line` clipped to leave one cell, then `…` (unless it already ends in one).
fn ends_in_ellipsis(line: Line<'static>, width: usize) -> Line<'static> {
    let mut l = text::truncate_line(line, width.saturating_sub(1));
    let has = l
        .spans
        .last()
        .is_some_and(|s| s.content.ends_with(ELLIPSIS));
    if !has && width > 0 {
        let style = l.spans.last().map_or(Style::new(), |s| s.style);
        l.spans.push(Span::styled(ELLIPSIS.to_string(), style));
    }
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::theme::Motion;
    use ratatui::style::Color;
    use unicode_width::UnicodeWidthChar;

    fn render(w: Working<'_>, t: Duration, theme: Theme, cols: u16, rows: u16) -> Buffer {
        let area = Rect::new(0, 0, cols, rows);
        let mut buf = Buffer::empty(area);
        w.at(t, theme).render(area, &mut buf);
        buf
    }

    /// Upstream `fmt_elapsed_compact_formats_seconds_minutes_hours`.
    #[test]
    fn fmt_elapsed_compact_formats_seconds_minutes_hours() {
        assert_eq!(fmt_elapsed_compact(0), "0s");
        assert_eq!(fmt_elapsed_compact(1), "1s");
        assert_eq!(fmt_elapsed_compact(59), "59s");
        assert_eq!(fmt_elapsed_compact(60), "1m 00s");
        assert_eq!(fmt_elapsed_compact(61), "1m 01s");
        assert_eq!(fmt_elapsed_compact(3 * 60 + 5), "3m 05s");
        assert_eq!(fmt_elapsed_compact(59 * 60 + 59), "59m 59s");
        assert_eq!(fmt_elapsed_compact(3600), "1h 00m 00s");
        assert_eq!(fmt_elapsed_compact(3600 + 60 + 1), "1h 01m 01s");
        assert_eq!(fmt_elapsed_compact(25 * 3600 + 2 * 60 + 3), "25h 02m 03s");
    }

    /// Upstream `timer_tests::pauses_and_resumes_without_counting_wait_time`,
    /// on the host's clock instead of `Instant`.
    #[test]
    fn timer_pauses_and_resumes_without_counting_wait_time() {
        let s = Duration::from_secs;
        let base = s(100);
        let mut timer = WorkTimer::started_at(base);
        timer.pause_at(base + s(5));
        timer.pause_at(base + s(8));
        assert_eq!(timer.elapsed_at(base + s(10)), s(5));
        timer.resume_at(base + s(10));
        timer.resume_at(base + s(12));
        assert_eq!(timer.elapsed_at(base + s(13)), s(8));
        timer.reset(s(24), base + s(20));
        assert_eq!(timer.elapsed_at(base + s(20)), s(24));
        timer.pause_at(base + s(20));
        timer.reset(Duration::ZERO, base + s(20));
        assert!(timer.is_paused());
        assert_eq!(timer.elapsed_at(base + s(140)), Duration::ZERO);
    }

    /// Upstream `renders_with_working_header`, with the narrow glyph set.
    #[test]
    fn renders_with_working_header() {
        let buf = render(
            Working::new("Working"),
            Duration::ZERO,
            Theme::ansi(),
            80,
            2,
        );
        assert_eq!(snapshot::text(&buf), "⠋ Working (0s - esc to interrupt)\n");
        let buf = render(
            Working::new("Working").glyphs(WorkingGlyphs::Unicode),
            Duration::ZERO,
            Theme::ansi(),
            80,
            1,
        );
        assert_eq!(snapshot::text(&buf), "⠋ Working (0s • esc to interrupt)");
    }

    /// Upstream `renders_truncated`.
    #[test]
    fn renders_truncated() {
        let buf = render(
            Working::new("Working"),
            Duration::ZERO,
            Theme::ansi(),
            20,
            2,
        );
        assert_eq!(snapshot::text(&buf), "⠋ Working (0s - esc…\n");
    }

    #[test]
    fn a_long_header_gives_way_before_the_clock() {
        let w = Working::new("Preparing bootstrap diagnostics")
            .elapsed(Duration::from_secs(221))
            .no_interrupt();
        let buf = render(w.clone(), Duration::ZERO, Theme::ansi(), 28, 1);
        assert_eq!(snapshot::text(&buf), "⠋ Preparing bootst… (3m 41s)");
        // Never below the floor: then the end is clipped instead.
        let buf = render(w, Duration::ZERO, Theme::ansi(), 16, 1);
        assert_eq!(snapshot::text(&buf), "⠋ Preparing… (3…");
    }

    /// Upstream `renders_wrapped_details_panama_two_lines`: a 4-cell prefix at
    /// 30 columns leaves 26, one short of the 27-cell phrase.
    #[test]
    fn renders_wrapped_details_panama_two_lines() {
        let w = Working::new("Working")
            .detail("A man a plan a canal panama")
            .no_interrupt();
        let theme = Theme::ansi().with_motion(Motion::Reduced);
        assert_eq!(w.height(30), 3);
        let buf = render(w, Duration::ZERO, theme, 30, 3);
        assert_eq!(
            snapshot::text(&buf),
            "⠋ Working (0s)\n  ↳ A man a plan a canal\n    panama"
        );
    }

    /// Upstream `renders_remapped_interrupt_hint`.
    #[test]
    fn renders_remapped_interrupt_hint() {
        let buf = render(
            Working::new("Working").interrupt("f12"),
            Duration::ZERO,
            Theme::ansi(),
            80,
            1,
        );
        assert_eq!(snapshot::text(&buf), "⠋ Working (0s - f12 to interrupt)");
    }

    /// Upstream `details_overflow_adds_ellipsis` and
    /// `details_args_can_disable_capitalization_and_limit_lines`.
    #[test]
    fn details_overflow_adds_ellipsis() {
        let w = Working::new("Working").detail("abcd abcd abcd abcd");
        let rows = w.detail_rows(6);
        assert_eq!(rows.len(), DETAIL_MAX_LINES);
        assert_eq!(rows.last().unwrap(), "a…");

        let w = Working::new("Working")
            .detail("cargo test -p codex-core and then cargo test -p codex-tui")
            .detail_lines(1);
        let rows = w.detail_rows(24);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].ends_with('…'), "{rows:?}");
        assert!(text::width(&rows[0]) <= 20);
    }

    #[test]
    fn paused_holds_the_clock_and_says_so() {
        let w = Working::new("Waiting for approval")
            .elapsed(Duration::from_secs(65))
            .paused(true);
        let a = render(w.clone(), Duration::ZERO, Theme::dark(), 60, 1);
        let b = render(w, Duration::from_secs(3), Theme::dark(), 60, 1);
        let ta = snapshot::text(&a);
        assert!(
            ta.ends_with("Waiting for approval (1m 05s paused - esc to interrupt)"),
            "{ta}"
        );
        // The clock text is the host's: same in both frames.
        assert_eq!(&snapshot::text(&b)[3..], &ta[3..]);
        assert_eq!(a[(2, 0)].fg, Theme::dark().wait);
    }

    #[test]
    fn timer_feeds_elapsed_and_pause() {
        let mut timer = WorkTimer::started_at(Duration::from_secs(10));
        timer.pause_at(Duration::from_secs(22));
        let w = Working::new("Working").timer(&timer, Duration::from_secs(90));
        let buf = render(w, Duration::ZERO, Theme::ansi(), 60, 1);
        assert_eq!(
            snapshot::text(&buf),
            "⠁ Working (12s paused - esc to interrupt)"
        );
    }

    #[test]
    fn done_and_failed_rows() {
        let done = Working::new("Done")
            .phase(Phase::Done)
            .elapsed(Duration::from_secs(252))
            .inline("14 files changed");
        let buf = render(done, Duration::ZERO, Theme::dark(), 60, 1);
        assert_eq!(snapshot::text(&buf), "✓ Done in 4m 12s - 14 files changed");
        assert_eq!(buf[(0, 0)].fg, Theme::dark().done);
        let failed = Working::new("Failed")
            .phase(Phase::Failed)
            .elapsed(Duration::from_secs(38));
        let buf = render(failed, Duration::ZERO, Theme::mono(), 60, 1);
        assert_eq!(snapshot::text(&buf), "✗ Failed after 38s");
        assert!(
            buf.content()
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
        );
    }

    #[test]
    fn text_after_the_clock_does_not_shift_within_a_unit() {
        let at = |secs| {
            let w = Working::new("Working")
                .elapsed(Duration::from_secs(secs))
                .inline("2 background tasks");
            let buf = render(w, Duration::ZERO, Theme::ansi(), 80, 1);
            snapshot::text(&buf).find(" - 2 background").unwrap()
        };
        assert_eq!(at(3), at(42));
        assert_eq!(at(61), at(3599));
    }

    #[test]
    fn spinner_turns_and_reduced_motion_holds() {
        let w = Working::new("Working");
        let a = render(w.clone(), Duration::ZERO, Theme::ansi(), 40, 1);
        let b = render(w.clone(), Duration::from_millis(120), Theme::ansi(), 40, 1);
        assert_ne!(a[(0, 0)].symbol(), b[(0, 0)].symbol());
        let r = Theme::ansi().with_motion(Motion::Reduced);
        let c = render(w.clone(), Duration::ZERO, r, 40, 1);
        let d = render(w, Duration::from_millis(120), r, 40, 1);
        assert_eq!(c, d);
        assert!(c[(2, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn short_area_says_detail_was_cut() {
        let w = Working::new("Working").detail("one two three four five six seven eight nine ten");
        let buf = render(w, Duration::ZERO, Theme::ansi(), 20, 2);
        let lines = snapshot::lines(&buf);
        assert!(lines[1].ends_with('…'), "{lines:?}");
        // Degenerate sizes do not panic.
        for (c, r) in [(1, 1), (2, 1), (4, 3), (5, 1)] {
            let w = Working::new("Working").detail("x y z");
            let _ = render(w, Duration::ZERO, Theme::ansi(), c, r);
        }
    }

    #[test]
    fn wrap_handles_wide_graphemes_and_long_words() {
        assert_eq!(wrap("한국어 문장", 4), ["한국", "어", "문장"]);
        assert_eq!(wrap("abcdefgh", 3), ["abc", "def", "gh"]);
        assert_eq!(wrap("界", 1), ["界"]);
        assert_eq!(wrap("a\nb", 10), ["a", "b"]);
    }

    #[test]
    fn default_glyphs_are_east_asian_narrow() {
        let g = WorkingGlyphs::Narrow;
        let all = format!("{}{}{}✓✗", g.hint_sep(), g.note_sep(), g.detail_prefix());
        for c in all
            .chars()
            .chain(SpinnerKind::Braille.frames().iter().copied())
        {
            assert_eq!(c.width_cjk(), Some(1), "{c:?} U+{:04X}", c as u32);
        }
        // The opt-in set is what the narrow set avoids.
        for c in ['•', '·', '└'] {
            assert_eq!(c.width_cjk(), Some(2), "{c:?}");
        }
    }
}
