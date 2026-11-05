use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::render::{
    CachedRenderable, ColumnRenderable, FlexRenderable, Insets, Renderable, RenderableExt,
    RenderableItem, RowRenderable, Width,
};
use crate::text;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::render::{FlexRenderable, Insets, Renderable, RenderableExt, RenderableItem};
use ratatui::widgets::{Paragraph, Wrap};

// Header and footer keep their rows; the body gets what is left.
let mut card = FlexRenderable::new();
card.push(0, RenderableItem::owned(title_line));
card.push(1, Paragraph::new(body).wrap(Wrap { trim: false }).inset(Insets::vh(1, 0)));
card.push(0, RenderableItem::owned(hint_line));

let height = card.desired_height(area.width); // measure…
card.render(area, buf);                       // …then paint"#;

/// Lines clipped to the width with `…`, one row each.
struct Lines(Vec<Line<'static>>);

impl Renderable for Lines {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        for (y, line) in (area.y..area.bottom()).zip(&self.0) {
            text::truncate_line(line.clone(), area.width as usize)
                .render(Rect::new(area.x, y, area.width, 1), buf);
        }
    }
    fn desired_height(&self, _width: u16) -> u16 {
        u16::try_from(self.0.len()).unwrap_or(u16::MAX)
    }
}

/// Paints its child; when the child wants more rows than it got, the last
/// row says how many were cut instead of dropping them silently.
struct Clipped<R> {
    inner: R,
    theme: Theme,
}

impl<R: Renderable> Renderable for Clipped<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let want = self.inner.desired_height(area.width);
        if want <= area.height {
            self.inner.render(area, buf);
            return;
        }
        if area.height == 0 {
            return;
        }
        let shown = area.height - 1;
        if shown > 0 {
            self.inner.render(
                Rect {
                    height: shown,
                    ..area
                },
                buf,
            );
        }
        let note = format!("+{} more rows", want - shown);
        Line::styled(
            text::truncate(&note, area.width as usize).into_owned(),
            self.theme.dim(),
        )
        .render(Rect::new(area.x, area.y + shown, area.width, 1), buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.inner.desired_height(width)
    }
}

fn hints(t: &Theme, pairs: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, (key, label)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(*key, t.accent()));
        spans.push(Span::styled(format!(" {label}"), t.dim()));
    }
    Line::from(spans)
}

const BODY: &str = "Routing 3 sessions. Last request 4 s ago: claude-opus-5-5, 18.2k tokens in, 1.1k out, 6.4 s. No cooldown. The 7-day window resets on Monday at 09:00; at this week's pace it ends near 61%.";

/// The account card: a header row, a subtitle, a body that flexes, a footer.
fn account_card(t: Theme) -> RenderableItem<'static> {
    let header = RowRenderable::new()
        .gap(2)
        .child(
            Width::Flex(1),
            Line::styled("zhuge@2lab.ai", t.text().add_modifier(Modifier::BOLD)),
        )
        .child(
            Width::Cells(7),
            Line::styled("Max 20x", t.accent()).right_aligned(),
        );
    let body = Clipped {
        inner: CachedRenderable::new(
            Paragraph::new(BODY)
                .style(t.text())
                .wrap(Wrap { trim: false }),
        ),
        theme: t,
    };
    let card = FlexRenderable::new()
        .child(0, header)
        .child(
            0,
            Line::styled(
                "5h window 42%, 7d window 18%, resets in 1h 12m",
                t.secondary(),
            ),
        )
        .child(0, "")
        .child(1, body)
        .child(0, "")
        .child(
            0,
            hints(
                &t,
                &[("enter", "switch"), ("r", "refresh"), ("esc", "close")],
            ),
        );
    card.inset(Insets::vh(0, 1))
}

fn card(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    account_card(cx.theme).render(area, buf);
}

