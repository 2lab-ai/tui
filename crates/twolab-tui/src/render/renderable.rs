// Ported from OpenAI Codex `codex-rs/tui/src/render/renderable.rs` and the
// `CachedRenderable` in `codex-rs/tui/src/pager_overlay.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: no crossterm `cursor_style`; flex factors are
// `u16`; one-row leaves cut with `…`; leaf impls for `Text` and a measured
// `Paragraph`; `RowRenderable`, `Width` and `WidgetRenderable` are new;
// `CachedRenderable` is generic.

use std::cell::Cell;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};

use super::{Insets, RectExt as _};
use crate::text;

/// Something that can say how tall it wants to be at a width, then paint.
///
/// `desired_height` must be a pure function of `width` (and the value's own
/// state), so parents can measure first and paint second.
pub trait Renderable {
    /// Paint into `area`. Never paint outside it.
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Rows wanted at `width`.
    fn desired_height(&self, width: u16) -> u16;

    /// Renders visible rows after `scroll_offset` when direct scrolling is supported.
    ///
    /// Implementations returning `false` must leave `buf` unchanged so callers can use their
    /// existing full-height rendering fallback. Supporting wrappers must forward this method.
    fn render_scrolled(&self, _area: Rect, _buf: &mut Buffer, _scroll_offset: u16) -> bool {
        false
    }

    /// Where the terminal cursor goes when this renderable is painted at
    /// `area`, if it has one (a text field).
    fn cursor_pos(&self, _area: Rect) -> Option<(u16, u16)> {
        None
    }
}

/// A child owned by its container or borrowed from the host.
pub enum RenderableItem<'a> {
    Owned(Box<dyn Renderable + 'a>),
    Borrowed(&'a dyn Renderable),
}

impl<'a> RenderableItem<'a> {
    /// Box `r` as an owned child.
    pub fn owned(r: impl Renderable + 'a) -> Self {
        RenderableItem::Owned(Box::new(r))
    }

    /// Borrow `r` for the container's lifetime.
    pub fn borrowed(r: &'a dyn Renderable) -> Self {
        RenderableItem::Borrowed(r)
    }

    fn get(&self) -> &dyn Renderable {
        match self {
            RenderableItem::Owned(child) => child.as_ref(),
            RenderableItem::Borrowed(child) => *child,
        }
    }
}

impl Renderable for RenderableItem<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.get().render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.get().desired_height(width)
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.get().render_scrolled(area, buf, scroll_offset)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.get().cursor_pos(area)
    }
}

impl<'a> From<Box<dyn Renderable + 'a>> for RenderableItem<'a> {
    fn from(value: Box<dyn Renderable + 'a>) -> Self {
        RenderableItem::Owned(value)
    }
}

impl<'a> From<&'a dyn Renderable> for RenderableItem<'a> {
    fn from(value: &'a dyn Renderable) -> Self {
        RenderableItem::Borrowed(value)
    }
}

impl<'a, R> From<R> for Box<dyn Renderable + 'a>
where
    R: Renderable + 'a,
{
    fn from(value: R) -> Self {
        Box::new(value)
    }
}

impl Renderable for () {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
    fn desired_height(&self, _width: u16) -> u16 {
        0
    }
}

