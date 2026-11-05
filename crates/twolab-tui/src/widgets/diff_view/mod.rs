// Derived from vercel-labs/fx `src/core/output/diff.zig` at commit
// 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025 Vercel,
// Inc.): `compute` (the line LCS and its trailing-newline markers),
// `formatUnified` (changes with context, one line-number column) and
// `appendDiffRenderLine` (number, sign, text). Translated from Zig to Rust and
// re-expressed as ratatui text; row rules checked against 2lab's xfx
// `src/tui/approval_view.rs` (Apache-2.0) at 93f516b. Modified by 2lab.

//! What changed, line by line: unified hunks with line numbers, a `+`/`-`
//! gutter, a hunk header per hunk and a `+N −M` summary.
//!
//! Ported from Vercel fx (`src/core/output/diff.zig`, Apache-2.0): the line
//! LCS ([`diff_lines`]), the single line-number column sized to the widest
//! number, and the `number sign text` row grammar. fx draws a change in one
//! grey and lets the sign carry it; here additions and deletions take the
//! theme's `done` and `fail` colours, and under [`Theme::mono`] additions are
//! bold, deletions dim, and the `+`/`-` gutter says which is which.
//!
//! The widget draws hunks it is given ([`Hunk`], [`DiffLine`]); it never
//! diffs anything itself. [`diff_lines`] and [`hunks`] are a small helper for
//! hosts without a diff library.
//!
//! A row longer than the area is clipped by cells and ends in `…`. Text from a
//! file is shown inert: a tab becomes spaces and a control character is
//! spelled (`\x1b`), so a file can never move the cursor or recolour the
//! screen. When the area is too narrow for line numbers and text, the view
//! shows the summary alone and says how many lines it left out.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::text;
use crate::theme::{Status, Theme};

/// How many unchanged lines [`hunks`] keeps on either side of a change by
/// default — the unified-diff convention.
pub const DEFAULT_CONTEXT: usize = 3;

/// Beyond this many LCS matrix cells, [`diff_lines`] stops searching for
/// matches inside the changed middle and reports it as removed then added
/// (fx `canonical_max_matrix_cells`).
pub const MAX_MATRIX_CELLS: usize = 1_000_000;

/// The narrowest text column worth showing; below it the view is a summary.
const MIN_TEXT: usize = 24;

/// Cells a tab takes when a line is made inert.
const TAB_CELLS: usize = 4;

/// The row a file whose last line gained a line break ends with (fx).
pub const TRAILING_NEWLINE_ADDED: &str = "(trailing newline added)";
/// The row a file whose last line lost its line break ends with (fx).
pub const TRAILING_NEWLINE_REMOVED: &str = "(trailing newline removed)";

/// What a line of a diff is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffKind {
    /// Unchanged, shown for context.
    Context,
    /// Only in the new text.
    Add,
    /// Only in the old text.
    Delete,
}

/// One line of a diff, with its number on either side (1-based; `None` on the
/// side it is not in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

impl DiffLine {
    pub fn context(old_no: u32, new_no: u32, text: impl Into<String>) -> DiffLine {
        DiffLine {
            kind: DiffKind::Context,
            old_no: Some(old_no),
            new_no: Some(new_no),
            text: text.into(),
        }
    }

    pub fn add(new_no: u32, text: impl Into<String>) -> DiffLine {
        DiffLine {
            kind: DiffKind::Add,
            old_no: None,
            new_no: Some(new_no),
            text: text.into(),
        }
    }

    pub fn delete(old_no: u32, text: impl Into<String>) -> DiffLine {
        DiffLine {
            kind: DiffKind::Delete,
            old_no: Some(old_no),
            new_no: None,
            text: text.into(),
        }
    }

    /// The number shown in the gutter: the new side's, or the old side's for a
    /// deleted line (fx shows one column).
    pub fn number(&self) -> Option<u32> {
        self.new_no.or(self.old_no)
    }
}

/// A run of changes with the context around them, and the ranges a
/// `@@ -a,b +c,d @@` header names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What the header names after the ranges (a function, a heading).
    pub section: Option<String>,
    pub lines: Vec<DiffLine>,
}

