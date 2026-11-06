// Ported from OpenAI Codex `codex-rs/tui/src/bottom_pane/selection_popup_common.rs`,
// `selection_row_layout.rs`, `selection_picker_layout.rs` and `picker_style.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: the generic subset only (no tabs, side panel,
// hyperlinks, category tags or shortcuts); the description column is measured
// over all rows so it never jumps while scrolling; wrapped descriptions are
// painted by ratatui's `Paragraph` and measured with `text::wrapped_rows`;
// styles come from the `Theme`; overflow glyphs are configurable.

//! Rows shared by `select_list` and `multi_select`: a gutter, a name, an
//! aligned description column, and the window of rows that fits.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use unicode_segmentation::UnicodeSegmentation;

use crate::render::Renderable;
use crate::scroll::ScrollState;
use crate::text;
use crate::theme::Theme;

/// Below this many cells the description column moves under the list
/// (selected row only) instead of being squeezed.
pub(crate) const MIN_DESC_WIDTH: usize = 20;

/// One row, before styling.
#[derive(Debug, Clone, Default)]
pub(crate) struct Row {
    /// Gutter after the cursor: `1. `, `[x] `, blank padding. Spans carry
    /// their normal-state styles.
    pub gutter: Vec<Span<'static>>,
    pub name: String,
    /// Char indices into `name` to highlight (fuzzy matches).
    pub match_indices: Option<Vec<usize>>,
    /// Shown after the name in the secondary style: ` (current)`.
    pub suffix: Option<String>,
    pub description: Option<String>,
    pub disabled_reason: Option<String>,
    pub disabled: bool,
    /// Values for the meta columns at the right of the row.
    pub meta: Vec<String>,
}

impl Row {
    fn gutter_width(&self) -> usize {
        text::spans_width(&self.gutter)
    }

    fn name_width(&self) -> usize {
        text::width(&self.name)
            + self.suffix.as_deref().map_or(0, text::width)
            + if self.disabled_reason.is_some() || self.disabled {
                text::width(DISABLED_SUFFIX)
            } else {
                0
            }
    }

    /// What goes in the description column.
    pub(crate) fn description_text(&self) -> Option<String> {
        match (&self.description, &self.disabled_reason) {
            (Some(desc), Some(reason)) => Some(format!("{desc} (disabled: {reason})")),
            (Some(desc), None) => Some(desc.clone()),
            (None, Some(reason)) => Some(format!("disabled: {reason}")),
            (None, None) => None,
        }
    }
}

const DISABLED_SUFFIX: &str = " (disabled)";

/// The cursor and the overflow marks.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Marks {
    pub cursor: &'static str,
    pub more_above: &'static str,
    pub more_below: &'static str,
}

/// Wrapped descriptions or one line per row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowDisplay {
    /// Descriptions wrap under their column; a row can take several lines.
    #[default]
    Wrapped,
    /// One line per row; long descriptions end in `…`.
    SingleLine,
}

/// Column where descriptions start, or 0 when they move below the list.
/// Measured over every row (Codex `AutoAllRows`) so it holds still while
/// the list scrolls.
pub(crate) fn desc_col(rows: &[Row], width: u16, cursor_w: usize) -> usize {
    if width <= 1 || !rows.iter().any(|r| r.description_text().is_some()) {
        return 0;
    }
    let width = width as usize;
    // Leave at least 30% of the content width available for descriptions.
    let max_desc_col = width - 1;
    let max_auto_desc_col = max_desc_col.min((width * 70 / 100).max(1));
    let max_name = rows
        .iter()
        .map(|r| cursor_w + 1 + r.gutter_width() + r.name_width())
        .max()
        .unwrap_or(0);
    let col = (max_name + 2).min(max_auto_desc_col);
    if width.saturating_sub(col) < MIN_DESC_WIDTH {
        0
    } else {
        col
    }
}