/// One row; cut with `…` at the right edge (no wrapping).
impl Renderable for &str {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Widget::render(
            text::truncate(self, area.width as usize).as_ref(),
            area,
            buf,
        );
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// One row; cut with `…` at the right edge (no wrapping).
impl Renderable for String {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_str().render(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// One row; cut with `…` at the right edge (no wrapping).
impl Renderable for Span<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if text::width(&self.content) <= area.width as usize {
            Widget::render(self, area, buf);
        } else {
            let cut = text::truncate(&self.content, area.width as usize).into_owned();
            Widget::render(Span::styled(cut, self.style), area, buf);
        }
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// One row; cut with `…` at the right edge (no wrapping).
impl Renderable for Line<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        match fit_line(self, area.width) {
            Some(cut) => Widget::render(&cut, area, buf),
            None => Widget::render(self, area, buf),
        }
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

/// One row per line, each cut with `…` at the right edge (ratatui's `Text`
/// widget does not wrap).
impl Renderable for Text<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if self
            .lines
            .iter()
            .all(|l| text::line_width(l) <= area.width as usize)
        {
            Widget::render(self, area, buf);
            return;
        }
        let lines: Vec<Line<'static>> = self
            .lines
            .iter()
            .map(|l| fit_line(l, area.width).unwrap_or_else(|| owned_line(l)))
            .collect();
        let mut cut = Text::from(lines).style(self.style);
        cut.alignment = self.alignment;
        Widget::render(&cut, area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }
}

fn owned_line(line: &Line<'_>) -> Line<'static> {
    let spans: Vec<Span<'static>> = line
        .spans
        .iter()
        .map(|s| Span::styled(s.content.to_string(), s.style))
        .collect();
    let mut l = Line::from(spans).style(line.style);
    l.alignment = line.alignment;
    l
}

/// `line` cut to `width` cells with `…`, or `None` when it already fits.
fn fit_line(line: &Line<'_>, width: u16) -> Option<Line<'static>> {
    (text::line_width(line) > width as usize)
        .then(|| text::truncate_line(owned_line(line), width as usize))
}

/// Rows up to the last one the paragraph paints at `width`.
///
/// Codex asks ratatui (`Paragraph::line_count`), which ratatui keeps behind
/// its `unstable-rendered-line-info` feature. Without it the paragraph's text
/// is private, so the height is measured by painting into a scratch buffer:
/// exact for wrapped and unwrapped text, except that trailing blank lines are
/// not reserved, and a paragraph with a `Block` measures as tall as the
/// largest probe (4096 rows) because its border follows the area. Pad with
/// [`InsetRenderable`] instead of a block, or use [`Text`] / your own
/// renderable when the height must be exact.
impl Renderable for Paragraph<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        Widget::render(self, area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        painted_rows(self, width)
    }
}

/// Probe heights for [`Paragraph`]: stop once the painted rows end in the
/// top half of the probe.
const PARAGRAPH_PROBES: [u16; 4] = [64, 256, 1024, 4096];

fn painted_rows(p: &Paragraph, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    let mut rows = 0;
    for h in PARAGRAPH_PROBES {
        let area = Rect::new(0, 0, width, h);
        let mut buf = Buffer::empty(area);
        Widget::render(p, area, &mut buf);
        rows = (0..h)
            .rev()
            .find(|&y| (0..width).any(|x| buf[(x, y)].symbol() != " "))
            .map_or(0, |y| y + 1);
        if rows <= h / 2 {
            break;
        }
    }
    rows
}

impl<R: Renderable> Renderable for Option<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(renderable) = self {
            renderable.render(area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().map_or(0, |r| r.desired_height(width))
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.as_ref()
            .is_some_and(|r| r.render_scrolled(area, buf, scroll_offset))
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_ref()
            .and_then(|renderable| renderable.cursor_pos(area))
    }
}

impl<R: Renderable + ?Sized> Renderable for Arc<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().desired_height(width)
    }
    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.as_ref().render_scrolled(area, buf, scroll_offset)
    }
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_ref().cursor_pos(area)
    }
}

/// Children stacked top to bottom, each at its desired height, clipped to
/// the area.
pub struct ColumnRenderable<'a> {
    children: Vec<RenderableItem<'a>>,
}

impl Default for ColumnRenderable<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderable for ColumnRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for child in &self.children {
            if y >= area.bottom() {
                break;
            }
            let child_area = Rect::new(area.x, y, area.width, child.desired_height(area.width))
                .intersection(area);
            if !child_area.is_empty() {
                child.render(child_area, buf);
            }
            y += child_area.height;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children.iter().fold(0u16, |h, child| {
            h.saturating_add(child.desired_height(width))
        })
    }

    /// Returns the cursor position of the first child that has a cursor position, offset by the
    /// child's position in the column.
    ///
    /// It is generally assumed that either zero or one child will have a cursor position.
    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        let mut y = area.y;
        for child in &self.children {
            let child_area = Rect::new(area.x, y, area.width, child.desired_height(area.width))
                .intersection(area);
            if !child_area.is_empty()
                && let Some((px, py)) = child.cursor_pos(child_area)
            {
                return Some((px, py));
            }
            y += child_area.height;
        }
        None
    }
}

impl<'a> ColumnRenderable<'a> {
    pub fn new() -> Self {
        Self { children: vec![] }
    }