impl Hunk {
    /// A hunk over `lines`, its ranges counted from their numbers.
    pub fn new(lines: Vec<DiffLine>) -> Hunk {
        let old_len = lines.iter().filter(|l| l.kind != DiffKind::Add).count() as u32;
        let new_len = lines.iter().filter(|l| l.kind != DiffKind::Delete).count() as u32;
        let first_old = lines.iter().find_map(|l| l.old_no);
        let first_new = lines.iter().find_map(|l| l.new_no);
        // A side with no lines in the hunk starts at the line before it, as
        // unified diffs count. Alone, a hunk can only guess that line from the
        // other side's first number; [`hunks`] knows it and sets it.
        let old_start = first_old.unwrap_or_else(|| first_new.map_or(0, |n| n.saturating_sub(1)));
        let new_start = first_new.unwrap_or_else(|| first_old.map_or(0, |n| n.saturating_sub(1)));
        Hunk {
            old_start,
            old_len,
            new_start,
            new_len,
            section: None,
            lines,
        }
    }

    pub fn section(mut self, section: impl Into<String>) -> Hunk {
        self.section = Some(section.into());
        self
    }

    /// `@@ -a,b +c,d @@`, the `,1` left out as git does.
    pub fn header(&self) -> String {
        fn range(start: u32, len: u32) -> String {
            if len == 1 {
                start.to_string()
            } else {
                format!("{start},{len}")
            }
        }
        format!(
            "@@ -{} +{} @@",
            range(self.old_start, self.old_len),
            range(self.new_start, self.new_len)
        )
    }
}

/// Lines added and removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

impl DiffStats {
    pub fn of_lines(lines: &[DiffLine]) -> DiffStats {
        let mut s = DiffStats::default();
        for l in lines {
            match l.kind {
                DiffKind::Add => s.added += 1,
                DiffKind::Delete => s.removed += 1,
                DiffKind::Context => {}
            }
        }
        s
    }

    pub fn of_hunks(hunks: &[Hunk]) -> DiffStats {
        hunks.iter().fold(DiffStats::default(), |acc, h| {
            let s = DiffStats::of_lines(&h.lines);
            DiffStats {
                added: acc.added + s.added,
                removed: acc.removed + s.removed,
            }
        })
    }
}

/// The lines of `text` as a file has them: a final line break ends the last
/// line rather than starting an empty one (fx `appendLines`).
fn split_lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal(usize, usize),
    Add(usize),
    Remove(usize),
}

/// The LCS edit script of `a` into `b`, walked back from the end preferring an
/// addition on a tie, so a replaced line reads `-old` then `+new` (fx
/// `compute`).
fn lcs_ops(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    let stride = m + 1;
    let mut t = vec![0u32; (n + 1) * stride];
    for i in 1..=n {
        for j in 1..=m {
            t[i * stride + j] = if a[i - 1] == b[j - 1] {
                t[(i - 1) * stride + j - 1] + 1
            } else {
                t[i * stride + j - 1].max(t[(i - 1) * stride + j])
            };
        }
    }
    let mut ops = Vec::with_capacity(n + m);
    let (mut i, mut j) = (n, m);
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            ops.push(Op::Equal(i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || t[i * stride + j - 1] >= t[(i - 1) * stride + j]) {
            ops.push(Op::Add(j - 1));
            j -= 1;
        } else {
            ops.push(Op::Remove(i - 1));
            i -= 1;
        }
    }
    ops.reverse();
    ops
}

