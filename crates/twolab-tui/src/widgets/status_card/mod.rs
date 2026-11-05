// Derived from OpenAI Codex (Apache-2.0), commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72:
//   codex-rs/tui/src/status/format.rs        FieldFormatter: aligned `Label:  value` rows
//   codex-rs/tui/src/status/rate_limits.rs   render_limit_progress_bar, "% left" summary
//   codex-rs/tui/src/status/card.rs          how a limit row narrows (bar dropped, reset wrapped)
// Modified by 2lab: a builder over plain data instead of protocol types; `—`
// for unknown values; the bar and the percentage take the theme's level
// colours; braille bar glyphs by default (codex's `█` is East-Asian
// Ambiguous) with codex's blocks and ASCII as options.

//! An account or session at a glance: aligned `label: value` rows and a
//! compact usage bar per limit, with when it resets.
//!
//! Ported from OpenAI Codex `codex-rs/tui/src/status/format.rs` and
//! `status/rate_limits.rs` (Apache-2.0) — the `/status` card:
//!
//! ```text
//!   Model:     opus-4.5 (thinking high)
//!   5h limit:  [⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣀⣀⣀⣀⣀⣀⣀⣀⣀] 55% left (resets 09:25)
//!   7d limit:  —
//! ```
//!
//! Labels are sized to the widest label in cells and dimmed; values line up
//! in one column. A limit shows a bar of what is *left* (the part you can
//! still spend), coloured by how much is used. When the row runs out of room
//! the bar goes first and the percentage stays, and the reset time moves to
//! its own line rather than being cut. Text values wrap by words under their
//! column. An unknown value is `—`, never a made-up `0`.
//!
//! Bar glyphs: codex draws `█░`; `█` (U+2588) is East-Asian *Ambiguous* and
//! takes two cells on many CJK terminals, so the default is braille `⣿⣀`
//! (Narrow). [`BarGlyphs::BLOCK`] opts in to codex's look.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::text::{self, ELLIPSIS};
use crate::theme::{Level, Theme};
use crate::widgets::working::wrap;

/// What a value that is not known shows.
pub const UNKNOWN: &str = "—";

/// Upstream's bar length.
pub const DEFAULT_SEGMENTS: usize = 20;

/// The two glyphs of a usage bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BarGlyphs {
    pub filled: char,
    pub empty: char,
}

impl BarGlyphs {
    /// Braille full and baseline cells: East-Asian Narrow. The default.
    pub const NARROW: BarGlyphs = BarGlyphs {
        filled: '⣿',
        empty: '⣀',
    };
    /// Codex's `█░`. `█` is East-Asian Ambiguous: opt in where the terminal
    /// is known to draw it in one cell.
    pub const BLOCK: BarGlyphs = BarGlyphs {
        filled: '█',
        empty: '░',
    };
    /// `#-`, for logs and plain-text copies.
    pub const ASCII: BarGlyphs = BarGlyphs {
        filled: '#',
        empty: '-',
    };
}

impl Default for BarGlyphs {
    fn default() -> Self {
        BarGlyphs::NARROW
    }
}

/// `[⣿⣿⣿⣀⣀]`: `segments` cells, filled for the part left. `percent_left` is
/// clamped to 0–100; pass what is left, not what is used, or the bar reads
/// backwards.
pub fn limit_bar(percent_left: f64, segments: usize, glyphs: BarGlyphs) -> String {
    let (filled, empty) = bar_split(percent_left, segments);
    let mut s = String::with_capacity(2 + 3 * segments);
    s.push('[');
    s.extend(std::iter::repeat_n(glyphs.filled, filled));
    s.extend(std::iter::repeat_n(glyphs.empty, empty));
    s.push(']');
    s
}

