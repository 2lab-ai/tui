// Derived from OpenAI Codex (Apache-2.0), commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72:
//   codex-rs/tui/src/key_hint.rs        key labels, key matching across terminals
//   codex-rs/tui/src/footer_hint.rs     whole-hint packing (wrap_hint_rows)
//   codex-rs/tui/src/shortcut_help.rs   the responsive shortcut grid
// Modified by 2lab: keys are `crate::input::KeyInput` (no crossterm); labels
// are plain text (`ctrl+c`, `esc`, `↑↓`) with an ASCII fallback for the
// arrows; the footer drops the lowest-priority hints first and says so; the
// grid takes any number of groups; colours come from the theme.
//
// The help grid's filter is derived from herdrdev/herdr
// `src/input/keybind_help.rs` (`filter_keybind_help_groups`) and its scroll
// from `src/client/shell/overlays.rs` (`render_help_overlay`) at
// ee5bfb5041bb3e4a49d591c72308aae3c81ecdde (Apache-2.0). Modified by 2lab: the
// scroll range is the grid's own row count, not an estimate from character
// counts; the key column is measured in cells; an empty result names the
// query; a position line says where the view is.

//! What you can press now: a footer line that fits its width, and a help
//! grid of every shortcut by group.
//!
//! Ported from OpenAI Codex `key_hint.rs`, `footer_hint.rs` and
//! `shortcut_help.rs` (Apache-2.0), and from llmux's dashboard footer
//! (`src/tui/ui.rs`, `draw_footer`), which builds one long keybar and lets the
//! terminal clip it mid-word at 80 columns. Here a [`KeyHints`] footer never
//! shows half a hint: it drops whole hints, lowest [`KeyHint::priority`]
//! first (the rightmost of equals first), and ends with `… ? more` so the
//! person knows there is more and where to find it.
//!
//! Key glyphs are plain text — `ctrl+c`, `esc`, `enter`, `↑↓`. The arrows
//! `← ↑ → ↓` (U+2190–2193) are East-Asian *Ambiguous*: one cell by
//! `unicode-width`, but two on many CJK terminals, which would push a fitted
//! footer past its edge there. [`KeyGlyphs::Ascii`] spells them out (`up`,
//! `down`, `up/down`) for those hosts.
//!
//! [`KeyHelp::filter`] narrows the grid to the shortcuts whose keys or
//! description contain a query, ignoring case (herdr's keybind help); groups
//! left empty go, and when nothing matches the grid says `no keys match
//! "<query>"`. Drawn with a [`KeyHelpState`], a grid taller than its area
//! scrolls and its last row says where the view is: `lines 8–14 of 31`.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use crate::input::{Key, KeyInput, Mods};
use crate::text::{self, ELLIPSIS};
use crate::theme::Theme;

/// How arrow keys are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum KeyGlyphs {
    /// `← ↑ → ↓` — compact, but East-Asian Ambiguous width.
    #[default]
    Unicode,
    /// `left up right down` — safe on CJK terminals.
    Ascii,
}

/// The arrow glyphs and their ASCII words.
const ARROWS: [(char, &str); 4] = [('←', "left"), ('↑', "up"), ('→', "right"), ('↓', "down")];

/// `keys` with arrows spelled out: `↑↓` → `up/down`, `↑/k ↓/j` → `up/k
/// down/j`. Borrowed when there is no arrow.
pub fn ascii_keys(keys: &str) -> Cow<'_, str> {
    if !keys.chars().any(|c| ARROWS.iter().any(|(a, _)| *a == c)) {
        return Cow::Borrowed(keys);
    }
    let mut out = String::with_capacity(keys.len() + 8);
    let mut after_arrow = false;
    for c in keys.chars() {
        match ARROWS.iter().find(|(a, _)| *a == c) {
            Some((_, word)) => {
                if after_arrow {
                    out.push('/');
                }
                out.push_str(word);
                after_arrow = true;
            }
            None => {
                out.push(c);
                after_arrow = false;
            }
        }
    }
    Cow::Owned(out)
}

fn keys_text(keys: &str, glyphs: KeyGlyphs) -> Cow<'_, str> {
    match glyphs {
        KeyGlyphs::Unicode => Cow::Borrowed(keys),
        KeyGlyphs::Ascii => ascii_keys(keys),
    }
}

/// A key as plain text: `ctrl+c`, `alt+enter`, `shift+tab`, `esc`, `↑`
/// (`up` with [`KeyGlyphs::Ascii`]), `pgdn`, `space`.
pub fn key_label(key: KeyInput, glyphs: KeyGlyphs) -> String {
    let mut s = String::new();
    let Mods { ctrl, alt, shift } = key.mods;
    if ctrl {
        s.push_str("ctrl+");
    }
    if alt {
        s.push_str("alt+");
    }
    if shift && key.key != Key::BackTab {
        s.push_str("shift+");
    }
    let arrow = |c: char| keys_text(&c.to_string(), glyphs).into_owned();
    match key.key {
        Key::Char(' ') => s.push_str("space"),
        Key::Char(c) => s.push(c),
        Key::Enter => s.push_str("enter"),
        Key::Esc => s.push_str("esc"),
        Key::Backspace => s.push_str("backspace"),
        Key::Delete => s.push_str("del"),
        Key::Tab => s.push_str("tab"),
        Key::BackTab => s.push_str("shift+tab"),
        Key::Left => s.push_str(&arrow('←')),
        Key::Right => s.push_str(&arrow('→')),
        Key::Up => s.push_str(&arrow('↑')),
        Key::Down => s.push_str(&arrow('↓')),
        Key::Home => s.push_str("home"),
        Key::End => s.push_str("end"),
        Key::PageUp => s.push_str("pgup"),
        Key::PageDown => s.push_str("pgdn"),
    }
    s
}

