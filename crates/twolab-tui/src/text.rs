//! Text measured in terminal cells.
//!
//! A terminal column is a *cell*, not a `char`: `한` takes two, `e\u{301}` one,
//! `❤️` (a heart plus VS16) two. Every function here measures with
//! `unicode-width` and splits by grapheme cluster — the unit ratatui renders —
//! so a padded column lines up on a Korean, Japanese or emoji-heavy row.
//!
//! Ported from llmux `src/tui/ui.rs` (`cell_width`, `truncate_cells`,
//! `pad_cells`, `pad_spans`, `wrapped_line_count`) with two changes:
//! truncation is linear (llmux re-measured the whole string per popped char),
//! and lines can be clipped without losing their styles.

use std::borrow::Cow;

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// The one-cell ellipsis that marks clipped text.
pub const ELLIPSIS: char = '…';

/// Display cells `s` takes.
pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
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
        let w = width(g);
        if used + w > max {
            break;
        }
        used += w;
        end = i + g.len();
    }
    // Whole-string measurement is the ground truth (context-sensitive
    // sequences can measure differently than the sum of their clusters).
    let mut prefix = &s[..end];
    while width(prefix) > max {
        match prefix.grapheme_indices(true).next_back() {
            Some((i, _)) => prefix = &prefix[..i],
            None => break,
        }
    }
    let w = width(prefix);
    (prefix, w)
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

/// Rows `text` takes when ratatui wraps it at `width` cells with
/// `Wrap { trim: false }`. Never under-counts (it may over-count by a row
/// where ratatui would trim), so a scroll limit computed from it always
/// reaches the last line.
pub fn wrapped_rows(text: &str, width: u16) -> usize {
    let width = (width as usize).max(1);
    text.split('\n')
        .map(|line| line_rows(line, width))
        .sum::<usize>()
        .max(1)
}

/// Rows of a set of lines (their spans joined), as [`wrapped_rows`].
pub fn wrapped_rows_lines(lines: &[Line], width: u16) -> usize {
    let width = (width as usize).max(1);
    lines
        .iter()
        .map(|l| {
            let s: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
            line_rows(&s, width)
        })
        .sum::<usize>()
        .max(1)
}

/// Rows one newline-free string wraps into: words packed greedily, each
/// space its own one-cell token, a word wider than a row split by grapheme.
fn line_rows(source: &str, width: usize) -> usize {
    let mut rows = 1usize;
    let mut col = 0usize;
    let mut word_start: Option<usize> = None;
    for (i, g) in source.grapheme_indices(true) {
        if g == " " {
            if let Some(s) = word_start.take() {
                place(&source[s..i], width, &mut rows, &mut col);
            }
            place(" ", width, &mut rows, &mut col);
        } else if word_start.is_none() {
            word_start = Some(i);
        }
    }
    if let Some(s) = word_start {
        place(&source[s..], width, &mut rows, &mut col);
    }
    rows
}

fn place(token: &str, width_max: usize, rows: &mut usize, col: &mut usize) {
    let w = width(token);
    if w == 0 {
        return;
    }
    if *col + w <= width_max {
        *col += w;
        return;
    }
    if *col > 0 {
        *rows += 1;
        *col = 0;
    }
    if w <= width_max {
        *col = w;
        return;
    }
    for g in token.graphemes(true) {
        let cw = width(g);
        if cw == 0 {
            continue;
        }
        if *col + cw > width_max {
            *rows += 1;
            *col = 0;
        }
        *col += cw;
    }
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

    /// The row count must never be below what ratatui actually draws.
    #[test]
    fn wrapped_rows_never_undercounts_ratatui() {
        let samples = [
            "",
            "short",
            "a few words that wrap around the edge of a narrow box",
            "    leading indentation counts too",
            "한국어 문장도 셀 폭으로 줄바꿈합니다 그리고 이모지 ❤\u{fe0f} 도",
            "averyveryveryverylongwordwithoutspacesthatmustbehardsplit",
            "line one\nline two is a little longer\n\nline four",
        ];
        for s in samples {
            for w in [1u16, 3, 7, 10, 16, 23, 40] {
                let rows = wrapped_rows(s, w);
                let area = Rect::new(0, 0, w, 64);
                let mut buf = Buffer::empty(area);
                Paragraph::new(s)
                    .wrap(Wrap { trim: false })
                    .render(area, &mut buf);
                let drawn = (0..64)
                    .rev()
                    .find(|&y| (0..w).any(|x| buf[(x, y)].symbol() != " "))
                    .map_or(1, |y| y as usize + 1);
                assert!(
                    rows >= drawn,
                    "{s:?} at {w}: counted {rows}, ratatui drew {drawn}"
                );
            }
        }
    }
}
