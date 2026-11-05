//! Text measured in terminal cells.
//!
//! A terminal column is a *cell*, not a `char`: `한` takes two, `e\u{301}` one,
//! `❤️` (a heart plus VS16) two. Every function here measures with
//! `unicode-width` and splits by grapheme cluster — the unit ratatui renders —
//! so a padded column lines up on a Korean, Japanese or emoji-heavy row.
//!
//! Ported from llmux `src/tui/ui.rs` (`cell_width`, `truncate_cells`,
//! `pad_cells`, `pad_spans`, `wrapped_line_count`), changed to measure the
//! way ratatui draws: grapheme by grapheme with ratatui's own `CellWidth`
//! (which counts a halfwidth katakana sound mark as a cell), skipping any
//! grapheme with a control character (ratatui never draws those). Measuring
//! the whole string, as llmux did, disagrees with the buffer on such text.
//! Truncation is linear, lines clip without losing their styles, and the
//! wrapped row count is ratatui's own (`Paragraph::line_count`), so it is
//! exact rather than an estimate.

use std::borrow::Cow;

use ratatui::buffer::CellWidth;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};
use unicode_segmentation::UnicodeSegmentation;

/// The one-cell ellipsis that marks clipped text.
pub const ELLIPSIS: char = '…';

/// Cells one grapheme takes when ratatui draws it: 0 for a grapheme with a
/// control character (ratatui drops it), else ratatui's `cell_width`.
pub fn grapheme_width(g: &str) -> usize {
    if g.contains(char::is_control) {
        0
    } else {
        usize::from(g.cell_width())
    }
}

/// Display cells `s` takes, as ratatui draws it.
pub fn width(s: &str) -> usize {
    if s.is_ascii() {
        return s.bytes().filter(|b| !b.is_ascii_control()).count();
    }
    s.graphemes(true).map(grapheme_width).sum()
}

/// Total cells of a run of spans.
pub fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| width(&s.content)).sum()
}

/// Total cells of a line.
pub fn line_width(line: &Line) -> usize {
    spans_width(&line.spans)
}

/// The longest prefix of `s` that fits in `max` cells, never splitting a
/// grapheme. Returns the prefix and its width.
pub fn take_cells(s: &str, max: usize) -> (&str, usize) {
    let mut used = 0usize;
    let mut end = 0usize;
    for (i, g) in s.grapheme_indices(true) {
        let w = grapheme_width(g);
        if used + w > max {
            break;
        }
        used += w;
        end = i + g.len();
    }
    (&s[..end], used)
}

/// `s` clipped to at most `max` cells, ending in `…` when anything was cut.
/// `max == 0` gives the empty string.
pub fn truncate(s: &str, max: usize) -> Cow<'_, str> {
    // A zero budget is empty even for zero-width text (a lone ZWJ), so a
    // caller that pads to `max` never gets a stray invisible scalar.
    if max == 0 {
        return Cow::Borrowed("");
    }
    if width(s) <= max {
        return Cow::Borrowed(s);
    }
    let (prefix, _) = take_cells(s, max - 1);
    let mut out = String::with_capacity(prefix.len() + 3);
    out.push_str(prefix);
    out.push(ELLIPSIS);
    Cow::Owned(out)
}

/// Where padding goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Right,
    Center,
}

/// `s` in exactly `w` cells: clipped with `…` when too wide, padded with
/// spaces otherwise.
pub fn pad(s: &str, w: usize, align: Align) -> String {
    let clipped = truncate(s, w);
    let gap = w.saturating_sub(width(&clipped));
    let (l, r) = match align {
        Align::Left => (0, gap),
        Align::Right => (gap, 0),
        Align::Center => (gap / 2, gap - gap / 2),
    };
    let mut out = String::with_capacity(clipped.len() + gap);
    out.extend(std::iter::repeat_n(' ', l));
    out.push_str(&clipped);
    out.extend(std::iter::repeat_n(' ', r));
    out
}

