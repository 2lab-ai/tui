//! A trend chart: how each top series moved day by day.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `draw_daily_chart`
//! (tokens per day per model, the top four as braille lines over dim axes
//! with three labels each, and a one-line legend), `draw_perf_chart`
//! (tokens/sec, where a day without samples is a gap, never a fabricated
//! zero, and a thin day — fewer than five samples — is drawn dim) and the
//! legend both drew inline.
//!
//! Drawn on ratatui's [`Canvas`] with braille lines, as llmux did through
//! `Chart`; the axes are drawn here so the plot rect is known exactly.
//! Changes from llmux:
//! - series colours come from a small theme ramp ([`series_color`]); under
//!   [`Theme::mono`] the series keep apart by a glyph at every sample
//!   (`*` `+` `x` `o`, also in the legend) and thin samples by the dim
//!   weight;
//! - the join between a thin and a confident sample is drawn dim in both
//!   directions (llmux dropped the segment from a thin day back into a
//!   confident one, which read as a gap);
//! - more than four series, or a legend wider than the row, are counted
//!   (`+N more`), not cut.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};

use crate::text::{self, Align};
use crate::theme::Theme;

/// Series drawn at once (llmux `DAILY_CHART_SERIES`): more turns a trend
/// into noise. The rest are counted in the legend.
pub const MAX_SERIES: usize = 4;

/// Under [`Theme::mono`], the glyph that marks each series' samples.
pub const MARKERS: [char; MAX_SERIES] = ['*', '+', 'x', 'o'];

/// The legend swatch in colour: a short braille line.
const SWATCH: &str = "⠒⠒";

/// The colour of series `i` (by rank): accent, wait, done, fail — four
/// hues every palette keeps apart. Mono gives `Color::Reset` for all; the
/// series then differ by [`MARKERS`].
pub fn series_color(theme: &Theme, i: usize) -> Color {
    [theme.accent, theme.wait, theme.done, theme.fail][i % MAX_SERIES]
}

/// One line of the chart: a value per day, `None` where nothing was seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Series<'a> {
    pub name: Cow<'a, str>,
    pub points: Vec<Option<f64>>,
    /// Per point: the sample is thin, draw it dim. Missing entries are
    /// confident.
    pub low: Vec<bool>,
    /// After the name in the legend (`41.2%`).
    pub note: Option<Cow<'a, str>>,
}

impl<'a> Series<'a> {
    pub fn new(
        name: impl Into<Cow<'a, str>>,
        points: impl IntoIterator<Item = Option<f64>>,
    ) -> Series<'a> {
        Series {
            name: name.into(),
            points: points.into_iter().collect(),
            low: Vec::new(),
            note: None,
        }
    }

    pub fn low(mut self, low: impl IntoIterator<Item = bool>) -> Series<'a> {
        self.low = low.into_iter().collect();
        self
    }

    pub fn note(mut self, note: impl Into<Cow<'a, str>>) -> Series<'a> {
        self.note = Some(note.into());
        self
    }

    fn is_low(&self, x: usize) -> bool {
        self.low.get(x).copied().unwrap_or(false)
    }

    fn value(&self, x: usize) -> Option<f64> {
        self.points
            .get(x)
            .copied()
            .flatten()
            .filter(|v| v.is_finite())
    }
}

/// A run of samples drawn as one line: series rank, thin or not, points.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub series: usize,
    pub low: bool,
    pub points: Vec<(f64, f64)>,
}

/// Split a series into segments at gaps and wherever the confidence
/// changes (llmux `draw_perf_chart`). The step between a thin and a
/// confident sample belongs to the thin segment, so no thin sample is ever
/// drawn bright and no step reads as a gap.
pub fn segments(series: &Series, rank: usize) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut current: Vec<(f64, f64)> = Vec::new();
    let mut current_low = false;
    let close = |out: &mut Vec<Segment>, points: Vec<(f64, f64)>, low: bool| {
        if !points.is_empty() {
            out.push(Segment {
                series: rank,
                low,
                points,
            });
        }
    };
    for x in 0..series.points.len() {
        let Some(y) = series.value(x) else {
            close(&mut out, std::mem::take(&mut current), current_low);
            continue;
        };
        let p = (x as f64, y);
        let low = series.is_low(x);
        if low != current_low && !current.is_empty() {
            if low {
                // Confident → thin: the thin run starts at the last
                // confident sample.
                let last = current[current.len() - 1];
                close(&mut out, std::mem::take(&mut current), current_low);
                current.push(last);
            } else {
                // Thin → confident: the thin run ends at this sample.
                current.push(p);
                close(&mut out, std::mem::take(&mut current), current_low);
            }
        }
        current_low = low;
        current.push(p);
    }
    close(&mut out, current, current_low);
    out
}

