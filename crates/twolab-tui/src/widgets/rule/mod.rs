// Derived from vercel-labs/fx `src/ui/footer/row_text.zig` (`composeDividerRow`)
// and `src/ui/approval_screen.zig` (`dotted_divider_glyph`,
// `writeDocumentDividerRow`) at 412d841d73dd6e1b8de551022728d9c929caae17
// (Apache-2.0, Copyright 2025 Vercel, Inc.); translated from Zig to Rust.
// Modified by 2lab: an optional left label, an ASCII fallback, ratatui output.

//! A horizontal rule: where a band starts.
//!
//! Ported from fx (`src/ui/footer/row_text.zig`, Apache-2.0), where one row of
//! `─` (U+2500) in the divider grey marks the edge of the footer band around
//! an open menu, approval or question, and the review screens split a diff
//! from its controls with a dotted `┄` (U+2504). fx's rule has no label; this
//! one takes an optional left label for a section heading (`── accounts ───`).
//!
//! # Width on a CJK terminal
//!
//! Both box-drawing glyphs are East-Asian *Ambiguous*: one cell by default
//! (what `unicode-width` and ratatui count), two cells on a terminal set to
//! draw ambiguous glyphs wide (common with Korean, Japanese and Chinese
//! locales). There a full-width rule of `─` would be twice the screen and
//! wrap. A host that knows its terminal draws them wide calls
//! [`Rule::ambiguous_wide`] (or [`Rule::ascii`]) and the rule falls back to
//! ASCII `-`, which is one cell everywhere. [`is_ambiguous`] answers the
//! question for any glyph.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;

use crate::text;

/// fx's divider: `─` (U+2500, East-Asian Ambiguous).
pub const SOLID: char = '\u{2500}';
/// fx's review-screen divider: `┄` (U+2504, East-Asian Ambiguous).
pub const DASHED: char = '\u{2504}';
/// One cell on every terminal.
pub const ASCII: char = '-';

/// Cells of rule drawn before a label.
const LEAD: usize = 2;

/// Whether `c` is East-Asian Ambiguous: one cell by default, two on a
/// terminal that draws ambiguous glyphs wide.
pub fn is_ambiguous(c: char) -> bool {
    c.width() != c.width_cjk()
}

/// A full-width rule, optionally with a label at its left.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// The glyph repeated across the row.
    pub glyph: char,
    /// The glyph's style — usually `theme.border()`.
    pub style: Style,
    /// Drawn after a two-cell lead, in its own styles.
    pub label: Option<Line<'static>>,
}

impl Default for Rule {
    fn default() -> Self {
        Rule::new()
    }
}

impl Rule {
    /// A solid `─` rule with no style and no label.
    pub const fn new() -> Rule {
        Rule {
            glyph: SOLID,
            style: Style::new(),
            label: None,
        }
    }

    /// A dotted `┄` rule, as fx's review screens draw.
    pub const fn dashed() -> Rule {
        Rule {
            glyph: DASHED,
            style: Style::new(),
            label: None,
        }
    }

    pub const fn glyph(mut self, glyph: char) -> Rule {
        self.glyph = glyph;
        self
    }

    pub const fn style(mut self, style: Style) -> Rule {
        self.style = style;
        self
    }

    pub fn label(mut self, label: impl Into<Line<'static>>) -> Rule {
        self.label = Some(label.into());
        self
    }

    /// Draw with ASCII `-`.
    pub const fn ascii(self) -> Rule {
        self.glyph(ASCII)
    }

    /// On a terminal that draws ambiguous glyphs two cells wide (`wide`),
    /// swap an ambiguous glyph for ASCII `-`; otherwise keep it.
    pub fn ambiguous_wide(self, wide: bool) -> Rule {
        if wide && is_ambiguous(self.glyph) {
            self.ascii()
        } else {
            self
        }
    }

    /// `cells` worth of the glyph. A glyph two cells wide leaves a trailing
    /// space on an odd count; a zero-width glyph draws as ASCII.
    fn fill(&self, cells: usize) -> String {
        let mut glyph = self.glyph;
        let mut buf = [0u8; 4];
        let mut w = text::width(glyph.encode_utf8(&mut buf));
        if w == 0 {
            glyph = ASCII;
            w = 1;
        }
        let mut out = String::with_capacity(cells * glyph.len_utf8());
        out.extend(std::iter::repeat_n(glyph, cells / w));
        out.extend(std::iter::repeat_n(' ', cells % w));
        out
    }

    /// The rule as one line exactly `width` cells wide. A label too long for
    /// the row is clipped with `…`; below five cells the label goes and the
    /// rule stays.
    pub fn line(&self, width: u16) -> Line<'static> {
        let w = usize::from(width);
        let plain = || Line::from(Span::styled(self.fill(w), self.style));
        let Some(label) = &self.label else {
            return plain();
        };
        // The lead, a space each side of the label, and at least one cell
        // for the label itself.
        let room = w.saturating_sub(LEAD + 2);
        if room == 0 || text::line_width(label) == 0 {
            return plain();
        }
        let label = text::truncate_line(label.clone(), room);
        let rest = room - text::line_width(&label);
        let mut spans = Vec::with_capacity(label.spans.len() + 4);
        spans.push(Span::styled(self.fill(LEAD), self.style));
        spans.push(Span::raw(" "));
        spans.extend(label.spans);
        spans.push(Span::raw(" "));
        if rest > 0 {
            spans.push(Span::styled(self.fill(rest), self.style));
        }
        Line::from(spans)
    }
}

