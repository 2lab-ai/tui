//! Something is on until a deadline and how long is left — and a flash
//! that says whether the last action worked.
//!
//! [`Banner`] is ported from llmux `src/tui/ui.rs` `event_banner_line`
//! (MIT, 2lab): a one-row strip over the dashboard — the event in bold,
//! `until 7/12 09:00`, and `3d 15h 15m 25s left` — shown only while an
//! event is on (`from <= now < until`, the earliest deadline winning; see
//! [`active`]). [`Flash`] is llmux's status-line and raw-viewer flash
//! (`STATUS_TTL`, `RawModal::flash`): a short message about the last action
//! that disappears after a few seconds.
//!
//! Neither reads a clock. The host passes `now` (for the banner) and how
//! long ago the action happened (for the flash), and the UTC offset for the
//! deadline's wall clock — llmux called `localtime_r` inside the draw.
//! At narrow widths the banner drops the deadline first, then clips the
//! event with `…`, then shortens the countdown.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::time::{Duration, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::format;
use crate::text;
use crate::theme::{Level, Status, Theme};

/// Something that is on from `from` until `until`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub text: String,
    pub from: SystemTime,
    pub until: SystemTime,
}

impl Event {
    pub fn new(text: impl Into<String>, from: SystemTime, until: SystemTime) -> Event {
        Event {
            text: text.into(),
            from,
            until,
        }
    }

    /// On at `now`: `from` inclusive, `until` exclusive.
    pub fn is_active(&self, now: SystemTime) -> bool {
        self.from <= now && now < self.until
    }
}

/// The event to show at `now`: of those on, the one ending first. `None`
/// when nothing is on — the banner then takes no row at all.
pub fn active(events: &[Event], now: SystemTime) -> Option<&Event> {
    events
        .iter()
        .filter(|e| e.is_active(now))
        .min_by_key(|e| e.until)
}

const SEP: &str = " · ";
const LEFT: &str = " left";
/// The event keeps at least this many cells before the countdown shortens.
const MIN_TEXT: usize = 6;

/// One event as a one-row strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    text: String,
    until: SystemTime,
    now: SystemTime,
    offset_secs: i64,
    theme: Theme,
}

impl Banner {
    /// `text` is on until `until`; `now` is the host's clock this frame.
    pub fn new(text: impl Into<String>, until: SystemTime, now: SystemTime) -> Banner {
        Banner {
            text: text.into(),
            until,
            now,
            offset_secs: 0,
            theme: Theme::default(),
        }
    }

    /// The banner for the [`active`] event, if any.
    pub fn for_events(events: &[Event], now: SystemTime) -> Option<Banner> {
        active(events, now).map(|e| Banner::new(e.text.clone(), e.until, now))
    }

    /// The host's UTC offset in seconds, for the deadline's wall clock.
    pub fn offset(mut self, offset_secs: i64) -> Banner {
        self.offset_secs = offset_secs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Banner {
        self.theme = theme;
        self
    }

    /// Time left; zero once the deadline has passed.
    pub fn remaining(&self) -> Duration {
        self.until.duration_since(self.now).unwrap_or_default()
    }

    /// `6d 14h 29m 59s left`, or `ended`.
    fn countdown(&self, short: bool) -> (String, Style) {
        let rem = self.remaining();
        if rem.is_zero() {
            return ("ended".to_string(), self.theme.dim());
        }
        let n = if short {
            format::countdown(rem)
        } else {
            format::remaining_hms(rem)
        };
        (
            format!("{n}{LEFT}"),
            self.theme
                .status(Status::Waiting)
                .add_modifier(Modifier::BOLD),
        )
    }

    /// The banner in at most `width` cells. Fit is decided on the widest
    /// countdown the current leading unit can show, so the strip does not
    /// reflow every second.
    pub fn line(&self, width: u16) -> Line<'static> {
        let w = usize::from(width);
        let t = &self.theme;
        let content = t.accent().add_modifier(Modifier::BOLD);
        let sep = || Span::styled(SEP, t.dim());
        let tw = text::width(&self.text);
        let rem = self.remaining();
        let long_w = if rem.is_zero() {
            "ended".len()
        } else {
            format::remaining_hms_bound(rem) + LEFT.len()
        };
        let short_w = format::COUNTDOWN_MAX_WIDTH + LEFT.len();
        let deadline = format::month_day_hm(self.until, self.offset_secs);
        let (long, long_style) = self.countdown(false);

        if w >= tw + 3 + "until ".len() + text::width(&deadline) + 3 + long_w {
            return Line::from(vec![
                Span::styled(self.text.clone(), content),
                sep(),
                Span::styled("until ", t.dim()),
                Span::styled(deadline, t.secondary()),
                sep(),
                Span::styled(long, long_style),
            ]);
        }
        let with = |text_room: usize, (cd, st): (String, Style)| {
            Line::from(vec![
                Span::styled(text::truncate(&self.text, text_room).into_owned(), content),
                sep(),
                Span::styled(cd, st),
            ])
        };
        let min_text = tw.min(MIN_TEXT);
        if w >= min_text + 3 + long_w {
            return with(w - 3 - long_w, (long, long_style));
        }
        if !rem.is_zero() && w >= min_text + 3 + short_w {
            return with(w - 3 - short_w, self.countdown(true));
        }
        Line::from(Span::styled(
            text::truncate(&self.text, w).into_owned(),
            content,
        ))
    }
}

