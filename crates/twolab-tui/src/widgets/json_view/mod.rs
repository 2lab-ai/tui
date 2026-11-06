//! A JSON viewer: this JSON, coloured — keys, strings, numbers, keywords —
//! wrapped to the width in cells, scrollable, with where you are in it.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `highlight_json_line`,
//! `raw_body_lines`, `wrap_raw_line` and the scrolling half of
//! `draw_raw_modal` (visible-slice rendering, a proportional scrollbar only
//! when the content overflows), with the keys of `on_key_raw_modal`
//! (`src/tui/mod.rs`): ↑/↓, PgUp/PgDn by 10, Home, End.
//!
//! Changes from llmux:
//! - colours come from the [`Theme`]: keys `accent`, strings `status(Done)`,
//!   numbers `level(Warn)`, `true`/`false`/`null` `status(Running)`,
//!   punctuation `dim`;
//! - no `serde_json`: [`pretty`] re-indents one JSON document itself and
//!   keeps the keys in the order they came over the wire (serde's `Value`
//!   sorted them);
//! - long lines wrap at the viewport width in cells (a wide glyph never
//!   straddles the edge, continuation rows keep the line's indent) instead
//!   of 2048-byte chunks that broke highlighting;
//! - PgUp/PgDn move by the real view height, and the render clamps the
//!   scroll itself (llmux returned the maximum for the runtime to clamp);
//! - a footer says `lines 12–40 of 213`, so a scrolled view is never mistaken
//!   for the whole body.
//! - the wrapped row counts are measured once per body and width and kept
//!   in the [`JsonViewState`] (llmux re-wrapped every line every frame).
//!   Keep one state per body: when the lines change, create a new state or
//!   call [`JsonViewState::invalidate`] / [`JsonViewState::set_generation`];
//!   the state cannot tell a new body of the same length from the old one.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols::scrollbar;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget};
use unicode_segmentation::UnicodeSegmentation;

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Level, Status, Theme};

/// Rows PgUp/PgDn move before the first render says how tall the view is
/// (llmux's `MODAL_PAGE`).
pub const PAGE: usize = 10;

/// Nesting deeper than this is not pretty-printed (it renders as plain
/// text): a hostile body cannot overflow the stack.
pub const MAX_DEPTH: usize = 128;

/// The style of each JSON token, from a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JsonStyles {
    pub key: Style,
    pub string: Style,
    pub number: Style,
    pub keyword: Style,
    pub punct: Style,
}

impl JsonStyles {
    pub fn new(theme: &Theme) -> JsonStyles {
        JsonStyles {
            key: theme.accent(),
            string: theme.status(Status::Done),
            number: theme.level(Level::Warn),
            keyword: theme.status(Status::Running),
            punct: theme.dim(),
        }
    }
}

/// One line of JSON as styled spans. Pretty-printed JSON never splits a
/// string across lines, so a per-line pass is lossless. The scan is
/// byte-based but only ever splits at ASCII bytes, so it is char-safe.
pub fn json_line(line: &str, theme: &Theme) -> Line<'static> {
    json_line_with(line, &JsonStyles::new(theme))
}

/// [`json_line`] with styles of the host's own (a dashboard keeping the
/// colours its users know).
pub fn json_line_with(line: &str, st: &JsonStyles) -> Line<'static> {
    let st = *st;
    let bytes = line.as_bytes();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        _ => i += 1,
                    }
                }
                let end = i.min(bytes.len());
                let is_key = line[end..].trim_start().starts_with(':');
                let style = if is_key { st.key } else { st.string };
                spans.push(Span::styled(line[start..end].to_string(), style));
            }
            b'0'..=b'9' | b'-' => {
                let start = i;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E')
                {
                    i += 1;
                }
                spans.push(Span::styled(line[start..i].to_string(), st.number));
            }
            b't' | b'f' | b'n' => {
                let rest = &line[i..];
                match ["true", "false", "null"]
                    .iter()
                    .find(|k| rest.starts_with(*k))
                {
                    Some(k) => {
                        spans.push(Span::styled((*k).to_string(), st.keyword));
                        i += k.len();
                    }
                    None => {
                        spans.push(Span::styled(rest[..1].to_string(), st.punct));
                        i += 1;
                    }
                }
            }
            _ => {
                // Whitespace, punctuation and non-ASCII bytes: batch to the
                // next token start. ASCII stops are always char boundaries.
                let start = i;
                i += 1;
                while i < bytes.len()
                    && !matches!(bytes[i], b'"' | b'0'..=b'9' | b'-' | b't' | b'f' | b'n')
                {
                    i += 1;
                }
                spans.push(Span::styled(line[start..i].to_string(), st.punct));
            }
        }
    }
    Line::from(spans)
}