/// Every line of `old` and `new` as a diff: unchanged lines as context,
/// the rest added or removed, numbered on both sides.
///
/// A line-level LCS (fx `compute`). The common head and tail are matched
/// first, and a changed middle whose matrix would pass [`MAX_MATRIX_CELLS`]
/// is reported as removed then added rather than searched. When both texts
/// are non-empty and only one ends in a line break, a last row says so
/// ([`TRAILING_NEWLINE_ADDED`], [`TRAILING_NEWLINE_REMOVED`]).
pub fn diff_lines(old: &str, new: &str) -> Vec<DiffLine> {
    let mut a = split_lines(old);
    let mut b = split_lines(new);
    if !old.is_empty() && !new.is_empty() {
        match (old.ends_with('\n'), new.ends_with('\n')) {
            (true, false) => a.push(TRAILING_NEWLINE_REMOVED),
            (false, true) => b.push(TRAILING_NEWLINE_ADDED),
            _ => {}
        }
    }
    let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let room = a.len().min(b.len()) - prefix;
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    let (am, bm) = (&a[prefix..a.len() - suffix], &b[prefix..b.len() - suffix]);
    let no = |i: usize| (i + 1) as u32;

    let mut out = Vec::with_capacity(a.len().max(b.len()) + 4);
    for (i, line) in a.iter().enumerate().take(prefix) {
        out.push(DiffLine::context(no(i), no(i), *line));
    }
    let fits = (am.len() + 1)
        .checked_mul(bm.len() + 1)
        .is_some_and(|cells| cells <= MAX_MATRIX_CELLS);
    let ops = if fits {
        lcs_ops(am, bm)
    } else {
        (0..am.len())
            .map(Op::Remove)
            .chain((0..bm.len()).map(Op::Add))
            .collect()
    };
    for op in ops {
        out.push(match op {
            Op::Equal(i, j) => DiffLine::context(no(prefix + i), no(prefix + j), am[i]),
            Op::Add(j) => DiffLine::add(no(prefix + j), bm[j]),
            Op::Remove(i) => DiffLine::delete(no(prefix + i), am[i]),
        });
    }
    let (a_tail, b_tail) = (a.len() - suffix, b.len() - suffix);
    for k in 0..suffix {
        out.push(DiffLine::context(
            no(a_tail + k),
            no(b_tail + k),
            a[a_tail + k],
        ));
    }
    out
}

/// The changes in `lines` grouped into hunks, each with up to `context`
/// unchanged lines on either side; hunks whose context would touch are one.
pub fn hunks(lines: &[DiffLine], context: usize) -> Vec<Hunk> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.kind == DiffKind::Context {
            continue;
        }
        let (s, e) = (
            i.saturating_sub(context),
            (i + context + 1).min(lines.len()),
        );
        match ranges.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => ranges.push((s, e)),
        }
    }
    ranges
        .into_iter()
        .map(|(s, e)| {
            let mut h = Hunk::new(lines[s..e].to_vec());
            // A side with no lines here starts after the last line before.
            if h.old_len == 0 {
                h.old_start = lines[..s].iter().rev().find_map(|l| l.old_no).unwrap_or(0);
            }
            if h.new_len == 0 {
                h.new_start = lines[..s].iter().rev().find_map(|l| l.new_no).unwrap_or(0);
            }
            h
        })
        .collect()
}

/// Whether `c` would be obeyed or reorder text rather than be drawn.
fn needs_spelling(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

/// `s` safe to put on a row: a tab becomes [`TAB_CELLS`] spaces, a line break
/// `\n`, a control `\xNN` and a bidirectional control `\u{NNNN}` — spelled,
/// so the reader sees it was there and the terminal never obeys it.
/// Borrowed when there is nothing to spell.
pub(crate) fn inert(s: &str) -> Cow<'_, str> {
    if !s.chars().any(needs_spelling) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '\t' => out.extend(std::iter::repeat_n(' ', TAB_CELLS)),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() && (c as u32) < 0x100 => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c if needs_spelling(c) => out.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// `s` clipped to `max` cells from the left, keeping its tail behind `…` —
/// how a path is shortened, so the file name survives.
pub(crate) fn truncate_left(s: &str, max: usize) -> Cow<'_, str> {
    if text::width(s) <= max {
        return Cow::Borrowed(s);
    }
    if max == 0 {
        return Cow::Borrowed("");
    }
    let mut start = s.len();
    let mut used = 0usize;
    for (i, g) in s.grapheme_indices(true).rev() {
        let w = text::width(g);
        if used + w > max - 1 {
            break;
        }
        used += w;
        start = i;
    }
    let mut out = String::with_capacity(s.len() - start + 3);
    out.push(text::ELLIPSIS);
    out.push_str(&s[start..]);
    Cow::Owned(out)
}

/// What happened to the file as a whole, for the summary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileChange {
    #[default]
    Modified,
    Added,
    Deleted,
}

/// The style of an added line: `done` in colour, bold in mono.
pub fn added_style(theme: &Theme) -> Style {
    if theme.is_mono() {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        theme.status(Status::Done)
    }
}

/// The style of a removed line: `fail` in colour, dim in mono.
pub fn removed_style(theme: &Theme) -> Style {
    if theme.is_mono() {
        Style::new().add_modifier(Modifier::DIM)
    } else {
        theme.status(Status::Failed)
    }
}