/// Lines each row takes at `width`.
pub(crate) fn heights(rows: &[Row], desc_col: usize, width: u16, display: RowDisplay) -> Vec<u16> {
    rows.iter()
        .map(|r| match (display, desc_col, r.description_text()) {
            (RowDisplay::Wrapped, col, Some(d)) if col > 0 => {
                let w = (width as usize).saturating_sub(col) as u16;
                u16::try_from(text::wrapped_rows(&d, w)).unwrap_or(u16::MAX)
            }
            _ => 1,
        })
        .collect()
}

/// Lines needed to show the tallest run of `max_items` consecutive rows —
/// so the list's height does not change as the cursor moves.
pub(crate) fn stable_lines(heights: &[u16], max_items: usize) -> u16 {
    if heights.is_empty() {
        return 1;
    }
    let k = max_items.clamp(1, heights.len());
    let mut sum: u32 = heights[..k].iter().map(|h| u32::from(*h)).sum();
    let mut best = sum;
    for i in k..heights.len() {
        sum = sum + u32::from(heights[i]) - u32::from(heights[i - k]);
        best = best.max(sum);
    }
    u16::try_from(best).unwrap_or(u16::MAX)
}

/// First row in view: the scroll state's window, moved so the selected row
/// fits in `view_lines` given wrapped heights.
pub(crate) fn window_start(
    heights: &[u16],
    state: &ScrollState,
    max_items: usize,
    view_lines: u16,
) -> usize {
    let len = heights.len();
    if len == 0 || max_items == 0 {
        return 0;
    }
    let mut start = state.scroll_top.min(len - 1);
    let Some(sel) = state.selected_idx.map(|s| s.min(len - 1)) else {
        return start;
    };
    if sel < start {
        start = sel;
    } else if sel > start + max_items - 1 {
        start = sel + 1 - max_items;
    }
    // Wrapped rows can push the selection out of a line-based viewport.
    while start < sel {
        let used: u32 = heights[start..=sel].iter().map(|h| u32::from(*h)).sum();
        if used <= u32::from(view_lines) {
            break;
        }
        start += 1;
    }
    start
}

/// What a paint showed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Painted {
    pub items: usize,
    pub has_above: bool,
    pub has_below: bool,
}

/// Everything a paint needs besides the rows.
pub(crate) struct Paint<'a> {
    pub theme: &'a Theme,
    pub marks: Marks,
    pub display: RowDisplay,
    pub desc_col: usize,
    pub selected: Option<usize>,
    /// The meta columns shown at this width, left to right: each one's
    /// index in the list's columns (and so in a row's `meta`) and width.
    pub columns: Vec<(usize, u16)>,
}

/// Cells the meta columns take: each column and the gap before it.
pub(crate) fn meta_width(columns: &[(usize, u16)]) -> u16 {
    columns
        .iter()
        .fold(0u16, |w, (_, c)| w.saturating_add(c.saturating_add(1)))
}

/// The meta columns of one row: a gap, then each value padded or cut with
/// `…` to its column; `—` where a row with meta has no value for a column.
fn paint_meta(area: Rect, buf: &mut Buffer, row: &Row, selected: bool, p: &Paint) {
    if row.meta.is_empty() || area.is_empty() {
        return;
    }
    let t = p.theme;
    let dis = row.disabled;
    let mut spans = Vec::with_capacity(p.columns.len() * 2);
    // A hidden column leaves its value out: each shown column draws the
    // value at its own index, not at its place among the shown ones.
    for &(i, w) in &p.columns {
        spans.push(Span::raw(" "));
        let (value, part) = match row.meta.get(i) {
            Some(v) => (v.as_str(), Part::Secondary),
            None => ("—", Part::Keep(t.dim())),
        };
        spans.push(Span::styled(
            text::pad(value, usize::from(w), text::Align::Left),
            style(t, part, selected, dis),
        ));
    }
    text::truncate_line(Line::from(spans), area.width as usize)
        .render(Rect { height: 1, ..area }, buf);
}

