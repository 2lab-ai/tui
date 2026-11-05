// Ported from OpenAI Codex `codex-rs/tui/src/pager_overlay.rs` (`PagerView`,
// `StaticOverlay`, the navigation hints and their tests)
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: keys come in as `KeyInput` with Codex's
// default pager bindings plus Esc, and return an `Outcome`; the scroll offset
// is clamped from the last paint so rendering takes `&self`; content
// positions are relative to the content area (Codex compares them with the
// area's absolute `y`); lines wrap through `WrappedLine`, measured exactly
// and cached per width; the bottom rule survives widths narrower than its
// label; one hint row; glyphs are configurable and Narrow by default; styles
// come from the `Theme`. The transcript and analytics overlays stay behind.

//! The full text, scrollable: a request body, a log, a transcript.
//!
//! Ported from OpenAI Codex `pager_overlay.rs` (Apache-2.0), the overlay
//! Codex opens for diffs, approvals and static text. It tells the person
//! where they are in something longer than the screen:
//!
//! ```text
//! / Request body / / / / / / / / / / /
//! {"model": "claude-opus-5-5",
//!  "max_tokens": 64000,
//! ~
//! ╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌
//! q/esc close  up/down scroll  space/pgdn page  home/end jump
//! ```
//!
//! A title rule, the content (lines wrap; nothing is cut), `~` past the end,
//! a bottom rule with how far down you are, and the keys. Heights are
//! measured once per width and cached, so a long transcript scrolls without
//! re-wrapping every line every frame.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::cell::Cell;
use std::fmt;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::input::{Key, KeyInput};
use crate::render::{CachedRenderable, Renderable, render_offset_content};
use crate::text;
use crate::theme::Theme;

/// One line that wraps at the width it is given (ratatui's word wrap,
/// leading spaces kept). Its height is measured exactly: the line is
/// painted with a marker line after it into a scratch buffer sized by
/// [`text::wrapped_rows_lines`], which never under-counts, and the marker's
/// row is the line's height. Scrolling paints only the visible rows.
#[derive(Debug, Clone, PartialEq)]
pub struct WrappedLine(pub Line<'static>);

impl WrappedLine {
    fn paragraph(&self) -> Paragraph<'static> {
        Paragraph::new(self.0.clone()).wrap(Wrap { trim: false })
    }
}

impl Renderable for WrappedLine {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Widget::render(self.paragraph(), area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        let bound = text::wrapped_rows_lines(std::slice::from_ref(&self.0), width);
        let h = u16::try_from(bound + 1).unwrap_or(u16::MAX);
        let area = Rect::new(0, 0, width, h);
        let mut buf = Buffer::empty(area);
        Paragraph::new(Text::from(vec![self.0.clone(), Line::from("|")]))
            .wrap(Wrap { trim: false })
            .render(area, &mut buf);
        // The marker is the last thing painted.
        (0..h)
            .rev()
            .find(|&y| (0..width).any(|x| buf[(x, y)].symbol() != " "))
            .map_or(1, |y| y.max(1))
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        Widget::render(self.paragraph().scroll((scroll_offset, 0)), area, buf);
        true
    }
}

/// The rule glyphs. Every default is East-Asian Narrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagerGlyphs {
    /// Repeated across the title row (Codex: `/ `).
    pub title_fill: &'static str,
    /// Repeated across the bottom rule.
    pub rule: &'static str,
    /// Marks each row past the end.
    pub eof: &'static str,
}

impl Default for PagerGlyphs {
    /// `/ `, `╌` (U+254C, Narrow), `~`.
    fn default() -> Self {
        Self {
            title_fill: "/ ",
            rule: "╌",
            eof: "~",
        }
    }
}

impl PagerGlyphs {
    /// Plain ASCII: `/ `, `-`, `~`.
    pub const fn ascii() -> Self {
        Self {
            title_fill: "/ ",
            rule: "-",
            eof: "~",
        }
    }

    /// Codex's solid `─` rule. U+2500 is East-Asian *Ambiguous*: two cells on
    /// a CJK terminal set to wide ambiguous glyphs. Opt in knowingly.
    pub const fn box_drawing() -> Self {
        Self {
            title_fill: "/ ",
            rule: "─",
            eof: "~",
        }
    }
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a pager key, or the view was already at that end.
    Ignored,
    /// The view moved; redraw.
    Scrolled,
    /// `q`, Esc or Ctrl-C: close the pager.
    Closed,
}