fn bar_split(percent_left: f64, segments: usize) -> (usize, usize) {
    let ratio = if percent_left.is_finite() {
        (percent_left / 100.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = ((ratio * segments as f64).round() as usize).min(segments);
    (filled, segments - filled)
}

/// One usage limit: how much is left and when it resets.
#[derive(Debug, Clone, PartialEq)]
pub struct Limit<'a> {
    left: Option<f64>,
    resets: Option<Cow<'a, str>>,
    warn_at: f64,
    crit_at: f64,
}

impl<'a> Limit<'a> {
    /// `percent` (0–100) still left. Not finite reads as unknown.
    pub fn left(percent: f64) -> Limit<'a> {
        Limit {
            left: percent.is_finite().then(|| percent.clamp(0.0, 100.0)),
            resets: None,
            warn_at: 0.8,
            crit_at: 0.95,
        }
    }

    /// `percent` (0–100) already used.
    pub fn used(percent: f64) -> Limit<'a> {
        Limit::left(100.0 - percent)
    }

    /// Not known (no data yet): shown as `—`, not as an empty bar.
    pub fn unknown() -> Limit<'a> {
        Limit {
            left: None,
            ..Limit::left(0.0)
        }
    }

    /// When it resets, as the host formats it (`09:25`, `Nov 9 09:55`, `in 2h`).
    pub fn resets(mut self, when: impl Into<Cow<'a, str>>) -> Limit<'a> {
        self.resets = Some(when.into());
        self
    }

    /// Used fractions (0.0–1.0) where the colour turns warn and crit.
    /// Default 0.8 and 0.95.
    pub fn thresholds(mut self, warn_at: f64, crit_at: f64) -> Limit<'a> {
        self.warn_at = warn_at;
        self.crit_at = crit_at;
        self
    }

    pub fn percent_left(&self) -> Option<f64> {
        self.left
    }

    /// How close to the end it is; unknown reads as Ok.
    pub fn level(&self) -> Level {
        self.left.map_or(Level::Ok, |l| {
            Level::of(1.0 - l / 100.0, self.warn_at, self.crit_at)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Value<'a> {
    Text(Cow<'a, str>),
    Limit(Limit<'a>),
}

#[derive(Debug, Clone, PartialEq)]
enum Entry<'a> {
    Field(Cow<'a, str>, Value<'a>),
    Gap,
}

/// Aligned `label: value` rows and usage bars.
#[derive(Debug, Clone, PartialEq)]
pub struct StatusCard<'a> {
    entries: Vec<Entry<'a>>,
    indent: usize,
    segments: usize,
    glyphs: BarGlyphs,
    theme: Theme,
}

impl Default for StatusCard<'_> {
    fn default() -> Self {
        StatusCard::new()
    }
}

impl<'a> StatusCard<'a> {
    pub fn new() -> StatusCard<'a> {
        StatusCard {
            entries: Vec::new(),
            indent: 2,
            segments: DEFAULT_SEGMENTS,
            glyphs: BarGlyphs::NARROW,
            theme: Theme::default(),
        }
    }

    /// A text row. Empty or blank text shows `—`.
    pub fn field(
        mut self,
        label: impl Into<Cow<'a, str>>,
        value: impl Into<Cow<'a, str>>,
    ) -> StatusCard<'a> {
        self.entries
            .push(Entry::Field(label.into(), Value::Text(value.into())));
        self
    }

    /// A row whose value is not known: `—`.
    pub fn unknown(self, label: impl Into<Cow<'a, str>>) -> StatusCard<'a> {
        self.field(label, "")
    }

    /// A usage-limit row.
    pub fn limit(mut self, label: impl Into<Cow<'a, str>>, limit: Limit<'a>) -> StatusCard<'a> {
        self.entries
            .push(Entry::Field(label.into(), Value::Limit(limit)));
        self
    }

    /// A blank row between sections.
    pub fn gap(mut self) -> StatusCard<'a> {
        self.entries.push(Entry::Gap);
        self
    }

    /// Cells before each label. Default 2.
    pub fn indent(mut self, indent: usize) -> StatusCard<'a> {
        self.indent = indent;
        self
    }

    /// Bar length in cells, brackets excluded. Default 20.
    pub fn segments(mut self, segments: usize) -> StatusCard<'a> {
        self.segments = segments.max(1);
        self
    }

    pub fn glyphs(mut self, glyphs: BarGlyphs) -> StatusCard<'a> {
        self.glyphs = glyphs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> StatusCard<'a> {
        self.theme = theme;
        self
    }

    fn label_width(&self) -> usize {
        self.entries
            .iter()
            .filter_map(|e| match e {
                Entry::Field(l, _) => Some(text::width(l)),
                Entry::Gap => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// Where values start: indent, the widest label, `:` and two spaces.
    pub fn value_offset(&self) -> usize {
        self.indent + self.label_width() + 3
    }

    /// The card laid out for `width` cells.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let width = width as usize;
        let t = &self.theme;
        let offset = self.value_offset();
        let value_w = width.saturating_sub(offset).max(1);
        let lw = self.label_width();
        let cont = || Span::raw(" ".repeat(offset));
        let mut out: Vec<Line<'static>> = Vec::new();
        for e in &self.entries {
            let (label, value) = match e {
                Entry::Gap => {
                    out.push(Line::default());
                    continue;
                }
                Entry::Field(l, v) => (l, v),
            };
            let head = Span::styled(
                format!(
                    "{}{label}:{}",
                    " ".repeat(self.indent),
                    " ".repeat(2 + lw - text::width(label))
                ),
                t.dim(),
            );
            match value {
                Value::Text(v) if v.trim().is_empty() => {
                    out.push(Line::from(vec![head, Span::styled(UNKNOWN, t.dim())]));
                }
                Value::Text(v) => {
                    for (i, row) in wrap(v.trim(), value_w).into_iter().enumerate() {
                        let lead = if i == 0 { head.clone() } else { cont() };
                        out.push(Line::from(vec![lead, Span::styled(row, t.text())]));
                    }
                }
                Value::Limit(limit) => {
                    let mut spans = vec![head];
                    match limit.left {
                        None => spans.push(Span::styled(UNKNOWN, t.dim())),
                        Some(left) => {
                            let summary =
                                Span::styled(format!("{left:.0}% left"), t.level(limit.level()));
                            let bar = self.bar_spans(left, limit.level());
                            let full = text::spans_width(&bar) + 1 + text::width(&summary.content);
                            if full <= value_w {
                                spans.extend(bar);
                                spans.push(Span::raw(" "));
                            }
                            spans.push(summary);
                        }
                    }
                    if let Some(r) = &limit.resets {
                        let note = format!("(resets {r})");
                        if text::spans_width(&spans) + 1 + text::width(&note) <= width {
                            spans.push(Span::styled(format!(" {note}"), t.dim()));
                            out.push(Line::from(spans));
                        } else {
                            // The reset time is what the person acts on: give
                            // it its own lines instead of cutting it.
                            out.push(Line::from(spans));
                            for row in wrap(&note, value_w) {
                                out.push(Line::from(vec![cont(), Span::styled(row, t.dim())]));
                            }
                        }
                    } else {
                        out.push(Line::from(spans));
                    }
                }
            }
        }
        out.into_iter()
            .map(|l| text::truncate_line(l, width))
            .collect()
    }

    pub fn height(&self, width: u16) -> u16 {
        self.lines(width).len().min(u16::MAX as usize) as u16
    }

    fn bar_spans(&self, left: f64, level: Level) -> Vec<Span<'static>> {
        let t = &self.theme;
        let (filled, empty) = bar_split(left, self.segments);
        // Colour carries the level on the bar; mono keeps it on the
        // percentage's weight instead (bold, reversed) — a reversed bar
        // would read as a different glyph.
        let fill = if t.is_mono() {
            Style::new()
        } else {
            Style::new().fg(t.level_color(level))
        };
        let g = self.glyphs;
        vec![
            Span::styled("[", t.dim()),
            Span::styled(g.filled.to_string().repeat(filled), fill),
            Span::styled(g.empty.to_string().repeat(empty), t.dim()),
            Span::styled("]", t.dim()),
        ]
    }
}