/// One key as terminals disagree on it, brought to one form: Shift+Tab is
/// BackTab, a raw C0 control character is its Ctrl chord (`\u{10}` → Ctrl+P),
/// an uppercase letter is Shift + the letter, and legacy Ctrl+5 is Ctrl+].
pub fn normalize(key: KeyInput) -> KeyInput {
    let mut mods = key.mods;
    match key.key {
        Key::BackTab => {
            mods.shift = false;
            KeyInput {
                key: Key::BackTab,
                mods,
            }
        }
        Key::Tab if mods.shift => {
            mods.shift = false;
            KeyInput {
                key: Key::BackTab,
                mods,
            }
        }
        Key::Char(c) => {
            if mods == Mods::default()
                && let Some(ch) = c0_to_ctrl_char(c)
            {
                return normalize(KeyInput {
                    key: Key::Char(ch),
                    mods: Mods {
                        ctrl: true,
                        ..Mods::default()
                    },
                });
            }
            if c.is_ascii_uppercase() {
                mods.shift = true;
                return KeyInput {
                    key: Key::Char(c.to_ascii_lowercase()),
                    mods,
                };
            }
            if c == '5' && mods.ctrl && !mods.alt && !mods.shift {
                return KeyInput {
                    key: Key::Char(']'),
                    mods,
                };
            }
            key
        }
        _ => key,
    }
}

fn c0_to_ctrl_char(c: char) -> Option<char> {
    let code = u32::from(c);
    match code {
        0x00 => Some(' '),
        0x01..=0x1a => char::from_u32(code - 0x01 + u32::from('a')),
        0x1c..=0x1f => char::from_u32(code - 0x1c + u32::from('4')),
        _ => None,
    }
}

/// Whether `event` is the key `binding` names, across the terminal quirks
/// [`normalize`] smooths over.
pub fn is_press(binding: KeyInput, event: KeyInput) -> bool {
    normalize(binding) == normalize(event)
}

/// A key and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyHint<'a> {
    /// As shown: `ctrl+c`, `esc`, `↑↓`, `g/Esc`.
    pub keys: Cow<'a, str>,
    pub label: Cow<'a, str>,
    /// Higher stays longer when the footer runs out of room. Default 0.
    pub priority: u8,
    /// Shown but not available right now (`R reload` while attached to a
    /// remote daemon): key and label both drawn dim.
    pub disabled: bool,
}

impl<'a> KeyHint<'a> {
    pub fn new(keys: impl Into<Cow<'a, str>>, label: impl Into<Cow<'a, str>>) -> KeyHint<'a> {
        KeyHint {
            keys: keys.into(),
            label: label.into(),
            priority: 0,
            disabled: false,
        }
    }

    /// A hint for a bound key, labelled with [`key_label`].
    pub fn for_key(
        key: KeyInput,
        label: impl Into<Cow<'a, str>>,
        glyphs: KeyGlyphs,
    ) -> KeyHint<'a> {
        KeyHint::new(key_label(key, glyphs), label)
    }

    pub fn priority(mut self, priority: u8) -> KeyHint<'a> {
        self.priority = priority;
        self
    }

    /// Draw the hint dim: the key exists but does nothing here now.
    pub fn disabled(mut self, disabled: bool) -> KeyHint<'a> {
        self.disabled = disabled;
        self
    }

    /// Cells as drawn: keys, a space, the label.
    pub fn width(&self, glyphs: KeyGlyphs) -> usize {
        let label = if self.label.is_empty() {
            0
        } else {
            1 + text::width(&self.label)
        };
        text::width(&keys_text(&self.keys, glyphs)) + label
    }

    fn spans(&self, theme: &Theme, glyphs: KeyGlyphs) -> Vec<Span<'static>> {
        let (key, label) = if self.disabled {
            (theme.dim(), theme.dim())
        } else {
            (key_style(theme), theme.secondary())
        };
        let mut v = vec![Span::styled(
            keys_text(&self.keys, glyphs).into_owned(),
            key,
        )];
        if !self.label.is_empty() {
            v.push(Span::styled(format!(" {}", self.label), label));
        }
        v
    }
}

/// Keys stand out from their labels: accent and bold (bold alone in mono).
fn key_style(theme: &Theme) -> Style {
    theme.accent().add_modifier(Modifier::BOLD)
}

/// Pack whole hints into rows of at most `width` cells, `separator_width`
/// between hints. A hint wider than a row gets a row of its own; no hint is
/// ever split. Always returns at least one (possibly empty) row.
pub fn pack_rows<T>(
    hints: impl IntoIterator<Item = T>,
    width: u16,
    separator_width: usize,
    hint_width: impl Fn(&T) -> usize,
) -> Vec<Vec<T>> {
    let width = usize::from(width.max(1));
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut used = 0usize;
    for hint in hints {
        let w = hint_width(&hint).min(width);
        let extra = if row.is_empty() {
            w
        } else {
            separator_width.saturating_add(w)
        };
        if !row.is_empty() && used.saturating_add(extra) > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        used = if row.is_empty() {
            w
        } else {
            used.saturating_add(separator_width).saturating_add(w)
        };
        row.push(hint);
    }
    rows.push(row);
    rows
}

/// Which hints a footer shows at a width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    /// Indices into the hints, in their original order.
    pub shown: Vec<usize>,
    /// How many were dropped (0: everything fits, no `more` marker).
    pub dropped: usize,
}

/// A one-row footer of key hints that fits its width.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyHints<'a> {
    hints: Vec<KeyHint<'a>>,
    title: Option<Cow<'a, str>>,
    more: Option<Cow<'a, str>>,
    separator: Cow<'a, str>,
    glyphs: KeyGlyphs,
    theme: Theme,
}

