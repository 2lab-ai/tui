//! An in-bar quota gauge: how much of a window is used (or left), when it
//! resets, and whether that is fine.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `quota_bar_line`,
//! `quota_bar_text`, `quota_cell_line` and `GaugeParts`, which draw the 5h
//! and 7d columns of the accounts table. The bar is a reverse-video fill over
//! the first `round(fill × width)` cells with the reset time centred on top
//! (its larger unit bold), then a space and a right-aligned percent label.
//!
//! Changes from llmux:
//! - the in-bar text is placed by grapheme and measured in cells (llmux
//!   placed it by `char`, so a wide label overran the bar);
//! - the colour bands are the web `ui-meter`'s `low` / `high` thresholds
//!   (default 0.70 / 0.90) and read [`Level`] from the theme, keyed on the
//!   *used* fraction whichever way the bar fills;
//! - the absolute reset stamp takes the host's UTC offset instead of UTC;
//! - at narrow widths the parts give way in a fixed order (see
//!   [`Meter::line`]) instead of overrunning, and a text that does not fit
//!   falls back to its bold unit rather than losing a character to the
//!   marker.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::time::{Duration, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::format;
use crate::text::{self, Align};
use crate::theme::{Level, Theme};

/// Used fraction at which a meter turns [`Level::Warn`].
pub const DEFAULT_LOW: f64 = 0.70;
/// Used fraction at which a meter turns [`Level::Crit`].
pub const DEFAULT_HIGH: f64 = 0.90;
/// Cells of the percent label slot: the widest label is `100%!`.
pub const LABEL_WIDTH: usize = 5;
/// The narrowest bar worth drawing beside a label.
pub const MIN_BAR: usize = 4;

/// Which way the bar fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Fill {
    /// The fill grows as the window is used; the label reads the used share.
    #[default]
    Used,
    /// The fill drains toward the reset; the label reads what is left.
    Left,
}

/// The text inside the bar.
#[derive(Debug, Clone, PartialEq)]
enum BarText {
    None,
    /// Time left: `7d 10h`, `2h 13m`, `42s` — the first unit bold.
    In(Duration),
    /// A wall-clock stamp `MM/DD HH:MM` at a UTC offset — the date bold.
    At {
        at: SystemTime,
        now: SystemTime,
        offset_secs: i64,
    },
    /// The host's own text and how many leading cells are bold.
    Custom {
        text: String,
        bold: usize,
    },
}

/// One quota window as a one-row gauge. Build it, then draw it with
/// [`Meter::line`] (a table cell) or as a [`Widget`].
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    used: Option<f64>,
    low: f64,
    high: f64,
    fill: Fill,
    text: BarText,
    marker: Option<char>,
    over: bool,
    level: Option<Level>,
    label: bool,
    theme: Theme,
}

impl Meter {
    /// A window with `used` of it gone (0.0–1.0; above 1.0 is over-reported
    /// and reads full). A non-finite value is [`Meter::unknown`].
    pub fn new(used: f64) -> Meter {
        Meter {
            used: used.is_finite().then_some(used),
            low: DEFAULT_LOW,
            high: DEFAULT_HIGH,
            fill: Fill::Used,
            text: BarText::None,
            marker: None,
            over: false,
            level: None,
            label: true,
            theme: Theme::default(),
        }
    }

    /// A window nobody has measured yet: no fill and `—` for the percent —
    /// never a made-up 0%.
    pub fn unknown() -> Meter {
        Meter {
            used: None,
            ..Meter::new(0.0)
        }
    }

    /// [`Meter::new`] or [`Meter::unknown`].
    pub fn from_option(used: Option<f64>) -> Meter {
        used.map_or_else(Meter::unknown, Meter::new)
    }

    /// Used fractions at which the meter turns warn (`low`) and crit
    /// (`high`). Defaults 0.70 / 0.90, the web `ui-meter`'s bands.
    pub fn thresholds(mut self, low: f64, high: f64) -> Meter {
        self.low = low.min(high);
        self.high = high.max(low);
        self
    }

    /// Fill with what is used (default) or what is left.
    pub fn fill(mut self, fill: Fill) -> Meter {
        self.fill = fill;
        self
    }

    /// The window resets after `remaining`: shown as `7d 10h` with the day
    /// bold; zero reads `0s`.
    pub fn resets_in(mut self, remaining: Duration) -> Meter {
        self.text = BarText::In(remaining);
        self
    }

