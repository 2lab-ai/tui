// Ported from OpenAI Codex `codex-rs/tui/src/pager_overlay/scrolling.rs` and
// its tests `scrolling_tests.rs`
// (https://github.com/openai/codex, commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72),
// Apache-2.0. Modified by 2lab: public, documented.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::Renderable;

/// Paint `renderable` scrolled down by `scroll_offset` rows into `area`, and
/// return how many rows it painted (0 when the offset is past its end).
///
/// Uses the renderable's own [`Renderable::render_scrolled`] when it has one;
/// otherwise renders the rows above and in view into a scratch buffer and
/// copies the visible ones, styles and wide glyphs included. This is how any
/// renderable becomes scrollable.
pub fn render_offset_content(
    area: Rect,
    buf: &mut Buffer,
    renderable: &dyn Renderable,
    scroll_offset: u16,
) -> u16 {
    let height = renderable.desired_height(area.width);
    let copy_height = area.height.min(height.saturating_sub(scroll_offset));
    if copy_height == 0 {
        return 0;
    }

    let visible_area = Rect::new(area.x, area.y, area.width, copy_height);
    if renderable.render_scrolled(visible_area, buf, scroll_offset) {
        return copy_height;
    }

    let mut tall_buf = Buffer::empty(Rect::new(
        0,
        0,
        area.width,
        scroll_offset.saturating_add(copy_height),
    ));
    renderable.render(*tall_buf.area(), &mut tall_buf);
    for y in 0..copy_height {
        let src_y = y + scroll_offset;
        for x in 0..area.width {
            buf[(area.x + x, area.y + y)] = tall_buf[(x, src_y)].clone();
        }
    }

    copy_height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{CachedRenderable, InsetRenderable, Insets};
    use ratatui::style::Stylize;
    use ratatui::text::{Line, Text};
    use ratatui::widgets::{Paragraph, Wrap};

    // Upstream: scrolling_tests.rs
    #[test]
    fn scrolled_static_renderables_match_clipped_buffer_slices() {
        let lines = vec![
            Line::from(vec![
                "prefix ".green(),
                "漢字 ｶﾞ ".bold(),
                "styled words that wrap across rows".blue(),
            ]),
            Line::default(),
            Line::from("last 漢字 ｶﾞ row".red()),
        ];
        for width in [5, 7, 13, 28] {
            let paragraph = || Paragraph::new(Text::from(lines.clone())).wrap(Wrap { trim: false });
            let renderables: Vec<Box<dyn Renderable>> = vec![
                Box::new(paragraph()),
                Box::new(CachedRenderable::new(paragraph())),
                Box::new(InsetRenderable::new(
                    Box::new(CachedRenderable::new(paragraph())) as Box<dyn Renderable>,
                    Insets::tlbr(2, 1, 1, 1),
                )),
            ];
            for renderable in renderables {
                let height = renderable.desired_height(width);
                for offset in [0, 1, 2, 3, height.saturating_sub(1), height] {
                    for (x, y, visible_height) in [(0, 0, 4), (3, 2, 3)] {
                        let area = Rect::new(x, y, width, visible_height);
                        let canvas = Rect::new(0, 0, area.right(), area.bottom());
                        let mut expected = Buffer::empty(canvas);
                        let mut actual = Buffer::empty(canvas);
                        let expected_height = visible_height.min(height.saturating_sub(offset));
                        let full_area = Rect::new(0, 0, width, offset + expected_height);
                        let mut full = Buffer::empty(full_area);
                        renderable.render(full_area, &mut full);
                        for row in 0..expected_height {
                            for column in 0..width {
                                expected[(x + column, y + row)] =
                                    full[(column, offset + row)].clone();
                            }
                        }
                        let actual_height =
                            render_offset_content(area, &mut actual, &*renderable, offset);
                        assert_eq!(
                            (actual_height, actual),
                            (expected_height, expected),
                            "width={width}, offset={offset}, area={area:?}"
                        );
                    }
                }
            }
        }
    }

    // Upstream: scrolling_tests.rs
    #[test]
    fn fallback_handles_offsets_near_maximum_height() {
        struct MaximumHeightRenderable;

        impl Renderable for MaximumHeightRenderable {
            fn render(&self, area: Rect, buf: &mut Buffer) {
                let last_row = area.bottom().saturating_sub(1);
                buf[(area.x, last_row)].set_symbol("x");
            }

            fn desired_height(&self, _width: u16) -> u16 {
                u16::MAX
            }
        }

        let area = Rect::new(0, 0, 1, 2);
        let mut actual = Buffer::empty(area);
        let mut expected = Buffer::empty(area);
        expected[(area.x, area.y)].set_symbol("x");
        let height =
            render_offset_content(area, &mut actual, &MaximumHeightRenderable, u16::MAX - 1);

        assert_eq!((height, actual), (1, expected));
    }
}