/// Paint rows from `start` into `area` until it is full.
pub(crate) fn paint_rows(
    area: Rect,
    buf: &mut Buffer,
    rows: &[Row],
    heights: &[u16],
    start: usize,
    max_items: usize,
    p: &Paint,
) -> Painted {
    let mut y = area.y;
    let mut items = 0;
    let mut clipped = false;
    for (i, row) in rows.iter().enumerate().skip(start).take(max_items) {
        if y >= area.bottom() {
            break;
        }
        let want = heights.get(i).copied().unwrap_or(1).max(1);
        let h = want.min(area.bottom() - y);
        clipped |= h < want;
        paint_row(
            Rect::new(area.x, y, area.width, h),
            buf,
            row,
            p.selected == Some(i) && !row.disabled,
            p,
        );
        y += h;
        items += 1;
    }
    Painted {
        items,
        has_above: start > 0,
        has_below: clipped || start + items < rows.len(),
    }
}

/// Styles for one part of a row.
#[derive(Clone, Copy)]
enum Part {
    Text,
    Secondary,
    Match,
    Keep(Style),
}

fn style(theme: &Theme, part: Part, selected: bool, disabled: bool) -> Style {
    if disabled {
        return theme.dim();
    }
    if !selected {
        return match part {
            Part::Text => theme.text(),
            Part::Secondary => theme.secondary(),
            Part::Match => theme.accent().add_modifier(Modifier::BOLD),
            Part::Keep(s) => s,
        };
    }
    let sel = theme.selected();
    let reversed = sel.add_modifier.contains(Modifier::REVERSED);
    match part {
        Part::Text => sel,
        // Secondary text keeps its colour on a raised surface; a reversed
        // row reverses uniformly so it reads as one bar.
        Part::Secondary if !reversed => {
            sel.fg(theme.text_secondary).remove_modifier(Modifier::BOLD)
        }
        Part::Secondary => sel,
        Part::Match => sel.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        Part::Keep(s) => sel
            .add_modifier(s.add_modifier)
            .remove_modifier(Modifier::DIM),
    }
}

fn name_spans(row: &Row, limit: usize, theme: &Theme, sel: bool) -> Vec<Span<'static>> {
    let dis = row.disabled;
    let cut = text::width(&row.name) > limit;
    // Leave a cell for the ellipsis when the name does not fit.
    let budget = if cut { limit.saturating_sub(1) } else { limit };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    let mut matches = row.match_indices.iter().flatten().peekable();
    let mut char_idx = 0usize;
    let mut run = String::new();
    let mut run_matched = false;
    let mut flush = |run: &mut String, matched: bool| {
        if !run.is_empty() {
            let part = if matched { Part::Match } else { Part::Text };
            spans.push(Span::styled(
                std::mem::take(run),
                style(theme, part, sel, dis),
            ));
        }
    };
    for g in row.name.graphemes(true) {
        let w = text::width(g);
        if used + w > budget {
            break;
        }
        used += w;
        let mut matched = false;
        for _ in g.chars() {
            matched |= matches.next_if(|m| **m == char_idx).is_some();
            char_idx += 1;
        }
        if matched != run_matched {
            flush(&mut run, run_matched);
            run_matched = matched;
        }
        run.push_str(g);
    }
    flush(&mut run, run_matched);
    if cut && limit > 0 {
        spans.push(Span::styled(
            text::ELLIPSIS.to_string(),
            style(theme, Part::Text, sel, dis),
        ));
    }
    spans
}