    pub fn with<I, T>(children: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<RenderableItem<'a>>,
    {
        Self {
            children: children.into_iter().map(Into::into).collect(),
        }
    }

    pub fn push(&mut self, child: impl Into<Box<dyn Renderable + 'a>>) {
        self.children.push(RenderableItem::Owned(child.into()));
    }

    /// Builder form of [`ColumnRenderable::push`].
    pub fn child(mut self, child: impl Into<Box<dyn Renderable + 'a>>) -> Self {
        self.push(child);
        self
    }
}

/// One child of a [`FlexRenderable`] and its flex factor.
pub struct FlexChild<'a> {
    flex: u16,
    child: RenderableItem<'a>,
    cached_height: Cell<Option<(u16, u16)>>,
}

/// Lays out children in a column, with the ability to specify a flex factor for each child.
///
/// Children with flex factor > 0 will be allocated the remaining space after the non-flex children,
/// proportional to the flex factor — but never more than they want: a flex
/// child shorter than its share gives the rest back to the others.
pub struct FlexRenderable<'a> {
    children: Vec<FlexChild<'a>>,
}

impl Default for FlexRenderable<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> FlexRenderable<'a> {
    pub fn new() -> Self {
        Self { children: vec![] }
    }

    /// Add a child. `flex == 0` takes its desired height first; `flex > 0`
    /// shares what is left.
    pub fn push(&mut self, flex: u16, child: impl Into<RenderableItem<'a>>) {
        self.children.push(FlexChild {
            flex,
            child: child.into(),
            cached_height: Cell::new(None),
        });
    }

    /// Builder form of [`FlexRenderable::push`] for an owned child.
    pub fn child(mut self, flex: u16, child: impl Renderable + 'a) -> Self {
        self.push(flex, RenderableItem::owned(child));
        self
    }

    /// Loosely inspired by Flutter's Flex widget.
    ///
    /// Ref https://github.com/flutter/flutter/blob/3fd81edbf1e015221e143c92b2664f4371bdc04a/packages/flutter/lib/src/rendering/flex.dart#L1205-L1209
    fn allocate(&self, area: Rect) -> Vec<Rect> {
        let mut allocated_rects = Vec::with_capacity(self.children.len());
        let mut child_sizes = vec![0u16; self.children.len()];
        let mut allocated_size: u16 = 0;
        let mut flex_children = Vec::new();

        // 1. Allocate space to non-flex children.
        let max_size = area.height;
        for (i, child) in self.children.iter().enumerate() {
            let desired_height = if let Some((width, height)) = child.cached_height.get()
                && width == area.width
            {
                height
            } else {
                let height = child.child.desired_height(area.width);
                child.cached_height.set(Some((area.width, height)));
                height
            };
            if child.flex > 0 {
                flex_children.push((i, u32::from(child.flex), desired_height));
            } else {
                child_sizes[i] = desired_height.min(max_size.saturating_sub(allocated_size));
                allocated_size += child_sizes[i];
            }
        }
        let free_space = max_size.saturating_sub(allocated_size);
        // 2. Satisfy flex children that need less than their proportional share so their unused
        // space can be redistributed instead of leaving blank rows.
        let mut remaining_space = free_space;
        while !flex_children.is_empty() {
            let total_flex = flex_children.iter().map(|(_, flex, _)| *flex).sum::<u32>();
            let mut satisfied_any = false;
            flex_children.retain(|(i, flex, desired_height)| {
                let proportional_share =
                    (u32::from(remaining_space) * *flex / total_flex.max(1)) as u16;
                if *desired_height <= proportional_share {
                    child_sizes[*i] = *desired_height;
                    remaining_space = remaining_space.saturating_sub(*desired_height);
                    satisfied_any = true;
                    false
                } else {
                    true
                }
            });
            if !satisfied_any {
                break;
            }
        }
        // 3. Divide the remaining space proportionally. The final child absorbs rounding slack.
        let total_flex = flex_children.iter().map(|(_, flex, _)| *flex).sum::<u32>();
        let mut allocated_flex_space: u16 = 0;
        let last_flex_child_idx = flex_children.last().map(|(i, _, _)| *i);
        for (i, flex, desired_height) in flex_children {
            let max_child_extent = if Some(i) == last_flex_child_idx {
                remaining_space.saturating_sub(allocated_flex_space)
            } else {
                (u32::from(remaining_space) * flex / total_flex.max(1)) as u16
            };
            let child_size = desired_height.min(max_child_extent);
            child_sizes[i] = child_size;
            allocated_flex_space += child_size;
        }

        let mut y = area.y;
        for size in child_sizes {
            let child_area = Rect::new(area.x, y, area.width, size);
            allocated_rects.push(child_area);
            y = y.saturating_add(child_area.height);
        }
        allocated_rects
    }
}