/// `+N −M`, each count in its line style.
pub fn stats_spans(stats: DiffStats, theme: &Theme) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!("+{}", stats.added), added_style(theme)),
        Span::raw(" "),
        Span::styled(format!("\u{2212}{}", stats.removed), removed_style(theme)),
    ]
}

/// Unified hunks as rows: a summary row on top (path and `+N −M`), then each
/// hunk's `@@` header and its lines, scrolled by [`DiffView::scroll`].
#[derive(Debug, Clone)]
pub struct DiffView<'a> {
    hunks: &'a [Hunk],
    path: Option<Cow<'a, str>>,
    renamed_from: Option<Cow<'a, str>>,
    change: FileChange,
    scroll: usize,
    summary: bool,
    summary_only: bool,
    theme: Theme,
}

/// One row of the body, before it is styled.
enum Row<'h> {
    Header(&'h Hunk),
    Line(&'h DiffLine),
    Empty,
}

impl<'a> DiffView<'a> {
    pub fn new(hunks: &'a [Hunk]) -> DiffView<'a> {
        DiffView {
            hunks,
            path: None,
            renamed_from: None,
            change: FileChange::Modified,
            scroll: 0,
            summary: true,
            summary_only: false,
            theme: Theme::default(),
        }
    }

    /// The file's path, shown on the summary row (shortened from the left).
    pub fn path(mut self, path: impl Into<Cow<'a, str>>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The path the file had before a rename.
    pub fn renamed_from(mut self, path: impl Into<Cow<'a, str>>) -> Self {
        self.renamed_from = Some(path.into());
        self
    }

    pub fn change(mut self, change: FileChange) -> Self {
        self.change = change;
        self
    }

    /// Body rows scrolled off the top. Clamped so the last row stays on the
    /// last line of the area.
    pub fn scroll(mut self, rows: usize) -> Self {
        self.scroll = rows;
        self
    }

    /// Whether the summary row is drawn (it always is when the view is too
    /// narrow for the body).
    pub fn summary(mut self, on: bool) -> Self {
        self.summary = on;
        self
    }

    /// Draw only the summary, whatever the width.
    pub fn summary_only(mut self, on: bool) -> Self {
        self.summary_only = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn stats(&self) -> DiffStats {
        DiffStats::of_hunks(self.hunks)
    }

    /// Cells of the line-number column: the widest number in any hunk, so
    /// scrolling never shifts the gutter.
    pub fn gutter_width(&self) -> usize {
        let widest = self
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter_map(DiffLine::number)
            .max()
            .unwrap_or(1);
        widest.to_string().len()
    }

    /// Rows the body takes (hunk headers and lines; one row for no changes).
    pub fn body_len(&self) -> usize {
        let n: usize = self.hunks.iter().map(|h| 1 + h.lines.len()).sum();
        n.max(1)
    }

    fn narrow(&self, width: u16) -> bool {
        self.summary_only || (width as usize) < self.gutter_width() + 3 + MIN_TEXT
    }

    fn has_summary(&self, width: u16) -> bool {
        self.summary || self.narrow(width)
    }

    /// The furthest [`DiffView::scroll`] goes in `area`.
    pub fn max_scroll(&self, area: Rect) -> usize {
        if self.narrow(area.width) {
            return 0;
        }
        let body_h =
            (area.height as usize).saturating_sub(usize::from(self.has_summary(area.width)));
        self.body_len().saturating_sub(body_h)
    }

    fn rows(&self) -> Vec<Row<'_>> {
        if self.hunks.is_empty() {
            return vec![Row::Empty];
        }
        let mut rows = Vec::with_capacity(self.body_len());
        for h in self.hunks {
            rows.push(Row::Header(h));
            rows.extend(h.lines.iter().map(Row::Line));
        }
        rows
    }

    fn row_line(&self, row: &Row<'_>, gutter: usize, width: usize) -> Line<'static> {
        let t = &self.theme;
        let line = match row {
            Row::Empty => Line::from(Span::styled("no changes", t.dim())),
            Row::Header(h) => {
                let mut spans = vec![Span::styled(h.header(), t.accent())];
                if let Some(s) = &h.section {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(inert(s).into_owned(), t.secondary()));
                }
                Line::from(spans)
            }
            Row::Line(l) => {
                let (sign, style, number_style) = match l.kind {
                    DiffKind::Context => (" ", t.dim(), t.dim()),
                    DiffKind::Add => ("+", added_style(t), added_style(t)),
                    DiffKind::Delete => ("-", removed_style(t), removed_style(t)),
                };
                let number = l.number().map(|n| n.to_string()).unwrap_or_default();
                Line::from(vec![
                    Span::styled(text::pad(&number, gutter, text::Align::Right), number_style),
                    Span::raw(" "),
                    Span::styled(sign, style),
                    Span::raw(" "),
                    Span::styled(inert(&l.text).into_owned(), style),
                ])
            }
        };
        text::truncate_line(line, width)
    }

    /// The body rows, each clipped to `width` cells — for a host that places
    /// the diff inside its own paragraph or list.
    pub fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let gutter = self.gutter_width();
        self.rows()
            .iter()
            .map(|r| self.row_line(r, gutter, width as usize))
            .collect()
    }

    /// The summary row at `width`: path (and what happened to it) on the
    /// left, the scroll position and `+N −M` on the right. Dropped in that
    /// order — position, then the rename or tag, then the path from the left
    /// — and never the counts while they fit.
    pub fn summary_line(
        &self,
        width: u16,
        position: Option<(usize, usize, usize)>,
    ) -> Line<'static> {
        self.summary_fit(width, position).0
    }

    /// What happened to the file, in words: a rename, a new or a deleted file.
    fn tag(&self) -> Option<String> {
        match (&self.renamed_from, self.change) {
            (Some(from), _) => Some(format!("renamed from {}", inert(from))),
            (None, FileChange::Added) => Some("new file".into()),
            (None, FileChange::Deleted) => Some("deleted".into()),
            (None, FileChange::Modified) => None,
        }
    }

    /// [`DiffView::summary_line`], and whether the tag made it onto the row.
    fn summary_fit(
        &self,
        width: u16,
        position: Option<(usize, usize, usize)>,
    ) -> (Line<'static>, bool) {
        let t = &self.theme;
        let width = width as usize;
        let counts = stats_spans(self.stats(), t);
        let counts_w = text::spans_width(&counts);
        let pos = position.map(|(a, b, n)| format!("rows {a}-{b} of {n}"));
        let tag = self.tag();
        let path = self.path.as_deref().map(inert);
        let path_style = t.text().add_modifier(Modifier::BOLD);

        let build = |left: Vec<Span<'static>>, pos: Option<&String>| -> Option<Line<'static>> {
            let mut right = Vec::new();
            if let Some(p) = pos {
                right.push(Span::styled(p.clone(), t.dim()));
                right.push(Span::raw("  "));
            }
            right.extend(counts.iter().cloned());
            let lw = text::spans_width(&left);
            let rw = text::spans_width(&right);
            let gap = if lw == 0 { 0 } else { 2 };
            if lw + gap + rw > width {
                return None;
            }
            let mut spans = left;
            spans.push(Span::raw(" ".repeat(width - lw - rw)));
            spans.extend(right);
            Some(Line::from(spans))
        };
        let left_full = |with_tag: bool| -> Vec<Span<'static>> {
            let mut v = Vec::new();
            if let Some(p) = &path {
                v.push(Span::styled(p.to_string(), path_style));
            }
            if with_tag && let Some(tag) = &tag {
                if !v.is_empty() {
                    v.push(Span::raw("  "));
                }
                v.push(Span::styled(tag.clone(), t.dim()));
            }
            v
        };
        for (with_tag, with_pos) in [(true, true), (true, false), (false, false)] {
            if with_pos && pos.is_none() {
                continue;
            }
            if let Some(l) = build(
                left_full(with_tag),
                if with_pos { pos.as_ref() } else { None },
            ) {
                return (l, with_tag || tag.is_none());
            }
        }
        // The path, shortened from the left so its file name stays.
        if let Some(p) = &path {
            let room = width.saturating_sub(counts_w + 2);
            if room >= 4 {
                let left = vec![Span::styled(
                    truncate_left(p, room).into_owned(),
                    path_style,
                )];
                if let Some(l) = build(left, None) {
                    return (l, tag.is_none());
                }
            }
        }
        (
            text::truncate_line(Line::from(counts), width),
            tag.is_none(),
        )
    }
}