fn json_lines(t: &Theme, rows: &[&'static str]) -> Lines {
    Lines(
        rows.iter()
            .map(|r| Line::styled(*r, t.text()))
            .collect::<Vec<_>>(),
    )
}

const REQUEST: [&str; 4] = [
    r#"{"model": "claude-opus-5-5", "max_tokens": 64000,"#,
    r#" "stream": true, "thinking": {"type": "adaptive"},"#,
    r#" "system": "You are a careful code reviewer.","#,
    r#" "messages": [3 messages, 18.2k tokens]}"#,
];

const RESPONSE: [&str; 20] = [
    "event: message_start",
    r#"data: {"type":"message_start","message":{"usage":{"input_tokens":18213}}}"#,
    "event: content_block_start",
    r#"data: {"type":"content_block_start","index":0,"content_block":{"type":"text"}}"#,
    "event: content_block_delta",
    r#"data: {"delta":{"type":"text_delta","text":"The flex allocation looks right"}}"#,
    "event: content_block_delta",
    r#"data: {"delta":{"type":"text_delta","text":", but the last child absorbs"}}"#,
    "event: content_block_delta",
    r#"data: {"delta":{"type":"text_delta","text":" rounding, so check that"}}"#,
    "event: content_block_delta",
    r#"data: {"delta":{"type":"text_delta","text":" the footer stays put."}}"#,
    "event: content_block_stop",
    r#"data: {"type":"content_block_stop","index":0}"#,
    "event: message_delta",
    r#"data: {"delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1104}}"#,
    "event: message_stop",
    r#"data: {"type":"message_stop"}"#,
    "",
    "(stream closed after 6.4 s)",
];

fn section(
    t: Theme,
    label: &'static str,
    note: &'static str,
    body: Lines,
) -> Clipped<ColumnRenderable<'static>> {
    let head = Line::from(vec![
        Span::styled(label, t.accent().add_modifier(Modifier::BOLD)),
        Span::styled(format!("  {note}"), t.dim()),
    ]);
    Clipped {
        inner: ColumnRenderable::new()
            .child(head)
            .child(body.inset(Insets::tlbr(0, 2, 0, 0))),
        theme: t,
    }
}

fn flex(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    let title = Line::from(vec![
        Span::styled("POST /v1/messages", t.text().add_modifier(Modifier::BOLD)),
        Span::styled("  200", t.status(crate::theme::Status::Done)),
        Span::styled("  6.4 s  ai3@iq.io", t.secondary()),
    ]);
    FlexRenderable::new()
        .child(0, title)
        .child(1, section(t, "request", "4 rows", json_lines(&t, &REQUEST)))
        .child(
            1,
            section(
                t,
                "response",
                "20 rows, streamed",
                json_lines(&t, &RESPONSE),
            ),
        )
        .child(
            0,
            hints(&t, &[("tab", "request/response"), ("esc", "close")]),
        )
        .inset(Insets::vh(0, 1))
        .render(area, buf);
}

const ACCOUNTS: [(&str, &str); 5] = [
    ("zhuge@2lab.ai", "42%"),
    ("simayi@2lab.ai", "7%"),
    ("ai3@iq.io", "88%"),
    ("dev1@2lab.ai", "100%"),
    ("ai2@iq.io", "96%"),
];