impl Renderable for FlexRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.allocate(area)
            .into_iter()
            .zip(self.children.iter())
            .for_each(|(rect, child)| {
                let rect = rect.intersection(area);
                if !rect.is_empty() {
                    child.child.render(rect, buf);
                }
            });
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.allocate(Rect::new(0, 0, width, u16::MAX))
            .last()
            .map(|rect| rect.bottom())
            .unwrap_or(0)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.allocate(area)
            .into_iter()
            .zip(self.children.iter())
            .find_map(|(rect, child)| child.child.cursor_pos(rect))
    }
}

/// A child padded by [`Insets`]. Its desired height includes the padding.
pub struct InsetRenderable<'a> {
    child: RenderableItem<'a>,
    insets: Insets,
}

impl Renderable for InsetRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let inner = area.inset(self.insets);
        if !inner.is_empty() {
            self.child.render(inner, buf);
        }
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.child
            .desired_height(
                width
                    .saturating_sub(self.insets.left)
                    .saturating_sub(self.insets.right),
            )
            .saturating_add(self.insets.top)
            .saturating_add(self.insets.bottom)
    }

    /// Preserve clipped inset padding while forwarding only visible child rows.
    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        let top_padding = self.insets.top.saturating_sub(scroll_offset);
        let child_width = area
            .width
            .saturating_sub(self.insets.left.saturating_add(self.insets.right));
        if child_width == 0 || top_padding >= area.height {
            return true;
        }

        let child_offset = scroll_offset.saturating_sub(self.insets.top);
        // The fallback applies bottom padding to its clipped scratch buffer, even mid-scroll.
        let child_height = self
            .child
            .desired_height(child_width)
            .saturating_sub(child_offset)
            .min(
                area.height
                    .saturating_sub(top_padding)
                    .saturating_sub(self.insets.bottom),
            );
        if child_height == 0 {
            return true;
        }

        let child_area = Rect::new(
            area.x.saturating_add(self.insets.left),
            area.y.saturating_add(top_padding),
            child_width,
            child_height,
        );
        if child_offset == 0 {
            self.child.render(child_area, buf);
            true
        } else {
            self.child.render_scrolled(child_area, buf, child_offset)
        }
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.child.cursor_pos(area.inset(self.insets))
    }
}

impl<'a> InsetRenderable<'a> {
    pub fn new(child: impl Into<RenderableItem<'a>>, insets: Insets) -> Self {
        Self {
            child: child.into(),
            insets,
        }
    }
}

/// `.inset(..)` on anything renderable.
pub trait RenderableExt<'a> {
    /// The value padded by `insets`.
    fn inset(self, insets: Insets) -> RenderableItem<'a>;
}

impl<'a, R> RenderableExt<'a> for R
where
    R: Renderable + 'a,
{
    fn inset(self, insets: Insets) -> RenderableItem<'a> {
        let child: RenderableItem<'a> =
            RenderableItem::Owned(Box::new(self) as Box<dyn Renderable + 'a>);
        RenderableItem::Owned(Box::new(InsetRenderable { child, insets }))
    }
}

/// How wide a [`RowRenderable`] column is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Width {
    /// Exactly this many cells (fewer when the row runs out).
    Cells(u16),
    /// A share of what the fixed columns leave, by factor.
    Flex(u16),
}

/// Children side by side, left to right — the horizontal counterpart Codex
/// does without. Fixed columns are placed first, flexible ones share the
/// rest by factor (the last absorbs rounding), and `gap` blank cells sit
/// between neighbours. The row wants the height of its tallest child; every
/// child is painted at the full row height.
pub struct RowRenderable<'a> {
    children: Vec<(Width, RenderableItem<'a>)>,
    gap: u16,
}