/// One JSON document re-indented two spaces a level (serde's
/// `to_string_pretty` shape), keys in their original order, strings and
/// numbers byte for byte. `None` when `src` is not exactly one JSON value.
pub fn pretty(src: &str) -> Option<String> {
    let mut p = Pretty {
        s: src.as_bytes(),
        src,
        i: 0,
        out: String::with_capacity(src.len() + src.len() / 2),
    };
    p.ws();
    p.value(0)?;
    p.ws();
    (p.i == p.s.len()).then_some(p.out)
}

struct Pretty<'s> {
    s: &'s [u8],
    src: &'s str,
    i: usize,
    out: String,
}

impl Pretty<'_> {
    fn ws(&mut self) {
        while self
            .s
            .get(self.i)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            self.i += 1;
        }
    }

    fn eat(&mut self, b: u8) -> bool {
        if self.s.get(self.i) == Some(&b) {
            self.i += 1;
            true
        } else {
            false
        }
    }

    fn indent(&mut self, depth: usize) {
        self.out.push('\n');
        self.out.extend(std::iter::repeat_n(' ', depth * 2));
    }

    fn value(&mut self, depth: usize) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        match *self.s.get(self.i)? {
            b'{' => self.container(depth, b'}', true),
            b'[' => self.container(depth, b']', false),
            b'"' => self.string(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => self.literal(),
        }
    }

    fn container(&mut self, depth: usize, close: u8, object: bool) -> Option<()> {
        let open = self.s[self.i];
        self.i += 1;
        self.out.push(open as char);
        self.ws();
        if self.eat(close) {
            self.out.push(close as char);
            return Some(());
        }
        loop {
            self.indent(depth + 1);
            if object {
                if self.s.get(self.i) != Some(&b'"') {
                    return None;
                }
                self.string()?;
                self.ws();
                if !self.eat(b':') {
                    return None;
                }
                self.out.push_str(": ");
                self.ws();
            }
            self.value(depth + 1)?;
            self.ws();
            if self.eat(b',') {
                self.out.push(',');
                self.ws();
                continue;
            }
            if self.eat(close) {
                self.indent(depth);
                self.out.push(close as char);
                return Some(());
            }
            return None;
        }
    }

    fn string(&mut self) -> Option<()> {
        let start = self.i;
        self.i += 1;
        loop {
            match *self.s.get(self.i)? {
                b'"' => break,
                b'\\' => match *self.s.get(self.i + 1)? {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.i += 2,
                    b'u' => {
                        let hex = self.s.get(self.i + 2..self.i + 6)?;
                        if !hex.iter().all(u8::is_ascii_hexdigit) {
                            return None;
                        }
                        self.i += 6;
                    }
                    _ => return None,
                },
                b if b < 0x20 => return None,
                _ => self.i += 1,
            }
        }
        self.i += 1;
        self.out.push_str(&self.src[start..self.i]);
        Some(())
    }

    fn number(&mut self) -> Option<()> {
        let start = self.i;
        self.eat(b'-');
        let digits = |p: &mut Self| {
            let from = p.i;
            while p.s.get(p.i).is_some_and(u8::is_ascii_digit) {
                p.i += 1;
            }
            p.i - from
        };
        if self.eat(b'0') {
            // No leading zeros.
        } else if digits(self) == 0 {
            return None;
        }
        if self.eat(b'.') && digits(self) == 0 {
            return None;
        }
        if self.eat(b'e') || self.eat(b'E') {
            let _ = self.eat(b'+') || self.eat(b'-');
            if digits(self) == 0 {
                return None;
            }
        }
        self.out.push_str(&self.src[start..self.i]);
        Some(())
    }

    fn literal(&mut self) -> Option<()> {
        let rest = &self.src[self.i..];
        let word = ["true", "false", "null"]
            .into_iter()
            .find(|w| rest.starts_with(w))?;
        self.i += word.len();
        self.out.push_str(word);
        Some(())
    }
}

