//! A small inline bar: this row's share of the biggest one.
//!
//! Ported from llmux `src/tui/format.rs` `gauge_bar` (MIT, 2lab), the
//! `▰▰▰▱▱▱` bar of the models strip (token share, ten cells) and the
//! heatmap (intensity, twelve cells, coloured by level). The share is
//! `value / max`; the filled cells are `round(share × width)`, so a sliver
//! shows from half a cell up and the bar is full only at the maximum.
//!
//! `▰` (U+25B0) and `▱` (U+25B1) are East-Asian *Neutral*: one cell on
//! every terminal, CJK included.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::format;
use crate::text::{self, Align};
use crate::theme::{Level, Theme};

pub const FILLED: char = '▰';
pub const EMPTY: char = '▱';
/// llmux's models-strip width.
pub const DEFAULT_WIDTH: u16 = 10;

/// `ratio` (clamped to 0–1) as `width` glyphs: `bar(0.5, 4)` is `▰▰▱▱`.
pub fn bar(ratio: f64, width: usize) -> String {
    let filled = filled_cells(ratio, width);
    let mut s = String::with_capacity(width * 3);
    s.extend(std::iter::repeat_n(FILLED, filled));
    s.extend(std::iter::repeat_n(EMPTY, width - filled));
    s
}

fn filled_cells(ratio: f64, width: usize) -> usize {
    let r = if ratio.is_finite() {
        ratio.clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((r * width as f64).round() as usize).min(width)
}

fn one_cell(c: char) -> bool {
    text::width(c.encode_utf8(&mut [0; 4])) == 1
}

/// A row's share of the biggest, as a bar of glyphs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MiniBar {
    value: f64,
    max: f64,
    width: u16,
    glyphs: (char, char),
    style: Option<Style>,
    levels: Option<(f64, f64)>,
    theme: Theme,
}

impl MiniBar {
    /// `value` out of `max` (usually the biggest row). A share that cannot
    /// be computed — `max` zero or either side non-finite — reads `—`.
    pub fn new(value: f64, max: f64) -> MiniBar {
        MiniBar {
            value,
            max,
            width: DEFAULT_WIDTH,
            glyphs: (FILLED, EMPTY),
            style: None,
            levels: None,
            theme: Theme::default(),
        }
    }

    /// A ratio already in 0–1.
    pub fn ratio(ratio: f64) -> MiniBar {
        MiniBar::new(ratio, 1.0)
    }

    /// Cells the bar takes (default 10).
    pub fn width(mut self, width: u16) -> MiniBar {
        self.width = width;
        self
    }

    /// Filled and empty glyphs, one cell each (`'⣿', '⣀'`, `'#', '.'`). A
    /// glyph wider than one cell is refused and the default kept. Block
    /// shades like `█` are East-Asian *Ambiguous* (two cells on a CJK
    /// terminal): pass them only when the host knows the terminal.
    pub fn glyphs(mut self, filled: char, empty: char) -> MiniBar {
        if one_cell(filled) && one_cell(empty) {
            self.glyphs = (filled, empty);
        }
        self
    }

    /// Style of the filled part (a series or group colour). Default: the
    /// theme's accent.
    pub fn style(mut self, style: Style) -> MiniBar {
        self.style = Some(style);
        self
    }

    /// Colour the filled part by how high the share is — warn from `low`,
    /// crit from `high` — the way llmux's heatmap does.
    pub fn levels(mut self, low: f64, high: f64) -> MiniBar {
        self.levels = Some((low.min(high), high.max(low)));
        self
    }

    pub fn theme(mut self, theme: Theme) -> MiniBar {
        self.theme = theme;
        self
    }

    /// `value / max` clamped to 0–1, or `None` when it cannot be known.
    pub fn share(&self) -> Option<f64> {
        (self.value.is_finite() && self.max.is_finite() && self.max > 0.0)
            .then(|| (self.value / self.max).clamp(0.0, 1.0))
    }

    /// Filled cells at `width`.
    pub fn filled(&self, width: usize) -> usize {
        self.share().map_or(0, |s| filled_cells(s, width))
    }

    /// The bar as plain text, `width` cells: glyphs, or `—` padded when the
    /// share is unknown.
    pub fn text(&self) -> String {
        self.spans_at(usize::from(self.width))
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    fn filled_style(&self, share: f64) -> Style {
        match (self.style, self.levels) {
            (_, Some((low, high))) => self.theme.level(Level::of(share, low, high)),
            (Some(s), None) if self.theme.is_mono() => Style::new().add_modifier(s.add_modifier),
            (Some(s), None) => s,
            (None, None) => self.theme.accent(),
        }
    }

    fn spans_at(&self, width: usize) -> Vec<Span<'static>> {
        let Some(share) = self.share() else {
            return vec![Span::styled(
                text::pad(format::UNKNOWN, width, Align::Left),
                self.theme.dim(),
            )];
        };
        let filled = filled_cells(share, width);
        let (f, e) = self.glyphs;
        let mut spans = Vec::with_capacity(2);
        if filled > 0 {
            spans.push(Span::styled(
                f.to_string().repeat(filled),
                self.filled_style(share),
            ));
        }
        if filled < width {
            spans.push(Span::styled(
                e.to_string().repeat(width - filled),
                self.theme.dim(),
            ));
        }
        spans
    }