fn paint_row(area: Rect, buf: &mut Buffer, row: &Row, selected: bool, p: &Paint) {
    let t = p.theme;
    let dis = row.disabled;
    if selected {
        buf.set_style(area, t.selected());
    }
    // Meta columns take the right end; the row's own parts get the rest.
    let meta_w = meta_width(&p.columns).min(area.width);
    if meta_w > 0 {
        let strip = Rect::new(area.right() - meta_w, area.y, meta_w, 1);
        paint_meta(strip, buf, row, selected, p);
    }
    let area = Rect {
        width: area.width - meta_w,
        ..area
    };
    let width = area.width as usize;
    let cursor = if selected { p.marks.cursor } else { "" };
    let cursor_w = text::width(p.marks.cursor);
    let mut spans = vec![Span::styled(
        format!("{}{} ", cursor, " ".repeat(cursor_w - text::width(cursor))),
        style(t, Part::Keep(t.accent()), selected, dis),
    )];
    for s in &row.gutter {
        spans.push(Span::styled(
            s.content.to_string(),
            style(t, Part::Keep(s.style), selected, dis),
        ));
    }
    let prefix_w = text::spans_width(&spans);
    let desc = row.description_text();
    let show_desc = p.desc_col > 0 && desc.is_some();
    let suffix_w = row.suffix.as_deref().map_or(0, text::width)
        + if dis { text::width(DISABLED_SUFFIX) } else { 0 };
    let name_room = if show_desc {
        p.desc_col.saturating_sub(2)
    } else {
        width
    }
    .saturating_sub(prefix_w + suffix_w);
    spans.extend(name_spans(row, name_room.max(1), t, selected));
    if let Some(suffix) = &row.suffix {
        spans.push(Span::styled(
            suffix.clone(),
            style(t, Part::Secondary, selected, dis),
        ));
    }
    if dis {
        spans.push(Span::styled(DISABLED_SUFFIX, t.dim()));
    }
    let first = Rect::new(area.x, area.y, area.width, 1);
    match (show_desc, desc, p.display) {
        (true, Some(d), RowDisplay::SingleLine) => {
            let used = text::spans_width(&spans);
            if used < p.desc_col {
                spans.push(Span::raw(" ".repeat(p.desc_col - used)));
            }
            spans.push(Span::styled(d, style(t, Part::Secondary, selected, dis)));
            text::truncate_line(Line::from(spans), width).render(first, buf);
        }
        (true, Some(d), RowDisplay::Wrapped) => {
            text::truncate_line(Line::from(spans), p.desc_col.min(width)).render(first, buf);
            let col = p.desc_col as u16;
            let desc_area = Rect::new(
                area.x + col,
                area.y,
                area.width.saturating_sub(col),
                area.height,
            );
            Paragraph::new(text::wrap_safe(&d).into_owned())
                .style(style(t, Part::Secondary, selected, dis))
                .wrap(Wrap { trim: false })
                .render(desc_area, buf);
        }
        _ => text::truncate_line(Line::from(spans), width).render(first, buf),
    }
}

/// `▴` / `▾` in the first cell of the spacer rows around the list, and
/// `N of M` at the right of the lower one, when rows are out of view.
pub(crate) fn paint_overflow(
    above: Rect,
    below: Rect,
    buf: &mut Buffer,
    painted: Painted,
    position: Option<(usize, usize)>,
    theme: &Theme,
    marks: Marks,
) {
    if painted.has_above && !above.is_empty() {
        Line::styled(marks.more_above, theme.accent()).render(above, buf);
    }
    if below.is_empty() {
        return;
    }
    if painted.has_below {
        Line::styled(marks.more_below, theme.accent()).render(below, buf);
    }
    if (painted.has_above || painted.has_below)
        && let Some((n, m)) = position
    {
        let label = format!("{n} of {m}");
        let w = text::width(&label) as u16;
        if w + 2 <= below.width {
            Line::styled(label, theme.dim())
                .render(Rect::new(below.right() - w, below.y, w, 1), buf);
        }
    }
}

/// Heights of the parts of a picker, in order.
pub(crate) struct PickerSizes {
    pub header: u16,
    pub header_gap: u16,
    pub search: u16,
    pub rows: u16,
}