/// A captured body as styled lines: one JSON document pretty-printed and
/// highlighted; otherwise its own lines, with the payload of SSE
/// `data: {json}` lines highlighted inline; anything else plain.
pub fn body_lines(body: &str, theme: &Theme) -> Vec<Line<'static>> {
    if body.is_empty() {
        return vec![Line::from(Span::styled("(empty body)", theme.dim()))];
    }
    if let Some(p) = pretty(body) {
        return p.lines().map(|l| json_line(l, theme)).collect();
    }
    body.lines()
        .map(|line| match line.strip_prefix("data: ") {
            Some(rest) if pretty(rest).is_some() => {
                let mut spans = vec![Span::styled("data: ", theme.dim())];
                spans.extend(json_line(rest, theme).spans);
                Line::from(spans)
            }
            _ => Line::from(line.to_string()),
        })
        .collect()
}

/// Continuation rows start under the line's own indent (at most half the
/// width), so a wrapped string stays inside its JSON level.
fn hang(line: &Line, width: usize) -> usize {
    let mut lead = 0;
    'spans: for span in &line.spans {
        for b in span.content.bytes() {
            if b != b' ' {
                break 'spans;
            }
            lead += 1;
        }
    }
    lead.min(width / 2)
}

/// Rows `line` takes hard-wrapped at `width` cells (see [`wrap_line`]).
pub fn wrapped_rows(line: &Line, width: usize) -> usize {
    let width = width.max(1);
    if text::line_width(line) <= width {
        return 1;
    }
    let hang = hang(line, width);
    let (mut rows, mut col, mut start) = (1, 0, 0);
    for span in &line.spans {
        for g in span.content.graphemes(true) {
            let w = text::width(g);
            if col + w > width && col > start {
                rows += 1;
                (col, start) = (hang, hang);
            }
            col += w;
        }
    }
    rows
}

/// `line` hard-wrapped at `width` cells, styles kept. Graphemes are never
/// split, a wide glyph that would straddle the edge starts the next row, and
/// continuation rows are indented like the line itself.
pub fn wrap_line(line: &Line<'static>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    if text::line_width(line) <= width {
        return vec![line.clone()];
    }
    let hang = hang(line, width);
    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let (mut col, mut start) = (0, 0);
    for span in &line.spans {
        let mut chunk = String::new();
        for g in span.content.graphemes(true) {
            let w = text::width(g);
            if col + w > width && col > start {
                if !chunk.is_empty() {
                    row.push(Span::styled(std::mem::take(&mut chunk), span.style));
                }
                rows.push(Line::from(std::mem::take(&mut row)));
                if hang > 0 {
                    row.push(Span::raw(" ".repeat(hang)));
                }
                (col, start) = (hang, hang);
            }
            chunk.push_str(g);
            col += w;
        }
        if !chunk.is_empty() {
            row.push(Span::styled(chunk, span.style));
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(Line::from(row));
    }
    rows
}

/// Scroll position, kept by the host between frames. It also remembers how
/// many rows each line wraps to, so a frame over a large body (llmux's raw
/// viewer shows bodies up to 8 MiB) does not re-measure every line.
///
/// **One `JsonViewState` per body.** The remembered rows belong to the
/// body they were measured on, and the state cannot see when the lines
/// change (a new body can sit at the old one's address with the same
/// length). When the lines change, create a new state, or call
/// [`JsonViewState::invalidate`] or [`JsonViewState::set_generation`] on
/// this one. The rows are kept per (generation, number of lines, width): a
/// body rebuilt each frame with the same content is measured once, and a
/// body of another length is never drawn with the old rows, but a body of
/// the same length is only told apart by its generation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JsonViewState {
    /// First visible row (wrapped rows, not source lines).
    pub scroll: usize,
    max_scroll: Option<usize>,
    page: usize,
    shown: Option<(usize, usize, usize)>,
    /// Which body the host says this is.
    generation: u64,
    /// Row starts per line at the last two widths (with and without the
    /// scrollbar column), most recent first.
    rows: Vec<RowIndex>,
}