/// A scrollable, read-only document with a title. Draw it with
/// `Widget for &Pager` or as a [`Renderable`].
pub struct Pager {
    title: String,
    content: Vec<Box<dyn Renderable>>,
    scroll: usize,
    /// Content rows shown by the last paint — the page size.
    last_page: Cell<Option<usize>>,
    /// Largest offset at the last paint's width and height.
    last_max: Cell<Option<usize>>,
    hints: bool,
    glyphs: PagerGlyphs,
    theme: Theme,
}

impl fmt::Debug for Pager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager")
            .field("title", &self.title)
            .field("items", &self.content.len())
            .field("scroll", &self.scroll)
            .finish_non_exhaustive()
    }
}

fn wrap_lines(lines: Vec<Line<'static>>) -> Vec<Box<dyn Renderable>> {
    lines
        .into_iter()
        .map(|l| Box::new(CachedRenderable::new(WrappedLine(l))) as Box<dyn Renderable>)
        .collect()
}

impl Pager {
    /// A pager over lines that wrap at the pager's width.
    pub fn new(title: impl Into<String>, lines: Vec<Line<'static>>) -> Self {
        Self::with_renderables(title, wrap_lines(lines))
    }

    /// A pager over plain text, one line per `\n`.
    pub fn from_text(title: impl Into<String>, text: &str) -> Self {
        let lines = text
            .split('\n')
            .map(|l| Line::from(l.to_string()))
            .collect();
        Self::new(title, lines)
    }

    /// A pager over any stack of renderables (Codex's `with_renderables`):
    /// a highlighted block, a table, a diff. Wrap expensive ones in
    /// [`CachedRenderable`]; the pager asks every item its height each paint.
    pub fn with_renderables(title: impl Into<String>, content: Vec<Box<dyn Renderable>>) -> Self {
        Self {
            title: title.into(),
            content,
            scroll: 0,
            last_page: Cell::new(None),
            last_max: Cell::new(None),
            hints: true,
            glyphs: PagerGlyphs::default(),
            theme: Theme::default(),
        }
    }

    /// Replace the lines, keeping the scroll position (clamped at the next
    /// paint).
    pub fn set_lines(&mut self, lines: Vec<Line<'static>>) {
        self.content = wrap_lines(lines);
        self.last_max.set(None);
    }

    /// Show the key-hint row under the bottom rule.
    pub fn hints(mut self, hints: bool) -> Self {
        self.hints = hints;
        self
    }

    pub fn glyphs(mut self, glyphs: PagerGlyphs) -> Self {
        self.glyphs = glyphs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
    }

    /// Start scrolled down by `rows` (clamped at the next paint).
    pub fn scroll_to(mut self, rows: usize) -> Self {
        self.scroll = rows;
        self
    }

    /// Rows scrolled past, as of the last paint.
    pub fn scroll_offset(&self) -> usize {
        self.offset()
    }

    /// Rows the content takes at `width`.
    pub fn content_height(&self, width: u16) -> usize {
        self.content
            .iter()
            .map(|c| c.desired_height(width) as usize)
            .sum()
    }

    fn offset(&self) -> usize {
        self.scroll.min(self.last_max.get().unwrap_or(usize::MAX))
    }

    /// Act on a key: ↑/k, ↓/j, PgUp/Shift-Space/Ctrl-B, PgDn/Space/Ctrl-F,
    /// Ctrl-U/Ctrl-D half a page, Home, End; q, Esc or Ctrl-C close.
    pub fn key(&mut self, key: KeyInput) -> Outcome {
        let ctrl = key.mods.ctrl && !key.mods.alt;
        let page = self.last_page.get().unwrap_or(1).max(1);
        let half = page.saturating_add(1) / 2;
        let cur = self.offset();
        let max = self.last_max.get().unwrap_or(usize::MAX);
        let next = match key.key {
            Key::Char('q') if !ctrl => return Outcome::Closed,
            Key::Char('c') if ctrl => return Outcome::Closed,
            Key::Esc => return Outcome::Closed,
            Key::Up => cur.saturating_sub(1),
            Key::Char('k') if !ctrl => cur.saturating_sub(1),
            Key::Down => cur.saturating_add(1),
            Key::Char('j') if !ctrl => cur.saturating_add(1),
            Key::PageUp => cur.saturating_sub(page),
            Key::Char('b') if ctrl => cur.saturating_sub(page),
            Key::Char(' ') if key.mods.shift => cur.saturating_sub(page),
            Key::PageDown => cur.saturating_add(page),
            Key::Char('f') if ctrl => cur.saturating_add(page),
            Key::Char(' ') if !ctrl => cur.saturating_add(page),
            Key::Char('u') if ctrl => cur.saturating_sub(half),
            Key::Char('d') if ctrl => cur.saturating_add(half),
            Key::Home => 0,
            Key::End => usize::MAX,
            _ => return Outcome::Ignored,
        }
        .min(max);
        if next == cur {
            return Outcome::Ignored;
        }
        self.scroll = next;
        Outcome::Scrolled
    }