/// Fit picker chrome before assigning result rows so short panels retain
/// their controls: `[header, gap, search, above, rows, below]`.
///
/// Optional gaps and the overflow spacer rows yield first; the header,
/// search and at least one result stay whenever the height allows.
pub(crate) fn picker_areas(area: Rect, sizes: PickerSizes) -> [Rect; 6] {
    let search = sizes.search.min(area.height);
    let minimum_rows = sizes.rows.min(3);
    // Keep the header when it fits alongside at least one result row.
    let minimum_header = u16::from(
        sizes.header > 0 && area.height.saturating_sub(search) > u16::from(sizes.rows > 0),
    );
    let header = sizes
        .header
        .min(area.height.saturating_sub(search + minimum_rows))
        .max(minimum_header)
        .min(area.height.saturating_sub(search));
    let mut remaining = area.height.saturating_sub(header + search);
    let indicators = u16::from(remaining >= minimum_rows + 2);
    remaining = remaining.saturating_sub(indicators * 2);
    let minimum_rows = sizes.rows.min(3).min(remaining);
    let header_gap = if header > 0 {
        sizes.header_gap.min(remaining.saturating_sub(minimum_rows))
    } else {
        0
    };
    remaining = remaining.saturating_sub(header_gap);
    let rows = sizes.rows.min(remaining);
    let mut y = area.y;
    [header, header_gap, search, indicators, rows, indicators].map(|height| {
        let rect = Rect::new(area.x, y, area.width, height);
        y = y.saturating_add(height);
        rect
    })
}

/// Header, search line and footer sit this many cells in from the left;
/// rows use the full width (the cursor takes the first cell).
pub(crate) const INSET: u16 = 2;

fn inset(r: Rect) -> Rect {
    Rect {
        x: r.x.saturating_add(INSET),
        width: r.width.saturating_sub(INSET),
        ..r
    }
}

/// A picker, top to bottom: header lines, a gap, an optional search line,
/// the rows between two overflow spacer rows, then tail lines (a note, the
/// key-hint footer). On a short screen the tail yields first — its first
/// lines first, so the footer goes last — then the spacers and the gap; the
/// header, search line and selected row stay as long as they fit.
pub(crate) struct Frame<'a> {
    pub header: Vec<Line<'static>>,
    pub search: Option<Line<'static>>,
    pub rows: &'a [Row],
    pub heights: &'a [u16],
    pub state: &'a ScrollState,
    pub max_rows: usize,
    pub paint: Paint<'a>,
    pub empty: Line<'static>,
    pub tail: Vec<Line<'static>>,
}

impl Frame<'_> {
    /// Rows the whole picker wants (the list at its tallest window).
    pub(crate) fn desired_height(&self) -> u16 {
        let header = self.header.len() as u16;
        header
            + u16::from(header > 0)
            + u16::from(self.search.is_some())
            + 2
            + stable_lines(self.heights, self.max_rows)
            + self.tail.len() as u16
    }

    /// Where the search line's text starts, if it is drawn.
    pub(crate) fn search_origin(&self, area: Rect) -> Option<(u16, u16)> {
        self.search.as_ref()?;
        let header = self.header.len() as u16;
        let y = area.y + header + u16::from(header > 0);
        (y < area.bottom()).then_some((area.x + INSET, y))
    }

    pub(crate) fn render(&self, area: Rect, buf: &mut Buffer) -> Painted {
        if area.is_empty() {
            return Painted::default();
        }
        let header_h = self.header.len() as u16;
        let search_h = u16::from(self.search.is_some());
        let minimum = header_h.min(1) + search_h + 1;
        let tail_h = (self.tail.len() as u16).min(area.height.saturating_sub(minimum));
        let body = Rect {
            height: area.height - tail_h,
            ..area
        };
        let [head_a, _, search_a, above, list_a, below] = picker_areas(
            body,
            PickerSizes {
                header: header_h,
                header_gap: 1,
                search: search_h,
                rows: stable_lines(self.heights, self.max_rows),
            },
        );
        for (y, line) in (head_a.y..head_a.bottom()).zip(&self.header) {
            Renderable::render(line, inset(Rect::new(area.x, y, area.width, 1)), buf);
        }
        if let Some(search) = &self.search
            && !search_a.is_empty()
        {
            Renderable::render(search, inset(search_a), buf);
        }
        let mut painted = Painted::default();
        if !list_a.is_empty() {
            if self.rows.is_empty() {
                Renderable::render(&self.empty, inset(list_a), buf);
            } else {
                let start = window_start(self.heights, self.state, self.max_rows, list_a.height);
                painted = paint_rows(
                    list_a,
                    buf,
                    self.rows,
                    self.heights,
                    start,
                    self.max_rows,
                    &self.paint,
                );
                let position = self.state.selected_idx.map(|s| (s + 1, self.rows.len()));
                paint_overflow(
                    above,
                    below,
                    buf,
                    painted,
                    position,
                    self.paint.theme,
                    self.paint.marks,
                );
            }
        }
        let skip = self.tail.len() - tail_h as usize;
        for (y, line) in (body.bottom()..area.bottom()).zip(self.tail.iter().skip(skip)) {
            Renderable::render(line, inset(Rect::new(area.x, y, area.width, 1)), buf);
        }
        painted
    }
}