/// Where each line's first row sits once wrapped at `width`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RowIndex {
    /// The lines measured: generation, number of lines, width.
    key: (u64, usize, usize),
    starts: Vec<usize>,
    total: usize,
}

impl JsonViewState {
    /// ↑/k ↓/j one row, PgUp/PgDn a page, Home/End the ends. Returns
    /// whether the view moved. Before the first render End scrolls as far
    /// as it can and the render clamps it.
    pub fn on_key(&mut self, key: KeyInput) -> bool {
        let page = if self.page == 0 { PAGE } else { self.page };
        let max = self.max_scroll.unwrap_or(usize::MAX);
        let to = match key.key {
            Key::Up | Key::Char('k') => self.scroll.saturating_sub(1),
            Key::Down | Key::Char('j') => self.scroll.saturating_add(1),
            Key::PageUp => self.scroll.saturating_sub(page),
            Key::PageDown => self.scroll.saturating_add(page),
            Key::Home => 0,
            Key::End => max,
            _ => return false,
        }
        .min(max);
        let moved = to != self.scroll;
        self.scroll = to;
        moved
    }

    /// `(first line, last line, total lines)`, 1-based, from the last
    /// render.
    pub fn shown(&self) -> Option<(usize, usize, usize)> {
        self.shown
    }

    /// The furthest `scroll` can go at the last render's size.
    pub fn max_scroll(&self) -> Option<usize> {
        self.max_scroll
    }

    /// The lines changed (a new body, or the old one edited): forget the
    /// remembered row counts and start a new generation. Call it, or
    /// [`JsonViewState::set_generation`], or use a new state, whenever the
    /// lines change; the scroll position stays (clamped at the next render).
    pub fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.rows.clear();
    }

    /// Say which body the next renders show, for a host that already
    /// numbers its bodies (a record id, a revision): a different number
    /// from the last one forgets the remembered row counts, the same number
    /// keeps them.
    pub fn set_generation(&mut self, generation: u64) {
        if generation != self.generation {
            self.generation = generation;
            self.rows.clear();
        }
    }

    /// The generation the remembered rows belong to (0 for a new state).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The row index for `lines` at `width`, measured once per (generation,
    /// number of lines, width).
    fn index(&mut self, lines: &[Line<'static>], width: usize) -> &RowIndex {
        let key = (self.generation, lines.len(), width);
        match self.rows.iter().position(|r| r.key == key) {
            Some(0) => {}
            Some(i) => self.rows.swap(0, i),
            None => {
                let mut starts = Vec::with_capacity(lines.len());
                let mut total = 0;
                for l in lines {
                    starts.push(total);
                    total += wrapped_rows(l, width);
                }
                self.rows.insert(0, RowIndex { key, starts, total });
                self.rows.truncate(2);
            }
        }
        &self.rows[0]
    }
}

/// Scrollable, wrapped, highlighted lines with a position footer and a
/// scrollbar when they overflow.
#[derive(Debug, Clone)]
pub struct JsonView<'a> {
    lines: &'a [Line<'static>],
    theme: Theme,
    hints: bool,
}

impl<'a> JsonView<'a> {
    /// `lines` from [`body_lines`] (built once, not per frame).
    pub fn new(lines: &'a [Line<'static>]) -> JsonView<'a> {
        JsonView {
            lines,
            theme: Theme::default(),
            hints: true,
        }
    }

    pub fn theme(mut self, theme: Theme) -> JsonView<'a> {
        self.theme = theme;
        self
    }

    /// The key legend at the left of the footer. Default on.
    pub fn hints(mut self, hints: bool) -> JsonView<'a> {
        self.hints = hints;
        self
    }
}