impl Widget for &Banner {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let line = self.line(area.width);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Widget for Banner {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// How the last action went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    Ok,
    Warn,
    Fail,
}

impl Outcome {
    /// The word that leads the flash, so it reads without colour.
    pub const fn tag(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Warn => "warn",
            Outcome::Fail => "fail",
        }
    }

    pub const fn level(self) -> Level {
        match self {
            Outcome::Ok => Level::Ok,
            Outcome::Warn => Level::Warn,
            Outcome::Fail => Level::Crit,
        }
    }
}

/// llmux's status-line flash lifetime.
pub const FLASH_TTL: Duration = Duration::from_secs(5);
/// Cells of the outcome tag, so messages start in one column.
const TAG_WIDTH: usize = 4;

/// A short message about the last action, shown for a few seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct Flash {
    text: String,
    outcome: Outcome,
    elapsed: Duration,
    ttl: Duration,
    theme: Theme,
}

impl Flash {
    pub fn new(outcome: Outcome, text: impl Into<String>) -> Flash {
        Flash {
            text: text.into(),
            outcome,
            elapsed: Duration::ZERO,
            ttl: FLASH_TTL,
            theme: Theme::default(),
        }
    }

    pub fn ok(text: impl Into<String>) -> Flash {
        Flash::new(Outcome::Ok, text)
    }

    pub fn warn(text: impl Into<String>) -> Flash {
        Flash::new(Outcome::Warn, text)
    }

    pub fn fail(text: impl Into<String>) -> Flash {
        Flash::new(Outcome::Fail, text)
    }

    /// How long ago the action happened (the host's clock).
    pub fn elapsed(mut self, elapsed: Duration) -> Flash {
        self.elapsed = elapsed;
        self
    }

    /// How long the flash stays (default 5 s).
    pub fn ttl(mut self, ttl: Duration) -> Flash {
        self.ttl = ttl;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Flash {
        self.theme = theme;
        self
    }

    pub fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// Still showing: younger than its ttl.
    pub fn is_live(&self) -> bool {
        self.elapsed < self.ttl
    }

    /// `ok   copied 4,132 bytes → pbcopy`, or `None` once it has expired —
    /// the host shows what was there before (a key legend).
    pub fn line(&self) -> Option<Line<'static>> {
        if !self.is_live() {
            return None;
        }
        let t = &self.theme;
        let tone = t.level(self.outcome.level()).add_modifier(Modifier::BOLD);
        let body = match self.outcome {
            Outcome::Ok => t.text(),
            Outcome::Warn | Outcome::Fail => t.level(self.outcome.level()),
        };
        let body = body.remove_modifier(Modifier::REVERSED);
        Some(Line::from(vec![
            Span::styled(self.outcome.tag(), tone),
            Span::raw(" ".repeat(TAG_WIDTH + 1 - self.outcome.tag().len())),
            Span::styled(self.text.clone(), body),
        ]))
    }
}