    /// The window resets at `at`: shown as `MM/DD HH:MM` on the wall clock
    /// `offset_secs` east of UTC (the host reads its own zone). A reset at or
    /// before `now` reads `0s`.
    pub fn resets_at(mut self, at: SystemTime, now: SystemTime, offset_secs: i64) -> Meter {
        self.text = BarText::At {
            at,
            now,
            offset_secs,
        };
        self
    }

    /// Any other text inside the bar, its first `bold_cells` cells bold
    /// (`"cold"`, `"paused"`).
    pub fn text(mut self, text: impl Into<String>, bold_cells: usize) -> Meter {
        self.text = BarText::Custom {
            text: text.into(),
            bold: bold_cells,
        };
        self
    }

    /// A one-cell glyph pinned to the bar's last cell (a freshness mark), so
    /// the centred time never moves when it comes and goes. Prefer an
    /// East-Asian Narrow glyph; a glyph that is not one cell wide is not
    /// drawn.
    pub fn marker(mut self, marker: char) -> Meter {
        self.marker = Some(marker);
        self
    }

    /// Mark the label with `!`: parked, or past the host's own limit.
    pub fn over(mut self, over: bool) -> Meter {
        self.over = over;
        self
    }

    /// Force a level — the host knows the limit is engaged even when the
    /// number looks calm.
    pub fn level(mut self, level: Level) -> Meter {
        self.level = Some(level);
        self
    }

    /// Show the percent label beside the bar (default on).
    pub fn label(mut self, on: bool) -> Meter {
        self.label = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Meter {
        self.theme = theme;
        self
    }

    /// How fine the reading is: the forced level, else the used fraction
    /// against the thresholds. Unknown reads [`Level::Ok`].
    pub fn current_level(&self) -> Level {
        self.level.unwrap_or_else(|| match self.used {
            Some(u) => Level::of(u, self.low, self.high),
            None => Level::Ok,
        })
    }

    /// The fraction of the bar that is filled.
    pub fn fill_fraction(&self) -> f64 {
        let used = self.used.unwrap_or(0.0).clamp(0.0, 1.0);
        match (self.used, self.fill) {
            (None, _) => 0.0,
            (Some(_), Fill::Used) => used,
            (Some(_), Fill::Left) => 1.0 - used,
        }
    }

    /// The percent label: the filled share (`42%`), `!` when over, `—` when
    /// unknown.
    pub fn label_text(&self) -> String {
        if self.used.is_none() {
            return format::UNKNOWN.to_string();
        }
        let mut l = format::percent(self.fill_fraction());
        if self.over {
            l.push('!');
        }
        l
    }

    /// The text inside the bar and how many of its leading cells are bold.
    pub fn bar_text(&self) -> (String, usize) {
        match &self.text {
            BarText::None => (String::new(), 0),
            BarText::In(rem) => {
                let (head, tail) = format::countdown_units(*rem);
                let bold = text::width(&head);
                match tail {
                    Some(tail) => (format!("{head} {tail}"), bold),
                    None => (head, bold),
                }
            }
            BarText::At {
                at,
                now,
                offset_secs,
            } => match at.duration_since(*now) {
                Ok(d) if !d.is_zero() => (format::absolute_stamp(*at, *offset_secs), 5),
                _ => ("0s".to_string(), 2),
            },
            BarText::Custom { text, bold } => (text.clone(), *bold),
        }
    }

    fn marker_cell(&self) -> Option<char> {
        self.marker
            .filter(|m| text::width(m.encode_utf8(&mut [0; 4])) == 1)
    }

    /// The widest the bar text can get for this kind of reset, so whether the
    /// label shows depends on the width alone — never on the ticking clock.
    fn widest_text(&self) -> usize {
        match &self.text {
            BarText::None => 0,
            BarText::In(_) => format::COUNTDOWN_MAX_WIDTH,
            BarText::At { .. } => 11,
            BarText::Custom { text, .. } => text::width(text),
        }
    }

    /// The narrowest width that shows everything: the bar holds the widest
    /// form of its text (and the marker), and the label keeps its slot.
    /// Below it the label gives way. A good column width for a table.
    pub fn full_width(&self) -> u16 {
        let bar = (self.widest_text() + usize::from(self.marker_cell().is_some())).max(MIN_BAR);
        let w = if self.label {
            bar + 1 + LABEL_WIDTH
        } else {
            bar
        };
        u16::try_from(w).unwrap_or(u16::MAX)
    }

    /// The meter in exactly `width` cells, for a table cell or a paragraph.
    ///
    /// From [`Meter::full_width`] up: bar, a space, the percent label. Below
    /// it the label gives way and the bar takes the whole width; the bar's
    /// text then drops its smaller unit (`2h 13m` → `2h`), and when even that
    /// does not fit the bar shows the percent instead (if there is no reset
    /// text) or the fill alone.
    pub fn line(&self, width: u16) -> Line<'static> {
        let w = usize::from(width);
        if w == 0 {
            return Line::default();
        }
        let level = self.current_level();
        let known = self.used.is_some();
        let base = if known {
            self.theme.level(level).remove_modifier(Modifier::REVERSED)
        } else {
            self.theme.dim()
        };
        let label_style = if known {
            self.theme.level(level)
        } else {
            self.theme.dim()
        };
        let marker = self.marker_cell();
        let (full, bold) = self.bar_text();
        let label = self.label_text();
        let fill = self.fill_fraction();

        if self.label && w >= usize::from(self.full_width()) {
            let bar_w = w - 1 - LABEL_WIDTH;
            let room = bar_w - usize::from(marker.is_some());
            let (t, b) = fit(&full, bold, room).unwrap_or_default();
            let mut spans = bar_spans(fill, base, &t, b, bar_w, marker);
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                text::pad(&label, LABEL_WIDTH, Align::Right),
                label_style,
            ));
            return Line::from(spans);
        }

        let room = w.saturating_sub(usize::from(marker.is_some()));
        let (t, b) = fit(&full, bold, room)
            .or_else(|| {
                (self.label && full.is_empty() && text::width(&label) <= room)
                    .then(|| (label.clone(), text::width(&label)))
            })
            .unwrap_or_default();
        Line::from(bar_spans(fill, base, &t, b, w, marker))
    }
}