/// `1.2k`, `3.4M`, `243.6G` (llmux `human_count`, for any non-negative value).
/// Values of a thousand and up use [`crate::format::human_count`]'s ladder, so
/// an axis label never reads `1000.0k` and never runs out of units.
pub fn human(v: f64) -> String {
    if v >= 1_000.0 {
        crate::format::human_count(v as u64)
    } else {
        format!("{v:.0}")
    }
}

/// Lines per day for the top series, with gaps and dim thin samples.
#[derive(Debug, Clone)]
pub struct TrendChart<'a> {
    series: Vec<Series<'a>>,
    x_labels: Vec<Cow<'a, str>>,
    y_format: fn(f64) -> String,
    y_unit: Option<Cow<'a, str>>,
    legend: bool,
    low_note: Cow<'a, str>,
    empty: Cow<'a, str>,
    theme: Theme,
}

impl<'a> TrendChart<'a> {
    /// Series in rank order; the first [`MAX_SERIES`] are drawn.
    pub fn new(series: impl IntoIterator<Item = Series<'a>>) -> TrendChart<'a> {
        TrendChart {
            series: series.into_iter().collect(),
            x_labels: Vec::new(),
            y_format: human,
            y_unit: None,
            legend: true,
            low_note: Cow::Borrowed("dim = few samples"),
            empty: Cow::Borrowed("no data in this span"),
            theme: Theme::default(),
        }
    }

    /// Labels under the x axis: first day, middle day, last day.
    pub fn x_labels<S: Into<Cow<'a, str>>>(mut self, labels: [S; 3]) -> TrendChart<'a> {
        self.x_labels = labels.into_iter().map(Into::into).collect();
        self
    }

    /// How a y value reads (default [`human`]: `48.2M`).
    pub fn y_format(mut self, f: fn(f64) -> String) -> TrendChart<'a> {
        self.y_format = f;
        self
    }

    /// After the top y label (`t/s`).
    pub fn y_unit(mut self, unit: impl Into<Cow<'a, str>>) -> TrendChart<'a> {
        self.y_unit = Some(unit.into());
        self
    }

    /// The legend line under the chart. Default on.
    pub fn legend(mut self, on: bool) -> TrendChart<'a> {
        self.legend = on;
        self
    }

