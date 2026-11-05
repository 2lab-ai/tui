//! A packed grid: many short records on few lines, each field lined up in
//! its column.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab), where the same packer was
//! written twice: the accounts fold (`fold_columns`, `fold_entry_width`,
//! `fold_line_width`, `fold_cells`, `draw_fold`, `FoldField`) and the models
//! strip (`strip_columns`, `strip_line_width`, `strip_fits`,
//! `strip_per_line`, `draw_models_strip_packed`). Entries go row-major,
//! `per_line` to a line; column `c` holds entries `c`, `c + per_line`, …, and
//! each field is as wide as the widest one in its column (or its header).
//! Fields sit one cell apart, entries three. The packer picks the most
//! entries per line that fit.
//!
//! One packer now, and what each copy had on its own: right-aligned fields,
//! a header line and a reversed cursor entry from the fold; a click rect per
//! entry from the fold; the widest-form bound from both (`FoldBounds`,
//! `strip_last_bound`), so a ticking value never re-packs the grid or moves
//! a column. Rows that do not fit end in `+N more` instead of being cut.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::text::{self, Align};
use crate::theme::Theme;

/// Blank cells between two entries on a line (llmux `FOLD_ENTRY_GAP`,
/// `STRIP_ENTRY_GAP`).
pub const ENTRY_GAP: usize = 3;

/// One field of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field<'a> {
    pub spans: Vec<Span<'a>>,
    pub align: Align,
    /// Cells of the widest form this value can take (`99 in-flight`,
    /// `23h 59m`); the column is measured on it, so a tick never moves
    /// anything.
    pub widest: usize,
}

impl<'a> Field<'a> {
    pub fn new(content: impl Into<Line<'a>>) -> Field<'a> {
        Field {
            spans: content.into().spans,
            align: Align::Left,
            widest: 0,
        }
    }

    pub fn right(mut self) -> Field<'a> {
        self.align = Align::Right;
        self
    }

    /// Reserve the width of `sample`, the widest text this field can show.
    pub fn widest(mut self, sample: &str) -> Field<'a> {
        self.widest = self.widest.max(text::width(sample));
        self
    }

    /// Cells the column must hold for this field.
    pub fn width(&self) -> usize {
        text::spans_width(&self.spans).max(self.widest)
    }
}

/// How many entries go on a line, and each entry column's field widths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridPlan {
    pub per_line: usize,
    /// `columns[c][f]`: width of field `f` in entry column `c`.
    pub columns: Vec<Vec<usize>>,
}

impl GridPlan {
    /// Cells one entry takes in column `c`.
    pub fn entry_width(&self, c: usize) -> usize {
        entry_width(&self.columns[c])
    }

    /// Cells a full line takes.
    pub fn line_width(&self) -> usize {
        line_width(&self.columns)
    }
}

/// The rect an entry was drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridHit {
    pub rect: Rect,
    pub index: usize,
}

/// Which entry a click lands on.
pub fn hit_test(hits: &[GridHit], col: u16, row: u16) -> Option<usize> {
    hits.iter()
        .find(|h| {
            row >= h.rect.y && row < h.rect.bottom() && col >= h.rect.x && col < h.rect.right()
        })
        .map(|h| h.index)
}

/// What one render drew.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GridLayout {
    pub hits: Vec<GridHit>,
    pub per_line: usize,
    /// Entries not drawn for want of rows (said on the last row).
    pub hidden: usize,
    /// No packing fit the width: one entry a line, each clipped with `…`.
    pub clipped: bool,
}

/// Width of one entry: its fields and a one-cell gap between each pair.
fn entry_width(column: &[usize]) -> usize {
    column.iter().sum::<usize>() + column.len().saturating_sub(1)
}

/// Width of a line: its entries and the gaps between them.
fn line_width(columns: &[Vec<usize>]) -> usize {
    columns.iter().map(|c| entry_width(c)).sum::<usize>()
        + ENTRY_GAP * columns.len().saturating_sub(1)
}

