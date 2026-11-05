//! Money in a column, lined up on the decimal point like a ledger.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `usage_cost_cell` and
//! `UsageCostTier`, the cost column of the usage overlay (with
//! `usage_cost_parts` and `group_thousands`, now in [`crate::format`]).
//!
//! The integer part is right-aligned in a fixed number of cells (with
//! thousands separators), so the decimal point sits in the same column on
//! every row. Sub-dollar amounts keep four fraction digits, larger ones
//! two. Dollars carry the weight, cents sit a tier dimmer, and the currency
//! sign is the quietest. A total row reads at full strength; a detail row
//! one tier down, so the eye lands on totals first. An unknown, negative or
//! non-finite amount reads `—` in the ones column — never a credible `$0`.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::format;
use crate::text;
use crate::theme::Theme;

/// Cells of the integer part: `999,999` fits.
pub const INT_WIDTH: usize = 7;
/// Cells of the widest fraction: `.0000`.
pub const FRAC_WIDTH: usize = 5;

/// How loud a row's amount is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tier {
    /// A bucket or grand total: dollars at full strength.
    #[default]
    Total,
    /// A row inside a bucket: one tier dimmer.
    Detail,
}

/// One amount in a ledger column.
#[derive(Debug, Clone, PartialEq)]
pub struct Amount {
    value: Option<f64>,
    tier: Tier,
    int_width: usize,
    currency: &'static str,
    marker: String,
    theme: Theme,
}

impl Amount {
    /// An amount, or `None` when no price is known.
    pub fn new(value: Option<f64>) -> Amount {
        Amount {
            value,
            tier: Tier::Total,
            int_width: INT_WIDTH,
            currency: "$",
            marker: String::new(),
            theme: Theme::default(),
        }
    }

    pub fn unknown() -> Amount {
        Amount::new(None)
    }

    pub fn tier(mut self, tier: Tier) -> Amount {
        self.tier = tier;
        self
    }

    /// Cells for the integer part (default 7, `999,999`). Larger amounts
    /// still print in full, just no longer aligned.
    pub fn int_width(mut self, cells: usize) -> Amount {
        self.int_width = cells.max(1);
        self
    }

    /// The currency sign before the integer part (default `$`).
    pub fn currency(mut self, sign: &'static str) -> Amount {
        self.currency = sign;
        self
    }

    /// A qualifier after the fraction, in its style — `+?` on a total that
    /// leaves out an unpriced part.
    pub fn marker(mut self, marker: impl Into<String>) -> Amount {
        self.marker = marker.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Amount {
        self.theme = theme;
        self
    }

    /// The amount if it can be shown as money: finite and not negative.
    pub fn valid(&self) -> Option<f64> {
        self.value.filter(|v| v.is_finite() && *v >= 0.0)
    }

    /// Cells of the widest form with these settings: sign, integer part,
    /// `.0000` and the marker. A column of amounts takes the widest row's.
    pub fn width(&self) -> usize {
        text::width(self.currency) + self.int_width + FRAC_WIDTH + text::width(&self.marker)
    }

    fn styles(&self) -> (Style, Style, Style) {
        let t = &self.theme;
        let quiet = t.dim().add_modifier(Modifier::DIM);
        let (int, frac) = match self.tier {
            Tier::Total => (t.text(), t.dim()),
            Tier::Detail => (t.dim(), quiet),
        };
        (quiet, int, frac)
    }

    /// The amount as a line, left-aligned: the decimal point always lands
    /// `currency + int_width` cells from the start.
    pub fn line(&self) -> Line<'static> {
        let (sign_style, int_style, frac_style) = self.styles();
        let lead = text::width(self.currency) + self.int_width;
        let Some(v) = self.valid() else {
            // The dash stands where the ones digit would.
            return Line::from(vec![
                Span::raw(" ".repeat(lead - 1)),
                Span::styled(format::UNKNOWN, frac_style),
                Span::styled(self.marker.clone(), frac_style),
            ]);
        };
        let (int, frac) = format::cost_parts(v);
        let mut spans = Vec::with_capacity(4);
        let pad = self.int_width.saturating_sub(text::width(&int));
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        spans.push(Span::styled(self.currency, sign_style));
        spans.push(Span::styled(int, int_style));
        spans.push(Span::styled(format!("{frac}{}", self.marker), frac_style));
        Line::from(spans)
    }
}

impl Widget for &Amount {
    /// One row, clipped with `…` when the area is narrower than the amount.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let line = text::truncate_line(self.line(), usize::from(area.width));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Widget for Amount {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    fn flat(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// The cell column of the first `ch`.
    fn col_of(s: &str, ch: char) -> Option<usize> {
        s.find(ch).map(|i| text::width(&s[..i]))
    }