    /// What dim means, said in the legend when a thin sample is drawn.
    pub fn low_note(mut self, note: impl Into<Cow<'a, str>>) -> TrendChart<'a> {
        self.low_note = note.into();
        self
    }

    /// Said instead of a chart when no series has a single value.
    pub fn empty(mut self, text: impl Into<Cow<'a, str>>) -> TrendChart<'a> {
        self.empty = text.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> TrendChart<'a> {
        self.theme = theme;
        self
    }

    fn shown(&self) -> &[Series<'a>] {
        &self.series[..self.series.len().min(MAX_SERIES)]
    }

    fn days(&self) -> usize {
        self.shown()
            .iter()
            .map(|s| s.points.len())
            .max()
            .unwrap_or(0)
    }

    fn y_max(&self) -> Option<f64> {
        self.shown()
            .iter()
            .flat_map(|s| (0..s.points.len()).filter_map(|x| s.value(x)))
            .reduce(f64::max)
    }

    fn any_low(&self) -> bool {
        self.shown()
            .iter()
            .any(|s| (0..s.points.len()).any(|x| s.is_low(x) && s.value(x).is_some()))
    }

    /// The legend line, fitted to `width`: the thin-sample note gives way
    /// first, then series from the end, which are counted.
    pub fn legend_line(&self, width: usize) -> Line<'static> {
        let t = &self.theme;
        let mono = t.is_mono();
        let item = |i: usize, s: &Series| {
            let mut spans = Vec::with_capacity(4);
            if mono {
                spans.push(Span::styled(MARKERS[i].to_string(), t.text()));
            } else {
                spans.push(Span::styled(SWATCH, Style::new().fg(series_color(t, i))));
            }
            spans.push(Span::styled(format!(" {}", s.name), t.text()));
            if let Some(note) = &s.note {
                spans.push(Span::styled(format!(" {note}"), t.dim()));
            }
            spans
        };
        let items: Vec<Vec<Span<'static>>> = self
            .shown()
            .iter()
            .enumerate()
            .map(|(i, s)| item(i, s))
            .collect();
        let extra = self.series.len() - self.shown().len();
        let low =
            (self.any_low() && !self.low_note.is_empty()).then(|| format!("  ({})", self.low_note));
        let build = |keep: usize, with_low: bool| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, it) in items.iter().take(keep).enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                spans.extend(it.iter().cloned());
            }
            let more = extra + items.len() - keep;
            if more > 0 {
                spans.push(Span::styled(format!("  +{more} more"), t.dim()));
            }
            if let (true, Some(note)) = (with_low, &low) {
                spans.push(Span::styled(note.clone(), t.dim()));
            }
            Line::from(spans)
        };
        // Everything; then without the note; then fewer series, counted.
        let n = items.len();
        let tries = std::iter::once((n, true)).chain((1..=n).rev().map(|k| (k, false)));
        for (keep, with_low) in tries {
            let line = build(keep, with_low);
            if text::line_width(&line) <= width {
                return line;
            }
        }
        text::truncate_line(build(n.min(1), false), width)
    }
}

/// The bits of a braille cell (`None` for anything else).
fn braille_bits(symbol: &str) -> Option<u8> {
    let mut cs = symbol.chars();
    match (cs.next(), cs.next()) {
        (Some(' '), None) => Some(0),
        (Some(c), None) if ('\u{2800}'..='\u{28FF}').contains(&c) => {
            Some((c as u32 - 0x2800) as u8)
        }
        _ => None,
    }
}

fn braille(bits: u8) -> char {
    char::from_u32(0x2800 + u32::from(bits)).unwrap_or(' ')
}

/// Draw `segments` on a braille canvas over `rect`, every line in `color`.
fn draw_canvas(
    segs: &[&Segment],
    rect: Rect,
    x_max: f64,
    y_max: f64,
    color: impl Fn(&Segment) -> Color,
    buf: &mut Buffer,
) {
    Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([0.0, x_max])
        .y_bounds([0.0, y_max])
        .paint(|ctx| {
            for seg in segs {
                let c = color(seg);
                ctx.draw(&Points {
                    coords: &seg.points,
                    color: c,
                });
                for w in seg.points.windows(2) {
                    ctx.draw(&CanvasLine::new(w[0].0, w[0].1, w[1].0, w[1].1, c));
                }
            }
        })
        .render(rect, buf);
}