impl Widget for &Flash {
    /// Draws nothing once expired; clips with `…` when narrow.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        if let Some(line) = self.line() {
            let line = text::truncate_line(line, usize::from(area.width));
            buf.set_line(area.x, area.y, &line, area.width);
        }
    }
}

impl Widget for Flash {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// A flash line padded to `width`, or `fallback` (the key legend it
/// stands in for) once expired — llmux's raw viewer does exactly this.
pub fn flash_or(flash: Option<&Flash>, fallback: Line<'static>, width: u16) -> Line<'static> {
    let line = flash.and_then(Flash::line).unwrap_or(fallback);
    Line::from(text::pad_spans(line.spans, usize::from(width)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    /// 2026-11-05 09:30:00 UTC.
    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_871_000)
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn flat(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn event_banner_shows_only_active_events() {
        let n = now();
        assert!(active(&[], n).is_none());
        let future = Event::new("future", at(4_070_908_800), at(4_073_587_200));
        assert!(active(std::slice::from_ref(&future), n).is_none());
        let stale = Event::new("stale", at(1_577_836_800), at(1_580_515_200));
        assert!(active(std::slice::from_ref(&stale), n).is_none());
        let on = Event::new(
            "Fable 5 Available until 7/12",
            at(1_577_836_800),
            at(4_102_444_800),
        );
        let b = Banner::for_events(&[future, stale, on], n).expect("active shown");
        let s = flat(&b.line(200));
        assert!(s.contains("Fable 5 Available until 7/12"), "{s}");
        assert!(s.contains("until "), "{s}");
        assert!(s.contains("left"), "{s}");
    }

    #[test]
    fn event_banner_picks_earliest_active_until() {
        let from = at(1_577_836_800);
        let later = Event::new("later end", from, at(4_102_444_800));
        let sooner = Event::new("sooner end", from, at(4_070_908_800));
        let all = [later, sooner];
        let e = active(&all, now()).expect("one active");
        assert_eq!(e.text, "sooner end");
    }

    #[test]
    fn from_is_inclusive_and_until_exclusive() {
        let e = Event::new("x", now(), now() + Duration::from_secs(60));
        assert!(e.is_active(now()));
        assert!(e.is_active(now() + Duration::from_secs(59)));
        assert!(!e.is_active(now() + Duration::from_secs(60)));
    }

    #[test]
    fn full_line_with_the_host_offset() {
        // Until 2026-11-12 00:00 UTC = 09:00 in Seoul.
        let b = Banner::new("Fable 5 available on every plan", at(1_794_441_600), now())
            .offset(9 * 3_600)
            .theme(Theme::ansi());
        let line = b.line(80);
        assert_eq!(
            flat(&line),
            "Fable 5 available on every plan · until 11/12 09:00 · 6d 14h 30m 0s left"
        );
        assert_eq!(line.spans[0].style.fg, Some(Color::Cyan));
        assert!(line.spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[5].style.fg, Some(Color::Yellow));
        let utc = Banner::new("x", at(1_794_441_600), now()).line(80);
        assert!(flat(&utc).contains("until 11/12 00:00"));
    }

    #[test]
    fn narrow_widths_drop_the_deadline_then_clip_then_shorten() {
        let b = Banner::new("Fable 5 available on every plan", at(1_794_441_600), now());
        let row = |w: u16| flat(&b.line(w));
        assert_eq!(
            row(56),
            "Fable 5 available on every plan · 6d 14h 30m 0s left"
        );
        assert_eq!(row(40), "Fable 5 available… · 6d 14h 30m 0s left");
        assert_eq!(
            row(23),
            "Fable 5… · 6d 14h left",
            "then the short countdown"
        );
        assert_eq!(row(21), "Fable… · 6d 14h left");
        assert_eq!(row(18), "Fable 5 available…", "then the event alone");
        assert_eq!(row(12), "Fable 5 ava…");
        for w in 0..90u16 {
            assert!(text::line_width(&b.line(w)) <= usize::from(w), "{w}");
        }
    }

    #[test]
    fn the_layout_holds_while_the_leading_unit_does() {
        // Every second of the 6-day stretch makes the same fit decision.
        let until = at(1_794_441_600);
        let w = 50;
        let shape = |rem: u64| {
            let b = Banner::new(
                "Fable 5 available on every plan",
                until,
                until - Duration::from_secs(rem),
            );
            b.line(w).spans.len()
        };
        let first = shape(6 * 86_400 + 23 * 3_600 + 59 * 60 + 59);
        for rem in [6 * 86_400, 6 * 86_400 + 1, 6 * 86_400 + 11 * 3_600 + 7] {
            assert_eq!(shape(rem), first, "{rem}");
        }
    }

    #[test]
    fn ended_is_said_not_counted() {
        let b = Banner::new("Codex maintenance", now(), now());
        assert_eq!(b.remaining(), Duration::ZERO);
        assert!(flat(&b.line(60)).ends_with("ended"));
    }

    #[test]
    fn mono_banner_has_no_colour() {
        let b = Banner::new("Fable 5 available", at(1_794_441_600), now()).theme(Theme::mono());
        let mut buf = Buffer::empty(Rect::new(0, 0, 70, 1));
        (&b).render(buf.area, &mut buf);
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
        }
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn flash_lives_for_its_ttl() {
        let f = Flash::ok("copied 4,132 bytes → pbcopy").ttl(Duration::from_secs(3));
        assert!(f.clone().elapsed(Duration::from_millis(2_999)).is_live());
        assert!(!f.clone().elapsed(Duration::from_secs(3)).is_live());
        assert_eq!(
            flat(&f.clone().line().expect("live")),
            "ok   copied 4,132 bytes → pbcopy"
        );
        assert!(f.clone().elapsed(Duration::from_secs(4)).line().is_none());
        assert_eq!(Flash::warn("x").ttl, FLASH_TTL);
    }

    #[test]
    fn flash_says_its_outcome_in_words_and_tone() {
        let t = Theme::ansi();
        let ok = Flash::ok("saved").theme(t).line().unwrap();
        let fail = Flash::fail("save failed: read-only")
            .theme(t)
            .line()
            .unwrap();
        assert_eq!(ok.spans[0].content, "ok");
        assert_eq!(ok.spans[0].style.fg, Some(Color::Green));
        assert_eq!(fail.spans[0].content, "fail");
        assert_eq!(fail.spans[0].style.fg, Some(Color::Red));
        assert_eq!(fail.spans[2].style.fg, Some(Color::Red));
        // Messages start in one column whatever the tag.
        for f in [Flash::ok("m"), Flash::warn("m"), Flash::fail("m")] {
            assert_eq!(flat(&f.line().unwrap()).find('m'), Some(5));
        }
    }

    #[test]
    fn flash_in_mono_keeps_fail_apart_by_weight() {
        let t = Theme::mono();
        let fail = Flash::fail("save failed").theme(t).line().unwrap();
        let ok = Flash::ok("saved").theme(t).line().unwrap();
        for l in [&fail, &ok] {
            for s in &l.spans {
                assert!(matches!(s.style.fg, None | Some(Color::Reset)));
                assert!(s.style.bg.is_none());
            }
        }
        assert!(
            fail.spans[0]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(
            !fail.spans[2]
                .style
                .add_modifier
                .contains(Modifier::REVERSED)
        );
        assert!(!ok.spans[0].style.add_modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn flash_or_falls_back_and_widget_clips() {
        let legend = Line::from("c copy · s save · esc");
        let f =
            Flash::fail("save failed: ~/Downloads is read-only").elapsed(Duration::from_secs(9));
        assert_eq!(
            flat(&flash_or(Some(&f), legend.clone(), 24)),
            "c copy · s save · esc   "
        );
        assert_eq!(
            flat(&flash_or(None, legend, 24)).trim_end(),
            "c copy · s save · esc"
        );
        let live = f.elapsed(Duration::ZERO);
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 1));
        (&live).render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "fail save faile…");
    }
}