impl StatefulWidget for &JsonView<'_> {
    type State = JsonViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut JsonViewState) {
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        let footer = area.height >= 2;
        let view_h = (area.height - u16::from(footer)) as usize;
        // A scrollbar column only when the content overflows without it.
        let mut text_w = area.width as usize;
        let mut total = state.index(self.lines, text_w).total;
        let bar = total > view_h && area.width >= 2;
        if bar {
            text_w -= 1;
            total = state.index(self.lines, text_w).total;
        }
        let max = total.saturating_sub(view_h);
        state.scroll = state.scroll.min(max);
        state.max_scroll = Some(max);
        state.page = view_h.saturating_sub(1).max(1);

        // The line holding the first visible row, and how far into it.
        let scroll = state.scroll;
        let starts = &state.index(self.lines, text_w).starts;
        let first = starts.partition_point(|&s| s <= scroll).saturating_sub(1);
        let mut skip = starts.get(first).map_or(0, |s| scroll - s);
        let mut y = area.y;
        let mut last = first;
        let bottom = area.y + view_h as u16;
        'lines: for (i, line) in self.lines.iter().enumerate().skip(first) {
            for row in wrap_line(line, text_w).into_iter().skip(skip) {
                if y >= bottom {
                    break 'lines;
                }
                buf.set_line(area.x, y, &row, text_w as u16);
                last = i;
                y += 1;
            }
            skip = 0;
        }
        let n = self.lines.len();
        state.shown = (n > 0).then_some((first + 1, last + 1, n));

        if bar {
            let mut sb = ScrollbarState::new(max + 1)
                .position(state.scroll)
                .viewport_content_length(view_h);
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .symbols(scrollbar::Set {
                    track: "│",
                    thumb: "┃",
                    begin: "",
                    end: "",
                })
                .begin_symbol(None)
                .end_symbol(None)
                .track_style(t.dim())
                .thumb_style(t.accent())
                .render(
                    Rect {
                        height: view_h as u16,
                        ..area
                    },
                    buf,
                    &mut sb,
                );
        }

        if footer {
            let at = if n == 0 {
                "empty".to_string()
            } else if max == 0 {
                format!("{n} lines")
            } else {
                format!("lines {}–{} of {n}", first + 1, last + 1)
            };
            let w = area.width as usize;
            let pos = text::truncate(&at, w).into_owned();
            let pos_w = text::width(&pos);
            let fy = area.bottom() - 1;
            buf.set_stringn(area.right() - pos_w as u16, fy, &pos, pos_w, t.secondary());
            let legend = if max == 0 {
                ""
            } else {
                " ↑↓ PgUp PgDn Home End"
            };
            if self.hints && !legend.is_empty() && text::width(legend) + 1 + pos_w <= w {
                buf.set_stringn(area.x, fy, legend, w - pos_w - 1, t.dim());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::{Color, Modifier};

    fn fg_of(lines: &[Line], needle: &str) -> Option<Color> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content.contains(needle))
            .and_then(|s| s.style.fg)
    }

    /// Port of llmux `raw_body_lines_pretty_print_and_highlight_json`, with
    /// theme tokens for llmux's literal colours.
    #[test]
    fn pretty_prints_and_highlights_with_theme_tokens() {
        let t = Theme::ansi();
        let lines = body_lines(r#"{"model":"opus","n":42,"ok":true,"nil":null}"#, &t);
        assert!(lines.len() > 4, "pretty print splits onto lines");
        assert_eq!(
            fg_of(&lines, "\"model\""),
            Some(Color::Cyan),
            "keys: accent"
        );
        assert_eq!(
            fg_of(&lines, "\"opus\""),
            Some(Color::Green),
            "strings: done"
        );
        assert_eq!(fg_of(&lines, "42"), Some(Color::Yellow), "numbers: warn");
        assert_eq!(fg_of(&lines, "true"), Some(t.run), "keywords: run");
        assert_eq!(fg_of(&lines, "null"), Some(t.run), "keywords: run");
        assert_eq!(
            fg_of(&lines, ","),
            Some(Color::DarkGray),
            "punctuation: dim"
        );
        let text: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("\"model\": \"opus\""), "{text}");
    }

    #[test]
    fn pretty_matches_serde_shape_and_keeps_key_order() {
        let got = pretty(r#" {"z":1,"a":[1,-2.5e3,{"k":"v"}],"e":[],"o":{},"s":"a\"b\u00e9"} "#);
        let want = r#"{
  "z": 1,
  "a": [
    1,
    -2.5e3,
    {
      "k": "v"
    }
  ],
  "e": [],
  "o": {},
  "s": "a\"b\u00e9"
}"#;
        assert_eq!(got.as_deref(), Some(want));
        for bad in [
            "",
            "{",
            "[1,]",
            "{\"a\" 1}",
            "01",
            "1.",
            "-",
            "tru",
            "\"a",
            "{} {}",
            "nul",
            "\"\u{1}\"",
            "\"\\x\"",
            "[1 2]",
        ] {
            assert_eq!(pretty(bad), None, "{bad:?}");
        }
        let deep = format!("{}{}", "[".repeat(MAX_DEPTH + 2), "]".repeat(MAX_DEPTH + 2));
        assert_eq!(pretty(&deep), None, "too deep stays plain");
    }

    /// Port of llmux `raw_body_lines_highlight_sse_data_payloads_inline`.
    #[test]
    fn sse_data_payloads_are_highlighted_inline() {
        let body = "event: message_start\ndata: {\"type\":\"message_start\"}\n\nevent: ping\ndata: not-json";
        let lines = body_lines(body, &Theme::ansi());
        assert_eq!(lines.len(), 5, "SSE keeps its own lines");
        assert!(lines[1].spans.len() > 2, "json payload splits into spans");
        assert_eq!(lines[4].spans.len(), 1, "non-json data stays one span");
        assert_eq!(
            body_lines("", &Theme::ansi())[0].spans[0].content,
            "(empty body)"
        );
    }

    #[test]
    fn json_line_is_char_safe_on_non_ascii() {
        let line = json_line(r#"  "이름": "계정 €", "n": -1"#, &Theme::ansi());
        let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, r#"  "이름": "계정 €", "n": -1"#);
        // An unterminated string with a trailing escape does not panic.
        let _ = json_line("\"abc\\", &Theme::ansi());
    }

    #[test]
    fn wrap_splits_in_cells_and_keeps_styles() {
        let red = Style::new().fg(Color::Red);
        let line = Line::from(vec![Span::raw("ab"), Span::styled("한글한글", red)]);
        let rows = wrap_line(&line, 5);
        let text: Vec<String> = rows
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        // `ab한` is 4 cells; the next `글` would straddle cell 5.
        assert_eq!(text, ["ab한", "글한", "글"]);
        assert_eq!(rows[1].spans[0].style, red);
        assert_eq!(wrapped_rows(&line, 5), 3);
        for w in 1..12 {
            let rows = wrap_line(&line, w);
            assert_eq!(rows.len(), wrapped_rows(&line, w), "{w}");
            assert!(rows.iter().all(|r| text::line_width(r) <= w.max(2)), "{w}");
        }
    }

    #[test]
    fn continuation_rows_keep_the_line_indent() {
        let line = Line::from("    \"text\": \"one two three four\"");
        let rows = wrap_line(&line, 16);
        let text: Vec<String> = rows
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(
            text,
            ["    \"text\": \"one", "     two three f", "    our\""]
        );
        assert_eq!(wrapped_rows(&line, 16), 3);
        // The indent is capped at half the width, so rows always advance.
        for w in 1..20 {
            assert_eq!(wrap_line(&line, w).len(), wrapped_rows(&line, w), "{w}");
        }
    }

    fn numbered(n: usize) -> Vec<Line<'static>> {
        (1..=n)
            .map(|i| Line::from(format!("\"line\": {i}")))
            .collect()
    }

    fn render(lines: &[Line<'static>], state: &mut JsonViewState, w: u16, h: u16) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        JsonView::new(lines)
            .theme(Theme::ansi())
            .render(area, &mut buf, state);
        buf
    }

    #[test]
    fn scrolls_by_keys_and_says_where_it_is() {
        let lines = numbered(40);
        let mut s = JsonViewState::default();
        let buf = render(&lines, &mut s, 40, 11);
        let rows = snapshot::lines(&buf);
        assert_eq!(rows[0], "\"line\": 1                              ┃");
        assert!(rows[10].ends_with("lines 1–10 of 40"), "{}", rows[10]);
        assert!(
            rows[10].starts_with(" ↑↓ PgUp PgDn Home End"),
            "{}",
            rows[10]
        );
        assert_eq!(s.shown(), Some((1, 10, 40)));
        assert_eq!(s.max_scroll(), Some(30));
        assert!(s.on_key(Key::Down.into()));
        assert!(s.on_key(Key::PageDown.into()));
        assert_eq!(s.scroll, 10, "a page is the view minus one row");
        assert!(s.on_key(Key::End.into()));
        assert_eq!(s.scroll, 30);
        assert!(!s.on_key(Key::End.into()), "already there");
        assert!(!s.on_key(Key::Down.into()));
        let buf = render(&lines, &mut s, 40, 11);
        assert!(snapshot::lines(&buf)[10].ends_with("lines 31–40 of 40"));
        // The thumb sits at the bottom of the track.
        assert_eq!(buf[(39, 9)].symbol(), "┃");
        assert_eq!(buf[(39, 0)].symbol(), "│");
        assert!(s.on_key(Key::Home.into()));
        assert_eq!(s.scroll, 0);
        assert!(!s.on_key('x'.into()));
    }

    #[test]
    fn row_counts_are_measured_once_per_width() {
        let lines = numbered(40);
        let mut s = JsonViewState::default();
        let first = snapshot::lines(&render(&lines, &mut s, 40, 11));
        assert_eq!(s.rows.len(), 2, "with and without the scrollbar column");
        let keys: Vec<_> = s.rows.iter().map(|r| r.key).collect();
        let again = snapshot::lines(&render(&lines, &mut s, 40, 11));
        assert_eq!(first, again);
        assert_eq!(
            s.rows.iter().map(|r| r.key).collect::<Vec<_>>(),
            keys,
            "nothing re-measured"
        );
        // The slice address is not the identity: the same body rebuilt in
        // another allocation (a host that builds its lines per frame) is
        // not measured again.
        let copy = lines.clone();
        assert_ne!(copy.as_ptr(), lines.as_ptr());
        assert_eq!(snapshot::lines(&render(&copy, &mut s, 40, 11)), first);
        assert_eq!(
            s.rows.iter().map(|r| r.key).collect::<Vec<_>>(),
            keys,
            "the copy is not re-measured"
        );
        // A body of another length never reuses the old rows (a guard, not
        // the contract: the host still says when its lines change).
        let fewer = numbered(3);
        let rows = snapshot::lines(&render(&fewer, &mut s, 40, 11));
        assert_eq!(rows[0], "\"line\": 1");
        assert_eq!(s.max_scroll(), Some(0));
        s.invalidate();
        assert!(s.rows.is_empty());
    }

    /// Review #2 (first reviewer): `["a"]` drawn at 5×3, then a body of the
    /// same length in the same allocation, one 20-cell line. The rows are
    /// the old body's until the host says the lines changed; after
    /// `invalidate()` the new body scrolls to 3, as a fresh state does.
    #[test]
    fn a_new_body_at_the_old_address_is_measured_after_invalidate() {
        let mut lines = vec![Line::from("a")];
        let mut s = JsonViewState::default();
        render(&lines, &mut s, 5, 3);
        assert_eq!(s.max_scroll(), Some(0));
        let at = lines.as_ptr();
        lines.clear();
        lines.push(Line::from("x".repeat(20)));
        assert_eq!(lines.as_ptr(), at, "same address, same length");
        let before = s.generation();
        s.invalidate();
        assert_ne!(s.generation(), before, "invalidate() is a new generation");
        render(&lines, &mut s, 5, 3);
        assert_eq!(s.max_scroll(), Some(3));
        let mut fresh = JsonViewState::default();
        render(&lines, &mut fresh, 5, 3);
        assert_eq!(fresh.max_scroll(), Some(3));
    }

    /// Review #2 (second reviewer): bodies A and B, five short lines each,
    /// then C, five 60-cell lines, in A's allocation. A generation per body
    /// tells them apart; C draws as on a fresh state.
    #[test]
    fn same_length_bodies_are_told_apart_by_generation() {
        let mut a = numbered(5);
        let b: Vec<Line<'static>> = (1..=5).map(|i| Line::from(format!("\"b\": {i}"))).collect();
        let mut s = JsonViewState::default();
        s.set_generation(1);
        render(&a, &mut s, 30, 5);
        s.set_generation(2);
        render(&b, &mut s, 30, 5);
        let at = a.as_ptr();
        a.clear();
        a.extend((0..5).map(|i| Line::from(format!("{i}{}", "c".repeat(59)))));
        assert_eq!(a.as_ptr(), at, "C in A's allocation");
        s.set_generation(3);
        assert_eq!(s.generation(), 3);
        let got = snapshot::lines(&render(&a, &mut s, 30, 5));
        let mut fresh = JsonViewState::default();
        let want = snapshot::lines(&render(&a, &mut fresh, 30, 5));
        assert_eq!(got, want);
        assert_eq!(s.max_scroll(), Some(11));
        assert_eq!(s.max_scroll(), fresh.max_scroll());
        assert_eq!(s.shown(), fresh.shown());
        // The same generation again keeps the rows.
        let keys: Vec<_> = s.rows.iter().map(|r| r.key).collect();
        s.set_generation(3);
        assert_eq!(s.rows.iter().map(|r| r.key).collect::<Vec<_>>(), keys);
    }

    #[test]
    fn the_first_row_is_found_inside_a_wrapped_line() {
        let t = Theme::ansi();
        let long = format!(r#"{{"text":"{}"}}"#, "word ".repeat(60).trim_end());
        let mut lines = body_lines(&long, &t);
        lines.extend(numbered(30));
        let mut s = JsonViewState::default();
        let full = snapshot::lines(&render(&lines, &mut s, 30, 8));
        s.scroll = 2;
        let scrolled = snapshot::lines(&render(&lines, &mut s, 30, 8));
        // Same text; only the scrollbar thumb (last column) moved.
        let text = |r: &str| r.chars().take(29).collect::<String>();
        assert_eq!(
            text(&scrolled[0]),
            text(&full[2]),
            "scrolling two rows starts mid-line"
        );
        assert_eq!(text(&scrolled[1]), text(&full[3]));
    }

    #[test]
    fn fits_without_a_scrollbar_and_says_so() {
        let lines = numbered(3);
        let mut s = JsonViewState::default();
        let buf = render(&lines, &mut s, 30, 6);
        let rows = snapshot::lines(&buf);
        assert_eq!(rows[0], "\"line\": 1", "no scrollbar column");
        assert_eq!(rows[5], "                       3 lines");
        assert_eq!(s.max_scroll(), Some(0));
    }

    #[test]
    fn long_lines_wrap_and_the_end_is_reachable() {
        let t = Theme::ansi();
        let long = format!(r#"{{"text":"{}"}}"#, "word ".repeat(60).trim_end());
        let lines = body_lines(&long, &t);
        assert_eq!(lines.len(), 3);
        let mut s = JsonViewState::default();
        s.on_key(Key::End.into());
        let buf = render(&lines, &mut s, 30, 6);
        let rows = snapshot::lines(&buf);
        assert_eq!(rows[4].trim_end_matches(['│', '┃', ' ']), "}");
        assert!(rows[5].ends_with("of 3"), "{}", rows[5]);
        for row in &rows[..5] {
            assert!(text::width(row) <= 30);
        }
        // Down to one cell: no panic.
        for (w, h) in [(1, 1), (2, 1), (1, 2), (3, 3), (12, 2)] {
            render(&lines, &mut s, w, h);
        }
    }

    #[test]
    fn mono_keeps_keys_bold_and_punctuation_dim() {
        let lines = body_lines(
            r#"{"model":"opus-4-8[1m]","max_tokens":32000}"#,
            &Theme::mono(),
        );
        let key = lines[1]
            .spans
            .iter()
            .find(|s| s.content == "\"model\"")
            .unwrap();
        assert!(key.style.add_modifier.contains(Modifier::BOLD));
        assert!(matches!(key.style.fg, None | Some(Color::Reset)));
        let punct = &lines[0].spans[0];
        assert!(punct.style.add_modifier.contains(Modifier::DIM));
    }
}