impl Widget for &TrendChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        let days = self.days();
        let Some(y_max) = self.y_max().filter(|_| days > 0) else {
            let msg = text::truncate(&self.empty, area.width as usize).into_owned();
            buf.set_stringn(area.x, area.y, msg, area.width as usize, t.dim());
            return;
        };
        let y_max = if y_max > 0.0 { y_max } else { 1.0 };

        // Rows: plot, x axis, x labels, legend — the legend stays longest,
        // then the axis, then the labels.
        let legend_h = u16::from(self.legend && area.height >= 2);
        let rem = area.height - legend_h;
        let labels_h = u16::from(rem >= 4 && !self.x_labels.is_empty());
        let axis_h = u16::from(rem >= 3);
        let plot_h = rem - labels_h - axis_h;

        let fmt = self.y_format;
        let mut top = fmt(y_max);
        if let Some(unit) = &self.y_unit {
            top = format!("{top} {unit}");
        }
        let mid = fmt(y_max / 2.0);
        let y_labels: Vec<(u16, String)> = match plot_h {
            0 => vec![],
            1 => vec![(0, top)],
            2 => vec![(0, top), (1, "0".into())],
            h => vec![(0, top), ((h - 1) / 2, mid), (h - 1, "0".into())],
        };
        let mut gutter = y_labels
            .iter()
            .map(|(_, l)| text::width(l))
            .max()
            .unwrap_or(0);
        if (area.width as usize) < gutter + 1 + 8 {
            gutter = 0;
        }
        let axis = gutter > 0 || axis_h > 0;
        let plot_x = area.x + gutter as u16 + u16::from(axis && gutter > 0);
        let plot = Rect::new(plot_x, area.y, area.right() - plot_x, plot_h);

        let dim = t.dim();
        for (row, label) in &y_labels {
            if gutter > 0 {
                let s = text::pad(label, gutter, Align::Right);
                buf.set_stringn(area.x, area.y + row, s, gutter, dim);
            }
        }
        if gutter > 0 {
            for y in plot.top()..plot.bottom() {
                buf.set_stringn(area.x + gutter as u16, y, "│", 1, dim);
            }
        }
        if axis_h > 0 {
            let y = plot.bottom();
            let x0 = if gutter > 0 {
                area.x + gutter as u16
            } else {
                plot.x
            };
            let rule = if gutter > 0 {
                format!("└{}", "─".repeat(plot.width as usize))
            } else {
                "─".repeat(plot.width as usize)
            };
            buf.set_stringn(x0, y, rule, (area.right() - x0) as usize, dim);
        }
        if labels_h > 0 {
            let y = plot.bottom() + axis_h;
            let w = plot.width as usize;
            let first = text::truncate(&self.x_labels[0], w).into_owned();
            let last = self
                .x_labels
                .get(2)
                .map(|s| s.to_string())
                .unwrap_or_default();
            let mid = self
                .x_labels
                .get(1)
                .map(|s| s.to_string())
                .unwrap_or_default();
            let (fw, lw, mw) = (text::width(&first), text::width(&last), text::width(&mid));
            buf.set_stringn(plot.x, y, &first, w, dim);
            if fw + 1 + lw <= w {
                buf.set_stringn(plot.right() - lw as u16, y, &last, lw, dim);
                let mx = (w / 2).saturating_sub(mw / 2);
                if mx > fw && mx + mw < w - lw {
                    buf.set_stringn(plot.x + mx as u16, y, &mid, mw, dim);
                }
            }
        }

        if !plot.is_empty() {
            let x_max = (days.saturating_sub(1)).max(1) as f64;
            let segs: Vec<Segment> = self
                .shown()
                .iter()
                .enumerate()
                .flat_map(|(i, s)| segments(s, i))
                .collect();
            let (low, sure): (Vec<&Segment>, Vec<&Segment>) = segs.iter().partition(|s| s.low);
            draw_canvas(
                &sure,
                plot,
                x_max,
                y_max,
                |s| series_color(t, s.series),
                buf,
            );
            if !low.is_empty() {
                // Thin runs on their own canvas, merged in under the
                // confident lines and painted dim.
                let mut scratch = Buffer::empty(plot);
                draw_canvas(&low, plot, x_max, y_max, |_| Color::Reset, &mut scratch);
                for y in plot.top()..plot.bottom() {
                    for x in plot.left()..plot.right() {
                        let Some(thin) = braille_bits(scratch[(x, y)].symbol()) else {
                            continue;
                        };
                        if thin == 0 {
                            continue;
                        }
                        let cell = &mut buf[(x, y)];
                        match braille_bits(cell.symbol()) {
                            Some(0) => {
                                cell.set_char(braille(thin));
                                cell.set_style(dim);
                            }
                            Some(sure) => {
                                cell.set_char(braille(sure | thin));
                            }
                            None => {}
                        }
                    }
                }
            }
            if t.is_mono() {
                // No colour: a glyph at every sample says which series.
                let (w, h) = (f64::from(plot.width), f64::from(plot.height));
                for (i, s) in self.shown().iter().enumerate() {
                    for x in 0..s.points.len() {
                        let Some(y) = s.value(x) else { continue };
                        let dx = (x as f64 * (w * 2.0 - 1.0) / x_max).round();
                        let dy = ((y_max - y) * (h * 4.0 - 1.0) / y_max).round();
                        let cx = plot.x + ((dx / 2.0) as u16).min(plot.width - 1);
                        let cy = plot.y + ((dy / 4.0) as u16).min(plot.height - 1);
                        let style = if s.is_low(x) {
                            dim
                        } else {
                            Style::new().remove_modifier(Modifier::DIM)
                        };
                        buf.set_stringn(cx, cy, MARKERS[i].to_string(), 1, style);
                    }
                }
            }
        }

        if legend_h > 0 {
            let line = self.legend_line(area.width as usize);
            buf.set_line(area.x, area.bottom() - 1, &line, area.width);
        }
    }
}