impl Widget for &Rule {
    /// Draws on the area's first row.
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_line(area.x, area.y, &self.line(area.width), area.width);
    }
}

impl Widget for Rule {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::theme::Theme;
    use ratatui::style::{Color, Modifier};

    fn draw(rule: &Rule, w: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, 1));
        rule.render(buf.area, &mut buf);
        buf
    }

    #[test]
    fn a_rule_is_the_glyph_across_the_whole_width() {
        // composeDividerRow: one `─` per column.
        let buf = draw(&Rule::new(), 12);
        assert_eq!(snapshot::text(&buf), "─".repeat(12));
        let buf = draw(&Rule::dashed(), 7);
        assert_eq!(snapshot::text(&buf), "┄".repeat(7));
        assert_eq!(text::line_width(&Rule::new().line(80)), 80);
        assert_eq!(text::line_width(&Rule::new().line(0)), 0);
    }

    #[test]
    fn every_glyph_cell_carries_the_rule_style() {
        let t = Theme::dark();
        let buf = draw(&Rule::new().style(t.border()), 10);
        for x in 0..10 {
            assert_eq!(buf[(x, 0)].fg, t.border, "{x}");
        }
        let mono = Theme::mono();
        let buf = draw(&Rule::new().style(mono.border()), 10);
        for x in 0..10 {
            assert_eq!(buf[(x, 0)].fg, Color::Reset, "{x}");
            assert!(buf[(x, 0)].modifier.contains(Modifier::DIM), "{x}");
        }
    }

    #[test]
    fn a_label_sits_after_a_two_cell_lead_in_its_own_style() {
        let t = Theme::dark();
        let rule = Rule::new()
            .style(t.border())
            .label(Span::styled("accounts", t.secondary()));
        let buf = draw(&rule, 20);
        assert_eq!(snapshot::text(&buf), "── accounts ────────");
        assert_eq!(buf[(0, 0)].fg, t.border);
        assert_eq!(buf[(3, 0)].fg, t.text_secondary, "the label's own style");
        assert_eq!(buf[(19, 0)].fg, t.border);
    }

    #[test]
    fn a_long_label_is_clipped_never_silently_and_never_overflows() {
        let rule = Rule::new().label("permissions and approvals");
        let buf = draw(&rule, 14);
        assert_eq!(snapshot::text(&buf), "── permissio…");
        for w in 0..30u16 {
            assert_eq!(text::line_width(&rule.line(w)), usize::from(w), "{w}");
        }
        // Too narrow for even one cell of label: the rule alone.
        assert_eq!(snapshot::text(&draw(&rule, 4)), "────");
        // A label of five cells at nine cells fits exactly, with no tail.
        assert_eq!(
            snapshot::text(&draw(&Rule::new().label("tools"), 9)),
            "── tools"
        );
    }

    #[test]
    fn cjk_labels_are_measured_in_cells() {
        let buf = draw(&Rule::new().label("계정"), 12);
        assert_eq!(snapshot::text(&buf), "── 계정 ────");
        assert_eq!(text::line_width(&Rule::new().label("계정 목록").line(9)), 9);
    }

    #[test]
    fn box_glyphs_are_ambiguous_and_ascii_is_the_fallback() {
        assert!(is_ambiguous(SOLID), "U+2500");
        assert!(is_ambiguous(DASHED), "U+2504");
        assert!(!is_ambiguous(ASCII));
        assert_eq!(Rule::new().ambiguous_wide(true).glyph, ASCII);
        assert_eq!(Rule::dashed().ambiguous_wide(true).glyph, ASCII);
        assert_eq!(Rule::new().ambiguous_wide(false).glyph, SOLID);
        // A glyph that is narrow everywhere is kept (braille is Neutral)...
        assert_eq!(Rule::new().glyph('⠤').ambiguous_wide(true).glyph, '⠤');
        // ...but most block elements are Ambiguous too, not Narrow.
        assert!(is_ambiguous('▔') && is_ambiguous('▀') && is_ambiguous('█'));
        assert!(!is_ambiguous('▐') && !is_ambiguous('░'));
        assert_eq!(
            snapshot::text(&draw(&Rule::new().ascii().label("계정"), 10)),
            "-- 계정 --"
        );
    }

    #[test]
    fn a_wide_glyph_never_overflows_the_row() {
        let rule = Rule::new().glyph('＝');
        for w in 0..9u16 {
            assert_eq!(text::line_width(&rule.line(w)), usize::from(w), "{w}");
        }
        assert_eq!(snapshot::text(&draw(&rule, 5)), "＝＝");
        // A zero-width glyph would draw nothing: it falls back to ASCII.
        assert_eq!(
            snapshot::text(&draw(&Rule::new().glyph('\u{301}'), 3)),
            "---"
        );
    }

    #[test]
    fn it_draws_on_the_first_row_only_and_survives_tiny_areas() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        Rule::new().label("x").render(buf.area, &mut buf);
        assert_eq!(snapshot::lines(&buf), vec!["── x ─", "", ""]);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        Rule::new().label("anything").render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "─");
        let mut buf = Buffer::empty(Rect::new(0, 0, 0, 0));
        Rule::new().render(buf.area, &mut buf);
    }
}