impl<'a> KeyHints<'a> {
    pub fn new(hints: impl IntoIterator<Item = KeyHint<'a>>) -> KeyHints<'a> {
        KeyHints {
            hints: hints.into_iter().collect(),
            title: None,
            more: Some(Cow::Borrowed("?")),
            separator: Cow::Borrowed("  "),
            glyphs: KeyGlyphs::Unicode,
            theme: Theme::default(),
        }
    }

    /// Where the person is (`accounts`), before the hints.
    pub fn title(mut self, title: impl Into<Cow<'a, str>>) -> KeyHints<'a> {
        self.title = Some(title.into());
        self
    }

    /// The key that opens the full list, named in the overflow marker:
    /// `… ? more`. Default `?`.
    pub fn more(mut self, key: impl Into<Cow<'a, str>>) -> KeyHints<'a> {
        self.more = Some(key.into());
        self
    }

    /// No help key: the overflow marker counts instead (`… +3 more`).
    pub fn no_more(mut self) -> KeyHints<'a> {
        self.more = None;
        self
    }

    /// Between hints. Default two spaces.
    pub fn separator(mut self, separator: impl Into<Cow<'a, str>>) -> KeyHints<'a> {
        self.separator = separator.into();
        self
    }

    pub fn glyphs(mut self, glyphs: KeyGlyphs) -> KeyHints<'a> {
        self.glyphs = glyphs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> KeyHints<'a> {
        self.theme = theme;
        self
    }

    pub fn hints(&self) -> &[KeyHint<'a>] {
        &self.hints
    }

    fn title_width(&self) -> usize {
        self.title.as_ref().map_or(0, |t| text::width(t) + 2)
    }

    fn marker_text(&self, dropped: usize) -> (String, Option<String>) {
        // `…` is East-Asian Ambiguous too; the ASCII set writes three dots.
        let dots = match self.glyphs {
            KeyGlyphs::Unicode => ELLIPSIS.to_string(),
            KeyGlyphs::Ascii => "...".to_string(),
        };
        match &self.more {
            Some(k) => (
                format!("{dots} "),
                Some(keys_text(k, self.glyphs).into_owned()),
            ),
            None => (format!("{dots} +{dropped} more"), None),
        }
    }

    fn marker_width(&self, dropped: usize) -> usize {
        match self.marker_text(dropped) {
            (lead, Some(k)) => text::width(&lead) + text::width(&k) + " more".len(),
            (lead, None) => text::width(&lead),
        }
    }

    /// Which hints fit in `width` cells: all of them, or the most important
    /// ones plus room for the overflow marker.
    pub fn fit(&self, width: usize) -> Fit {
        let sep = text::width(&self.separator);
        let widths: Vec<usize> = self.hints.iter().map(|h| h.width(self.glyphs)).collect();
        let total = |shown: &[usize]| -> usize {
            let body: usize = shown.iter().map(|&i| widths[i]).sum();
            body + sep * shown.len().saturating_sub(1)
        };
        let mut shown: Vec<usize> = (0..self.hints.len()).collect();
        if self.title_width() + total(&shown) <= width {
            return Fit { shown, dropped: 0 };
        }
        // Drop order: lowest priority first, the rightmost of equals first.
        let mut order: Vec<usize> = shown.clone();
        order.sort_by_key(|&i| (self.hints[i].priority, std::cmp::Reverse(i)));
        let mut dropped = 0;
        for i in order {
            shown.retain(|&s| s != i);
            dropped += 1;
            let marker = self.marker_width(dropped) + if shown.is_empty() { 0 } else { sep };
            if self.title_width() + total(&shown) + marker <= width {
                break;
            }
        }
        Fit { shown, dropped }
    }

    /// The footer fitted to `width` cells.
    pub fn line(&self, width: u16) -> Line<'static> {
        let t = &self.theme;
        let fit = self.fit(width as usize);
        let mut spans: Vec<Span<'static>> = Vec::new();
        if let Some(title) = &self.title {
            spans.push(Span::styled(
                title.to_string(),
                t.text().add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw("  "));
        }
        for (n, &i) in fit.shown.iter().enumerate() {
            if n > 0 {
                spans.push(Span::raw(self.separator.to_string()));
            }
            spans.extend(self.hints[i].spans(t, self.glyphs));
        }
        if fit.dropped > 0 {
            if !fit.shown.is_empty() {
                spans.push(Span::raw(self.separator.to_string()));
            }
            match self.marker_text(fit.dropped) {
                (lead, Some(k)) => {
                    spans.push(Span::styled(lead, t.dim()));
                    spans.push(Span::styled(k, key_style(t)));
                    spans.push(Span::styled(" more", t.secondary()));
                }
                (lead, None) => spans.push(Span::styled(lead, t.dim())),
            }
        }
        // Only a lone hint wider than the whole row is left to clip, and the
        // clip ends in `…`.
        text::truncate_line(Line::from(spans), width as usize)
    }

    /// Every hint, packed whole into as many rows as `width` needs.
    pub fn rows(&self, width: u16) -> Vec<Line<'static>> {
        let sep = text::width(&self.separator);
        let packed = pack_rows(self.hints.iter(), width, sep, |h| h.width(self.glyphs));
        packed
            .into_iter()
            .map(|row| {
                let mut spans = Vec::new();
                for (n, h) in row.into_iter().enumerate() {
                    if n > 0 {
                        spans.push(Span::raw(self.separator.to_string()));
                    }
                    spans.extend(h.spans(&self.theme, self.glyphs));
                }
                text::truncate_line(Line::from(spans), width as usize)
            })
            .collect()
    }
}

impl Widget for KeyHints<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

impl Widget for &KeyHints<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        buf.set_line(area.x, area.y, &self.line(area.width), area.width);
    }
}

/// A titled set of shortcuts in the help grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintGroup<'a> {
    pub title: Cow<'a, str>,
    pub hints: Vec<KeyHint<'a>>,
}

impl<'a> HintGroup<'a> {
    pub fn new(
        title: impl Into<Cow<'a, str>>,
        hints: impl IntoIterator<Item = KeyHint<'a>>,
    ) -> HintGroup<'a> {
        HintGroup {
            title: title.into(),
            hints: hints.into_iter().collect(),
        }
    }
}

/// Every shortcut, grouped, in as many columns (up to three) as fit.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyHelp<'a> {
    groups: Vec<HintGroup<'a>>,
    query: Cow<'a, str>,
    columns: usize,
    gap: usize,
    glyphs: KeyGlyphs,
    theme: Theme,
}

