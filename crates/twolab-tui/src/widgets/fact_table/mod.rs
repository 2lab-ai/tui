// Derived from vercel-labs/fx `src/ui/footer/usage_presentation.zig`
// (`Cell`, `appendCells`, `fitColumns`, `grouped`, `compact`, `money`),
// `src/ui/footer/row_text.zig` (`composeFactRow`) and
// `src/ui/footer/info_panel_presentation.zig` (indent, column gap) at
// 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025
// Vercel, Inc.); translated from Zig to Rust. Modified by 2lab: label/value
// facts packed into as many columns as fit, units, `—` for unknowns, ratatui
// output.

//! The numbers, aligned, in as many columns as fit.
//!
//! Ported from fx's `/usage` dashboard (`src/ui/footer/usage_presentation.zig`,
//! Apache-2.0), which prints the way `df` does: labels dim, numbers
//! right-aligned so their last digits line up, tables as wide as their content
//! and never stretched to the terminal, columns dropped before anything is
//! clipped. Here the cells are label/value facts (fx's `composeFactRow`): each
//! fact is a dim label, its value right-aligned under the others in its
//! column, and an optional unit in a column of its own. The facts fill down
//! then across, like `ls`, in as many columns as the width holds — three on a
//! wide terminal, one on a narrow one — and only a single column too wide for
//! the screen cuts its labels, with `…`.
//!
//! An unknown value is `—`, never a made-up `0`. A value that changes live can
//! [`Fact::reserve`] the width of its widest form, so the columns do not
//! jitter as it grows.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::text::{self, Align};
use crate::theme::Theme;

/// Cells before the first column (fx info panel `indent`).
pub const INDENT: usize = 2;
/// Cells between two columns (fx info panel `column_gap`).
pub const COLUMN_GAP: usize = 4;
/// Cells between a label and its value column.
const LABEL_GAP: usize = 2;
/// What an unknown value reads as.
pub const UNKNOWN: &str = "—";

/// One label and its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    pub label: String,
    /// `None` reads as `—`.
    pub value: Option<String>,
    /// Drawn after the value, in a column of its own: `%`, `s`, `tok/s`.
    pub unit: String,
    /// The value's style; `None` is `theme.text()`.
    pub style: Option<Style>,
    /// Cells the value column keeps at least, for a value that grows.
    pub reserve: usize,
}

impl Fact {
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Fact {
        Fact {
            label: label.into(),
            value: Some(value.into()),
            unit: String::new(),
            style: None,
            reserve: 0,
        }
    }

    /// A fact nobody measured: `—`.
    pub fn unknown(label: impl Into<String>) -> Fact {
        Fact {
            value: None,
            ..Fact::new(label, "")
        }
    }

    /// [`Fact::new`] or [`Fact::unknown`].
    pub fn maybe(label: impl Into<String>, value: Option<impl Into<String>>) -> Fact {
        match value {
            Some(v) => Fact::new(label, v),
            None => Fact::unknown(label),
        }
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Fact {
        self.unit = unit.into();
        self
    }

    pub fn style(mut self, style: Style) -> Fact {
        self.style = Some(style);
        self
    }

    /// Keep the value column at least `cells` wide — the widest form the value
    /// can take, so a live number does not move its neighbours.
    pub fn reserve(mut self, cells: usize) -> Fact {
        self.reserve = cells;
        self
    }

    fn value_text(&self) -> &str {
        self.value.as_deref().unwrap_or(UNKNOWN)
    }
}

/// One column's widths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Col {
    label: usize,
    value: usize,
    unit: usize,
}

impl Col {
    fn width(self) -> usize {
        let unit = if self.unit > 0 { 1 + self.unit } else { 0 };
        self.label + LABEL_GAP + self.value + unit
    }
}

/// Facts packed into columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactTable {
    facts: Vec<Fact>,
    theme: Theme,
    max_columns: usize,
}

impl FactTable {
    /// Labels and units in `theme.dim()`, values in `theme.text()`.
    pub fn new(theme: Theme) -> FactTable {
        FactTable {
            facts: Vec::new(),
            theme,
            max_columns: usize::MAX,
        }
    }

    pub fn fact(mut self, fact: Fact) -> FactTable {
        self.facts.push(fact);
        self
    }

    pub fn facts(mut self, facts: impl IntoIterator<Item = Fact>) -> FactTable {
        self.facts.extend(facts);
        self
    }

