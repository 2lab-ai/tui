//! A one-row tab strip: which view you are on, and how to get to the others.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `draw_tabs`, `TABS`,
//! `TabHit`, `hit_test_tabs`, `tabs_room` and `draw_tabs_tail`. Every label's
//! rect comes back from the render, so a mouse click maps to the tab that was
//! actually drawn there; a right-aligned tail (llmux puts the header's `cfg`
//! path there) takes the room the labels leave and gives way first.
//!
//! Changes from llmux:
//! - widths are terminal cells, not `chars().count()`, so a Korean label gets
//!   a click rect as wide as it is drawn (llmux defect §9.1, `ui.rs:684`);
//! - key hints are kept when they fit, measured, instead of a fixed 140-column
//!   threshold; a key letter found in its label is underlined at no cost;
//! - when even the plain labels do not fit, the strip scrolls to keep the
//!   active tab on screen and counts the hidden ones (`‹2 … 3›`) instead of
//!   clipping them silently.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::Theme;

/// One tab: its label and, optionally, the key that opens it (`"a"`,
/// `"Esc"`, `"?"`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab<'a> {
    pub label: Cow<'a, str>,
    pub key: Option<Cow<'a, str>>,
}

impl<'a> Tab<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>) -> Tab<'a> {
        Tab {
            label: label.into(),
            key: None,
        }
    }

    /// The key that opens this tab. A single character is matched against
    /// typed text; `Esc`, `Enter` and `Tab` name those keys.
    pub fn key(mut self, key: impl Into<Cow<'a, str>>) -> Tab<'a> {
        self.key = Some(key.into());
        self
    }
}

/// How a tab's key is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hints {
    /// The key letter is underlined inside the label (`a` in `accounts`);
    /// a key the label does not contain gets a `[Esc] ` prefix while it fits.
    #[default]
    Mnemonic,
    /// llmux's form: every keyed tab reads `[a] accounts` while it fits.
    Bracket,
    /// No hints.
    Off,
}

/// The rect one tab label was drawn in, in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabHit {
    pub rect: Rect,
    pub index: usize,
}

impl TabHit {
    pub fn contains(&self, col: u16, row: u16) -> bool {
        row >= self.rect.y
            && row < self.rect.bottom()
            && col >= self.rect.x
            && col < self.rect.right()
    }
}

/// Which tab, if any, a click at `(col, row)` lands on.
pub fn hit_test(hits: &[TabHit], col: u16, row: u16) -> Option<usize> {
    hits.iter().find(|h| h.contains(col, row)).map(|h| h.index)
}

/// Where everything went in one render.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TabsLayout {
    /// One rect per drawn label, left to right.
    pub hits: Vec<TabHit>,
    /// The `[k] ` prefixes are shown.
    pub hinted: bool,
    /// Tabs scrolled off the left and right ends (`‹N` / `N›`).
    pub hidden_before: usize,
    pub hidden_after: usize,
    /// Cells left free at the right end of the row after the labels.
    pub room: usize,
    /// The tail was drawn.
    pub tail: bool,
}

/// One label as drawn: an optional `[k] ` prefix, and the byte range of an
/// underlined key letter.
struct Piece<'s> {
    prefix: Option<String>,
    label: &'s str,
    mnemonic: Option<(usize, usize)>,
}

impl Piece<'_> {
    fn width(&self) -> usize {
        self.prefix.as_deref().map_or(0, text::width) + text::width(self.label)
    }
}

/// A tab strip. Draws one row.
#[derive(Debug, Clone)]
pub struct Tabs<'a> {
    tabs: Vec<Tab<'a>>,
    active: usize,
    theme: Theme,
    hints: Hints,
    divider: Cow<'a, str>,
    indent: u16,
    tail: Vec<Span<'a>>,
}

impl<'a> Tabs<'a> {
    pub fn new(tabs: impl IntoIterator<Item = Tab<'a>>) -> Tabs<'a> {
        Tabs {
            tabs: tabs.into_iter().collect(),
            active: 0,
            theme: Theme::default(),
            hints: Hints::default(),
            divider: Cow::Borrowed(" │ "),
            indent: 1,
            tail: Vec::new(),
        }
    }

    /// Index of the view you are on (clamped to the tabs given).
    pub fn active(mut self, index: usize) -> Tabs<'a> {
        self.active = index;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Tabs<'a> {
        self.theme = theme;
        self
    }