impl Default for RowRenderable<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> RowRenderable<'a> {
    pub fn new() -> Self {
        Self {
            children: vec![],
            gap: 0,
        }
    }

    /// Blank cells between neighbouring children.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    pub fn push(&mut self, width: Width, child: impl Into<Box<dyn Renderable + 'a>>) {
        self.children
            .push((width, RenderableItem::Owned(child.into())));
    }

    /// Builder form of [`RowRenderable::push`].
    pub fn child(mut self, width: Width, child: impl Into<Box<dyn Renderable + 'a>>) -> Self {
        self.push(width, child);
        self
    }

    /// One rect per child, in order. Children that get no cells have an
    /// empty rect.
    fn allocate(&self, area: Rect) -> Vec<Rect> {
        let n = self.children.len();
        if n == 0 {
            return Vec::new();
        }
        let gaps = self
            .gap
            .saturating_mul(u16::try_from(n - 1).unwrap_or(u16::MAX));
        let mut free = area.width.saturating_sub(gaps);
        let mut widths = vec![0u16; n];
        let mut total_flex = 0u32;
        for (i, (w, _)) in self.children.iter().enumerate() {
            match *w {
                Width::Cells(c) => {
                    widths[i] = c.min(free);
                    free -= widths[i];
                }
                Width::Flex(f) => total_flex += u32::from(f),
            }
        }
        let last_flex = self
            .children
            .iter()
            .rposition(|(w, _)| matches!(w, Width::Flex(f) if *f > 0));
        let mut given = 0u16;
        for (i, (w, _)) in self.children.iter().enumerate() {
            if let Width::Flex(f) = *w
                && f > 0
            {
                widths[i] = if Some(i) == last_flex {
                    free.saturating_sub(given)
                } else {
                    (u32::from(free) * u32::from(f) / total_flex.max(1)) as u16
                };
                given += widths[i];
            }
        }
        let mut x = area.x;
        widths
            .into_iter()
            .map(|w| {
                let r = Rect::new(x, area.y, w, area.height).intersection(area);
                x = x.saturating_add(w).saturating_add(self.gap);
                r
            })
            .collect()
    }
}

impl Renderable for RowRenderable<'_> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        for (rect, (_, child)) in self.allocate(area).into_iter().zip(&self.children) {
            if !rect.is_empty() {
                child.render(rect, buf);
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.allocate(Rect::new(0, 0, width, 1))
            .into_iter()
            .zip(&self.children)
            .filter(|(rect, _)| rect.width > 0)
            .map(|(rect, (_, child))| child.desired_height(rect.width))
            .max()
            .unwrap_or(0)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.allocate(area)
            .into_iter()
            .zip(&self.children)
            .filter(|(rect, _)| !rect.is_empty())
            .find_map(|(rect, (_, child))| child.cursor_pos(rect))
    }
}

/// Any ratatui `Widget + Clone` at a fixed height — a `Gauge`, a
/// `Sparkline`, a bordered `Block`. The widget is cloned for each paint,
/// since ratatui widgets consume themselves.
#[derive(Clone, Debug)]
pub struct WidgetRenderable<W> {
    widget: W,
    height: u16,
}

impl<W: Widget + Clone> WidgetRenderable<W> {
    pub fn new(widget: W, height: u16) -> Self {
        Self { widget, height }
    }
}

impl<W: Widget + Clone> Renderable for WidgetRenderable<W> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.widget.clone().render(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        self.height
    }
}

/// A renderable that caches its desired height for the last width asked —
/// for children whose measurement wraps a lot of text (a pager body).
pub struct CachedRenderable<R> {
    renderable: R,
    cache: Cell<Option<(u16, u16)>>,
}

impl<R: Renderable> CachedRenderable<R> {
    pub fn new(renderable: R) -> Self {
        Self {
            renderable,
            cache: Cell::new(None),
        }
    }

    pub fn get_ref(&self) -> &R {
        &self.renderable
    }

    pub fn into_inner(self) -> R {
        self.renderable
    }
}