    /// Never more columns than this (at least one).
    pub fn max_columns(mut self, columns: usize) -> FactTable {
        self.max_columns = columns.max(1);
        self
    }

    /// Rows for `columns` columns, and the columns that are really used.
    fn grid(&self, columns: usize) -> (usize, usize) {
        let n = self.facts.len();
        if n == 0 {
            return (0, 0);
        }
        let rows = n.div_ceil(columns.max(1));
        (rows, n.div_ceil(rows))
    }

    fn cols(&self, columns: usize) -> Vec<Col> {
        let (rows, used) = self.grid(columns);
        (0..used)
            .map(|c| {
                let chunk = &self.facts[c * rows..((c + 1) * rows).min(self.facts.len())];
                chunk.iter().fold(Col::default(), |acc, f| Col {
                    label: acc.label.max(text::width(&f.label)),
                    value: acc.value.max(text::width(f.value_text())).max(f.reserve),
                    unit: acc.unit.max(text::width(&f.unit)),
                })
            })
            .collect()
    }

    fn total(cols: &[Col]) -> usize {
        INDENT
            + cols.iter().map(|c| c.width()).sum::<usize>()
            + COLUMN_GAP * cols.len().saturating_sub(1)
    }

    /// How many columns the facts take at `width`: the most that fit, one
    /// when even one does not.
    pub fn columns(&self, width: u16) -> usize {
        let n = self.facts.len();
        let most = n.min(self.max_columns);
        (1..=most)
            .rev()
            .find(|&k| Self::total(&self.cols(k)) <= usize::from(width))
            .map_or(usize::from(n > 0), |k| self.grid(k).1)
    }

    /// Rows the table takes at `width`.
    pub fn height(&self, width: u16) -> u16 {
        let (rows, _) = self.grid(self.columns(width));
        u16::try_from(rows).unwrap_or(u16::MAX)
    }

    /// Every row at `width`, each at most `width` cells.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let w = usize::from(width);
        let k = self.columns(width);
        let (rows, _) = self.grid(k);
        let mut cols = self.cols(k);
        // A single column too wide: the labels give way, never the values.
        if let [col] = cols.as_mut_slice() {
            let total = INDENT + col.width();
            if total > w {
                col.label = col.label.saturating_sub(total - w);
            }
        }
        (0..rows).map(|r| self.row(r, rows, &cols, w)).collect()
    }

    fn row(&self, r: usize, rows: usize, cols: &[Col], w: usize) -> Line<'static> {
        let t = &self.theme;
        let mut spans = vec![Span::raw(" ".repeat(INDENT))];
        for (c, col) in cols.iter().enumerate() {
            let Some(fact) = self.facts.get(c * rows + r) else {
                break;
            };
            if c > 0 {
                spans.push(Span::raw(" ".repeat(COLUMN_GAP)));
            }
            if col.label > 0 {
                spans.push(Span::styled(
                    text::pad(&fact.label, col.label, Align::Left),
                    t.dim(),
                ));
            }
            spans.push(Span::raw(" ".repeat(LABEL_GAP)));
            let value_style = match (&fact.value, fact.style) {
                (None, _) => t.dim(),
                (Some(_), Some(style)) => style,
                (Some(_), None) => t.text(),
            };
            spans.push(Span::styled(
                text::pad(fact.value_text(), col.value, Align::Right),
                value_style,
            ));
            if col.unit > 0 {
                let unit = if fact.value.is_some() {
                    fact.unit.as_str()
                } else {
                    ""
                };
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    text::pad(unit, col.unit, Align::Left),
                    t.dim(),
                ));
            }
        }
        text::truncate_line(Line::from(spans), w)
    }
}

impl Widget for &FactTable {
    /// Draws the rows from the top. When the area is shorter than the table,
    /// the last row it has counts the facts it could not show (`+4 more`).
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let lines = self.lines(area.width);
        let h = usize::from(area.height);
        let mut shown = lines.len().min(h);
        let mut more = None;
        if lines.len() > h {
            shown = h - 1;
            let rows = lines.len();
            let k = self.columns(area.width);
            let visible = (0..k)
                .map(|c| {
                    let in_col = self.facts.len().saturating_sub(c * rows).min(rows);
                    in_col.min(shown)
                })
                .sum::<usize>();
            let hidden = self.facts.len() - visible;
            more = Some(Line::from(vec![
                Span::raw(" ".repeat(INDENT)),
                Span::styled(format!("+{hidden} more"), self.theme.dim()),
            ]));
        }
        for (y, line) in (area.y..).zip(lines.iter().take(shown).chain(more.iter())) {
            buf.set_line(area.x, y, line, area.width);
        }
    }
}