    /// `/ Title / / / /`: the fill starts one cell after the title, so it
    /// never touches it whatever the title's width.
    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let t = &self.theme;
        let w = area.width as usize;
        let lead = self.glyphs.title_fill;
        let title_room = w.saturating_sub(text::width(lead));
        let title = text::truncate(&self.title, title_room).into_owned();
        let used = text::width(lead) + text::width(&title) + 1;
        let fill_w = text::width(lead).max(1);
        let fill = lead.repeat(w.saturating_sub(used) / fill_w + 1);
        let line = Line::from(vec![
            Span::styled(lead, t.dim()),
            Span::styled(title, t.text().add_modifier(Modifier::BOLD)),
            Span::raw(" "),
            Span::styled(
                text::take_cells(&fill, w.saturating_sub(used))
                    .0
                    .to_string(),
                t.dim(),
            ),
        ]);
        text::truncate_line(line, w).render(area, buf);
    }

    /// Paint the content scrolled by `offset`; returns the rows painted.
    fn render_content(&self, area: Rect, buf: &mut Buffer, offset: usize) -> u16 {
        buf.set_style(area, self.theme.text());
        let mut y = -(offset as isize);
        let mut drawn: u16 = 0;
        let view = area.height as isize;
        for item in &self.content {
            let top = y;
            let height = item.desired_height(area.width) as isize;
            y += height;
            let bottom = y;
            if bottom <= 0 {
                continue;
            }
            if top >= view {
                break;
            }
            if top < 0 {
                let skip = u16::try_from(-top).unwrap_or(u16::MAX);
                drawn = drawn.max(render_offset_content(area, buf, item.as_ref(), skip));
            } else {
                let top = top as u16;
                let h = (height as u16).min(area.height - top);
                item.render(Rect::new(area.x, area.y + top, area.width, h), buf);
                drawn = drawn.max(top + h);
            }
        }
        drawn
    }

    fn render_eof(&self, area: Rect, buf: &mut Buffer, drawn: u16) {
        for y in area.y + drawn..area.bottom() {
            Line::styled(self.glyphs.eof, self.theme.dim())
                .render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }

    fn render_bottom_bar(&self, area: Rect, buf: &mut Buffer, percent: u8) {
        let t = &self.theme;
        let rule_w = text::width(self.glyphs.rule).max(1);
        let rule = self.glyphs.rule.repeat(area.width as usize / rule_w);
        Line::styled(rule, t.border()).render(area, buf);
        let label = format!(" {percent}% ");
        let w = text::width(&label) as u16;
        if area.width > w {
            let x = area.right() - w - 1;
            Line::styled(label, t.secondary()).render(Rect::new(x, area.y, w, 1), buf);
        } else {
            let short = format!("{percent}%");
            Line::styled(
                text::truncate(&short, area.width as usize).into_owned(),
                t.secondary(),
            )
            .render(area, buf);
        }
    }

    fn hint_line(&self) -> Line<'static> {
        let t = &self.theme;
        let mut spans = Vec::new();
        for (i, (key, label)) in [
            ("q/esc", "close"),
            ("up/down", "scroll"),
            ("space/pgdn", "page"),
            ("home/end", "jump"),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(key, t.accent()));
            spans.push(Span::styled(format!(" {label}"), t.dim()));
        }
        Line::from(spans)
    }
}

/// `scroll / max_scroll` as a percentage, or 100 when everything fits.
fn percent(offset: usize, total: usize, view: usize) -> u8 {
    let max_scroll = total.saturating_sub(view);
    if total == 0 || max_scroll == 0 {
        100
    } else {
        ((offset.min(max_scroll) as f32 / max_scroll as f32) * 100.0).round() as u8
    }
}