/// One rendered line of the grid and how many shortcuts it lists.
type Row = (Line<'static>, usize);

impl<'a> KeyHelp<'a> {
    pub fn new(groups: impl IntoIterator<Item = HintGroup<'a>>) -> KeyHelp<'a> {
        KeyHelp {
            groups: groups.into_iter().collect(),
            query: Cow::Borrowed(""),
            columns: 3,
            gap: 4,
            glyphs: KeyGlyphs::Unicode,
            theme: Theme::default(),
        }
    }

    /// Only the shortcuts whose keys (as written or as drawn) or description
    /// contain `query`, ignoring case; group titles are not searched (herdr
    /// `filter_keybind_help_groups`). Empty: every shortcut.
    pub fn filter(mut self, query: impl Into<Cow<'a, str>>) -> KeyHelp<'a> {
        self.query = query.into();
        self
    }

    /// How many shortcuts the filter keeps.
    pub fn matches(&self) -> usize {
        self.shown_groups().iter().map(|g| g.hints.len()).sum()
    }

    /// The groups with only the shortcuts that match, empty groups gone.
    fn shown_groups(&self) -> Vec<HintGroup<'a>> {
        if self.query.is_empty() {
            return self.groups.clone();
        }
        let q = self.query.to_lowercase();
        let hit = |h: &KeyHint<'_>| {
            h.keys.to_lowercase().contains(&q)
                || keys_text(&h.keys, self.glyphs).to_lowercase().contains(&q)
                || h.label.to_lowercase().contains(&q)
        };
        self.groups
            .iter()
            .filter_map(|g| {
                let hints: Vec<KeyHint<'a>> = g.hints.iter().filter(|h| hit(h)).cloned().collect();
                (!hints.is_empty()).then(|| HintGroup {
                    title: g.title.clone(),
                    hints,
                })
            })
            .collect()
    }

    /// `no keys match "<query>"`, the query cut so the closing quote stays.
    fn no_match(&self, width: usize) -> Line<'static> {
        let frame = text::width("no keys match \"\"");
        let msg = if width > frame {
            format!(
                "no keys match \"{}\"",
                text::truncate(&self.query, width - frame)
            )
        } else {
            text::truncate(&format!("no keys match \"{}\"", self.query), width).into_owned()
        };
        Line::from(Span::styled(msg, self.theme.dim()))
    }

    /// Most columns. Default 3.
    pub fn columns(mut self, columns: usize) -> KeyHelp<'a> {
        self.columns = columns.max(1);
        self
    }

    /// Cells between columns. Default 4.
    pub fn gap(mut self, gap: usize) -> KeyHelp<'a> {
        self.gap = gap;
        self
    }

    pub fn glyphs(mut self, glyphs: KeyGlyphs) -> KeyHelp<'a> {
        self.glyphs = glyphs;
        self
    }

    pub fn theme(mut self, theme: Theme) -> KeyHelp<'a> {
        self.theme = theme;
        self
    }

    /// One group as lines: a bold title, then keys aligned in a column.
    fn block(&self, g: &HintGroup<'_>) -> Vec<Row> {
        let t = &self.theme;
        let key_w = g
            .hints
            .iter()
            .map(|h| text::width(&keys_text(&h.keys, self.glyphs)))
            .max()
            .unwrap_or(0);
        let mut v: Vec<Row> = vec![(
            Line::from(Span::styled(
                g.title.to_string(),
                t.text().add_modifier(Modifier::BOLD),
            )),
            0,
        )];
        for h in &g.hints {
            let keys = keys_text(&h.keys, self.glyphs);
            let pad = key_w - text::width(&keys) + 2;
            v.push((
                Line::from(vec![
                    Span::styled(keys.into_owned(), key_style(t)),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(h.label.to_string(), t.secondary()),
                ]),
                1,
            ));
        }
        v
    }

    /// The grid laid out for `width` cells.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        self.rows(width).into_iter().map(|(l, _)| l).collect()
    }

    pub fn height(&self, width: u16) -> u16 {
        self.rows(width).len().min(u16::MAX as usize) as u16
    }

    fn rows(&self, width: u16) -> Vec<Row> {
        let width = width.max(1) as usize;
        let groups = self.shown_groups();
        let blocks: Vec<Vec<Row>> = groups.iter().map(|g| self.block(g)).collect();
        if blocks.is_empty() {
            if !self.query.is_empty() && !self.groups.is_empty() {
                return vec![(self.no_match(width), 0)];
            }
            return Vec::new();
        }
        let widths: Vec<usize> = blocks
            .iter()
            .map(|b| {
                b.iter()
                    .map(|(l, _)| text::line_width(l))
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let heights: Vec<usize> = blocks.iter().map(Vec::len).collect();
        for k in (2..=self.columns.min(blocks.len())).rev() {
            let cols = partition(&heights, k);
            let col_w: Vec<usize> = cols
                .iter()
                .map(|r| widths[r.clone()].iter().copied().max().unwrap_or(0))
                .collect();
            if col_w.iter().sum::<usize>() + self.gap * (cols.len() - 1) <= width {
                let stacked: Vec<Vec<Row>> =
                    cols.iter().map(|r| stack(&blocks[r.clone()])).collect();
                return side_by_side(&stacked, &col_w, self.gap);
            }
        }
        stack(&blocks)
            .into_iter()
            .map(|(l, n)| (text::truncate_line(l, width), n))
            .collect()
    }
}

/// Blocks one under another with a blank line between.
fn stack(blocks: &[Vec<Row>]) -> Vec<Row> {
    let mut out = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        if i > 0 {
            out.push((Line::default(), 0));
        }
        out.extend(b.iter().cloned());
    }
    out
}

/// Columns joined row by row, each padded to its width plus the gap.
fn side_by_side(cols: &[Vec<Row>], widths: &[usize], gap: usize) -> Vec<Row> {
    let height = cols.iter().map(Vec::len).max().unwrap_or(0);
    (0..height)
        .map(|r| {
            let mut spans = Vec::new();
            let mut n = 0;
            for (c, col) in cols.iter().enumerate() {
                let (line, count) = col.get(r).cloned().unwrap_or_default();
                n += count;
                let pad = widths[c].saturating_sub(text::line_width(&line)) + gap;
                spans.extend(line.spans);
                if c + 1 < cols.len() {
                    spans.push(Span::raw(" ".repeat(pad)));
                }
            }
            let mut line = Line::from(spans);
            // Trailing padding of empty right-hand cells is noise.
            while line
                .spans
                .last()
                .is_some_and(|s| s.content.trim().is_empty())
            {
                line.spans.pop();
            }
            (line, n)
        })
        .collect()
}