fn row(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    let mut list = vec![Line::from(vec![
        Span::styled(
            format!("  {}", text::pad("account", 15, text::Align::Left)),
            t.dim(),
        ),
        Span::styled(text::pad("5h", 4, text::Align::Right), t.dim()),
    ])];
    for (i, (name, used)) in ACCOUNTS.iter().enumerate() {
        let cursor = if i == 0 { "› " } else { "  " };
        let label = format!(
            "{cursor}{}{}",
            text::pad(name, 15, text::Align::Left),
            text::pad(used, 4, text::Align::Right)
        );
        let style = if i == 0 { t.selected() } else { t.text() };
        list.push(Line::styled(label, style));
    }
    let sidebar = Lines(list);
    RowRenderable::new()
        .gap(2)
        .child(Width::Cells(22), sidebar.inset(Insets::tlbr(0, 1, 0, 0)))
        .child(Width::Flex(1), account_card(t))
        .render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "layout",
    name: "Layout",
    ty: "twolab_tui::render::FlexRenderable",
    summary: "Who gets the rows when there are not enough: fixed parts keep theirs, flexible parts share the rest.",
    description: "Codex's measure-then-paint composition. A Renderable says how many rows it wants at a width, then paints into the rect it gets, so containers stack children without a layout solver: ColumnRenderable stacks, FlexRenderable gives leftover rows to flex children (a child shorter than its share hands the rest back), RowRenderable places fixed and flexible columns side by side, InsetRenderable pads, and render_offset_content scrolls anything.",
    used_in: "Codex builds every popup, picker and pager from renderables; here select-list, multi-select and pager are Renderables. llmux's account modal and raw request viewer are the first planned hosts.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/render/renderable.rs#L16-L495",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "Renderable::desired_height(width)",
            ty: "fn(u16) -> u16",
            default: None,
            description: "Rows wanted at a width. Parents measure first, then paint.",
        },
        PropDoc {
            name: "ColumnRenderable::push(child)",
            ty: "impl Into<Box<dyn Renderable>>",
            default: None,
            description: "Stack a child at its desired height; clipped at the bottom.",
        },
        PropDoc {
            name: "FlexRenderable::push(flex, child)",
            ty: "u16, impl Into<RenderableItem>",
            default: None,
            description: "flex 0 takes its height first; flex > 0 shares the rest by factor, never past what it wants.",
        },
        PropDoc {
            name: "RowRenderable::push(width, child) / .gap(n)",
            ty: "Width::Cells(n) | Width::Flex(f)",
            default: Some("gap 0"),
            description: "Side-by-side columns: fixed cells first, flexible shares of the rest.",
        },
        PropDoc {
            name: ".inset(Insets::vh(v, h))",
            ty: "RenderableItem",
            default: None,
            description: "Padding counted in the desired height and honoured when scrolled.",
        },
        PropDoc {
            name: "render_offset_content(area, buf, &r, offset)",
            ty: "-> u16",
            default: None,
            description: "Paint any renderable scrolled by offset rows; returns the rows painted.",
        },
        PropDoc {
            name: "CachedRenderable::new(r) / WidgetRenderable::new(widget, h)",
            ty: "Renderable",
            default: None,
            description: "Remember a height per width; adapt any ratatui Widget + Clone at a fixed height.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Reading order is the push order: top to bottom, then left to right.",
        "Layout adds no colour of its own, so a mono (NO_COLOR) screen keeps every child's weight and glyphs.",
        "Measured before painted: a squeezed child can tell it was cut and say so (+N more rows), as the demos do.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "card",
            title: "Card",
            description: "Header row, subtitle, a wrapped body that flexes, a key-hint footer. Narrow it: the body wraps taller; shorten it: the body gives way first.",
            cols: 60,
            rows: 10,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(card)),
        },
        DemoDoc {
            id: "flex",
            title: "Flex shares the rest",
            description: "Request and response share the rows between title and footer; the short request hands its unused share to the response, which says how much it cut.",
            cols: 76,
            rows: 16,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(flex)),
        },
        DemoDoc {
            id: "row",
            title: "Row of columns",
            description: "A fixed 22-cell account list beside the flexible card from the first demo.",
            cols: 90,
            rows: 9,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(row)),
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::render_demo;
    use crate::snapshot;
    use ratatui::style::Style;
    use std::time::Duration;

    fn cx() -> DemoCx {
        DemoCx {
            theme: Theme::ansi(),
            t: Duration::ZERO,
        }
    }

    #[test]
    fn flex_demo_gives_the_short_request_its_rows_and_the_response_the_rest() {
        let mut d = FnDemo(flex);
        let buf = render_demo(&mut d, 76, 16, &cx());
        let lines = snapshot::lines(&buf);
        assert!(lines[1].contains("request"), "{lines:#?}");
        assert!(lines[6].contains("response"), "{lines:#?}");
        assert!(lines[14].contains("more rows"), "{lines:#?}");
        assert!(lines[15].contains("esc close"), "{lines:#?}");
    }

    #[test]
    fn card_body_says_what_it_cut() {
        let mut d = FnDemo(card);
        let buf = render_demo(&mut d, 60, 7, &cx());
        let text = snapshot::text(&buf);
        assert!(text.contains("more rows"), "{text}");
        assert!(text.contains("enter switch"), "{text}");
        assert!(text.contains("zhuge@2lab.ai"), "{text}");
    }

    #[test]
    fn selected_account_row_uses_the_theme_selection() {
        let mut d = FnDemo(row);
        let buf = render_demo(&mut d, 90, 9, &cx());
        assert_eq!(buf[(1, 1)].symbol(), "›");
        assert_eq!(
            buf[(1, 1)].style().add_modifier,
            Style::new().patch(Theme::ansi().selected()).add_modifier
        );
    }
}