    #[test]
    fn decimal_point_sits_in_one_column() {
        let rows: Vec<String> = [0.0076, 1.25, 994.99, 1_234.499, 999_999.0, 0.0]
            .into_iter()
            .map(|v| flat(&Amount::new(Some(v)).line()))
            .collect();
        assert_eq!(
            rows,
            [
                "      $0.0076",
                "      $1.25",
                "    $994.99",
                "  $1,234.50",
                "$999,999.00",
                "      $0.0000",
            ]
        );
        for r in &rows {
            assert_eq!(col_of(r, '.'), Some(1 + INT_WIDTH), "{r:?}");
        }
    }

    #[test]
    fn invalid_amounts_render_the_dash_in_the_ones_column() {
        for v in [None, Some(-0.42), Some(f64::NAN), Some(f64::INFINITY)] {
            let a = Amount::new(v);
            assert_eq!(a.valid(), None);
            let s = flat(&a.line());
            assert_eq!(col_of(&s, '—'), Some(INT_WIDTH), "{v:?}: {s:?}");
            assert!(
                !s.contains('$') && !s.contains('0'),
                "never a fake $0: {s:?}"
            );
        }
        // The ones digit of a real amount is in the same column.
        let one = flat(&Amount::new(Some(7.0)).line());
        assert_eq!(col_of(&one, '7'), Some(INT_WIDTH));
    }

    #[test]
    fn carry_and_grouping_survive_into_the_cell() {
        assert_eq!(flat(&Amount::new(Some(0.99999)).line()), "      $1.00");
        assert_eq!(
            flat(&Amount::new(Some(1_234_567.891)).line()),
            "$1,234,567.89",
            "past int_width: in full, just unaligned"
        );
    }

    #[test]
    fn marker_currency_and_width() {
        let a = Amount::new(Some(96.19)).marker("+?");
        assert_eq!(flat(&a.line()), "     $96.19+?");
        assert_eq!(a.width(), 1 + 7 + 5 + 2);
        let won = Amount::new(Some(12.5)).currency("US$").int_width(3);
        assert_eq!(flat(&won.line()), " US$12.50");
        assert_eq!(col_of(&flat(&won.line()), '.'), Some(3 + 3));
        assert_eq!(
            flat(&Amount::unknown().currency("US$").int_width(3).line()),
            "     —"
        );
    }

    /// llmux `usage_cost_cells_carry_style_tiers`: `$` at the quietest
    /// tier on both rows; total dollars at full strength over a dim
    /// fraction; detail dollars dim over a dimmer fraction.
    #[test]
    fn style_tiers() {
        let t = Theme::ansi();
        let total = Amount::new(Some(994.99)).theme(t).line();
        let detail = Amount::new(Some(994.99)).tier(Tier::Detail).theme(t).line();
        let by = |l: &Line, part: &str| {
            l.spans
                .iter()
                .find(|s| s.content.contains(part))
                .map(|s| s.style)
                .expect(part)
        };
        for l in [&total, &detail] {
            let s = by(l, "$");
            assert_eq!(s.fg, Some(Color::DarkGray));
            assert!(s.add_modifier.contains(Modifier::DIM));
        }
        assert_ne!(by(&total, "994").fg, Some(Color::DarkGray));
        let tf = by(&total, ".99");
        assert_eq!(tf.fg, Some(Color::DarkGray));
        assert!(!tf.add_modifier.contains(Modifier::DIM));
        let di = by(&detail, "994");
        assert_eq!(di.fg, Some(Color::DarkGray));
        assert!(!di.add_modifier.contains(Modifier::DIM));
        let df = by(&detail, ".99");
        assert_eq!(df.fg, Some(Color::DarkGray));
        assert!(df.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn mono_keeps_the_tiers_in_weight_only() {
        let t = Theme::mono();
        let total = Amount::new(Some(3.07)).theme(t).line();
        let detail = Amount::new(Some(3.07)).tier(Tier::Detail).theme(t).line();
        for l in [&total, &detail] {
            for s in &l.spans {
                assert!(matches!(s.style.fg, None | Some(Color::Reset)));
                assert!(s.style.bg.is_none());
            }
        }
        let int = |l: &Line| l.spans.iter().find(|s| s.content == "3").unwrap().style;
        assert!(!int(&total).add_modifier.contains(Modifier::DIM));
        assert!(int(&detail).add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn widget_clips_with_an_ellipsis() {
        let a = Amount::new(Some(1_204.5));
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 1));
        (&a).render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "  $1,204.50");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        a.render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "  $1,…");
    }
}