/// Split `heights` into at most `k` contiguous runs, as even as possible
/// (each run's height counts a blank line between its blocks).
fn partition(heights: &[usize], k: usize) -> Vec<std::ops::Range<usize>> {
    let greedy = |cap: usize| -> Vec<std::ops::Range<usize>> {
        let mut out = Vec::new();
        let (mut start, mut h) = (0usize, 0usize);
        for (i, &b) in heights.iter().enumerate() {
            let add = if i == start { b } else { h + 1 + b };
            if i > start && add > cap {
                out.push(start..i);
                start = i;
                h = b;
            } else {
                h = add;
            }
        }
        out.push(start..heights.len());
        out
    };
    let lo = heights.iter().copied().max().unwrap_or(0);
    let hi = heights.iter().sum::<usize>() + heights.len();
    let (mut lo, mut hi) = (lo, hi);
    while lo < hi {
        let mid = (lo + hi) / 2;
        if greedy(mid).len() <= k {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    greedy(lo)
}

impl Widget for KeyHelp<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Widget::render(&self, area, buf);
    }
}

impl Widget for &KeyHelp<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let mut rows = self.rows(area.width);
        let fit = area.height as usize;
        if rows.len() > fit {
            // Never cut silently: the last row counts the shortcuts below it.
            let hidden: usize = rows[fit - 1..].iter().map(|(_, n)| n).sum();
            rows.truncate(fit - 1);
            let note = if hidden > 0 {
                format!("{ELLIPSIS} +{hidden} more")
            } else {
                ELLIPSIS.to_string()
            };
            rows.push((Line::from(Span::styled(note, self.theme.dim())), 0));
        }
        for (i, (line, _)) in rows.iter().enumerate() {
            let line = text::truncate_line(line.clone(), area.width as usize);
            buf.set_line(area.x, area.y + i as u16, &line, area.width);
        }
    }
}

/// Where a scrolling [`KeyHelp`] is, kept by the host between frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyHelpState {
    /// The first grid row in view.
    pub scroll: usize,
    max_scroll: Option<usize>,
    page: usize,
    shown: Option<(usize, usize, usize)>,
}

impl KeyHelpState {
    pub fn new() -> KeyHelpState {
        KeyHelpState::default()
    }