impl Renderable for Pager {
    fn desired_height(&self, width: u16) -> u16 {
        let content = u16::try_from(self.content_height(width)).unwrap_or(u16::MAX);
        content
            .saturating_add(2)
            .saturating_add(u16::from(self.hints))
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        // Rows yield from the top of this list: hints, title, bottom rule.
        let h = area.height;
        let hints = self.hints && h >= 4;
        let header = h >= 3;
        let rule = h >= 2;
        let mut y = area.y;
        if header {
            self.render_header(Rect::new(area.x, y, area.width, 1), buf);
            y += 1;
        }
        let content_h = h - u16::from(header) - u16::from(rule) - u16::from(hints);
        let content = Rect::new(area.x, y, area.width, content_h);
        let total = self.content_height(area.width);
        let view = content_h as usize;
        let max = total.saturating_sub(view);
        self.last_max.set(Some(max));
        self.last_page.set(Some(view));
        let offset = self.scroll.min(max);
        let drawn = self.render_content(content, buf, offset);
        self.render_eof(content, buf, drawn);
        y += content_h;
        if rule {
            self.render_bottom_bar(
                Rect::new(area.x, y, area.width, 1),
                buf,
                percent(offset, total, view),
            );
            y += 1;
        }
        if hints {
            Renderable::render(
                &self.hint_line(),
                Rect::new(area.x + 1, y, area.width - 1, 1),
                buf,
            );
        }
    }
}

impl Widget for &Pager {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Renderable::render(self, area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    fn draw(p: &Pager, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        Renderable::render(p, area, &mut buf);
        snapshot::lines(&buf)
    }

    fn paragraph_block(label: &str, lines: usize) -> Box<dyn Renderable> {
        let text = Text::from(
            (0..lines)
                .map(|i| Line::from(format!("{label}{i}")))
                .collect::<Vec<_>>(),
        );
        Box::new(Paragraph::new(text)) as Box<dyn Renderable>
    }

    // Upstream: `static_overlay_snapshot_basic` (one hint row; Narrow rule).
    #[test]
    fn static_overlay_snapshot_basic() {
        let p = Pager::new(
            "S T A T I C",
            vec!["one".into(), "two".into(), "three".into()],
        );
        assert_eq!(
            draw(&p, 40, 9),
            vec![
                "/ S T A T I C / / / / / / / / / / / / /",
                "one",
                "two",
                "three",
                "~",
                "~",
                "~",
                "╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌",
                " q/esc close  up/down scroll  space/pgd…",
            ]
        );
    }

    // Upstream: `static_overlay_wraps_long_lines`.
    #[test]
    fn static_overlay_wraps_long_lines() {
        let p = Pager::new(
            "S T A T I C",
            vec![
                "a very long line that should wrap when rendered within a narrow pager overlay width"
                    .into(),
            ],
        )
        .glyphs(PagerGlyphs::box_drawing());
        // Three content rows of four: at the top, 0%.
        assert_eq!(
            draw(&p, 24, 6),
            vec![
                "/ S T A T I C / / / / /",
                "a very long line that",
                "should wrap when",
                "rendered within a narrow",
                "─────────────────── 0% ─",
                " q/esc close  up/down s…",
            ]
        );
        assert_eq!(draw(&p, 24, 8)[4], "pager overlay width");
    }

    // Upstream: `pager_view_content_height_counts_renderables`.
    #[test]
    fn pager_view_content_height_counts_renderables() {
        let p =
            Pager::with_renderables("T", vec![paragraph_block("a", 2), paragraph_block("b", 3)]);
        assert_eq!(p.content_height(80), 5);
    }

    #[test]
    fn keys_scroll_page_and_clamp_at_the_ends() {
        let lines: Vec<Line<'static>> = (1..=50).map(|i| Line::from(format!("line {i}"))).collect();
        let mut p = Pager::new("log", lines);
        // 12 rows: title, 9 content rows, rule, hints.
        let _ = draw(&p, 30, 12);
        assert_eq!(
            p.key(Key::Up.into()),
            Outcome::Ignored,
            "already at the top"
        );
        assert_eq!(p.key(Key::Down.into()), Outcome::Scrolled);
        assert_eq!(p.scroll_offset(), 1);
        p.key(KeyInput::from(' '));
        assert_eq!(p.scroll_offset(), 10);
        p.key(KeyInput::ctrl(Key::Char('d')));
        assert_eq!(p.scroll_offset(), 15);
        p.key(Key::End.into());
        assert_eq!(p.scroll_offset(), 41);
        assert_eq!(p.key(Key::PageDown.into()), Outcome::Ignored);
        let lines = draw(&p, 30, 12);
        assert_eq!(lines[1], "line 42");
        assert_eq!(lines[9], "line 50");
        assert!(lines[10].ends_with("100% ╌"), "{lines:#?}");
        p.key(Key::Home.into());
        assert!(draw(&p, 30, 12)[10].ends_with(" 0% ╌"));
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        p.key(Key::Down.into());
        assert!(draw(&p, 30, 12)[10].ends_with(" 10% ╌"));
        for k in [
            KeyInput::from('q'),
            Key::Esc.into(),
            KeyInput::ctrl(Key::Char('c')),
        ] {
            assert_eq!(p.key(k), Outcome::Closed);
        }
    }

    #[test]
    fn wrapped_body_matches_one_wrapped_paragraph() {
        let lines: Vec<Line<'static>> = [
            "  - removed the sleep(50ms) and awaited the frame tick instead; the flake rate went from 1 in 40 to 0 in 500 runs",
            "",
            "word  double  spaces   triple    end",
            "    fn render(&self, area: Rect, buf: &mut Buffer) { let x = area.width.saturating_sub(1); }",
            "한국어 문장도 셀 폭으로 줄바꿈합니다 그리고 끝",
        ]
        .into_iter()
        .map(Line::from)
        .collect();
        for w in [7u16, 13, 24, 31, 57] {
            let p = Pager::new("t", lines.clone()).hints(false);
            let total = p.content_height(w);
            let area = Rect::new(0, 0, w, total as u16);
            let mut expected = Buffer::empty(area);
            Paragraph::new(Text::from(lines.clone()))
                .wrap(Wrap { trim: false })
                .render(area, &mut expected);
            let got = draw(&p, w, total as u16 + 2);
            assert_eq!(&got[1..=total], &snapshot::lines(&expected)[..], "w={w}");
        }
    }