impl Widget for FactTable {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

/// `3,665` (fx `grouped`).
pub fn grouped(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (i, c) in raw.chars().enumerate() {
        if i > 0 && (raw.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `950`, `12K`, `6.9M`, `1.2B` (fx `compact`): one decimal under ten.
pub fn compact(value: u64) -> String {
    for (div, suffix) in [
        (1_000_000_000_000u64, "T"),
        (1_000_000_000, "B"),
        (1_000_000, "M"),
        (1_000, "K"),
    ] {
        if value >= div {
            let scaled = value as f64 / div as f64;
            return if scaled < 10.0 {
                format!("{scaled:.1}{suffix}")
            } else {
                format!("{scaled:.0}{suffix}")
            };
        }
    }
    value.to_string()
}

/// `$1,234.56`, or four decimals below one dollar (fx `money`). Not a number
/// reads as `—`.
pub fn money(value: f64) -> String {
    if !value.is_finite() {
        return UNKNOWN.to_string();
    }
    if value < 0.0 {
        return format!("-{}", money(-value));
    }
    if value < 1.0 {
        return format!("${value:.4}");
    }
    let cents = (value * 100.0).round() as u64;
    format!("${}.{:02}", grouped(cents / 100), cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    /// Grouped by meaning: counts and cost, tokens, cache and latency.
    fn usage(t: Theme) -> FactTable {
        unbounded(t).max_columns(3)
    }

    fn unbounded(t: Theme) -> FactTable {
        FactTable::new(t).facts([
            Fact::new("requests", grouped(3_665)),
            Fact::new("failed", "14"),
            Fact::new("cost", money(14.10)),
            Fact::new("tokens in", compact(41_200_000)),
            Fact::new("tokens out", compact(6_900_000)),
            Fact::new("cache read", compact(38_000_000)),
            Fact::new("cache hit", "95.4").unit("%"),
            Fact::new("ttft", "2.1").unit("s"),
            Fact::new("throughput", "58").unit("tok/s"),
        ])
    }

    fn draw(table: &FactTable, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        table.render(buf.area, &mut buf);
        buf
    }

    /// The column where `needle` ends on `row`, in cells.
    fn end(row: &str, needle: &str) -> usize {
        let at = row
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} in {row:?}"));
        text::width(&row[..at]) + text::width(needle)
    }

    #[test]
    fn the_formatters_are_fxs() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(950), "950");
        assert_eq!(grouped(3_665), "3,665");
        assert_eq!(grouped(49_923), "49,923");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(compact(950), "950");
        assert_eq!(compact(12_000), "12K");
        assert_eq!(compact(6_900_000), "6.9M");
        assert_eq!(compact(1_200_000_000), "1.2B");
        assert_eq!(compact(14_500_000_000), "14B");
        assert_eq!(money(6_551.12), "$6,551.12");
        assert_eq!(money(14.1), "$14.10");
        assert_eq!(money(0.5), "$0.5000");
        assert_eq!(money(f64::NAN), "—");
        assert_eq!(money(-2.5), "-$2.50");
    }

    #[test]
    fn facts_take_as_many_columns_as_fit() {
        let t = unbounded(Theme::ansi());
        // Two rows of five fit 103 cells; one cell less, three rows of three.
        assert_eq!((t.columns(120), t.height(120)), (5, 2));
        assert_eq!((t.columns(103), t.height(103)), (5, 2));
        assert_eq!((t.columns(102), t.height(102)), (3, 3));
        assert_eq!(t.columns(0), 1);
        assert_eq!(FactTable::new(Theme::ansi()).columns(80), 0);
    }

    #[test]
    fn facts_fill_down_then_across() {
        let t = usage(Theme::ansi());
        assert_eq!(t.columns(120), 3);
        assert_eq!(t.height(120), 3);
        let rows = snapshot::lines(&draw(&t, 120, 3));
        assert!(
            rows[0].starts_with("  requests   3,665    tokens in    41M"),
            "{rows:?}"
        );
        assert!(
            rows[2].starts_with("  cost      $14.10    cache read   38M"),
            "{rows:?}"
        );
        // Content-sized: never stretched to the terminal.
        let wide = snapshot::lines(&draw(&t, 140, 3));
        assert_eq!(rows, wide);
        assert!(rows.iter().all(|r| text::width(r) < 100), "{rows:?}");
    }

    #[test]
    fn numbers_line_up_on_their_last_digit_and_units_on_their_first() {
        let t = usage(Theme::ansi());
        let rows = snapshot::lines(&draw(&t, 120, 3));
        assert_eq!(end(&rows[0], "3,665"), end(&rows[1], "14"));
        assert_eq!(end(&rows[0], "3,665"), end(&rows[2], "$14.10"));
        assert_eq!(end(&rows[0], "41M"), end(&rows[1], "6.9M"));
        assert_eq!(end(&rows[0], "95.4"), end(&rows[2], "58"));
        assert_eq!(end(&rows[0], "95.4 %"), end(&rows[1], "2.1 s"));
    }

    #[test]
    fn a_narrow_width_drops_columns_before_cutting_anything() {
        let t = usage(Theme::ansi());
        assert_eq!((t.columns(64), t.height(64)), (3, 3));
        assert_eq!((t.columns(63), t.height(63)), (2, 5));
        assert_eq!((t.columns(45), t.height(45)), (1, 9));
        let rows = snapshot::lines(&draw(&t, 40, 9));
        assert_eq!(rows[0], "  requests     3,665");
        assert_eq!(rows[8], "  throughput      58 tok/s");
        assert!(rows.iter().all(|r| !r.contains('…')), "{rows:?}");
        // Too narrow for even one column: the labels give way, not the values.
        let rows = snapshot::lines(&draw(&t, 20, 9));
        assert_eq!(rows[0], "  req…   3,665");
        assert!(rows[8].ends_with("58 tok/s"), "{rows:?}");
        for w in 0..130u16 {
            assert!(
                t.lines(w)
                    .iter()
                    .all(|l| text::line_width(l) <= usize::from(w)),
                "{w}"
            );
        }
    }

    #[test]
    fn unknown_values_say_so_and_drop_their_unit() {
        let theme = Theme::dark();
        let t = FactTable::new(theme).facts([
            Fact::unknown("cost"),
            Fact::maybe("cache hit", None::<String>).unit("%"),
            Fact::maybe("5h used", Some("42")).unit("%"),
        ]);
        let buf = draw(&t, 30, 3);
        let rows = snapshot::lines(&buf);
        assert_eq!(
            rows,
            ["  cost        —", "  cache hit   —", "  5h used    42 %"]
        );
        assert_eq!(buf[(14, 0)].fg, theme.muted, "an unknown is dim");
        assert_eq!(buf[(14, 2)].fg, theme.text, "a value is text");
        assert_eq!(buf[(2, 2)].fg, theme.muted, "a label is dim");
    }

    #[test]
    fn a_reserved_value_column_does_not_jitter() {
        let at = |value: &str, reserve: usize| {
            let line = &FactTable::new(Theme::ansi())
                .fact(Fact::new("tokens", value).reserve(reserve))
                .fact(Fact::new("cost", "$1.20"))
                .lines(80)[0];
            let s: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            s.find("cost").expect("the second column")
        };
        // Without a reserve the second column moves as the number grows...
        assert_ne!(at("9", 0), at("12,400", 0));
        // ...with one it stays put.
        assert_eq!(at("9", 6), at("12,400", 6));
    }

    #[test]
    fn a_short_area_counts_the_facts_it_hides() {
        let t = usage(Theme::ansi());
        // Two columns of five rows in four: three rows of each, then `+N more`.
        let rows = snapshot::lines(&draw(&t, 63, 4));
        assert_eq!(rows[3], "  +3 more");
        let rows = snapshot::lines(&draw(&t, 40, 3));
        assert_eq!(rows[2], "  +7 more");
        let _ = draw(&t, 1, 1);
        let _ = draw(&FactTable::new(Theme::ansi()), 10, 2);
    }

    #[test]
    fn mono_keeps_no_colour() {
        let t = usage(Theme::mono()).fact(Fact::unknown("ttfb"));
        let buf = draw(&t, 120, 4);
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
    }
}