impl<R: Renderable> Renderable for CachedRenderable<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable.render(area, buf);
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.renderable.render_scrolled(area, buf, scroll_offset)
    }

    fn desired_height(&self, width: u16) -> u16 {
        match self.cache.get() {
            Some((w, h)) if w == width => h,
            _ => {
                let h = self.renderable.desired_height(width);
                self.cache.set(Some((width, h)));
                h
            }
        }
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.renderable.cursor_pos(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::widgets::{Block, Borders, Wrap};

    struct HeightRenderable(u16);

    impl HeightRenderable {
        fn with_height(height: u16) -> Self {
            Self(height)
        }
    }

    impl Renderable for HeightRenderable {
        fn render(&self, _area: Rect, _buf: &mut Buffer) {}

        fn desired_height(&self, _width: u16) -> u16 {
            self.0
        }
    }

    // Upstream: renderable_tests.rs
    #[test]
    fn flex_redistributes_space_unused_by_short_children() {
        let mut flex = FlexRenderable::new();
        flex.push(
            1,
            RenderableItem::Owned(Box::new(HeightRenderable::with_height(20))),
        );
        flex.push(
            1,
            RenderableItem::Owned(Box::new(HeightRenderable::with_height(2))),
        );

        let allocated = flex.allocate(Rect::new(0, 0, 80, 10));

        assert_eq!(
            allocated
                .into_iter()
                .map(|area| area.height)
                .collect::<Vec<_>>(),
            vec![8, 2],
        );
    }

    // Upstream: renderable_tests.rs
    #[test]
    fn flex_reserves_non_flex_space_before_flexible_children() {
        let mut flex = FlexRenderable::new();
        flex.push(
            1,
            RenderableItem::Owned(Box::new(HeightRenderable::with_height(20))),
        );
        flex.push(
            0,
            RenderableItem::Owned(Box::new(HeightRenderable::with_height(2))),
        );
        flex.push(
            1,
            RenderableItem::Owned(Box::new(HeightRenderable::with_height(20))),
        );

        let allocated = flex.allocate(Rect::new(0, 0, 80, 10));

        assert_eq!(
            allocated
                .into_iter()
                .map(|area| area.height)
                .collect::<Vec<_>>(),
            vec![4, 2, 4],
        );
    }

    // Upstream: renderable_tests.rs (cursor_style dropped).
    #[test]
    fn flex_caches_child_height_across_frame_passes() {
        struct CountingRenderable<'a>(&'a Cell<usize>);

        impl Renderable for CountingRenderable<'_> {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}

            fn desired_height(&self, _width: u16) -> u16 {
                self.0.set(self.0.get() + 1);
                1
            }

            fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
                Some((area.x, area.y))
            }
        }

        let calls = Cell::new(0);
        let renderable = CountingRenderable(&calls);
        let mut flex = FlexRenderable::new();
        flex.push(1, RenderableItem::Borrowed(&renderable));
        let area = Rect::new(0, 0, 80, 10);
        let mut buf = Buffer::empty(area);

        assert_eq!(flex.desired_height(area.width), 1);
        flex.render(area, &mut buf);
        assert_eq!(flex.cursor_pos(area), Some((0, 0)));
        assert_eq!(calls.get(), 1);

        assert_eq!(flex.desired_height(100), 1);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn column_stacks_at_desired_heights_and_clips() {
        let col = ColumnRenderable::new()
            .child("header")
            .child(Text::from(vec![Line::from("a"), Line::from("b")]))
            .child(Line::from("footer"));
        assert_eq!(col.desired_height(10), 4);
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        col.render(area, &mut buf);
        assert_eq!(snapshot::text(&buf), "header\na\nb");
    }

    #[test]
    fn one_row_leaves_say_when_they_are_cut() {
        let area = Rect::new(0, 0, 6, 1);
        let mut buf = Buffer::empty(area);
        // Widget::render would win method resolution on a value; name the trait.
        Renderable::render(&"resets in 1h", area, &mut buf);
        assert_eq!(snapshot::text(&buf), "reset…");
        let mut buf = Buffer::empty(area);
        Renderable::render(
            &Line::from(vec![Span::raw("ab "), Span::raw("한글입니다")]),
            area,
            &mut buf,
        );
        assert_eq!(snapshot::text(&buf), "ab 한…");
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 2));
        Renderable::render(
            &Text::from(vec![Line::from("short"), Line::from("much longer")]),
            Rect::new(0, 0, 6, 2),
            &mut buf,
        );
        assert_eq!(snapshot::text(&buf), "short\nmuch …");
    }

    #[test]
    fn inset_pads_and_measures() {
        let item = "hi".inset(Insets::tlbr(1, 2, 1, 0));
        assert_eq!(item.desired_height(10), 3);
        let area = Rect::new(0, 0, 6, 3);
        let mut buf = Buffer::empty(area);
        item.render(area, &mut buf);
        assert_eq!(snapshot::lines(&buf), vec!["", "  hi", ""]);
    }

    #[test]
    fn row_places_fixed_then_flex_with_gaps() {
        let row = RowRenderable::new()
            .gap(1)
            .child(Width::Cells(4), "left")
            .child(Width::Flex(1), "mid")
            .child(Width::Flex(1), "right");
        let rects = row.allocate(Rect::new(0, 0, 20, 1));
        let xs: Vec<(u16, u16)> = rects.iter().map(|r| (r.x, r.width)).collect();
        // 20 − 2 gaps − 4 fixed = 14, shared 7/7.
        assert_eq!(xs, vec![(0, 4), (5, 7), (13, 7)]);
        let area = Rect::new(0, 0, 20, 1);
        let mut buf = Buffer::empty(area);
        row.render(area, &mut buf);
        assert_eq!(snapshot::text(&buf), "left mid     right");
    }

    #[test]
    fn row_wants_its_tallest_child_and_survives_tiny_widths() {
        let tall = Text::from(vec![Line::from("1"), Line::from("2"), Line::from("3")]);
        let row = RowRenderable::new()
            .child(Width::Cells(3), "x")
            .child(Width::Flex(1), tall);
        assert_eq!(row.desired_height(10), 3);
        // Only the fixed column fits: the flex child gets nothing.
        assert_eq!(row.desired_height(3), 1);
        let area = Rect::new(0, 0, 1, 1);
        let mut buf = Buffer::empty(area);
        row.render(area, &mut buf);
        assert_eq!(row.desired_height(0), 0);
    }

    #[test]
    fn paragraph_height_follows_wrapping() {
        let p = Paragraph::new("one two three four five six").wrap(Wrap { trim: false });
        assert_eq!(p.desired_height(80), 1);
        // "one two" / "three" / "four five" / "six"
        assert_eq!(p.desired_height(9), 4);
        let unwrapped = Paragraph::new("a\n\nc");
        assert_eq!(unwrapped.desired_height(1), 3);
        assert_eq!(p.desired_height(0), 0);
    }

    #[test]
    fn paragraph_height_grows_past_the_first_probe() {
        let text: Vec<Line> = (0..300).map(|i| Line::from(format!("row {i}"))).collect();
        let p = Paragraph::new(text);
        assert_eq!(p.desired_height(10), 300);
    }

    #[test]
    fn widget_adapter_paints_at_fixed_height() {
        let w = WidgetRenderable::new(Block::new().borders(Borders::TOP).title("t"), 1);
        assert_eq!(w.desired_height(30), 1);
        let area = Rect::new(0, 0, 5, 1);
        let mut buf = Buffer::empty(area);
        w.render(area, &mut buf);
        assert_eq!(snapshot::text(&buf), "t────");
    }

    #[test]
    fn cached_renderable_measures_once_per_width() {
        struct Count<'a>(&'a Cell<usize>);
        impl Renderable for Count<'_> {
            fn render(&self, _area: Rect, _buf: &mut Buffer) {}
            fn desired_height(&self, width: u16) -> u16 {
                self.0.set(self.0.get() + 1);
                width / 2
            }
        }
        let calls = Cell::new(0);
        let c = CachedRenderable::new(Count(&calls));
        assert_eq!(c.desired_height(10), 5);
        assert_eq!(c.desired_height(10), 5);
        assert_eq!(calls.get(), 1);
        assert_eq!(c.desired_height(4), 2);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn option_and_arc_forward() {
        let none: Option<&str> = None;
        assert_eq!(none.desired_height(5), 0);
        let some = Some("x");
        assert_eq!(some.desired_height(5), 1);
        let shared: Arc<dyn Renderable> = Arc::new(Line::from("y"));
        assert_eq!(shared.desired_height(5), 1);
        assert_eq!(().desired_height(5), 0);
    }
}