    #[test]
    fn heights_are_cached_per_width() {
        let p = Pager::from_text("t", "alpha beta gamma\ndelta");
        assert_eq!(p.content_height(80), 2);
        assert_eq!(p.content_height(6), 4);
        assert_eq!(p.content_height(80), 2);
    }

    #[test]
    fn scrolling_through_a_partly_visible_line_paints_its_tail() {
        let p = Pager::new(
            "t",
            vec![Line::from("aaaa bbbb cccc dddd"), Line::from("next")],
        )
        .hints(false)
        .scroll_to(2);
        // At width 5 the first line is four rows; offset 2 starts at "cccc".
        // Three content rows: five rows in all, so the offset can reach 2.
        let lines = draw(&p, 5, 5);
        assert_eq!(&lines[1..4], &["cccc", "dddd", "next"]);
        assert_eq!(p.scroll_offset(), 2);
    }

    #[test]
    fn tiny_areas_keep_the_content_then_the_rule() {
        let p = Pager::from_text("Request body", "{}");
        assert_eq!(draw(&p, 20, 1), vec!["{}"]);
        assert_eq!(draw(&p, 20, 2)[1], "╌╌╌╌╌╌╌╌╌╌╌╌╌ 100% ╌");
        assert!(draw(&p, 20, 3)[0].starts_with("/ Request body"));
        assert_eq!(draw(&p, 4, 2)[1], "100%");
        assert_eq!(draw(&p, 3, 2)[1], "10…");
        let _ = draw(&p, 1, 1);
    }

    #[test]
    fn mono_rules_and_eof_carry_no_colour() {
        let p = Pager::from_text("t", "x").theme(Theme::mono());
        let area = Rect::new(0, 0, 20, 6);
        let mut buf = Buffer::empty(area);
        Renderable::render(&p, area, &mut buf);
        for c in buf.content() {
            assert!(matches!(c.fg, ratatui::style::Color::Reset), "{c:?}");
            assert!(matches!(c.bg, ratatui::style::Color::Reset), "{c:?}");
        }
        assert!(buf[(0, 2)].modifier.contains(Modifier::DIM), "~ is dim");
    }
}