    /// ↑↓ a row, PgUp/PgDn a page, Home/End the ends; returns whether the
    /// view moved. Letters are left to the host — a filter field types them.
    /// Before the first render End goes as far as it can and the render
    /// clamps it.
    pub fn on_key(&mut self, key: KeyInput) -> bool {
        let page = self.page.max(1);
        let max = self.max_scroll.unwrap_or(usize::MAX);
        let to = match key.key {
            Key::Up => self.scroll.saturating_sub(1),
            Key::Down => self.scroll.saturating_add(1),
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

    /// `(first row, last row, total rows)`, 1-based, from the last render;
    /// `None` before one or when the grid is empty.
    pub fn shown(&self) -> Option<(usize, usize, usize)> {
        self.shown
    }

    /// The furthest `scroll` can go at the last render's size.
    pub fn max_scroll(&self) -> Option<usize> {
        self.max_scroll
    }
}

impl StatefulWidget for KeyHelp<'_> {
    type State = KeyHelpState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut KeyHelpState) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

/// The grid scrolled to `state.scroll`. When it is taller than the area the
/// last row says `lines a–b of n`; the range is the grid's own rows, so the
/// end is exact (herdr estimated it and could scroll past the end or stop
/// short of it).
impl StatefulWidget for &KeyHelp<'_> {
    type State = KeyHelpState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut KeyHelpState) {
        if area.is_empty() {
            return;
        }
        let rows = self.rows(area.width);
        let n = rows.len();
        let h = area.height as usize;
        let overflow = n > h;
        // An overflowing grid gives its last row to the position.
        let body = if overflow { h - 1 } else { h };
        let max = if overflow { n - body } else { 0 };
        state.scroll = state.scroll.min(max);
        state.max_scroll = Some(max);
        state.page = body.max(1);
        let first = state.scroll;
        let last = (first + body).min(n);
        state.shown = (last > first).then_some((first + 1, last, n));
        for (i, (line, _)) in rows[first..last].iter().enumerate() {
            let line = text::truncate_line(line.clone(), area.width as usize);
            buf.set_line(area.x, area.y + i as u16, &line, area.width);
        }
        if overflow {
            let at = if body == 0 {
                format!("{n} lines")
            } else {
                format!("lines {}\u{2013}{} of {n}", first + 1, last)
            };
            let w = area.width as usize;
            let pos = text::truncate(&at, w).into_owned();
            let pos_w = text::width(&pos);
            buf.set_stringn(
                area.right() - pos_w as u16,
                area.bottom() - 1,
                &pos,
                pos_w,
                self.theme.secondary(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_disabled_hint_is_dim_not_a_key() {
        let t = Theme::ansi();
        let on = KeyHint::new("R", "reload").spans(&t, KeyGlyphs::Unicode);
        let off = KeyHint::new("R", "reload")
            .disabled(true)
            .spans(&t, KeyGlyphs::Unicode);
        assert_ne!(on[0].style, off[0].style);
        assert_eq!(off[0].style, t.dim());
        assert_eq!(off[1].style, t.dim());
    }
    use crate::snapshot;
    use ratatui::style::Color;
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    fn ctrl(c: char) -> KeyInput {
        KeyInput::ctrl(Key::Char(c))
    }

    fn with(key: Key, ctrl: bool, alt: bool, shift: bool) -> KeyInput {
        KeyInput {
            key,
            mods: Mods { ctrl, alt, shift },
        }
    }

    fn llmux() -> KeyHints<'static> {
        KeyHints::new([
            KeyHint::new("s", "switch").priority(7),
            KeyHint::new("f", "refresh").priority(4),
            KeyHint::new("R", "reset").priority(3),
            KeyHint::new("a", "add").priority(5),
            KeyHint::new("n", "login").priority(3),
            KeyHint::new("r", "remove").priority(5),
            KeyHint::new("u", "used/left").priority(2),
            KeyHint::new("t", "eta/utc").priority(1),
            KeyHint::new("o", "sort").priority(1),
            KeyHint::new("esc", "back").priority(8),
            KeyHint::new("q", "quit").priority(9),
        ])
        .title("accounts")
    }

    fn text_at(h: &KeyHints, w: u16) -> String {
        let area = Rect::new(0, 0, w, 1);
        let mut buf = Buffer::empty(area);
        h.render(area, &mut buf);
        snapshot::text(&buf)
    }

    #[test]
    fn everything_fits_at_120() {
        assert_eq!(
            text_at(&llmux(), 120),
            "accounts  s switch  f refresh  R reset  a add  n login  r remove  u used/left  t eta/utc  o sort  esc back  q quit"
        );
        assert_eq!(llmux().fit(120).dropped, 0);
    }

    #[test]
    fn narrow_footer_drops_lowest_priority_first_and_says_so() {
        let s = text_at(&llmux(), 80);
        assert_eq!(
            s,
            "accounts  s switch  f refresh  a add  r remove  esc back  q quit  … ? more"
        );
        // Lowest first: sort and eta/utc go before switch and quit.
        assert!(!s.contains("o sort") && !s.contains("t eta/utc"), "{s}");
        assert_eq!(
            text_at(&llmux(), 40),
            "accounts  esc back  q quit  … ? more"
        );
    }

    #[test]
    fn equal_priorities_drop_from_the_right() {
        let h = KeyHints::new([
            KeyHint::new("a", "one"),
            KeyHint::new("b", "two"),
            KeyHint::new("c", "three"),
        ])
        .no_more();
        // "a one  b two  c three" = 21; at 20 one hint goes, plus "  … +1 more".
        assert_eq!(text_at(&h, 20), "a one  … +2 more");
        assert_eq!(text_at(&h, 21), "a one  b two  c three");
        assert_eq!(h.fit(20).shown, vec![0]);
    }

    #[test]
    fn a_lone_hint_wider_than_the_row_clips_with_an_ellipsis() {
        let h = KeyHints::new([KeyHint::new("ctrl+shift+p", "command palette")]);
        let s = text_at(&h, 6);
        assert!(UnicodeWidthStr::width(s.as_str()) <= 6, "{s}");
        assert!(s.ends_with('…'), "{s}");
        // Degenerate widths do not panic.
        for w in 1..4 {
            let _ = text_at(&llmux(), w);
        }
    }

    #[test]
    fn keys_are_accent_bold_labels_secondary_and_mono_has_no_colour() {
        let h = KeyHints::new([KeyHint::new("esc", "back")]).theme(Theme::dark());
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        (&h).render(area, &mut buf);
        assert_eq!(buf[(0, 0)].fg, Theme::dark().accent);
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(4, 0)].fg, Theme::dark().text_secondary);
        let mut buf = Buffer::empty(area);
        llmux().theme(Theme::mono()).render(area, &mut buf);
        assert!(
            buf.content()
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset)
        );
        // "accounts  … ? more": the title and the help key stay bold.
        assert_eq!(snapshot::text(&buf), "accounts  … ? more");
        assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        assert!(buf[(12, 0)].modifier.contains(Modifier::BOLD));
        assert!(buf[(10, 0)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn arrows_are_ambiguous_width_and_ascii_spells_them_out() {
        for c in ['←', '↑', '→', '↓'] {
            assert_eq!(c.width(), Some(1), "{c:?}");
            assert_eq!(c.width_cjk(), Some(2), "{c:?} is East-Asian Ambiguous");
        }
        assert_eq!(ascii_keys("↑↓"), "up/down");
        assert_eq!(ascii_keys("↑/k ↓/j"), "up/k down/j");
        assert_eq!(ascii_keys("←→/tab"), "left/right/tab");
        assert!(matches!(ascii_keys("ctrl+c"), Cow::Borrowed(_)));
        let h = KeyHints::new([KeyHint::new("↑↓", "scroll")]).glyphs(KeyGlyphs::Ascii);
        assert_eq!(text_at(&h, 40), "up/down scroll");
        // Everything the ASCII set draws is narrow on a CJK terminal too,
        // overflow marker included.
        let s = text_at(&llmux().glyphs(KeyGlyphs::Ascii), 80);
        assert!(s.ends_with("... ? more"), "{s}");
        assert_eq!(
            UnicodeWidthStr::width_cjk(s.as_str()),
            UnicodeWidthStr::width(s.as_str())
        );
    }

    #[test]
    fn key_labels_are_plain_text() {
        assert_eq!(key_label(ctrl('c'), KeyGlyphs::Unicode), "ctrl+c");
        assert_eq!(
            key_label(KeyInput::new(Key::Esc), KeyGlyphs::Unicode),
            "esc"
        );
        assert_eq!(key_label(KeyInput::new(Key::Up), KeyGlyphs::Unicode), "↑");
        assert_eq!(key_label(KeyInput::new(Key::Up), KeyGlyphs::Ascii), "up");
        assert_eq!(
            key_label(KeyInput::new(Key::BackTab), KeyGlyphs::Ascii),
            "shift+tab"
        );
        assert_eq!(
            key_label(with(Key::Enter, false, true, false), KeyGlyphs::Ascii),
            "alt+enter"
        );
        assert_eq!(key_label(KeyInput::from(' '), KeyGlyphs::Ascii), "space");
        assert_eq!(
            key_label(KeyInput::new(Key::PageDown), KeyGlyphs::Ascii),
            "pgdn"
        );
        assert_eq!(
            KeyHint::for_key(ctrl('t'), "transcript", KeyGlyphs::Unicode).keys,
            "ctrl+t"
        );
    }

    /// Upstream `shifted_letter_binding_matches_uppercase_char_events` and
    /// its two siblings.
    #[test]
    fn shifted_letters_match_uppercase_events() {
        let b = with(Key::Char('a'), false, false, true);
        assert!(is_press(b, with(Key::Char('a'), false, false, true)));
        assert!(is_press(b, KeyInput::from('A')));
        assert!(is_press(b, with(Key::Char('A'), false, false, true)));
        let ci = with(Key::Char('i'), true, false, true);
        assert!(is_press(ci, with(Key::Char('I'), true, false, false)));
        let o = with(Key::Char('o'), false, false, true);
        assert!(!is_press(o, KeyInput::from('o')));
        assert!(!is_press(o, KeyInput::from('P')));
    }

    /// Upstream `ctrl_bindings_match_all_supported_c0_control_char_events`.
    #[test]
    fn ctrl_bindings_match_raw_c0_characters() {
        let cases = [
            (' ', '\u{00}'),
            ('a', '\u{01}'),
            ('b', '\u{02}'),
            ('c', '\u{03}'),
            ('h', '\u{08}'),
            ('i', '\u{09}'),
            ('j', '\u{0a}'),
            ('m', '\u{0d}'),
            ('p', '\u{10}'),
            ('r', '\u{12}'),
            ('s', '\u{13}'),
            ('z', '\u{1a}'),
            ('4', '\u{1c}'),
            ('5', '\u{1d}'),
            (']', '\u{1d}'),
            ('6', '\u{1e}'),
            ('7', '\u{1f}'),
        ];
        for (ch, c0) in cases {
            assert!(
                is_press(ctrl(ch), KeyInput::from(c0)),
                "ctrl-{ch} vs {c0:?}"
            );
            assert!(
                !is_press(ctrl(ch), with(Key::Char(c0), false, true, false)),
                "alt+{c0:?} is not ctrl-{ch}"
            );
        }
        for code in 1u32..=26 {
            let c0 = char::from_u32(code).unwrap();
            let letter = char::from_u32(u32::from('a') + code - 1).unwrap();
            assert!(is_press(ctrl(letter), KeyInput::from(c0)));
        }
    }

    /// Upstream `ctrl_binding_does_not_match_ambiguous_c0_escape_or_delete`.
    #[test]
    fn escape_and_delete_bytes_stay_ambiguous() {
        assert!(!is_press(ctrl('['), KeyInput::from('\u{1b}')));
        assert!(!is_press(ctrl('?'), KeyInput::from('\u{7f}')));
    }

    #[test]
    fn shift_tab_is_backtab() {
        assert!(is_press(
            KeyInput::new(Key::BackTab),
            with(Key::Tab, false, false, true)
        ));
        assert!(is_press(
            KeyInput::new(Key::BackTab),
            with(Key::BackTab, false, false, true)
        ));
        assert!(!is_press(
            KeyInput::new(Key::BackTab),
            KeyInput::new(Key::Tab)
        ));
    }

    /// Upstream `footer_hint_tests::wraps_whole_hints_using_display_width`.
    #[test]
    fn packs_whole_hints_using_display_width() {
        let hints = ["↑ 上", "↓ 下", "enter select"];
        assert_eq!(
            pack_rows(hints, 9, 1, |h| UnicodeWidthStr::width(*h)),
            vec![vec!["↑ 上", "↓ 下"], vec!["enter select"]]
        );
    }

    /// Upstream `footer_hint_tests::retains_oversized_hints_and_an_empty_row`.
    #[test]
    fn retains_oversized_hints_and_an_empty_row() {
        assert_eq!(
            pack_rows(["ctrl+x stop", "esc back"], 0, 3, |h| h.len()),
            vec![vec!["ctrl+x stop"], vec!["esc back"]]
        );
        assert_eq!(
            pack_rows(Vec::<String>::new(), 40, 3, String::len),
            vec![Vec::<String>::new()]
        );
    }

    #[test]
    fn rows_wrap_every_hint_whole() {
        let rows = llmux().rows(40);
        assert!(rows.len() >= 3);
        for r in &rows {
            assert!(text::line_width(r) <= 40);
        }
        let all: String = rows
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(all.contains("u used/left") && all.contains("q quit"));
    }

    fn help() -> KeyHelp<'static> {
        KeyHelp::new([
            HintGroup::new(
                "Navigate",
                [
                    KeyHint::new("↑↓", "scroll"),
                    KeyHint::new("pgup/pgdn", "page"),
                    KeyHint::new("tab", "next panel"),
                ],
            ),
            HintGroup::new(
                "Accounts",
                [
                    KeyHint::new("s", "switch"),
                    KeyHint::new("a", "add"),
                    KeyHint::new("r", "remove"),
                ],
            ),
            HintGroup::new(
                "App",
                [KeyHint::new("?", "help"), KeyHint::new("q", "quit")],
            ),
        ])
    }

    fn help_text(h: &KeyHelp, w: u16, rows: u16) -> String {
        let area = Rect::new(0, 0, w, rows);
        let mut buf = Buffer::empty(area);
        Widget::render(h, area, &mut buf);
        snapshot::text(&buf)
    }

    #[test]
    fn help_grid_flows_into_three_two_or_one_columns() {
        assert_eq!(
            help_text(&help(), 80, 4),
            "\
Navigate                 Accounts     App
↑↓         scroll        s  switch    ?  help
pgup/pgdn  page          a  add       q  quit
tab        next panel    r  remove"
        );
        assert_eq!(
            help_text(&help(), 40, 8),
            "\
Navigate                 Accounts
↑↓         scroll        s  switch
pgup/pgdn  page          a  add
tab        next panel    r  remove

                         App
                         ?  help
                         q  quit"
        );
        assert_eq!(help().height(20), 4 + 1 + 4 + 1 + 3);
        assert_eq!(help().lines(20)[0].spans[0].content, "Navigate");
    }

    #[test]
    fn help_grid_says_how_many_shortcuts_are_below_the_fold() {
        let s = help_text(&help(), 20, 6);
        let last = s.lines().last().unwrap();
        // Shown: Navigate (3 shortcuts), blank, Accounts title; hidden:
        // Accounts' 3 + App's 2.
        assert_eq!(last, "… +5 more", "{s}");
        // Degenerate sizes do not panic.
        for (w, h) in [(1, 1), (2, 1), (12, 2), (5, 3)] {
            let _ = help_text(&help(), w, h);
        }
        assert_eq!(help_text(&KeyHelp::new([]), 10, 2), "\n");
    }

    /// herdr `keybind_help.rs` `filter_matches_labels_and_shortcuts_case_insensitively`.
    #[test]
    fn the_filter_matches_keys_and_descriptions_ignoring_case() {
        let groups = || {
            KeyHelp::new([
                HintGroup::new(
                    "workspaces / tabs",
                    [
                        KeyHint::new("w", "workspace navigation"),
                        KeyHint::new("c", "new tab"),
                    ],
                ),
                HintGroup::new(
                    "panes",
                    [
                        KeyHint::new("v", "split vertical"),
                        KeyHint::new("x", "close pane"),
                    ],
                ),
            ])
        };
        let work = groups().filter("WoRk");
        assert_eq!(work.matches(), 1);
        assert_eq!(
            work.lines(80)
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>(),
            ["workspaces / tabs", "w  workspace navigation"]
        );
        let x = groups().filter("x");
        assert_eq!(x.matches(), 1);
        assert_eq!(x.lines(80)[1].to_string(), "x  close pane");
        // Group titles are not searched.
        let panes = groups().filter("panes");
        assert_eq!(panes.matches(), 0);
        assert_eq!(help_text(&panes, 40, 2), "no keys match \"panes\"\n");
        assert_eq!(groups().filter("").matches(), 4);
    }

    #[test]
    fn an_empty_result_names_the_query_and_keeps_its_quotes() {
        let h = help().filter("zz").theme(Theme::mono());
        assert_eq!(h.height(40), 1);
        assert_eq!(help_text(&h, 18, 1), "no keys match \"zz\"");
        assert_eq!(help_text(&h, 17, 1), "no keys match \"…\"");
        assert_eq!(help_text(&h, 6, 1), "no ke…");
        let area = Rect::new(0, 0, 30, 2);
        let mut buf = Buffer::empty(area);
        Widget::render(&h, area, &mut buf);
        assert!(buf[(0, 0)].modifier.contains(Modifier::DIM));
        // No groups at all is still a blank grid, not a "no match".
        assert_eq!(KeyHelp::new([]).filter("q").height(20), 0);
    }

    #[test]
    fn the_filter_searches_keys_as_drawn() {
        let h = help().glyphs(KeyGlyphs::Ascii).filter("UP");
        // `↑↓` is drawn `up/down`; `pgup/pgdn` has it too.
        assert_eq!(h.matches(), 2);
        assert_eq!(help().filter("up").matches(), 1, "unicode arrows stay ↑↓");
        assert_eq!(help().filter("↑").matches(), 1);
    }

    fn scrolled(h: &KeyHelp, st: &mut KeyHelpState, w: u16, rows: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, rows);
        let mut buf = Buffer::empty(area);
        StatefulWidget::render(h, area, &mut buf, st);
        snapshot::lines(&buf)
    }

    #[test]
    fn a_tall_grid_scrolls_to_its_exact_end_with_a_position() {
        // One column at 20 cells: 13 rows in a 6-row area.
        let h = help();
        let mut st = KeyHelpState::new();
        let top = scrolled(&h, &mut st, 20, 6);
        assert_eq!(top[0], "Navigate");
        assert_eq!(top[5], format!("{:>20}", "lines 1\u{2013}5 of 13"));
        assert_eq!(st.max_scroll(), Some(8));
        assert_eq!(st.shown(), Some((1, 5, 13)));
        assert!(st.on_key(KeyInput::new(Key::End)));
        let end = scrolled(&h, &mut st, 20, 6);
        // The last shortcut on the last body row: no blank rows past the end
        // and nothing left unreachable (herdr's estimate missed by 1–3 rows).
        assert_eq!(end[4], "q  quit");
        assert_eq!(end[5], format!("{:>20}", "lines 9\u{2013}13 of 13"));
        assert!(!st.on_key(KeyInput::new(Key::Down)), "at the end");
        assert!(st.on_key(KeyInput::new(Key::PageUp)));
        assert_eq!(st.scroll, 3);
        assert!(st.on_key(KeyInput::new(Key::Home)));
        assert!(!st.on_key(KeyInput::from('k')), "letters are the host's");
        // A filter that shrinks the grid clamps a scroll that was further.
        st.scroll = 8;
        let few = scrolled(&help().filter("a"), &mut st, 20, 6);
        assert_eq!(st.scroll, st.max_scroll().unwrap());
        assert!(few.iter().all(|l| text::width(l) <= 20));
        // Everything fits: no position, scroll pinned at 0.
        let mut st = KeyHelpState {
            scroll: 4,
            ..KeyHelpState::new()
        };
        let all = scrolled(&h, &mut st, 80, 6);
        assert!(!all.iter().any(|l| l.contains("lines")));
        assert_eq!(st.scroll, 0);
        // Degenerate sizes do not panic.
        for (w, r) in [(1, 1), (2, 1), (12, 2), (5, 3)] {
            scrolled(&h, &mut KeyHelpState::new(), w, r);
        }
    }

    /// herdr padded the key column with `{:<N}` (chars); here it is cells.
    #[test]
    fn the_key_column_is_measured_in_cells() {
        let h = KeyHelp::new([HintGroup::new(
            "한글",
            [
                KeyHint::new("가나", "wide keys"),
                KeyHint::new("abc", "narrow"),
            ],
        )]);
        let lines: Vec<String> = h.lines(40).iter().map(|l| l.to_string()).collect();
        let col = |l: &str, label: &str| text::width(&l[..l.find(label).unwrap()]);
        assert_eq!(col(&lines[1], "wide"), col(&lines[2], "narrow"));
        assert_eq!(col(&lines[1], "wide"), 6);
    }

    #[test]
    fn partition_keeps_order_and_balances() {
        assert_eq!(partition(&[4, 4, 3], 2), vec![0..1, 1..3]);
        assert_eq!(partition(&[4, 4, 3], 3), vec![0..1, 1..2, 2..3]);
        assert_eq!(partition(&[2, 2, 2, 2], 2), vec![0..2, 2..4]);
        assert_eq!(partition(&[5], 3), vec![0..1]);
    }
}