    pub fn hints(mut self, hints: Hints) -> Tabs<'a> {
        self.hints = hints;
        self
    }

    /// Between two labels. Default `" │ "`.
    pub fn divider(mut self, divider: impl Into<Cow<'a, str>>) -> Tabs<'a> {
        self.divider = divider.into();
        self
    }

    /// Blank cells before the first label. Default 1.
    pub fn indent(mut self, cells: u16) -> Tabs<'a> {
        self.indent = cells;
        self
    }

    /// Spans right-aligned at the end of the row, drawn only when they fit
    /// the room the labels leave (with a one-cell gap).
    pub fn tail(mut self, spans: impl IntoIterator<Item = Span<'a>>) -> Tabs<'a> {
        self.tail = spans.into_iter().collect();
        self
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    fn active_index(&self) -> usize {
        self.active.min(self.tabs.len().saturating_sub(1))
    }

    /// The tab a key selects: ←/→ (and Tab/Shift-Tab) step with wrap-around,
    /// Home/End jump to the ends, and a tab's own key opens it.
    pub fn on_key(&self, key: KeyInput) -> Option<usize> {
        let n = self.tabs.len();
        if n == 0 {
            return None;
        }
        let at = self.active_index();
        if let Some(i) = self.tabs.iter().position(|t| {
            t.key
                .as_deref()
                .is_some_and(|k| key_matches(k, key) && !key.mods.ctrl && !key.mods.alt)
        }) {
            return Some(i);
        }
        match key.key {
            Key::Left | Key::BackTab => Some((at + n - 1) % n),
            Key::Right | Key::Tab => Some((at + 1) % n),
            Key::Home => Some(0),
            Key::End => Some(n - 1),
            _ => None,
        }
    }

    fn piece(&self, i: usize, full: bool) -> Piece<'_> {
        let tab = &self.tabs[i];
        let label = tab.label.as_ref();
        let key = match (self.hints, tab.key.as_deref()) {
            (Hints::Off, _) | (_, None) => {
                return Piece {
                    prefix: None,
                    label,
                    mnemonic: None,
                };
            }
            (_, Some(k)) => k,
        };
        if self.hints == Hints::Mnemonic {
            let mut cs = key.chars();
            if let (Some(c), None) = (cs.next(), cs.next())
                && let Some(at) = label.find(c)
            {
                return Piece {
                    prefix: None,
                    label,
                    mnemonic: Some((at, at + c.len_utf8())),
                };
            }
        }
        Piece {
            prefix: full.then(|| format!("[{key}] ")),
            label,
            mnemonic: None,
        }
    }

    /// Where the labels, hints and tail go at `area`, without drawing.
    pub fn layout(&self, area: Rect) -> TabsLayout {
        self.place(area, None)
    }

    /// Draw and return where everything went (the hit rects included).
    pub fn render_layout(&self, area: Rect, buf: &mut Buffer) -> TabsLayout {
        self.place(area, Some(buf))
    }

    /// Draw and return one rect per drawn label.
    pub fn render_hits(&self, area: Rect, buf: &mut Buffer) -> Vec<TabHit> {
        self.place(area, Some(buf)).hits
    }

    /// The whole strip, unclipped, as a line (for hosts that place it in
    /// their own paragraph; no hit rects).
    pub fn line(&self) -> Line<'static> {
        let mut out = Vec::new();
        let full = self.layout(Rect::new(0, 0, u16::MAX, 1)).hinted;
        out.push(Span::raw(" ".repeat(self.indent as usize)));
        for i in 0..self.tabs.len() {
            if i > 0 {
                out.push(Span::styled(self.divider.to_string(), self.theme.dim()));
            }
            out.extend(self.piece_spans(i, full, usize::MAX));
        }
        Line::from(out)
    }

    fn label_style(&self, i: usize) -> Style {
        let t = &self.theme;
        if i == self.active_index() {
            t.selected().patch(t.accent()).add_modifier(Modifier::BOLD)
        } else {
            t.accent()
        }
    }

    /// The spans of one label, clipped to `max` cells.
    fn piece_spans(&self, i: usize, full: bool, max: usize) -> Vec<Span<'static>> {
        let p = self.piece(i, full);
        let style = self.label_style(i);
        let mut spans = Vec::with_capacity(4);
        if let Some(prefix) = p.prefix {
            spans.push(Span::styled(prefix, style));
        }
        match p.mnemonic {
            Some((a, b)) => {
                if a > 0 {
                    spans.push(Span::styled(p.label[..a].to_string(), style));
                }
                spans.push(Span::styled(
                    p.label[a..b].to_string(),
                    style.add_modifier(Modifier::UNDERLINED),
                ));
                if b < p.label.len() {
                    spans.push(Span::styled(p.label[b..].to_string(), style));
                }
            }
            None => spans.push(Span::styled(p.label.to_string(), style)),
        }
        text::truncate_line(Line::from(spans), max).spans
    }

    /// Which form fits at `avail` cells after `indent`.
    fn fit(&self, avail: usize, indent: usize) -> Fit {
        let n = self.tabs.len();
        let div = text::width(&self.divider);
        let strip = |full: bool| {
            indent + (0..n).map(|i| self.piece(i, full).width()).sum::<usize>() + div * (n - 1)
        };
        let has_prefix = (0..n).any(|i| self.piece(i, true).prefix.is_some());
        let whole = |hinted| Fit {
            hinted,
            start: 0,
            end: n,
            markers: false,
        };
        if has_prefix && strip(true) <= avail {
            return whole(true);
        }
        if strip(false) <= avail {
            return whole(false);
        }
        // Scroll: a window of plain labels around the active tab, with the
        // hidden tabs counted at each end.
        let w: Vec<usize> = (0..n).map(|i| self.piece(i, false).width()).collect();
        let cost = |s: usize, e: usize| {
            let before = if s > 0 {
                text::width(&marker_before(s))
            } else {
                0
            };
            let after = if e < n {
                text::width(&marker_after(n - e))
            } else {
                0
            };
            indent + before + w[s..e].iter().sum::<usize>() + div * (e - s - 1) + after
        };
        let at = self.active_index();
        match (0..=at).find(|&s| cost(s, at + 1) <= avail) {
            Some(start) => Fit {
                hinted: false,
                start,
                end: (at + 1..=n)
                    .filter(|&e| cost(start, e) <= avail)
                    .max()
                    .unwrap_or(at + 1),
                markers: true,
            },
            // Not even the active label with its counts fits: the label
            // alone, clipped with `…` (the counts stay in the layout).
            None => Fit {
                hinted: false,
                start: at,
                end: at + 1,
                markers: false,
            },
        }
    }

    fn place(&self, area: Rect, mut buf: Option<&mut Buffer>) -> TabsLayout {
        let mut out = TabsLayout {
            room: area.width as usize,
            ..TabsLayout::default()
        };
        if area.is_empty() {
            return out;
        }
        let n = self.tabs.len();
        let y = area.y;
        let right = area.right();
        // The indent gives way before the labels do.
        let indent = if area.width > self.indent {
            self.indent
        } else {
            0
        };
        let mut x = area.x + indent;
        let t = &self.theme;
        let put = |x: &mut u16, spans: &[Span<'static>], buf: &mut Option<&mut Buffer>| {
            for s in spans {
                let w = (text::width(&s.content) as u16).min(right - *x);
                if let Some(b) = buf.as_deref_mut() {
                    b.set_span(*x, y, s, w);
                }
                *x += w;
            }
        };
        if n > 0 {
            let Fit {
                hinted: full,
                start,
                end,
                markers,
            } = self.fit(area.width as usize, indent as usize);
            out.hinted = full;
            out.hidden_before = start;
            out.hidden_after = n - end;
            if markers && start > 0 {
                put(
                    &mut x,
                    &[Span::styled(marker_before(start), t.dim())],
                    &mut buf,
                );
            }
            let after = (markers && end < n).then(|| marker_after(n - end));
            let reserve = after.as_deref().map_or(0, text::width);
            for i in start..end {
                if i > start {
                    put(
                        &mut x,
                        &[Span::styled(self.divider.to_string(), t.dim())],
                        &mut buf,
                    );
                }
                let max = (right - x) as usize;
                let max = if i + 1 == end {
                    max.saturating_sub(reserve).max(1).min(max)
                } else {
                    max
                };
                let spans = self.piece_spans(i, full, max);
                let x0 = x;
                put(&mut x, &spans, &mut buf);
                if x > x0 {
                    out.hits.push(TabHit {
                        rect: Rect::new(x0, y, x - x0, 1),
                        index: i,
                    });
                }
            }
            if let Some(m) = after {
                put(&mut x, &[Span::styled(m, t.dim())], &mut buf);
            }
        }
        out.room = (right - x) as usize;
        let tail_w = text::spans_width(&self.tail);
        if tail_w > 0 && tail_w < out.room {
            out.tail = true;
            if let Some(b) = buf {
                b.set_line(
                    right - tail_w as u16,
                    y,
                    &Line::from(self.tail.clone()),
                    tail_w as u16,
                );
            }
        }
        out
    }
}

