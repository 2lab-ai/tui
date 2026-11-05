// Ported from OpenAI Codex `codex-rs/tui/src/render/mod.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: public fields' constructors are `const`, the
// highlight and theme modules stay behind, docs added.

//! Measure-then-paint composition: the [`Renderable`] trait and the
//! containers that stack, flex, pad and scroll anything that implements it.
//!
//! Ported from OpenAI Codex (`codex-rs/tui/src/render/`, Apache-2.0), where
//! every overlay, popup and pager is a tree of renderables. A renderable says
//! how many rows it wants at a width ([`Renderable::desired_height`]) and
//! paints into the rect it is given, so a parent can stack children without a
//! layout solver:
//!
//! - [`ColumnRenderable`] stacks children at their desired heights.
//! - [`FlexRenderable`] stacks them too, but gives leftover rows to children
//!   with a flex factor — a body between a fixed header and footer.
//! - [`RowRenderable`] (new here; Codex has none) puts children side by side
//!   in fixed or flexible columns.
//! - [`InsetRenderable`] (or `.inset(..)` from [`RenderableExt`]) pads a child.
//! - [`render_offset_content`] scrolls any renderable by `n` rows, through the
//!   child's own [`Renderable::render_scrolled`] or a scratch buffer.
//! - [`CachedRenderable`] remembers a child's height per width.
//! - [`WidgetRenderable`] lets any ratatui `Widget + Clone` take part.
//!
//! Leaf impls cover `()`, `&str`, `String`, `Span`, `Line`, `Text`,
//! `Paragraph`, `Option<R>` and `Arc<R>`. The one-row leaves end in `…` when
//! the width cuts them (Codex clips silently). Codex's crossterm-only
//! `cursor_style` method is dropped: the library has no terminal backend.
//!
//! On a plain value ratatui's consuming `Widget::render` wins method
//! resolution over [`Renderable::render`]; write `Renderable::render(&x, ..)`
//! or put the value in a container.

mod renderable;
mod scrolling;

use ratatui::layout::Rect;

pub use renderable::{
    CachedRenderable, ColumnRenderable, FlexChild, FlexRenderable, InsetRenderable, Renderable,
    RenderableExt, RenderableItem, RowRenderable, WidgetRenderable, Width,
};
pub use scrolling::render_offset_content;

/// Padding around a child, in cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
}

impl Insets {
    /// No padding.
    pub const ZERO: Insets = Insets::vh(0, 0);

    /// Top, left, bottom, right — Codex's argument order.
    pub const fn tlbr(top: u16, left: u16, bottom: u16, right: u16) -> Self {
        Self {
            top,
            left,
            bottom,
            right,
        }
    }

    /// `v` rows above and below, `h` cells left and right.
    pub const fn vh(v: u16, h: u16) -> Self {
        Self {
            top: v,
            left: h,
            bottom: v,
            right: h,
        }
    }

    pub const fn top(&self) -> u16 {
        self.top
    }

    pub const fn left(&self) -> u16 {
        self.left
    }

    pub const fn bottom(&self) -> u16 {
        self.bottom
    }

    pub const fn right(&self) -> u16 {
        self.right
    }
}

/// `Rect::inset` with [`Insets`] (ratatui's own `Rect::inner` takes a
/// symmetric `Margin`).
pub trait RectExt {
    /// The rect shrunk by `insets`, saturating at zero size.
    fn inset(&self, insets: Insets) -> Rect;
}

impl RectExt for Rect {
    fn inset(&self, insets: Insets) -> Rect {
        let horizontal = insets.left.saturating_add(insets.right);
        let vertical = insets.top.saturating_add(insets.bottom);
        Rect {
            x: self.x.saturating_add(insets.left),
            y: self.y.saturating_add(insets.top),
            width: self.width.saturating_sub(horizontal),
            height: self.height.saturating_sub(vertical),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_saturates() {
        let r = Rect::new(2, 3, 10, 4);
        assert_eq!(r.inset(Insets::vh(1, 2)), Rect::new(4, 4, 6, 2));
        assert_eq!(r.inset(Insets::tlbr(9, 9, 9, 9)), Rect::new(11, 12, 0, 0));
        assert_eq!(r.inset(Insets::ZERO), r);
    }
}