/// `text` if it fits `room` cells, else its bold unit alone, else nothing.
fn fit(text: &str, bold: usize, room: usize) -> Option<(String, usize)> {
    if text.is_empty() {
        return None;
    }
    if text::width(text) <= room {
        return Some((text.to_string(), bold));
    }
    let (head, hw) = text::take_cells(text, bold);
    let head = head.trim_end();
    (!head.is_empty() && hw <= room).then(|| (head.to_string(), text::width(head)))
}

/// One bar: `width` cells, the first `round(fill × width)` reversed (the
/// fill), `text` centred on top with its first `bold` cells bold, and an
/// optional one-cell marker in the last cell. Spans split wherever the style
/// changes, so the fill shows behind the text. Only spaces, the text and the
/// marker are drawn — no bar glyphs.
fn bar_spans(
    fill: f64,
    base: Style,
    text: &str,
    bold: usize,
    width: usize,
    marker: Option<char>,
) -> Vec<Span<'static>> {
    let fill_cols = ((fill.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    let (text, tw) = text::take_cells(text, width);
    let start = (width - tw) / 2;
    let marker_col = marker.map(|_| width - 1);

    // (symbol, first column, bold) per drawn cell or grapheme.
    let mut cells: Vec<(String, usize, bool)> = Vec::with_capacity(width);
    let mut col = 0usize;
    while col < start {
        cells.push((" ".to_string(), col, false));
        col += 1;
    }
    let mut into = 0usize;
    for g in text.graphemes(true) {
        let gw = text::width(g);
        if gw == 0 {
            if let Some(last) = cells.last_mut() {
                last.0.push_str(g);
            }
            continue;
        }
        if marker_col.is_some_and(|m| col + gw > m) {
            break;
        }
        cells.push((g.to_string(), col, into < bold));
        col += gw;
        into += gw;
    }
    while col < width {
        let sym = match marker {
            Some(m) if Some(col) == marker_col => m.to_string(),
            _ => " ".to_string(),
        };
        cells.push((sym, col, false));
        col += 1;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut run_style: Option<Style> = None;
    for (sym, col, b) in cells {
        let mut style = base;
        if col < fill_cols {
            style = style.add_modifier(Modifier::REVERSED);
        }
        if b {
            style = style.add_modifier(Modifier::BOLD);
        }
        if run_style != Some(style) {
            if let Some(prev) = run_style {
                spans.push(Span::styled(std::mem::take(&mut run), prev));
            }
            run_style = Some(style);
        }
        run.push_str(&sym);
    }
    if let Some(prev) = run_style {
        spans.push(Span::styled(run, prev));
    }
    spans
}

impl Widget for &Meter {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let line = self.line(area.width);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Widget for Meter {
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

    fn flat(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// (symbol, style) per cell, wide graphemes once.
    fn cells(spans: &[Span]) -> Vec<(String, Style)> {
        spans
            .iter()
            .flat_map(|s| {
                s.content
                    .graphemes(true)
                    .map(move |g| (g.to_string(), s.style))
            })
            .collect()
    }

    fn green() -> Style {
        Style::new().fg(Color::Green)
    }

    fn render(m: &Meter, w: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, 1));
        m.render(buf.area, &mut buf);
        buf
    }

    // ---- level bands (llmux format.rs gauge tests) ----

    #[test]
    fn gauge_green_below_70() {
        for u in [0.0, 0.42, 0.699] {
            assert_eq!(Meter::new(u).current_level(), Level::Ok, "{u}");
        }
    }

    #[test]
    fn gauge_yellow_from_70_to_below_90() {
        for u in [0.70, 0.85, 0.899] {
            assert_eq!(Meter::new(u).current_level(), Level::Warn, "{u}");
        }
    }

    #[test]
    fn gauge_red_at_90_and_above() {
        for u in [0.90, 1.0, 1.5] {
            assert_eq!(Meter::new(u).current_level(), Level::Crit, "{u}");
        }
    }

    #[test]
    fn thresholds_and_forced_level() {
        let m = Meter::new(0.6).thresholds(0.5, 0.8);
        assert_eq!(m.current_level(), Level::Warn);
        // Swapped bounds are put back in order.
        assert_eq!(
            Meter::new(0.6).thresholds(0.8, 0.5).current_level(),
            Level::Warn
        );
        assert_eq!(
            Meter::new(0.1).level(Level::Crit).current_level(),
            Level::Crit
        );
        assert_eq!(Meter::unknown().current_level(), Level::Ok);
    }

    // ---- the bar (llmux quota_bar_line tests) ----

    #[test]
    fn quota_bar_line_is_cell_width_and_splits_at_fill_boundary() {
        let spans = bar_spans(0.5, green(), "7d 10h", 2, 16, None);
        let c = cells(&spans);
        assert_eq!(c.len(), 16, "cell is exactly bar-width wide");
        for (col, (_, st)) in c.iter().enumerate() {
            assert_eq!(
                st.add_modifier.contains(Modifier::REVERSED),
                col < 8,
                "column {col} reversal"
            );
        }
    }

    #[test]
    fn quota_bar_time_position_is_fixed_regardless_of_marker() {
        let without = flat(&bar_spans(0.5, green(), "7d 10h", 2, 16, None));
        let with = flat(&bar_spans(0.5, green(), "7d 10h", 2, 16, Some('*')));
        assert_eq!(without.find("7d 10h"), with.find("7d 10h"));
        assert_eq!(
            with.chars().last(),
            Some('*'),
            "marker pinned to last column"
        );
        assert_eq!(text::width(&with), 16);
    }

    #[test]
    fn quota_bar_line_empty_and_full_fill() {
        for (fill, want) in [(0.0, 0usize), (1.0, 16usize)] {
            let spans = bar_spans(fill, green(), "17m 35s", 3, 16, None);
            let rev = cells(&spans)
                .iter()
                .filter(|(_, s)| s.add_modifier.contains(Modifier::REVERSED))
                .count();
            assert_eq!(rev, want, "fill {fill}");
        }
    }

    #[test]
    fn quota_bar_line_bolds_only_the_leading_unit() {
        let spans = bar_spans(0.0, green(), "7d 10h", 2, 16, None);
        let bold: String = cells(&spans)
            .iter()
            .filter(|(_, s)| s.add_modifier.contains(Modifier::BOLD))
            .map(|(g, _)| g.as_str())
            .collect();
        assert_eq!(bold, "7d");
    }

    #[test]
    fn quota_bar_countdown_stands_alone() {
        for marker in [None, Some('*'), Some('~')] {
            let c = cells(&bar_spans(0.5, green(), "7d 10h", 2, 32, marker));
            let text: String = c.iter().map(|(g, _)| g.as_str()).collect();
            assert_eq!(c.len(), 32);
            assert_eq!(text.find("7d 10h"), Some(13), "{text:?}");
            assert_eq!(
                c[31].0,
                marker.map_or(" ".to_string(), |m| m.to_string()),
                "the marker keeps the last column"
            );
            for (col, (_, st)) in c.iter().enumerate() {
                assert_eq!(st.fg, Some(Color::Green), "column {col} colour");
                assert_eq!(st.add_modifier.contains(Modifier::REVERSED), col < 16);
            }
            assert!(text[..13].trim().is_empty(), "nothing before the time");
        }
    }

    #[test]
    fn quota_bar_long_countdown_keeps_its_centered_column() {
        for marker in [None, Some('*')] {
            let text = flat(&bar_spans(0.5, green(), "1234d 10h", 5, 16, marker));
            assert_eq!(text::width(&text), 16, "{text:?}");
            assert_eq!(text.find("1234d 10h"), Some(3), "{text:?}");
            assert_eq!(text.chars().last(), Some(marker.unwrap_or(' ')));
        }
    }

    #[test]
    fn quota_bar_minimum_width_shows_the_bare_time() {
        for (time, bold, marker, expected) in [
            ("10/20 12:34", 5, None, "10/20 12:34"),
            ("0s", 2, Some('*'), "    0s    *"),
        ] {
            let text = flat(&bar_spans(0.5, green(), time, bold, 11, marker));
            assert_eq!(text, expected);
        }
    }

    /// The defect llmux had: text placed by `char`, so a two-cell grapheme
    /// counted as one and pushed the bar past its width.
    #[test]
    fn wide_text_is_placed_in_cells_and_never_runs_over_the_marker() {
        for w in 4..20 {
            for marker in [None, Some('*')] {
                let spans = bar_spans(0.3, green(), "리셋 3시간", 2, w, marker);
                let s = flat(&spans);
                assert_eq!(text::width(&s), w, "{w} {marker:?}: {s:?}");
                if let Some(m) = marker {
                    assert!(s.ends_with(m), "{w}: {s:?}");
                }
            }
        }
        // "리셋 3시간" is 10 cells: centred in 16 it starts at cell 3.
        let s = flat(&bar_spans(0.0, green(), "리셋 3시간", 2, 16, None));
        assert_eq!(s, "   리셋 3시간   ");
        let bold: String = cells(&bar_spans(0.0, green(), "리셋 3시간", 2, 16, None))
            .iter()
            .filter(|(_, st)| st.add_modifier.contains(Modifier::BOLD))
            .map(|(g, _)| g.as_str())
            .collect();
        assert_eq!(bold, "리", "two bold cells are one wide grapheme");
    }

    // ---- the whole cell (llmux quota_cell_line) ----

    #[test]
    fn cell_is_bar_space_and_right_aligned_label() {
        let m = Meter::new(0.42).resets_in(Duration::from_secs(2 * 3600 + 13 * 60));
        let line = m.line(18);
        assert_eq!(text::line_width(&line), 18);
        let s = flat(&line.spans);
        assert_eq!(s, "   2h 13m      42%");
        let over = Meter::new(1.0).over(true).line(18);
        assert!(flat(&over.spans).ends_with(" 100%!"));
    }

    #[test]
    fn left_fill_reads_what_is_left_but_colours_by_used() {
        let m = Meter::new(0.25).fill(Fill::Left).theme(Theme::ansi());
        assert_eq!(m.fill_fraction(), 0.75);
        assert_eq!(m.label_text(), "75%");
        let line = m.line(16);
        let c = cells(&line.spans);
        let rev = c
            .iter()
            .take(10)
            .filter(|(_, s)| s.add_modifier.contains(Modifier::REVERSED))
            .count();
        assert_eq!(rev, 8, "round(0.75 × 10)");
        assert!(c.iter().take(10).all(|(_, s)| s.fg == Some(Color::Green)));
        let crit = Meter::new(0.95).fill(Fill::Left).theme(Theme::ansi());
        assert_eq!(crit.label_text(), "5%");
        assert_eq!(crit.line(16).spans[0].style.fg, Some(Color::Red));
    }

    #[test]
    fn unknown_is_an_honest_dash_with_no_fill() {
        let m = Meter::unknown().text("cold", 0).theme(Theme::ansi());
        let line = m.line(16);
        let s = flat(&line.spans);
        assert_eq!(s, "   cold        —");
        assert!(
            cells(&line.spans)
                .iter()
                .all(|(_, st)| !st.add_modifier.contains(Modifier::REVERSED))
        );
        assert_eq!(Meter::new(f64::NAN).label_text(), "—");
        assert_eq!(Meter::from_option(Some(0.5)).label_text(), "50%");
    }

    #[test]
    fn absolute_stamp_uses_the_host_offset_and_expired_reads_zero() {
        // 2026-11-05 09:30 UTC.
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_793_871_000);
        let at = now + Duration::from_secs(14 * 3600 + 30 * 60); // 11-06 00:00 UTC
        assert_eq!(
            Meter::new(0.5).resets_at(at, now, 0).bar_text(),
            ("11/06 00:00".to_string(), 5)
        );
        assert_eq!(
            Meter::new(0.5).resets_at(at, now, 9 * 3600).bar_text(),
            ("11/06 09:00".to_string(), 5)
        );
        assert_eq!(
            Meter::new(0.5).resets_at(now, now, 0).bar_text(),
            ("0s".to_string(), 2)
        );
        assert_eq!(
            Meter::new(0.5).resets_in(Duration::ZERO).bar_text(),
            ("0s".to_string(), 2)
        );
    }

    // ---- narrow widths ----

    #[test]
    fn every_width_is_exact() {
        let metas = [
            Meter::new(0.64).resets_in(Duration::from_secs(8000)),
            Meter::new(0.97).over(true).marker('*'),
            Meter::unknown().text("cold", 0),
            Meter::new(0.3).text("리셋 3시간", 2).marker('*'),
            Meter::new(0.5).label(false),
        ];
        for m in &metas {
            for w in 0..40u16 {
                assert_eq!(text::line_width(&m.line(w)), usize::from(w), "{m:?} {w}");
            }
        }
    }

    #[test]
    fn the_label_gives_way_first_then_the_small_unit() {
        let m = Meter::new(0.64).resets_in(Duration::from_secs(2 * 3600 + 13 * 60));
        assert_eq!(m.full_width(), 13);
        let row = |w: u16| flat(&m.line(w).spans);
        assert_eq!(row(24), "      2h 13m         64%");
        assert_eq!(row(13), "2h 13m    64%", "the narrowest full form");
        assert_eq!(row(12), "   2h 13m   ", "the label gives way");
        assert_eq!(row(6), "2h 13m");
        assert_eq!(row(5), " 2h  ", "then the smaller unit");
        assert_eq!(row(2), "2h");
        assert_eq!(row(1), " ", "then only the fill");
        // Without a reset the percent moves into the bar.
        let bare = Meter::new(0.64);
        assert_eq!(bare.full_width(), 10);
        assert_eq!(flat(&bare.line(9).spans), "   64%   ");
        assert_eq!(flat(&bare.line(2).spans), "  ");
    }

    #[test]
    fn whether_the_label_shows_never_depends_on_the_clock() {
        // A countdown that shrinks from `10h 15m` to `9h 05m` to `42s`
        // keeps the same layout at the same width.
        for w in [12u16, 13, 18] {
            let shows = |secs: u64| {
                flat(
                    &Meter::new(0.5)
                        .resets_in(Duration::from_secs(secs))
                        .line(w)
                        .spans,
                )
                .ends_with("50%")
            };
            assert_eq!(shows(10 * 3600 + 15 * 60), shows(42), "{w}");
            assert_eq!(shows(9 * 3600 + 5 * 60), shows(42), "{w}");
        }
    }

    // ---- themes ----

    #[test]
    fn mono_has_no_colour_and_crit_keeps_its_fill_readable() {
        let t = Theme::mono();
        for m in [
            Meter::new(0.2),
            Meter::new(0.8),
            Meter::new(0.95).over(true),
            Meter::unknown(),
        ] {
            let buf = render(&m.clone().theme(t).resets_in(Duration::from_secs(99)), 20);
            for c in buf.content() {
                assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{m:?}");
            }
        }
        // Crit in mono is bold + reversed on the label, but the bar's empty
        // part must stay un-reversed or the meter would read full.
        let buf = render(&Meter::new(0.95).fill(Fill::Left).theme(t), 20);
        let reversed: Vec<bool> = (0..14)
            .map(|x| buf[(x, 0)].modifier.contains(Modifier::REVERSED))
            .collect();
        assert_eq!(reversed.iter().filter(|r| **r).count(), 1, "{reversed:?}");
        assert!(buf[(19, 0)].modifier.contains(Modifier::REVERSED), "label");
    }

    #[test]
    fn widget_draws_one_row_clipped_to_the_area() {
        let m = Meter::new(0.5)
            .resets_in(Duration::from_secs(42))
            .theme(Theme::ansi().with_motion(Motion::Reduced));
        let mut buf = Buffer::empty(Rect::new(0, 0, 30, 2));
        (&m).render(Rect::new(2, 1, 13, 1), &mut buf);
        assert_eq!(snapshot::lines(&buf), ["", "    42s     50%"]);
        // Out of the buffer: nothing, no panic.
        m.render(Rect::new(40, 5, 10, 1), &mut buf);
    }
}