/// The form [`Tabs::fit`] picked: hints or not, the window of tabs shown,
/// and whether the hidden counts are drawn.
struct Fit {
    hinted: bool,
    start: usize,
    end: usize,
    markers: bool,
}

fn marker_before(hidden: usize) -> String {
    format!("‹{hidden} ")
}

fn marker_after(hidden: usize) -> String {
    format!(" {hidden}›")
}

fn key_matches(spec: &str, key: KeyInput) -> bool {
    let mut cs = spec.chars();
    if let (Some(c), None) = (cs.next(), cs.next()) {
        return key.key == Key::Char(c);
    }
    match spec {
        "Esc" => key.key == Key::Esc,
        "Enter" => key.key == Key::Enter,
        "Tab" => key.key == Key::Tab,
        _ => false,
    }
}

impl Widget for Tabs<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_hits(area, buf);
    }
}

impl Widget for &Tabs<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_hits(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    /// llmux's `TABS`, with the keys `draw_tabs` hard-codes.
    fn llmux_tabs(keys: bool, only_dashboard: bool) -> Vec<Tab<'static>> {
        [
            ("dashboard", "Esc"),
            ("accounts", "a"),
            ("stats", "g"),
            ("usage", "U"),
            ("keys", "K"),
            ("perf", "p"),
            ("logs", "l"),
            ("sessions", "s"),
            ("misc", "?"),
            ("config", "c"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (label, key))| {
            let tab = Tab::new(label);
            if keys && (!only_dashboard || i == 0) {
                tab.key(key)
            } else {
                tab
            }
        })
        .collect()
    }

    fn render(tabs: &Tabs, width: u16) -> (Buffer, TabsLayout) {
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
        let layout = tabs.render_layout(Rect::new(0, 1, width, 1), &mut buf);
        (buf, layout)
    }

    /// Port of llmux `assert_c2a_tab_strip` (`ui.rs:15806`): the row text,
    /// one hit per tab at the expected cells, the active style only on the
    /// active label, nothing on the rows around it.
    fn assert_strip(tabs: &Tabs, width: u16, row: &str, expected: &[(&str, u16)]) {
        let (buf, layout) = render(tabs, width);
        assert_eq!(snapshot::lines(&buf)[1], row);
        assert_eq!(layout.hits.len(), expected.len(), "one hit per tab");
        for (hit, (label, x)) in layout.hits.iter().zip(expected) {
            let w = text::width(label) as u16;
            assert_eq!(hit.rect, Rect::new(*x, 1, w, 1), "{label}");
            for col in hit.rect.x..hit.rect.right() {
                assert_eq!(hit_test(&layout.hits, col, 1), Some(hit.index), "{label}");
                let cell = &buf[(col, 1)];
                assert_eq!(cell.fg, Color::Cyan, "{label}");
                assert_eq!(
                    cell.modifier.contains(Modifier::REVERSED | Modifier::BOLD),
                    hit.index == tabs.active_index(),
                    "{label} active style"
                );
            }
        }
        for col in 0..width {
            if !layout.hits.iter().any(|h| h.contains(col, 1)) {
                assert_eq!(hit_test(&layout.hits, col, 1), None, "gap at {col}");
            }
            for row in [0, 2] {
                assert_eq!(buf[(col, row)].symbol(), " ", "one-row strip");
                assert_eq!(hit_test(&layout.hits, col, row), None);
            }
        }
    }

    const PLAIN: &[(&str, u16)] = &[
        ("dashboard", 1),
        ("accounts", 13),
        ("stats", 24),
        ("usage", 32),
        ("keys", 40),
        ("perf", 47),
        ("logs", 54),
        ("sessions", 61),
        ("misc", 72),
        ("config", 79),
    ];

    #[test]
    fn plain_labels_keep_llmux_hit_targets() {
        for active in 0..10 {
            let tabs = Tabs::new(llmux_tabs(false, false))
                .active(active)
                .theme(Theme::ansi());
            assert_strip(
                &tabs,
                139,
                " dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
                PLAIN,
            );
        }
    }

    #[test]
    fn bracket_hints_show_when_they_fit_with_full_hit_targets() {
        let tabs = Tabs::new(llmux_tabs(true, false))
            .hints(Hints::Bracket)
            .theme(Theme::ansi());
        let row = " [Esc] dashboard │ [a] accounts │ [g] stats │ [U] usage │ [K] keys │ [p] perf │ [l] logs │ [s] sessions │ [?] misc │ [c] config";
        let hits = &[
            ("[Esc] dashboard", 1),
            ("[a] accounts", 19),
            ("[g] stats", 34),
            ("[U] usage", 46),
            ("[K] keys", 58),
            ("[p] perf", 69),
            ("[l] logs", 80),
            ("[s] sessions", 91),
            ("[?] misc", 106),
            ("[c] config", 117),
        ];
        assert_strip(&tabs, 140, row, hits);
        // Measured, not a 140-column threshold: the hinted strip is 127
        // cells, so it holds down to 127 and gives way at 126.
        assert_strip(&tabs, 127, row, hits);
        assert_strip(
            &tabs,
            126,
            " dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
            PLAIN,
        );
    }

    #[test]
    fn overlay_shows_only_the_dashboard_return_hint() {
        for active in 1..10 {
            let tabs = Tabs::new(llmux_tabs(true, true))
                .active(active)
                .hints(Hints::Bracket)
                .theme(Theme::ansi());
            assert_strip(
                &tabs,
                140,
                " [Esc] dashboard │ accounts │ stats │ usage │ keys │ perf │ logs │ sessions │ misc │ config",
                &[
                    ("[Esc] dashboard", 1),
                    ("accounts", 19),
                    ("stats", 30),
                    ("usage", 38),
                    ("keys", 46),
                    ("perf", 53),
                    ("logs", 60),
                    ("sessions", 67),
                    ("misc", 78),
                    ("config", 85),
                ],
            );
        }
    }

    /// llmux measured `chars().count()`: `계정` got a 2-cell rect for a
    /// 4-cell label. Rects are cells here.
    #[test]
    fn korean_labels_get_rects_in_cells() {
        let tabs = Tabs::new(["대시보드", "계정", "통계", "로그"].map(Tab::new))
            .active(1)
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 40);
        assert_eq!(snapshot::lines(&buf)[1], " 대시보드 │ 계정 │ 통계 │ 로그");
        let rects: Vec<(u16, u16)> = layout
            .hits
            .iter()
            .map(|h| (h.rect.x, h.rect.width))
            .collect();
        assert_eq!(rects, vec![(1, 8), (12, 4), (19, 4), (26, 4)]);
        // The last cell of a wide label still maps to it.
        assert_eq!(hit_test(&layout.hits, 15, 1), Some(1));
        assert_eq!(hit_test(&layout.hits, 16, 1), None);
    }

    #[test]
    fn mnemonic_underlines_the_key_letter_at_no_cost() {
        let tabs = Tabs::new([
            Tab::new("dashboard").key("Esc"),
            Tab::new("accounts").key("a"),
            Tab::new("logs").key("l"),
        ])
        .theme(Theme::mono());
        let (buf, layout) = render(&tabs, 60);
        assert_eq!(
            snapshot::lines(&buf)[1],
            " [Esc] dashboard │ accounts │ logs"
        );
        assert!(layout.hinted);
        let a = layout.hits[1].rect.x;
        assert!(buf[(a, 1)].modifier.contains(Modifier::UNDERLINED));
        assert!(!buf[(a + 1, 1)].modifier.contains(Modifier::UNDERLINED));
        // Narrower: the bracket goes, the underline stays.
        let (buf, layout) = render(&tabs, 30);
        assert_eq!(snapshot::lines(&buf)[1], " dashboard │ accounts │ logs");
        assert!(!layout.hinted);
        assert!(
            buf[(layout.hits[2].rect.x, 1)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
    }

    #[test]
    fn tail_takes_the_room_and_gives_way_first() {
        let tabs = Tabs::new(["dashboard", "accounts", "stats"].map(Tab::new))
            .tail([Span::raw(" cfg ~/.config/llmux.json ")])
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 70);
        let row = &snapshot::lines(&buf)[1];
        assert!(row.ends_with(" cfg ~/.config/llmux.json"), "{row}");
        assert!(layout.tail);
        assert_eq!(layout.room, 70 - 29);
        // 29 cells of tabs + 1 gap + 26 of tail = 56: at 55 the tail goes,
        // the tabs stay whole.
        let (buf, layout) = render(&tabs, 56);
        assert!(layout.tail, "{:?}", snapshot::lines(&buf)[1]);
        let (buf, layout) = render(&tabs, 55);
        assert!(!layout.tail);
        assert_eq!(snapshot::lines(&buf)[1], " dashboard │ accounts │ stats");
        assert_eq!(layout.hits.len(), 3);
    }

    #[test]
    fn too_narrow_scrolls_to_the_active_tab_and_counts_the_rest() {
        let tabs = Tabs::new(llmux_tabs(false, false))
            .active(5)
            .theme(Theme::ansi());
        let (buf, layout) = render(&tabs, 30);
        let row = &snapshot::lines(&buf)[1];
        assert!(layout.hits.iter().any(|h| h.index == 5), "{row}");
        assert!(layout.hidden_before > 0 && layout.hidden_after > 0, "{row}");
        assert!(row.contains(&format!("‹{}", layout.hidden_before)), "{row}");
        assert!(row.ends_with(&format!("{}›", layout.hidden_after)), "{row}");
        assert_eq!(
            layout.hidden_before + layout.hits.len() + layout.hidden_after,
            10
        );
        for h in &layout.hits {
            assert!(h.rect.right() <= 30);
        }
        // Every width down to one cell: no panic, the active tab drawn.
        for w in 1..30 {
            let (_, layout) = render(&tabs, w);
            assert!(layout.hits.iter().all(|h| h.rect.right() <= w), "{w}");
            assert!(layout.hits.iter().any(|h| h.index == 5), "{w}");
        }
    }

    #[test]
    fn keys_step_wrap_and_open_by_shortcut() {
        let tabs = Tabs::new(llmux_tabs(true, false)).active(0);
        assert_eq!(tabs.on_key(Key::Right.into()), Some(1));
        assert_eq!(tabs.on_key(Key::Left.into()), Some(9));
        assert_eq!(tabs.on_key(Key::End.into()), Some(9));
        assert_eq!(tabs.on_key('l'.into()), Some(6));
        assert_eq!(tabs.on_key('?'.into()), Some(8));
        assert_eq!(tabs.on_key(Key::Esc.into()), Some(0));
        assert_eq!(tabs.on_key(KeyInput::ctrl(Key::Char('l'))), None);
        assert_eq!(tabs.on_key('x'.into()), None);
    }

    #[test]
    fn mono_marks_the_active_tab_by_weight() {
        let tabs = Tabs::new(["dashboard", "accounts"].map(Tab::new))
            .active(1)
            .theme(Theme::mono());
        let (buf, layout) = render(&tabs, 40);
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        let x = layout.hits[1].rect.x;
        assert!(buf[(x, 1)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(1, 1)].modifier.contains(Modifier::REVERSED));
    }
}