impl Widget for DiffView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

impl Widget for &DiffView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let t = &self.theme;
        let width = area.width as usize;
        let mut y = area.y;
        let bottom = area.bottom();

        if self.narrow(area.width) {
            // The summary, the tag it had no room for, and what was left out
            // — the tag is the first to go when the area is short.
            let (summary, tag_shown) = self.summary_fit(area.width, None);
            let mut rows = vec![summary];
            let tag = self.tag().filter(|_| !tag_shown && area.height >= 3);
            if let Some(tag) = tag {
                let tag = match (&self.renamed_from, tag.strip_prefix("renamed from ")) {
                    (Some(_), Some(from)) => {
                        format!(
                            "renamed from {}",
                            truncate_left(from, width.saturating_sub(13))
                        )
                    }
                    _ => tag,
                };
                rows.push(Line::from(Span::styled(tag, t.dim())));
            }
            let hidden: usize = self.hunks.iter().map(|h| h.lines.len()).sum();
            let note = format!("{hidden} lines hidden, widen to review");
            rows.push(Line::from(Span::styled(note, t.dim())));
            for row in rows {
                if y >= bottom {
                    break;
                }
                buf.set_line(area.x, y, &text::truncate_line(row, width), area.width);
                y += 1;
            }
            return;
        }

        let rows = self.rows();
        let summary = self.has_summary(area.width);
        let body_h = (area.height as usize).saturating_sub(usize::from(summary));
        let offset = self.scroll.min(rows.len().saturating_sub(body_h));
        if summary {
            let shown = body_h.min(rows.len() - offset);
            let position = (shown < rows.len()).then_some((offset + 1, offset + shown, rows.len()));
            buf.set_line(
                area.x,
                y,
                &self.summary_line(area.width, position),
                area.width,
            );
            y += 1;
        }
        let gutter = self.gutter_width();
        for row in rows.iter().skip(offset).take(body_h) {
            if y >= bottom {
                break;
            }
            buf.set_line(area.x, y, &self.row_line(row, gutter, width), area.width);
            y += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    fn kinds(d: &[DiffLine]) -> Vec<DiffKind> {
        d.iter().map(|l| l.kind).collect()
    }

    fn render(view: &DiffView, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        view.render(buf.area, &mut buf);
        buf
    }

    // fx `compute` tests (diff.zig:2038-2134).
    #[test]
    fn identical_inputs_produce_only_context() {
        let d = diff_lines("a\nb\nc\n", "a\nb\nc\n");
        assert_eq!(kinds(&d), [DiffKind::Context; 3]);
        assert_eq!(DiffStats::of_lines(&d), DiffStats::default());
    }

    #[test]
    fn a_pure_insert_in_the_middle() {
        let d = diff_lines("a\nb\n", "a\nX\nb\n");
        assert_eq!(
            kinds(&d),
            [DiffKind::Context, DiffKind::Add, DiffKind::Context]
        );
        assert_eq!(d[1], DiffLine::add(2, "X"));
        assert_eq!(
            DiffStats::of_lines(&d),
            DiffStats {
                added: 1,
                removed: 0
            }
        );
    }

    #[test]
    fn a_pure_delete() {
        let d = diff_lines("a\nb\nc\n", "a\nc\n");
        assert_eq!(d.len(), 3);
        assert_eq!(d[1], DiffLine::delete(2, "b"));
    }

    #[test]
    fn a_replaced_line_reads_removed_then_added() {
        let d = diff_lines("a\nold\nc\n", "a\nnew\nc\n");
        assert_eq!(
            d,
            [
                DiffLine::context(1, 1, "a"),
                DiffLine::delete(2, "old"),
                DiffLine::add(2, "new"),
                DiffLine::context(3, 3, "c"),
            ]
        );
    }

    #[test]
    fn a_trailing_newline_change_is_visible() {
        let d = diff_lines("a\n", "a");
        assert_eq!(d[1], DiffLine::delete(2, TRAILING_NEWLINE_REMOVED));
        let d = diff_lines("a", "a\n");
        assert_eq!(d[1], DiffLine::add(2, TRAILING_NEWLINE_ADDED));
        // A new file is not a newline change.
        assert_eq!(diff_lines("", "first\nsecond\n").len(), 2);
    }

    #[test]
    fn line_numbers_increase_on_both_sides() {
        let d = diff_lines("a\nb\nc\n", "a\nX\nb\nc\n");
        let nos: Vec<_> = d.iter().map(|l| (l.old_no, l.new_no)).collect();
        assert_eq!(
            nos,
            [
                (Some(1), Some(1)),
                (None, Some(2)),
                (Some(2), Some(3)),
                (Some(3), Some(4))
            ]
        );
    }

    #[test]
    fn the_lcs_finds_moves_between_matches() {
        let d = diff_lines("x\na\ny\nb\nz\n", "a\nQ\nb\n");
        let s = DiffStats::of_lines(&d);
        assert_eq!((s.added, s.removed), (1, 3));
        let kept: Vec<&str> = d
            .iter()
            .filter(|l| l.kind == DiffKind::Context)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(kept, ["a", "b"]);
    }

    #[test]
    fn a_middle_past_the_matrix_bound_is_removed_then_added() {
        let old: String = (0..1200).map(|i| format!("o{i}\n")).collect();
        let new: String = (0..1200).map(|i| format!("n{i}\n")).collect();
        let d = diff_lines(&format!("head\n{old}tail\n"), &format!("head\n{new}tail\n"));
        assert_eq!(d.first(), Some(&DiffLine::context(1, 1, "head")));
        assert_eq!(d.last(), Some(&DiffLine::context(1202, 1202, "tail")));
        assert!(d[1..1201].iter().all(|l| l.kind == DiffKind::Delete));
        assert!(d[1201..2401].iter().all(|l| l.kind == DiffKind::Add));
    }

    // fx "only emits changed hunks with limited context" / "separated hunks".
    #[test]
    fn hunks_keep_context_and_split_far_changes() {
        let old: String = (1..=16)
            .map(|i| {
                if i == 3 || i == 14 {
                    format!("old-{i}\n")
                } else {
                    "same\n".into()
                }
            })
            .collect();
        let new: String = (1..=16)
            .map(|i| {
                if i == 3 || i == 14 {
                    format!("new-{i}\n")
                } else {
                    "same\n".into()
                }
            })
            .collect();
        let hs = hunks(&diff_lines(&old, &new), 2);
        assert_eq!(hs.len(), 2);
        assert_eq!(hs[0].header(), "@@ -1,5 +1,5 @@");
        assert_eq!(hs[1].header(), "@@ -12,5 +12,5 @@");
        // Close changes share one hunk.
        let hs = hunks(&diff_lines(&old, &new), 6);
        assert_eq!(hs.len(), 1);
    }

    #[test]
    fn a_hunk_header_counts_a_pure_insertion_from_the_line_before() {
        let hs = hunks(
            &diff_lines("a\nb\nc\nd\ne\nf\ng\nh\n", "a\nb\nc\nd\nNEW\ne\nf\ng\nh\n"),
            0,
        );
        assert_eq!(hs[0].header(), "@@ -4,0 +5 @@");
        let hs = hunks(&diff_lines("", "one\ntwo\n"), 3);
        assert_eq!(hs[0].header(), "@@ -0,0 +1,2 @@");
    }

    #[test]
    fn rows_are_number_sign_text_in_one_column() {
        let hs = hunks(&diff_lines("a\nold\nc\n", "a\nnew\nc\n"), 3);
        let v = DiffView::new(&hs).path("src/lib.rs");
        let buf = render(&v, 40, 6);
        assert_eq!(
            snapshot::lines(&buf),
            [
                "src/lib.rs                         +1 −1",
                "@@ -1,3 +1,3 @@",
                "1   a",
                "2 - old",
                "2 + new",
                "3   c",
            ]
        );
    }

    #[test]
    fn the_gutter_fits_the_widest_number_in_every_hunk() {
        let old: String = (1..=12).map(|i| format!("line{i}\n")).collect();
        let new = format!("{old}extra\n");
        let hs = hunks(&diff_lines(&old, &new), 3);
        let v = DiffView::new(&hs).summary(false);
        let rows = snapshot::lines(&render(&v, 40, 6));
        assert!(rows.contains(&"13 + extra".to_string()), "{rows:?}");
        assert!(rows.contains(&"10   line10".to_string()), "{rows:?}");
        assert_eq!(v.gutter_width(), 2);
    }

    #[test]
    fn long_lines_are_clipped_by_cells_with_an_ellipsis() {
        let hs = hunks(
            &diff_lines("", "let 인사 = \"안녕하세요, 반갑습니다\";\n"),
            3,
        );
        let v = DiffView::new(&hs).summary(false);
        let buf = render(&v, 30, 2);
        let row = &snapshot::lines(&buf)[1];
        assert!(row.ends_with('…'), "{row:?}");
        assert!(text::width(row) <= 30);
    }

    #[test]
    fn control_characters_in_a_file_are_spelled_not_obeyed() {
        let hs = hunks(&diff_lines("", "a\u{1b}[2Jb\tc\u{202e}d\n"), 3);
        let v = DiffView::new(&hs).summary(false);
        let rows = snapshot::lines(&render(&v, 60, 2));
        assert_eq!(rows[1], "1 + a\\x1b[2Jb    c\\u{202e}d");
    }

    #[test]
    fn colours_come_from_done_and_fail_and_mono_uses_weight() {
        let hs = hunks(&diff_lines("old\n", "new\n"), 3);
        let t = Theme::dark();
        let buf = render(&DiffView::new(&hs).theme(t).summary(false), 40, 3);
        assert_eq!(buf[(2, 1)].symbol(), "-");
        assert_eq!(buf[(2, 1)].fg, t.fail);
        assert_eq!(buf[(4, 2)].fg, t.done);
        let m = Theme::mono();
        let buf = render(&DiffView::new(&hs).theme(m), 40, 4);
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        assert!(
            buf[(4, 3)].modifier.contains(Modifier::BOLD),
            "added is bold"
        );
        assert!(
            buf[(4, 2)].modifier.contains(Modifier::DIM),
            "removed is dim"
        );
    }

    #[test]
    fn scrolling_moves_the_body_under_a_sticky_summary_that_says_where() {
        let old: String = (1..=30).map(|i| format!("l{i}\n")).collect();
        let new = old.replace("l5\n", "L5\n").replace("l25\n", "L25\n");
        let hs = hunks(&diff_lines(&old, &new), 3);
        let v = DiffView::new(&hs).path("a.txt");
        assert_eq!(v.body_len(), 2 * (1 + 8));
        let area = Rect::new(0, 0, 40, 5);
        assert_eq!(v.max_scroll(area), 18 - 4);
        let top = snapshot::lines(&render(&v, 40, 5));
        assert!(top[0].contains("rows 1-4 of 18"), "{top:?}");
        assert_eq!(top[1], "@@ -2,7 +2,7 @@");
        let v = v.scroll(99);
        let end = snapshot::lines(&render(&v, 40, 5));
        assert!(end[0].contains("rows 15-18 of 18"), "{end:?}");
        assert_eq!(end[4], "28   l28");
    }

    #[test]
    fn a_narrow_area_shows_the_summary_and_says_what_it_left_out() {
        let hs = hunks(&diff_lines("a\nb\n", "a\nc\n"), 3);
        let v = DiffView::new(&hs)
            .path("src/net/client.rs")
            .renamed_from("src/client.rs");
        let rows = snapshot::lines(&render(&v, 24, 2));
        assert_eq!(rows[0], "src/net/client.rs  +1 −1");
        assert_eq!(rows[1], "3 lines hidden, widen t…");
        // With a third row the rename gets one of its own.
        let rows = snapshot::lines(&render(&v, 24, 3));
        assert_eq!(rows[1], "renamed from …/client.rs");
        assert_eq!(rows[2], "3 lines hidden, widen t…");
        let tighter = snapshot::lines(&render(&v, 20, 1));
        assert_eq!(
            tighter[0], "…et/client.rs  +1 −1",
            "the path keeps its tail"
        );
        let wide = snapshot::lines(&render(&v, 60, 1));
        assert_eq!(
            wide[0],
            format!(
                "src/net/client.rs  renamed from src/client.rs{}+1 −1",
                " ".repeat(10)
            )
        );
    }

    #[test]
    fn no_hunks_is_said_rather_than_left_blank() {
        let v = DiffView::new(&[]).path("README.md");
        let rows = snapshot::lines(&render(&v, 30, 2));
        assert_eq!(rows[1], "no changes");
    }

    #[test]
    fn tiny_areas_do_not_panic() {
        let hs = hunks(&diff_lines("a\n", "b\n"), 3);
        for (w, h) in [(1, 1), (2, 1), (12, 2), (3, 7)] {
            let _ = render(&DiffView::new(&hs).path("x"), w, h);
        }
    }
}
