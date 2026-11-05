//! A rendered [`Buffer`] as text or as styled runs.
//!
//! [`text`] is what tests assert against. [`runs_json`] is what the browser
//! catalog paints. Both walk cells the way a terminal backend does: a wide
//! grapheme (CJK, most emoji) takes two cells, and the cell after it is a
//! continuation that prints nothing.

use ratatui::buffer::{Buffer, Cell, CellDiffOption};
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

/// Visible width of one cell's symbol (0-width symbols count as one cell, the
/// way ratatui's own diff counts them).
fn cell_width(cell: &Cell) -> usize {
    UnicodeWidthStr::width(cell.symbol()).max(1)
}

/// Rows of the buffer as plain strings, trailing spaces trimmed.
pub fn lines(buf: &Buffer) -> Vec<String> {
    let area = buf.area;
    let mut out = Vec::with_capacity(area.height as usize);
    for y in area.top()..area.bottom() {
        let mut row = String::new();
        let mut x = area.left();
        while x < area.right() {
            let cell = &buf[(x, y)];
            let w = cell_width(cell);
            if cell.diff_option == CellDiffOption::Skip {
                row.push(' ');
            } else {
                row.push_str(cell.symbol());
            }
            x = x.saturating_add(w as u16);
        }
        out.push(row.trim_end().to_string());
    }
    out
}

/// The buffer as one string, rows joined by `\n`, trailing spaces trimmed.
pub fn text(buf: &Buffer) -> String {
    lines(buf).join("\n")
}

/// Colour as the catalog encodes it: `null` for the terminal default, a
/// number for an indexed colour (0–15 are the named ANSI colours, in the
/// standard order), a `"#rrggbb"` string for true colour.
fn color_json(c: Color, out: &mut String) {
    use std::fmt::Write;
    let idx = match c {
        Color::Reset => {
            out.push_str("null");
            return;
        }
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(i) => i,
        Color::Rgb(r, g, b) => {
            let _ = write!(out, "\"#{r:02x}{g:02x}{b:02x}\"");
            return;
        }
    };
    let _ = write!(out, "{idx}");
}

/// Append `s` as a JSON string literal.
pub(crate) fn json_str(s: &str, out: &mut String) {
    use std::fmt::Write;
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(PartialEq)]
struct RunStyle {
    fg: Color,
    bg: Color,
    mods: Modifier,
}

/// The buffer as styled runs:
///
/// ```json
/// {"cols":80,"rows":2,"lines":[[["text",fg,bg,mods,width],…],…]}
/// ```
///
/// A run is either consecutive ASCII cells of one style, or exactly one
/// non-ASCII grapheme with its cell width — so the page can give every
/// non-ASCII glyph (box drawing, braille, CJK) a fixed cell box and the grid
/// stays aligned whatever the browser's fallback font measures.
/// `mods` is ratatui's [`Modifier`] bit set.
pub fn runs_json(buf: &Buffer) -> String {
    use std::fmt::Write;
    let area = buf.area;
    let mut out = String::with_capacity(area.area() as usize * 4);
    let _ = write!(
        out,
        "{{\"cols\":{},\"rows\":{},\"lines\":[",
        area.width, area.height
    );
    for (row_i, y) in (area.top()..area.bottom()).enumerate() {
        if row_i > 0 {
            out.push(',');
        }
        out.push('[');
        let mut first = true;
        let mut pending: Option<(String, RunStyle, usize)> = None;
        let flush = |p: Option<(String, RunStyle, usize)>, out: &mut String, first: &mut bool| {
            if let Some((text, st, w)) = p {
                if !*first {
                    out.push(',');
                }
                *first = false;
                out.push('[');
                json_str(&text, out);
                out.push(',');
                color_json(st.fg, out);
                out.push(',');
                color_json(st.bg, out);
                let _ = write!(out, ",{},{}]", st.mods.bits(), w);
            }
        };
        let mut x = area.left();
        while x < area.right() {
            let cell = &buf[(x, y)];
            let w = cell_width(cell);
            let sym = if cell.diff_option == CellDiffOption::Skip {
                " "
            } else {
                cell.symbol()
            };
            let st = RunStyle {
                fg: cell.fg,
                bg: cell.bg,
                mods: cell.modifier,
            };
            let ascii = sym.is_ascii() && w == 1;
            match (&mut pending, ascii) {
                (Some((text, pst, pw)), true) if *pst == st && text.is_ascii() => {
                    text.push_str(sym);
                    *pw += 1;
                }
                _ => {
                    flush(pending.take(), &mut out, &mut first);
                    if ascii {
                        pending = Some((sym.to_string(), st, 1));
                    } else {
                        // One glyph, its own run, never merged.
                        flush(Some((sym.to_string(), st, w)), &mut out, &mut first);
                    }
                }
            }
            x = x.saturating_add(w as u16);
        }
        flush(pending.take(), &mut out, &mut first);
        out.push(']');
    }
    out.push_str("]}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn wide_glyph_takes_two_cells_and_prints_once() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 1));
        buf.set_string(0, 0, "a한b", Style::new());
        assert_eq!(text(&buf), "a한b");
    }

    #[test]
    fn runs_split_non_ascii_into_single_glyphs() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        buf.set_string(0, 0, "ab█한", Style::new().fg(Color::Green));
        let j = runs_json(&buf);
        assert!(j.starts_with("{\"cols\":6,\"rows\":1,"), "{j}");
        assert!(j.contains("[\"ab\",2,null,0,2]"), "{j}");
        assert!(j.contains("[\"█\",2,null,0,1]"), "{j}");
        assert!(j.contains("[\"한\",2,null,0,2]"), "{j}");
        // The trailing blank cell is the terminal default.
        assert!(j.contains("[\" \",null,null,0,1]"), "{j}");
    }

    #[test]
    fn runs_escape_json() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        buf.set_string(0, 0, "\"\\<", Style::new().fg(Color::Rgb(1, 2, 3)));
        let j = runs_json(&buf);
        assert!(j.contains("\"\\\"\\\\<\""), "{j}");
        assert!(j.contains("\"#010203\""), "{j}");
    }
}