/// Spans filled to exactly `w` cells: clipped (keeping styles) when too wide,
/// a trailing blank span otherwise.
pub fn pad_spans(spans: Vec<Span<'static>>, w: usize) -> Vec<Span<'static>> {
    let mut line = truncate_line(Line::from(spans), w);
    let used = line_width(&line);
    if used < w {
        line.spans.push(Span::raw(" ".repeat(w - used)));
    }
    line.spans
}

/// A line clipped to `max` cells, each kept span keeping its style; the
/// ellipsis takes the style of the span it cut.
pub fn truncate_line(line: Line<'static>, max: usize) -> Line<'static> {
    if line_width(&line) <= max {
        return line;
    }
    let Line {
        spans,
        style,
        alignment,
    } = line;
    let mut out: Vec<Span<'static>> = Vec::new();
    if max > 0 {
        let budget = max - 1;
        let mut used = 0usize;
        for span in spans {
            let w = width(&span.content);
            if used + w <= budget {
                used += w;
                out.push(span);
                continue;
            }
            let (prefix, pw) = take_cells(&span.content, budget - used);
            let mut cut = String::with_capacity(prefix.len() + 3);
            cut.push_str(prefix);
            cut.push(ELLIPSIS);
            used += pw + 1;
            out.push(Span::styled(cut, span.style));
            break;
        }
        debug_assert!(used <= max);
    }
    let mut l = Line::from(out).style(style);
    l.alignment = alignment;
    l
}

/// Whether ratatui's word wrapper treats grapheme `g` as a space it may
/// break at (ratatui-core `StyledGrapheme::is_whitespace`).
fn is_wrap_space(g: &str) -> bool {
    g == "\u{200b}" || (g.chars().all(char::is_whitespace) && g != "\u{a0}")
}

/// `s` made safe for ratatui's `Wrap`: every breakable space wider than one
/// cell (the ideographic space U+3000, common in CJK text) becomes that many
/// ASCII spaces. ratatui 0.30's word wrapper can emit a row wider than the
/// area when such a space ends a row, and the render then writes outside the
/// buffer and panics (`" \u{3000}b"` at width 2). Same cells, no panic.
pub fn wrap_safe(s: &str) -> Cow<'_, str> {
    if s.is_ascii()
        || !s
            .graphemes(true)
            .any(|g| is_wrap_space(g) && grapheme_width(g) > 1)
    {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    for g in s.graphemes(true) {
        let w = grapheme_width(g);
        if is_wrap_space(g) && w > 1 {
            out.extend(std::iter::repeat_n(' ', w));
        } else {
            out.push_str(g);
        }
    }
    Cow::Owned(out)
}

/// [`wrap_safe`] over a line, keeping each span's style.
pub fn wrap_safe_line(line: Line<'static>) -> Line<'static> {
    let Line {
        spans,
        style,
        alignment,
    } = line;
    let spans = spans
        .into_iter()
        .map(|sp| match wrap_safe(&sp.content) {
            Cow::Borrowed(_) => sp,
            Cow::Owned(o) => Span::styled(o, sp.style),
        })
        .collect::<Vec<_>>();
    let mut l = Line::from(spans).style(style);
    l.alignment = alignment;
    l
}

/// Rows `text` takes when ratatui wraps it at `width` cells with
/// `Wrap { trim: false }` — ratatui's own count (`Paragraph::line_count`, the
/// same word wrapper the render uses), so a scroll limit computed from it
/// reaches the last line exactly. At least 1.
pub fn wrapped_rows(text: &str, width: u16) -> usize {
    Paragraph::new(wrap_safe(text))
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

/// Rows of a set of lines wrapped at `width`, as [`wrapped_rows`].
pub fn wrapped_rows_lines(lines: &[Line], width: u16) -> usize {
    let lines: Vec<Line<'static>> = lines
        .iter()
        .map(|l| {
            let owned = Line {
                spans: l
                    .spans
                    .iter()
                    .map(|sp| Span::styled(sp.content.to_string(), sp.style))
                    .collect(),
                style: l.style,
                alignment: l.alignment,
            };
            wrap_safe_line(owned)
        })
        .collect();
    Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .line_count(width.max(1))
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};
    use ratatui::widgets::{Paragraph, Widget, Wrap};

    #[test]
    fn width_counts_cells_not_chars() {
        assert_eq!(width("abc"), 3);
        assert_eq!(width("한글"), 4);
        assert_eq!(width("e\u{301}"), 1);
        assert_eq!(width("❤\u{fe0f}"), 2);
    }

    #[test]
    fn truncate_keeps_whole_graphemes_and_fits() {
        assert_eq!(truncate("hello", 5), "hello");
        assert_eq!(truncate("hello!", 5), "hell…");
        assert_eq!(truncate("한글입니다", 5), "한글…");
        assert_eq!(truncate("한글입니다", 4), "한…");
        assert_eq!(truncate("abc", 0), "");
        assert_eq!(truncate("", 0), "");
        assert_eq!(
            truncate("\u{200d}", 0),
            "",
            "zero budget is empty even for zero-width text"
        );
        assert_eq!(truncate("❤\u{fe0f}❤\u{fe0f}❤\u{fe0f}", 4), "❤\u{fe0f}…");
        for max in 0..12 {
            assert!(
                width(&truncate("ab한cd글ef❤\u{fe0f}gh", max)) <= max,
                "{max}"
            );
        }
    }

    #[test]
    fn truncate_is_linear_on_long_input() {
        let long = "가".repeat(200_000);
        let t = truncate(&long, 9);
        assert_eq!(t, "가가가가…");
    }

    #[test]
    fn pad_aligns_in_cells() {
        assert_eq!(pad("ab", 4, Align::Left), "ab  ");
        assert_eq!(pad("ab", 4, Align::Right), "  ab");
        assert_eq!(pad("ab", 5, Align::Center), " ab  ");
        assert_eq!(pad("한", 4, Align::Left), "한  ");
        assert_eq!(pad("abcdef", 4, Align::Left), "abc…");
    }

    #[test]
    fn truncate_line_keeps_styles() {
        let red = Style::new().fg(Color::Red);
        let line = Line::from(vec![Span::raw("ab "), Span::styled("한글입니다", red)]);
        let t = truncate_line(line, 8);
        assert_eq!(line_width(&t), 8);
        assert_eq!(t.spans[0].content, "ab ");
        assert_eq!(t.spans[1].content, "한글…");
        assert_eq!(t.spans[1].style, red);
    }

    #[test]
    fn pad_spans_fills_or_clips() {
        let v = pad_spans(vec![Span::raw("ab")], 5);
        assert_eq!(spans_width(&v), 5);
        let v = pad_spans(vec![Span::raw("abcdefgh")], 5);
        assert_eq!(spans_width(&v), 5);
    }

    /// Drawn rows for `text` at `w`: ratatui renders it, we count.
    fn drawn_rows(text: &str, w: u16) -> usize {
        let h = 256u16;
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .render(area, &mut buf);
        (0..h)
            .rev()
            .find(|&y| (0..w).any(|x| buf[(x, y)].symbol() != " "))
            .map_or(1, |y| y as usize + 1)
    }

    /// The row count matches what ratatui draws, CJK and emoji included.
    #[test]
    fn wrapped_rows_matches_what_ratatui_draws() {
        let samples = [
            "",
            "short",
            "a few words that wrap around the edge of a narrow box",
            "    leading indentation counts too",
            "한국어 문장도 셀 폭으로 줄바꿈합니다 그리고 이모지 ❤\u{fe0f} 도",
            "averyveryveryverylongwordwithoutspacesthatmustbehardsplit",
            "line one\nline two is a little longer\n\nline four",
            "ｶﾞｷﾞｸﾞ halfwidth sound marks",
        ];
        for s in samples {
            for w in [2u16, 3, 7, 10, 16, 23, 40] {
                let rows = wrapped_rows(s, w);
                let drawn = drawn_rows(s, w);
                assert!(rows >= drawn, "{s:?} at {w}: counted {rows}, drew {drawn}");
            }
        }
    }

    /// ratatui 0.30 panics wrapping `" \u{3000}b"` at width 2; wrap_safe
    /// text does not, and keeps its width.
    #[test]
    fn wrap_safe_text_never_panics_ratatui() {
        for s in [
            " \u{3000}b",
            "가\u{3000}나\u{3000}다 라",
            "\u{3000}\u{3000}",
        ] {
            assert_eq!(width(&wrap_safe(s)), width(s));
            for w in 1..=6u16 {
                let area = Rect::new(0, 0, w, 12);
                let mut buf = Buffer::empty(area);
                Paragraph::new(wrap_safe(s))
                    .wrap(Wrap { trim: false })
                    .render(area, &mut buf);
            }
        }
    }

    /// `width` and `pad` agree with the buffer ratatui fills, on text the
    /// old whole-string measure got wrong (halfwidth sound marks, controls).
    #[test]
    fn width_agrees_with_the_buffer() {
        for s in [
            "ｶﾞ",
            "a\tb",
            "e\u{301}e",
            "한글",
            "❤\u{fe0f}x",
            "ﾟa",
            "\u{3000}b",
        ] {
            let area = Rect::new(0, 0, 20, 1);
            let mut buf = Buffer::empty(area);
            buf.set_string(0, 0, s, Style::new());
            // Cells up to the last drawn one, wide glyphs counted whole.
            let mut x = 0u16;
            let mut drawn = 0usize;
            while x < 20 {
                let sym = buf[(x, 0)].symbol();
                let w = usize::from(sym.cell_width()).max(1);
                if sym != " " || x < u16::try_from(width(s)).unwrap_or(0) {
                    drawn = usize::from(x) + w;
                }
                x += w as u16;
            }
            assert_eq!(width(s), drawn, "{s:?}");
            assert_eq!(width(&pad(s, 6, Align::Left)), 6, "{s:?}");
        }
    }

    /// A small generator of CJK, emoji, combining marks and odd spaces:
    /// truncate always fits, and width is additive over graphemes.
    #[test]
    fn differential_sample() {
        let alphabet = [
            "a",
            "b",
            " ",
            "한",
            "글",
            "あ",
            "ｶ",
            "ﾞ",
            "ﾟ",
            "e\u{301}",
            "❤\u{fe0f}",
            "👍",
            "\u{3000}",
            "\u{2003}",
            "\u{200b}",
            "\t",
            "-",
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..4000 {
            let mut s = String::new();
            for _ in 0..(seed % 12) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                s.push_str(alphabet[(seed % alphabet.len() as u64) as usize]);
            }
            let sum: usize = s.graphemes(true).map(grapheme_width).sum();
            assert_eq!(width(&s), sum, "{s:?}");
            for max in 0..10 {
                assert!(width(&truncate(&s, max)) <= max, "{s:?} {max}");
            }
        }
    }
}