    /// The bar's spans at its own width.
    pub fn spans(&self) -> Vec<Span<'static>> {
        self.spans_at(usize::from(self.width))
    }

    /// The bar as a line, for a table cell or beside a label.
    pub fn line(&self) -> Line<'static> {
        Line::from(self.spans())
    }
}

impl Widget for &MiniBar {
    /// Draws on the first row; a narrower area gets a shorter bar of the
    /// same share rather than a clipped one.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let w = self.width.min(area.width);
        let line = Line::from(self.spans_at(usize::from(w)));
        buf.set_line(area.x, area.y, &line, w);
    }
}

impl Widget for MiniBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::{Color, Modifier};
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn gauge_bar_fills_proportionally_and_clamps() {
        assert_eq!(bar(0.0, 4), "▱▱▱▱");
        assert_eq!(bar(0.5, 4), "▰▰▱▱");
        assert_eq!(bar(1.0, 4), "▰▰▰▰");
        assert_eq!(bar(7.0, 4), "▰▰▰▰", "clamped above 1.0");
        assert_eq!(bar(-1.0, 4), "▱▱▱▱", "clamped below 0.0");
        assert_eq!(bar(f64::NAN, 4), "▱▱▱▱");
        assert_eq!(bar(0.5, 0), "", "zero width degrades to empty");
    }

    #[test]
    fn share_of_the_biggest() {
        let m = MiniBar::new(415_000.0, 1_204_000.0).width(10);
        assert_eq!(m.text(), "▰▰▰▱▱▱▱▱▱▱");
        assert_eq!(MiniBar::new(1.0, 1.0).width(3).text(), "▰▰▰");
        // Half a cell up shows; less does not.
        assert_eq!(MiniBar::new(0.05, 1.0).width(10).text(), "▰▱▱▱▱▱▱▱▱▱");
        assert_eq!(MiniBar::new(0.04, 1.0).width(10).text(), "▱▱▱▱▱▱▱▱▱▱");
    }

    #[test]
    fn unknown_share_is_a_dash_not_an_empty_bar() {
        for m in [
            MiniBar::new(3.0, 0.0),
            MiniBar::new(f64::NAN, 10.0),
            MiniBar::new(3.0, f64::INFINITY),
        ] {
            assert_eq!(m.share(), None);
            assert_eq!(m.width(6).text(), "—     ");
        }
    }

    #[test]
    fn glyphs_are_one_cell_even_on_cjk_terminals() {
        for g in ["▰▱", "⣿⣀", "#."] {
            assert_eq!(UnicodeWidthStr::width_cjk(g), 2, "{g}");
        }
        let m = MiniBar::ratio(0.5).width(4).glyphs('⣿', '⣀');
        assert_eq!(m.text(), "⣿⣿⣀⣀");
        // A block shade is the host's call (East-Asian Ambiguous): kept.
        assert_eq!(MiniBar::ratio(0.5).width(2).glyphs('█', '░').text(), "█░");
        // A wide glyph is refused.
        assert_eq!(
            MiniBar::ratio(0.5).width(4).glyphs('한', '·').text(),
            "▰▰▱▱"
        );
    }

    #[test]
    fn styles_come_from_the_theme() {
        let t = Theme::ansi();
        let spans = MiniBar::ratio(0.5).width(4).theme(t).spans();
        assert_eq!(spans[0].style, t.accent());
        assert_eq!(spans[1].style, t.dim());
        let hot = MiniBar::ratio(0.95).levels(0.7, 0.9).theme(t).spans();
        assert_eq!(hot[0].style.fg, Some(Color::Red));
        let warm = MiniBar::ratio(0.75).levels(0.7, 0.9).theme(t).spans();
        assert_eq!(warm[0].style.fg, Some(Color::Yellow));
        let own = Style::new().fg(Color::Magenta);
        assert_eq!(
            MiniBar::ratio(0.5).style(own).theme(t).spans()[0].style,
            own
        );
    }

    #[test]
    fn mono_keeps_the_glyphs_and_drops_every_colour() {
        let t = Theme::mono();
        for m in [
            MiniBar::ratio(0.5),
            MiniBar::ratio(0.95).levels(0.7, 0.9),
            MiniBar::ratio(0.5).style(Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            MiniBar::new(1.0, 0.0),
        ] {
            let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
            m.theme(t).render(buf.area, &mut buf);
            for c in buf.content() {
                assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset), "{m:?}");
            }
        }
        let own = MiniBar::ratio(0.5)
            .style(Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD))
            .theme(t)
            .spans();
        assert!(
            own[0].style.add_modifier.contains(Modifier::BOLD),
            "weight kept"
        );
    }

    #[test]
    fn a_narrow_area_gets_a_shorter_bar_of_the_same_share() {
        let m = MiniBar::ratio(0.5).width(10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        (&m).render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "▰▰▱▱");
        let mut buf = Buffer::empty(Rect::new(0, 0, 14, 2));
        m.render(Rect::new(2, 1, 12, 1), &mut buf);
        assert_eq!(snapshot::lines(&buf), ["", "  ▰▰▰▰▰▱▱▱▱▱"]);
    }
}