/// Field widths per entry column when `per_line` entries share a line
/// (llmux `fold_columns` / `strip_columns`).
pub fn columns(widths: &[Vec<usize>], headers: &[usize], per_line: usize) -> Vec<Vec<usize>> {
    let fields = widths
        .iter()
        .map(Vec::len)
        .chain([headers.len()])
        .max()
        .unwrap_or(0);
    (0..per_line.min(widths.len()).max(1))
        .map(|c| {
            (0..fields)
                .map(|f| {
                    widths
                        .iter()
                        .skip(c)
                        .step_by(per_line.max(1))
                        .map(|w| w.get(f).copied().unwrap_or(0))
                        .max()
                        .unwrap_or(0)
                        .max(headers.get(f).copied().unwrap_or(0))
                })
                .collect()
        })
        .collect()
}

/// The most entries per line (from `min` up to all of them) whose aligned
/// line fits `width` (llmux `strip_per_line`).
pub fn per_line(
    widths: &[Vec<usize>],
    headers: &[usize],
    width: usize,
    min: usize,
) -> Option<usize> {
    (min.max(1)..=widths.len())
        .rev()
        .find(|&k| line_width(&columns(widths, headers, k)) <= width)
}

/// Short records packed onto few lines.
#[derive(Debug, Clone)]
pub struct PackedGrid<'a> {
    headers: Vec<Cow<'a, str>>,
    entries: Vec<Vec<Field<'a>>>,
    cursor: Option<usize>,
    min_per_line: usize,
    theme: Theme,
}

impl<'a> PackedGrid<'a> {
    pub fn new(entries: impl IntoIterator<Item = Vec<Field<'a>>>) -> PackedGrid<'a> {
        PackedGrid {
            headers: Vec::new(),
            entries: entries.into_iter().collect(),
            cursor: None,
            min_per_line: 1,
            theme: Theme::default(),
        }
    }

    /// One word per field, drawn dim over every entry column.
    pub fn headers<S: Into<Cow<'a, str>>>(mut self, headers: impl IntoIterator<Item = S>) -> Self {
        self.headers = headers.into_iter().map(Into::into).collect();
        self
    }

    /// The entry under the cursor, drawn selected.
    pub fn cursor(mut self, index: Option<usize>) -> Self {
        self.cursor = index;
        self
    }

