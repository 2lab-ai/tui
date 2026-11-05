//! The catalog page for [`crate::render`]: columns, flex, rows and insets
//! composing real content.
//!
//! There is no widget type here — the page shows the composition model that
//! `select_list`, `multi_select` and `pager` are built on, ported from OpenAI
//! Codex (`codex-rs/tui/src/render/renderable.rs`, Apache-2.0). It tells the person nothing by itself; it decides who gets
//! the rows when there are not enough: fixed parts (a header, a key-hint
//! footer) keep theirs, flexible parts (a body) share the rest, and a part
//! that wants less than its share gives the remainder back.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::{Line, Text};

    use crate::render::{
        ColumnRenderable, FlexRenderable, Insets, Renderable, RenderableExt, RenderableItem,
        RowRenderable, Width,
    };
    use crate::snapshot;

    fn lines(prefix: &str, n: usize) -> Text<'static> {
        Text::from(
            (1..=n)
                .map(|i| Line::from(format!("{prefix}{i}")))
                .collect::<Vec<_>>(),
        )
    }

    fn card(body_rows: usize) -> FlexRenderable<'static> {
        let mut card = FlexRenderable::new();
        card.push(0, RenderableItem::owned(Line::from("title")));
        card.push(1, lines("body ", body_rows).inset(Insets::vh(0, 2)));
        card.push(0, RenderableItem::owned(Line::from("footer")));
        card
    }

    fn draw(r: &dyn Renderable, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        r.render(area, &mut buf);
        snapshot::lines(&buf)
    }

    #[test]
    fn header_and_footer_keep_their_rows_when_the_body_is_squeezed() {
        let c = card(6);
        assert_eq!(c.desired_height(20), 8);
        assert_eq!(
            draw(&c, 20, 5),
            vec!["title", "  body 1", "  body 2", "  body 3", "footer"]
        );
    }

    #[test]
    fn short_body_leaves_spare_rows_below_the_footer() {
        let c = card(2);
        assert_eq!(
            draw(&c, 20, 6),
            vec!["title", "  body 1", "  body 2", "footer", "", ""]
        );
    }

    #[test]
    fn a_row_of_columns_inside_a_column() {
        let row = RowRenderable::new()
            .gap(2)
            .child(Width::Cells(6), lines("a", 3))
            .child(Width::Flex(1), lines("b", 2));
        let col = ColumnRenderable::new().child("head").child(row);
        assert_eq!(col.desired_height(20), 4);
        assert_eq!(
            draw(&col, 20, 4),
            vec!["head", "a1      b1", "a2      b2", "a3"]
        );
    }

    #[test]
    fn every_container_survives_one_cell() {
        let c = card(3);
        let _ = draw(&c, 1, 1);
        let _ = draw(&c, 0, 0);
        let row = RowRenderable::new()
            .gap(3)
            .child(Width::Cells(9), "x")
            .child(Width::Flex(2), "y");
        let _ = draw(&row, 1, 1);
    }
}