impl Widget for TrendChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    fn pts(v: &[Option<f64>]) -> Vec<Option<f64>> {
        v.to_vec()
    }

    #[test]
    fn gaps_split_segments_and_never_become_zero() {
        let s = Series::new("x", pts(&[Some(1.0), Some(2.0), None, None, Some(3.0)]));
        let segs = segments(&s, 0);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].points, vec![(0.0, 1.0), (1.0, 2.0)]);
        assert_eq!(
            segs[1].points,
            vec![(4.0, 3.0)],
            "an isolated day stays a point"
        );
        assert!(segs.iter().all(|s| s.points.iter().all(|p| p.1 != 0.0)));
        // NaN is no data too.
        let s = Series::new("x", pts(&[Some(1.0), Some(f64::NAN), Some(2.0)]));
        assert_eq!(segments(&s, 0).len(), 2);
    }

    /// llmux `draw_perf_chart`: thin samples are their own dim segment; the
    /// join into and out of it is dim, so no thin sample is drawn bright.
    #[test]
    fn thin_runs_own_both_joins() {
        let s = Series::new("x", (0..6).map(|i| Some(i as f64 + 1.0)))
            .low([false, false, true, true, false, false]);
        let segs = segments(&s, 2);
        let shape: Vec<(bool, Vec<f64>)> = segs
            .iter()
            .map(|g| (g.low, g.points.iter().map(|p| p.0).collect()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (false, vec![0.0, 1.0]),
                (true, vec![1.0, 2.0, 3.0, 4.0]),
                (false, vec![4.0, 5.0]),
            ]
        );
        assert!(segs.iter().all(|g| g.series == 2));
    }

    fn render(chart: &TrendChart, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        chart.render(buf.area, &mut buf);
        buf
    }

    fn week() -> [&'static str; 3] {
        ["10-30", "11-02", "11-05"]
    }

    #[test]
    fn draws_axes_labels_and_legend() {
        let chart = TrendChart::new([
            Series::new("opus-4-8[1m]", (0..7).map(|i| Some(40e6 + i as f64 * 2e6))).note("62.0%"),
            Series::new("grok-5", (0..7).map(|_| Some(9.4e6))).note("38.0%"),
        ])
        .x_labels(week())
        .theme(Theme::ansi());
        let buf = render(&chart, 60, 10);
        let rows = snapshot::lines(&buf);
        assert!(rows[0].starts_with("52.0M│"), "{rows:?}");
        assert!(rows[6].starts_with("    0│"), "{rows:?}");
        assert!(rows[7].starts_with("     └────"), "{rows:?}");
        assert!(rows[8].starts_with("      10-30"), "{rows:?}");
        assert!(rows[8].ends_with("11-05"), "{rows:?}");
        assert!(rows[8].contains("11-02"), "{rows:?}");
        assert_eq!(rows[9], "⠒⠒ opus-4-8[1m] 62.0%  ⠒⠒ grok-5 38.0%");
        // Series colours from the ramp, in the plot and the legend.
        let colours: Vec<Color> = buf.content().iter().map(|c| c.fg).collect();
        assert!(colours.contains(&Color::Cyan) && colours.contains(&Color::Yellow));
        assert_eq!(buf[(0, 9)].fg, Color::Cyan);
        assert_eq!(buf[(23, 9)].fg, Color::Yellow);
    }

    #[test]
    fn a_gap_is_drawn_as_nothing() {
        let mut v: Vec<Option<f64>> = vec![Some(10.0); 7];
        v[3] = None;
        let chart = TrendChart::new([Series::new("fable-5", v)])
            .legend(false)
            .theme(Theme::ansi());
        // Plot 60 wide from x = 3 (gutter `10` + axis): day 2 is at dot
        // 40 (cell 20), day 4 at dot 79 (cell 39).
        let buf = render(&chart, 63, 6);
        let line_row = 0;
        let lit = |x: u16| braille_bits(buf[(3 + x, line_row)].symbol()).unwrap_or(0) != 0;
        assert!(lit(20) && lit(39), "{:?}", snapshot::lines(&buf));
        for x in 21..39 {
            assert!(
                !lit(x),
                "cell {x} inside the gap: {:?}",
                snapshot::lines(&buf)
            );
        }
    }

    #[test]
    fn thin_samples_are_dim_and_confident_ones_are_not() {
        let s = Series::new("gpt-5.6-sol", (0..7).map(|_| Some(80.0)))
            .low([false, false, false, false, true, true, true]);
        for theme in [Theme::ansi(), Theme::mono()] {
            let chart = TrendChart::new([s.clone()]).legend(false).theme(theme);
            let buf = render(&chart, 63, 6);
            let first = &buf[(3, 0)];
            let last = &buf[(62, 0)];
            if theme.is_mono() {
                assert!(!first.modifier.contains(Modifier::DIM));
                assert!(last.modifier.contains(Modifier::DIM));
            } else {
                assert_eq!(first.fg, Color::Cyan);
                assert_eq!(last.fg, Color::DarkGray);
            }
        }
        let chart = TrendChart::new([s])
            .low_note("dim = n<5")
            .theme(Theme::ansi());
        let legend = chart.legend_line(80);
        let text: String = legend.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.ends_with("(dim = n<5)"), "{text}");
    }

    #[test]
    fn mono_marks_series_by_glyph_and_has_no_colour() {
        let chart = TrendChart::new([
            Series::new("opus-4-8[1m]", (0..5).map(|_| Some(100.0))),
            Series::new("gpt-5.6-sol", (0..5).map(|_| Some(10.0))),
        ])
        .theme(Theme::mono());
        let buf = render(&chart, 50, 10);
        for c in buf.content() {
            assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
        }
        let rows = snapshot::lines(&buf);
        assert_eq!(rows[0].matches('*').count(), 5, "{rows:?}");
        assert_eq!(rows[7].matches('+').count(), 5, "{rows:?}");
        assert!(
            rows[9].starts_with("* opus-4-8[1m]  + gpt-5.6-sol"),
            "{rows:?}"
        );
    }

    #[test]
    fn extra_series_and_a_narrow_legend_are_counted() {
        let names = [
            "opus-4-8[1m]",
            "gpt-5.6-sol",
            "fable-5",
            "grok-5",
            "opus-4-8",
            "fable-5-mini",
        ];
        let chart = TrendChart::new(names.map(|n| Series::new(n, [Some(1.0), Some(2.0)])))
            .theme(Theme::ansi());
        let wide: String = chart
            .legend_line(200)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(wide.ends_with("grok-5  +2 more"), "{wide}");
        let narrow: String = chart
            .legend_line(36)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(narrow, "⠒⠒ opus-4-8[1m]  +5 more");
    }

    #[test]
    fn no_data_says_so_instead_of_a_zero_line() {
        let chart =
            TrendChart::new([Series::new("grok-5", [None, None, None])]).theme(Theme::ansi());
        let buf = render(&chart, 40, 6);
        assert_eq!(snapshot::lines(&buf)[0], "no data in this span");
        assert!(snapshot::text(&buf).lines().skip(1).all(|l| l.is_empty()));
    }

    #[test]
    fn every_size_down_to_one_cell() {
        let chart = TrendChart::new([
            Series::new("a", (0..14).map(|i| (i % 5 != 2).then_some(i as f64))).low([true; 3]),
            Series::new("b", [Some(3.0)]),
        ])
        .x_labels(["10-23", "10-30", "11-05"])
        .theme(Theme::mono());
        for w in [1u16, 2, 5, 9, 12, 20, 40] {
            for h in [1u16, 2, 3, 4, 5, 8] {
                let buf = render(&chart, w, h);
                for row in snapshot::lines(&buf) {
                    assert!(text::width(&row) <= w as usize);
                }
            }
        }
    }
}