    /// Fewer entries a line than this and [`PackedGrid::plan`] says no
    /// (llmux's strip falls back to its table below two).
    pub fn min_per_line(mut self, n: usize) -> Self {
        self.min_per_line = n.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn widths(&self) -> (Vec<Vec<usize>>, Vec<usize>) {
        let widths = self
            .entries
            .iter()
            .map(|e| e.iter().map(Field::width).collect())
            .collect();
        let headers = self.headers.iter().map(|h| text::width(h)).collect();
        (widths, headers)
    }

    /// The packing at `width` cells, or `None` when fewer than
    /// `min_per_line` entries fit a line (or there are none).
    pub fn plan(&self, width: u16) -> Option<GridPlan> {
        let (widths, headers) = self.widths();
        let k = per_line(&widths, &headers, width as usize, self.min_per_line)?;
        Some(GridPlan {
            per_line: k,
            columns: columns(&widths, &headers, k),
        })
    }

    /// Rows the whole grid takes at `width`: the header line and one line
    /// per `per_line` entries.
    pub fn height(&self, width: u16) -> u16 {
        let k = self.plan(width).map_or(1, |p| p.per_line);
        let lines = self.entries.len().div_ceil(k) + usize::from(!self.headers.is_empty());
        lines.min(u16::MAX as usize) as u16
    }

    /// Lay one entry's (or the header's) fields out at the column widths
    /// (llmux `fold_cells`); `selected` paints the whole entry.
    fn cells(
        &self,
        fields: &[(Vec<Span<'a>>, Align)],
        widths: &[usize],
        selected: bool,
    ) -> Vec<Span<'a>> {
        let paint = |style: Style| {
            if selected {
                style.patch(self.theme.selected())
            } else {
                style
            }
        };
        let blank = |n: usize| Span::styled(" ".repeat(n), paint(Style::new()));
        let mut out = Vec::new();
        for (f, &width) in widths.iter().enumerate() {
            if f > 0 {
                out.push(blank(1));
            }
            let (spans, align) = fields.get(f).cloned().unwrap_or((Vec::new(), Align::Left));
            let fill = width.saturating_sub(text::spans_width(&spans));
            let (l, r) = match align {
                Align::Left => (0, fill),
                Align::Right => (fill, 0),
                Align::Center => (fill / 2, fill - fill / 2),
            };
            if l > 0 {
                out.push(blank(l));
            }
            out.extend(
                spans
                    .into_iter()
                    .map(|s| Span::styled(s.content, paint(s.style))),
            );
            if r > 0 {
                out.push(blank(r));
            }
        }
        out
    }

    /// Draw from the top of `area` and return a rect per drawn entry.
    pub fn render_layout(&self, area: Rect, buf: &mut Buffer) -> GridLayout {
        let mut out = GridLayout::default();
        if area.is_empty() || self.entries.is_empty() {
            return out;
        }
        let t = &self.theme;
        let (widths, headers) = self.widths();
        let width = area.width as usize;
        let (k, cols) = match per_line(&widths, &headers, width, self.min_per_line) {
            Some(k) => (k, columns(&widths, &headers, k)),
            None => {
                out.clipped = true;
                (1, columns(&widths, &headers, 1))
            }
        };
        out.per_line = k;
        let gap = || Span::raw(" ".repeat(ENTRY_GAP));
        let mut y = area.y;
        let header = !self.headers.is_empty() && area.height >= 2;
        if header {
            let first = &self.entries[0];
            let mut spans = Vec::new();
            for (c, w) in cols.iter().enumerate() {
                if c > 0 {
                    spans.push(gap());
                }
                let fields: Vec<(Vec<Span>, Align)> = (0..w.len())
                    .map(|f| {
                        let word = self.headers.get(f).cloned().unwrap_or_default();
                        let align = first.get(f).map_or(Align::Left, |x| x.align);
                        (vec![Span::styled(word, t.dim())], align)
                    })
                    .collect();
                spans.extend(self.cells(&fields, w, false));
            }
            self.put(Line::from(spans), area, y, buf);
            y += 1;
        }
        let rows = (area.bottom() - y) as usize;
        let lines = self.entries.len().div_ceil(k);
        // Every entry fits, or the last row says how many did not.
        let shown_lines = if lines <= rows {
            lines
        } else {
            rows.saturating_sub(1)
        };
        for (i, chunk) in self.entries.chunks(k).take(shown_lines).enumerate() {
            let mut spans = Vec::new();
            let mut x = 0usize;
            for (c, entry) in chunk.iter().enumerate() {
                if c > 0 {
                    spans.push(gap());
                    x += ENTRY_GAP;
                }
                let index = i * k + c;
                let fields: Vec<(Vec<Span>, Align)> =
                    entry.iter().map(|f| (f.spans.clone(), f.align)).collect();
                spans.extend(self.cells(&fields, &cols[c], self.cursor == Some(index)));
                let ew = entry_width(&cols[c]);
                if x < width {
                    out.hits.push(GridHit {
                        rect: Rect::new(area.x + x as u16, y, ew.min(width - x) as u16, 1),
                        index,
                    });
                }
                x += ew;
            }
            self.put(Line::from(spans), area, y, buf);
            y += 1;
        }
        let shown = (shown_lines * k).min(self.entries.len());
        out.hidden = self.entries.len() - shown;
        if out.hidden > 0 && y < area.bottom() {
            let more = Line::from(Span::styled(format!("+{} more", out.hidden), t.dim()));
            self.put(more, area, y, buf);
        }
        out
    }

    /// Draw and return a rect per drawn entry.
    pub fn render_hits(&self, area: Rect, buf: &mut Buffer) -> Vec<GridHit> {
        self.render_layout(area, buf).hits
    }

    fn put(&self, mut line: Line<'a>, area: Rect, y: u16, buf: &mut Buffer) {
        // Padding after the last field is not content: drop it, so a line
        // only ends in `…` when a value was cut.
        while line
            .spans
            .last()
            .is_some_and(|s| s.style == Style::new() && s.content.bytes().all(|b| b == b' '))
        {
            line.spans.pop();
        }
        let line = text::truncate_line(into_static(line), area.width as usize);
        buf.set_line(area.x, y, &line, area.width);
    }
}

fn into_static(line: Line<'_>) -> Line<'static> {
    Line::from(
        line.spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect::<Vec<_>>(),
    )
}

impl Widget for PackedGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_layout(area, buf);
    }
}

impl Widget for &PackedGrid<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_layout(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Modifier;

    /// Port of llmux `strip_per_line_packs_the_most_entries_that_fit`: the
    /// same field widths, the same answers (llmux's strip trimmed the marker
    /// past the first column; here every column is measured alike, so the
    /// breakpoints are one cell later).
    #[test]
    fn packs_the_most_entries_that_fit() {
        let entry = vec![2, 6, 15, 10, 6, 9, 9, 11];
        let five = vec![entry.clone(); 5];
        // 68 cells of fields + 7 gaps = 75; two entries are 75 + 3 + 75.
        assert_eq!(line_width(&columns(&five, &[], 2)), 153);
        assert_eq!(per_line(&five, &[], 179, 2), Some(2));
        assert_eq!(per_line(&five, &[], 153, 2), Some(2));
        assert_eq!(per_line(&five, &[], 152, 2), None, "below two: no packing");
        assert_eq!(per_line(&five, &[], 152, 1), Some(1));
        assert_eq!(per_line(&five, &[], 231, 2), Some(3));
        assert_eq!(per_line(&five, &[], 230, 2), Some(2));
        assert_eq!(
            per_line(&five[..1], &[], 400, 2),
            None,
            "one entry never packs"
        );
        // Columns align per entry column: a long name in column 0 does not
        // widen column 1.
        let mut long = five.clone();
        long[2][2] = 30;
        let cols = columns(&long, &[], 2);
        assert_eq!((cols[0][2], cols[1][2]), (30, 15));
        // A header wider than every value widens its field.
        let cols = columns(&five, &[0, 0, 20], 2);
        assert_eq!((cols[0][2], cols[1][2]), (20, 20));
    }

    fn account(n: usize, name: &str, reset: &str, pct: &str) -> Vec<Field<'static>> {
        vec![
            Field::new(n.to_string()).right(),
            Field::new(name.to_string()),
            Field::new(reset.to_string()).widest("99d 23h"),
            Field::new(pct.to_string()).right(),
        ]
    }