impl Widget for StatusCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

impl Widget for &StatusCard<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut lines = self.lines(area.width);
        let rows = area.height as usize;
        if lines.len() > rows {
            // Never cut silently: the last row says how many are hidden.
            let hidden = lines.len() - rows + 1;
            lines.truncate(rows - 1);
            let note = format!("{}{ELLIPSIS} +{hidden} more", " ".repeat(self.indent));
            lines.push(text::truncate_line(
                Line::from(Span::styled(note, self.theme.dim())),
                area.width as usize,
            ));
        }
        for (i, line) in lines.iter().enumerate() {
            buf.set_line(area.x, area.y + i as u16, line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::{Color, Modifier};
    use unicode_width::UnicodeWidthChar;

    fn render(c: &StatusCard, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        c.render(area, &mut buf);
        buf
    }

    fn card() -> StatusCard<'static> {
        StatusCard::new()
            .field("Model", "opus-4.5 (thinking high)")
            .field("Account", "zhuge@2lab.ai (Max 20x)")
            .gap()
            .limit("5h limit", Limit::left(55.0).resets("09:25"))
            .limit("Weekly limit", Limit::used(30.0).resets("09:55"))
    }

    /// Upstream `render_limit_progress_bar` geometry: 20 cells, rounded,
    /// clamped.
    #[test]
    fn bar_rounds_and_clamps() {
        assert_eq!(
            limit_bar(55.0, 20, BarGlyphs::BLOCK),
            "[███████████░░░░░░░░░]"
        );
        assert_eq!(
            limit_bar(70.0, 20, BarGlyphs::BLOCK),
            "[██████████████░░░░░░]"
        );
        assert_eq!(
            limit_bar(28.0, 20, BarGlyphs::ASCII),
            "[######--------------]"
        );
        assert_eq!(limit_bar(150.0, 4, BarGlyphs::ASCII), "[####]");
        assert_eq!(limit_bar(-5.0, 4, BarGlyphs::ASCII), "[----]");
        assert_eq!(limit_bar(f64::NAN, 4, BarGlyphs::ASCII), "[----]");
    }

    /// Upstream `status_snapshot_includes_credits_and_limits`, the limit rows.
    #[test]
    fn rows_align_on_the_widest_label() {
        let buf = render(&card().glyphs(BarGlyphs::BLOCK), 80, 5);
        assert_eq!(
            snapshot::text(&buf),
            "  Model:         opus-4.5 (thinking high)
  Account:       zhuge@2lab.ai (Max 20x)

  5h limit:      [███████████░░░░░░░░░] 55% left (resets 09:25)
  Weekly limit:  [██████████████░░░░░░] 70% left (resets 09:55)"
        );
        assert_eq!(card().value_offset(), 2 + 12 + 3);
    }

    /// Upstream narrow behaviour (`status_snapshot_truncates_in_narrow_terminal`):
    /// the bar goes before the percentage, the reset time wraps instead of
    /// being cut, text values wrap under their column.
    #[test]
    fn narrow_drops_the_bar_and_wraps_the_reset() {
        let c = card();
        assert_eq!(
            c.lines(36)
                .iter()
                .map(|l| l
                    .spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>())
                .map(|s| s.trim_end().to_string())
                .collect::<Vec<_>>(),
            [
                "  Model:         opus-4.5 (thinking",
                "                 high)",
                "  Account:       zhuge@2lab.ai (Max",
                "                 20x)",
                "",
                "  5h limit:      55% left",
                "                 (resets 09:25)",
                "  Weekly limit:  70% left",
                "                 (resets 09:55)",
            ]
        );
        assert_eq!(c.height(36), 9);
        for l in c.lines(36) {
            assert!(text::line_width(&l) <= 36);
        }
    }

    #[test]
    fn unknown_values_are_a_dash_never_zero() {
        let c = StatusCard::new()
            .unknown("Plan")
            .field("Credits", "  ")
            .limit("7d limit", Limit::unknown().resets("Nov 9 09:55"))
            .limit("5h limit", Limit::left(f64::NAN))
            .theme(Theme::dark());
        let buf = render(&c, 60, 4);
        assert_eq!(
            snapshot::text(&buf),
            "  Plan:      —
  Credits:   —
  7d limit:  — (resets Nov 9 09:55)
  5h limit:  —"
        );
        assert!(!snapshot::text(&buf).contains("0%"));
        assert_eq!(buf[(13, 0)].fg, Theme::dark().muted);
    }

    #[test]
    fn levels_colour_the_bar_and_the_percentage() {
        let t = Theme::dark();
        let c = StatusCard::new()
            .limit("5h", Limit::left(3.0))
            .limit("7d", Limit::left(15.0))
            .limit("opus", Limit::left(60.0))
            .theme(t);
        let buf = render(&c, 60, 3);
        // `  5h:    [` → the bar's first cell is at 10.
        let first_fill = |y: u16| buf[(10, y)].fg;
        assert_eq!(first_fill(0), t.crit);
        assert_eq!(first_fill(1), t.warn);
        assert_eq!(first_fill(2), t.ok);
        // Bar from 9 to 30, a space, then the percentage.
        let pct_x = 9 + 22 + 1;
        assert_eq!(buf[(pct_x, 0)].fg, t.crit);
        assert_eq!(buf[(pct_x - 3, 0)].fg, t.muted, "empty segments are dim");
    }

    #[test]
    fn mono_keeps_levels_in_weight_without_colour() {
        let c = StatusCard::new()
            .limit("5h", Limit::left(3.0))
            .limit("7d", Limit::left(15.0))
            .theme(Theme::mono());
        let buf = render(&c, 60, 2);
        assert!(
            buf.content()
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
        );
        let pct_x = 9 + 22 + 1;
        assert!(buf[(pct_x, 0)].modifier.contains(Modifier::REVERSED));
        assert!(buf[(pct_x, 1)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(pct_x, 1)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn short_area_says_rows_were_cut() {
        let buf = render(&card(), 80, 3);
        assert_eq!(snapshot::lines(&buf)[2], "  … +3 more");
        for (w, h) in [(1, 1), (2, 2), (12, 2), (20, 1)] {
            let _ = render(&card(), w, h);
        }
        assert_eq!(StatusCard::new().height(20), 0);
    }

    #[test]
    fn cjk_labels_align_by_cells() {
        let c = StatusCard::new()
            .field("모델", "opus-4.5")
            .field("Plan", "Max 20x");
        let buf = render(&c, 40, 2);
        assert_eq!(snapshot::text(&buf), "  모델:  opus-4.5\n  Plan:  Max 20x");
    }

    #[test]
    fn default_bar_glyphs_are_east_asian_narrow() {
        for c in [BarGlyphs::NARROW.filled, BarGlyphs::NARROW.empty, '[', ']'] {
            assert_eq!(c.width_cjk(), Some(1), "{c:?}");
        }
        assert_eq!(
            BarGlyphs::BLOCK.filled.width_cjk(),
            Some(2),
            "█ is Ambiguous"
        );
    }
}