/// A footer of key hints: keys in the accent style, labels dim.
pub(crate) fn hint_line(theme: &Theme, pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key.to_string(), theme.accent()));
        spans.push(Span::styled(format!(" {label}"), theme.dim()));
    }
    Line::from(spans)
}

/// The search line: `query▏` with a caret cell, or the placeholder.
pub(crate) fn search_line(
    theme: &Theme,
    label: Option<&'static str>,
    query: &str,
    placeholder: &str,
) -> Line<'static> {
    let mut spans = Vec::new();
    if let Some(label) = label {
        spans.push(Span::styled(label, theme.accent()));
    }
    if query.is_empty() {
        spans.push(Span::styled(" ", theme.selected()));
        spans.push(Span::styled(
            placeholder.to_string(),
            theme.dim().add_modifier(Modifier::ITALIC),
        ));
    } else {
        spans.push(Span::styled(query.to_string(), theme.text()));
        spans.push(Span::styled(" ", theme.selected()));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;

    fn row(name: &str, desc: Option<&str>) -> Row {
        Row {
            name: name.into(),
            description: desc.map(Into::into),
            ..Row::default()
        }
    }

    const MARKS: Marks = Marks {
        cursor: "›",
        more_above: "▴",
        more_below: "▾",
    };

    fn paint(rows: &[Row], w: u16, h: u16, display: RowDisplay, sel: Option<usize>) -> Buffer {
        let theme = Theme::ansi();
        let col = desc_col(rows, w, 1);
        let hs = heights(rows, col, w, display);
        let state = ScrollState {
            selected_idx: sel,
            scroll_top: 0,
        };
        let start = window_start(&hs, &state, rows.len(), h);
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        paint_rows(
            area,
            &mut buf,
            rows,
            &hs,
            start,
            rows.len(),
            &Paint {
                theme: &theme,
                marks: MARKS,
                display,
                desc_col: col,
                selected: sel,
                columns: Vec::new(),
            },
        );
        buf
    }

    // Upstream: selection_popup_common.rs `exact_fit_description_reserves_only_the_rendered_row`.
    #[test]
    fn exact_fit_description_takes_one_row() {
        let rows = [row("/model", Some("fits exactly"))];
        let col = desc_col(&rows, 40, 1);
        assert_eq!(col, 10);
        assert_eq!(heights(&rows, col, 40, RowDisplay::Wrapped), vec![1]);
    }

    #[test]
    fn descriptions_align_in_one_column_and_wrap_under_it() {
        let rows = [
            row("opus", Some("deepest reasoning, slowest of the three")),
            row("haiku-long-name", Some("fastest")),
        ];
        let buf = paint(&rows, 46, 3, RowDisplay::Wrapped, Some(0));
        assert_eq!(
            snapshot::lines(&buf),
            vec![
                "› opus             deepest reasoning, slowest",
                "                   of the three",
                "  haiku-long-name  fastest",
            ]
        );
    }

    #[test]
    fn narrow_widths_move_descriptions_out_of_the_row() {
        let rows = [row("日 本 語", Some("Details"))];
        assert_eq!(desc_col(&rows, 29, 1), 0);
        assert!(desc_col(&rows, 32, 1) > 0);
        let buf = paint(&rows, 29, 1, RowDisplay::Wrapped, Some(0));
        assert_eq!(snapshot::text(&buf), "› 日 本 語");
    }

    // Upstream: selection_row_layout.rs name truncation counts cells.
    #[test]
    fn names_are_cut_in_cells_with_an_ellipsis() {
        let long = "가나다라마바사아자차카타파하".repeat(3);
        let rows = [row(&long, Some("description text")), row("x", None)];
        let buf = paint(&rows, 80, 2, RowDisplay::SingleLine, None);
        // The column is capped at 70% of 80 = 56; the name gets 56 − 2 gap
        // − 2 gutter = 52 cells: 25 two-cell syllables and the ellipsis.
        assert_eq!(desc_col(&rows, 80, 1), 56);
        let line = &snapshot::lines(&buf)[0];
        assert!(line.starts_with("  가나다"), "{line}");
        assert_eq!(buf[(52, 0)].symbol(), "…");
        assert_eq!(buf[(56, 0)].symbol(), "d");
    }

    #[test]
    fn disabled_rows_say_why_and_are_dim() {
        let rows = [Row {
            disabled_reason: Some("needs a Max plan".into()),
            disabled: true,
            ..row("fable", None)
        }];
        let buf = paint(&rows, 60, 1, RowDisplay::SingleLine, Some(0));
        let text = snapshot::text(&buf);
        assert!(text.contains("fable (disabled)"), "{text}");
        assert!(text.contains("disabled: needs a Max plan"), "{text}");
        assert_eq!(buf[(2, 0)].fg, Theme::ansi().muted);
        // A disabled row is never drawn selected.
        assert_eq!(buf[(0, 0)].symbol(), " ");
    }

    #[test]
    fn matched_graphemes_are_highlighted_without_moving_the_column() {
        let rows = [Row {
            match_indices: Some(vec![1, 3]),
            ..row("한글xe\u{301}", Some("description"))
        }];
        let buf = paint(&rows, 40, 1, RowDisplay::SingleLine, None);
        assert!(!buf[(2, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(buf[(4, 0)].symbol(), "글");
        assert!(buf[(4, 0)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(6, 0)].modifier.contains(Modifier::BOLD));
        // "e" + combining acute is two chars, one grapheme, one cell.
        assert!(buf[(7, 0)].modifier.contains(Modifier::BOLD));
        let col = desc_col(&rows, 40, 1);
        assert_eq!(col, 10);
        assert_eq!(buf[(col as u16, 0)].symbol(), "d");
    }

    #[test]
    fn window_keeps_wrapped_selection_in_view() {
        let hs = [3, 3, 3, 3];
        let state = ScrollState {
            selected_idx: Some(2),
            scroll_top: 0,
        };
        // 4 lines fit one 3-line row: the window starts at the selection.
        assert_eq!(window_start(&hs, &state, 4, 4), 2);
        assert_eq!(window_start(&hs, &state, 4, 9), 0);
        assert_eq!(stable_lines(&[1, 3, 1, 2], 2), 4);
        assert_eq!(stable_lines(&[], 8), 1);
    }

    // Upstream: multi_select_picker.rs `short_picker_preserves_selected_row_when_overflow_spacers_yield`
    // (the layout half: spacer rows yield before the search line and the rows).
    #[test]
    fn short_pickers_drop_spacers_before_rows() {
        let sizes = |h| {
            picker_areas(
                Rect::new(0, 0, 10, h),
                PickerSizes {
                    header: 0,
                    header_gap: 1,
                    search: 1,
                    rows: 8,
                },
            )
            .map(|r| r.height)
        };
        assert_eq!(sizes(1), [0, 0, 1, 0, 0, 0]);
        assert_eq!(sizes(3), [0, 0, 1, 0, 2, 0]);
        assert_eq!(sizes(5), [0, 0, 1, 0, 4, 0]);
        assert_eq!(sizes(6), [0, 0, 1, 1, 3, 1]);
        assert_eq!(sizes(12), [0, 0, 1, 1, 8, 1]);
    }
}