    fn accounts() -> Vec<Vec<Field<'static>>> {
        vec![
            account(1, "ai3@iq.io", "2d 4h", "100%"),
            account(2, "zhuge@2lab.ai", "5h 12m", "99%"),
            account(3, "dev1@2lab.ai", "6d 1h", "100%"),
            account(10, "ai2@iq.io", "41m", "99%"),
            account(11, "simayi@2lab.ai", "3d", "100%"),
        ]
    }

    fn render(grid: &PackedGrid, w: u16, h: u16) -> (Buffer, GridLayout) {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        let layout = grid.render_layout(buf.area, &mut buf);
        (buf, layout)
    }

    #[test]
    fn fields_line_up_down_each_column_under_a_header() {
        let grid = PackedGrid::new(accounts())
            .headers(["#", "account", "7d reset", "7d"])
            .theme(Theme::ansi());
        let plan = grid.plan(80).expect("fits");
        assert_eq!(plan.per_line, 2);
        let (buf, layout) = render(&grid, 80, 5);
        assert_eq!(
            snapshot::lines(&buf),
            [
                " # account        7d reset   7d    # account       7d reset  7d",
                " 1 ai3@iq.io      2d 4h    100%    2 zhuge@2lab.ai 5h 12m   99%",
                " 3 dev1@2lab.ai   6d 1h    100%   10 ai2@iq.io     41m      99%",
                "11 simayi@2lab.ai 3d       100%",
                "",
            ]
        );
        assert_eq!(layout.per_line, 2);
        assert_eq!(layout.hidden, 0);
        assert_eq!(grid.height(80), 4);
        // Each entry column is as wide as its own widest entry (column 1
        // has no `simayi@2lab.ai`); one rect per entry, as wide as that.
        let rects: Vec<(usize, u16, u16, u16)> = layout
            .hits
            .iter()
            .map(|h| (h.index, h.rect.x, h.rect.y, h.rect.width))
            .collect();
        assert_eq!(
            rects,
            vec![
                (0, 0, 1, 31),
                (1, 34, 1, 29),
                (2, 0, 2, 31),
                (3, 34, 2, 29),
                (4, 0, 3, 31)
            ]
        );
        assert_eq!(hit_test(&layout.hits, 40, 2), Some(3));
        assert_eq!(hit_test(&layout.hits, 32, 2), None, "the gap");
    }

    /// llmux `fold_geometry_does_not_follow_the_clock` and
    /// `strip_packing_survives_the_last_column_changing`: a value that
    /// ticks inside its widest form moves nothing.
    #[test]
    fn a_ticking_value_moves_nothing() {
        let at = |reset: &str| {
            let mut e = accounts();
            e[0][2] = Field::new(reset.to_string()).widest("99d 23h");
            let grid = PackedGrid::new(e).headers(["#", "account", "7d reset", "7d"]);
            (grid.plan(80), render(&grid, 80, 5).1.hits)
        };
        let (plan_a, hits_a) = at("9m");
        let (plan_b, hits_b) = at("23h 59m");
        assert_eq!(plan_a, plan_b);
        assert_eq!(hits_a, hits_b);
    }

    #[test]
    fn wide_names_are_measured_in_cells() {
        let mut e = accounts();
        e[1][1] = Field::new("계정관리자@2lab.ai");
        let grid = PackedGrid::new(e).headers(["#", "account", "7d reset", "7d"]);
        let (buf, layout) = render(&grid, 80, 5);
        let rows = snapshot::lines(&buf);
        for row in &rows {
            assert!(text::width(row) <= 80, "{row}");
        }
        // Column 1 widened by the 18-cell name; both entries still align.
        assert_eq!(layout.per_line, 2);
        assert!(rows[1].contains("계정관리자@2lab.ai 5h 12m"), "{rows:?}");
        assert!(rows[2].contains("ai2@iq.io          41m"), "{rows:?}");
    }

    #[test]
    fn too_few_rows_say_how_many_entries_are_hidden() {
        let grid = PackedGrid::new(accounts()).headers(["#", "account", "7d reset", "7d"]);
        let (buf, layout) = render(&grid, 40, 3);
        let rows = snapshot::lines(&buf);
        assert_eq!(layout.per_line, 1);
        assert_eq!(rows[2], "+4 more");
        assert_eq!(layout.hidden, 4);
        assert_eq!(layout.hits.len(), 1);
    }

    #[test]
    fn too_narrow_clips_each_entry_and_says_so() {
        let grid = PackedGrid::new(accounts()).min_per_line(1);
        let (buf, layout) = render(&grid, 12, 6);
        assert!(layout.clipped);
        for row in snapshot::lines(&buf).iter().take(5) {
            assert!(row.ends_with('…'), "{row}");
            assert!(text::width(row) <= 12);
        }
        assert!(layout.hits.iter().all(|h| h.rect.right() <= 12));
        // Down to one cell: no panic.
        for (w, h) in [(1, 1), (2, 1), (1, 3), (12, 2)] {
            render(&grid, w, h);
        }
        assert_eq!(PackedGrid::new(accounts()).min_per_line(2).plan(40), None);
    }

    #[test]
    fn the_cursor_entry_is_selected_edge_to_edge() {
        let grid = PackedGrid::new(accounts())
            .cursor(Some(3))
            .theme(Theme::mono());
        let (buf, layout) = render(&grid, 80, 4);
        let r = layout.hits.iter().find(|h| h.index == 3).unwrap().rect;
        for x in r.x..r.right() {
            assert!(buf[(x, r.y)].modifier.contains(Modifier::REVERSED), "{x}");
        }
        assert!(!buf[(r.x - 1, r.y)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(0, r.y)].modifier.contains(Modifier::REVERSED));
    }
}
