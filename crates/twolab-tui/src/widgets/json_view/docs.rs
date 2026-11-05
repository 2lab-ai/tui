use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use super::{JsonView, JsonViewState, body_lines};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::json_view::{body_lines, JsonView, JsonViewState};

// Once, when the body arrives (not per frame).
let lines = body_lines(&record.request_body, &theme);

// Every frame: wraps to the width, clamps the scroll, draws the position.
frame.render_stateful_widget(
    &JsonView::new(&lines).theme(theme),
    area,
    &mut app.body_scroll,
);

// ↑↓ PgUp PgDn Home End.
if app.body_scroll.on_key(key) { redraw(); }"#;

/// A Claude Code turn as llmux captured it on the way to the API.
const REQUEST: &str = r#"{"model":"opus-4-8[1m]","max_tokens":32000,"stream":true,"thinking":{"type":"enabled","budget_tokens":16000},"system":[{"type":"text","text":"You are Claude Code, Anthropic's official CLI for Claude.","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":[{"type":"text","text":"Why does the accounts table jump when a countdown ticks?"}]},{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01HfQ7cZ8k2VbXr9mT3nLwYe","name":"Grep","input":{"pattern":"fn table_layout","path":"src/tui/ui.rs"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01HfQ7cZ8k2VbXr9mT3nLwYe","content":"src/tui/ui.rs:5113:fn table_layout(view: &DashboardView, ctx: &FrameCtx, width: u16) -> TableLayout {","is_error":false}]}],"tools":[{"name":"Grep","description":"Search file contents with ripgrep.","input_schema":{"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"-n":{"type":"boolean"}},"required":["pattern"]}}],"metadata":{"user_id":"user_2f9c41e0_account_ai3_session_7d1b"},"temperature":1,"top_k":null}"#;

/// The reply: a thinking block and an answer, both long strings.
const RESPONSE: &str = r#"{"id":"msg_01XFDUDYJgAACzvnptvVoYEL","type":"message","role":"assistant","model":"opus-4-8[1m]","content":[{"type":"thinking","thinking":"The table re-solves its column widths from the live countdown text, so 9m 59s to 10m 0s widens the reset column by one cell and every column to its right shifts. Measuring the widest form a countdown can take fixes it.","signature":"EqQBCgIYAhIM1gbcDa9GJwZA2b3hGgxWa8JtY4z0w6kT0wEiMHnZ"},{"type":"text","text":"The jump comes from table_layout measuring the live reset text. When a countdown crosses a digit boundary (9m 59s to 10m 0s) the column grows by a cell and the gauges shift right. Measure the widest form instead: bound_now() with BOUND_SPANS already computes it for the fold, so the table can reuse it."}],"stop_reason":"end_turn","stop_sequence":null,"usage":{"input_tokens":18342,"cache_creation_input_tokens":0,"cache_read_input_tokens":171204,"output_tokens":412,"service_tier":"standard"}}"#;

const ERROR: &str = r#"{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed the rate limit for your organization (8f2c1d4e) of 4,000,000 input tokens per minute. For details, refer to: https://docs.anthropic.com/en/api/rate-limits."},"request_id":"req_011CUzE4sQ8vNcT3pYk2HbJm"}"#;

const ERROR_HEADERS: &[(&str, &str)] = &[
    ("status", "429 Too Many Requests"),
    ("retry-after", "23"),
    ("anthropic-ratelimit-input-tokens-remaining", "0"),
    (
        "anthropic-ratelimit-input-tokens-reset",
        "2026-11-05T09:41:23Z",
    ),
    ("request-id", "req_011CUzE4sQ8vNcT3pYk2HbJm"),
];

/// A streamed reply that ends in an overload: SSE lines, JSON payloads
/// highlighted inline.
const STREAM: &str = "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_01J7mVbX2c\",\"model\":\"fable-5\",\"usage\":{\"input_tokens\":2048,\"output_tokens\":1}}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking the fold bounds\"}}\n\nevent: ping\ndata: {\"type\":\"ping\"}\n\nevent: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}";

fn section(title: &str, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("── {title} ──"),
        theme.dim().add_modifier(Modifier::BOLD),
    ))
}

/// A body (and optional header block) in a scrollable view.
struct Viewer {
    body: &'static str,
    headers: &'static [(&'static str, &'static str)],
    state: JsonViewState,
}

impl Viewer {
    fn new(body: &'static str, headers: &'static [(&'static str, &'static str)]) -> Viewer {
        Viewer {
            body,
            headers,
            state: JsonViewState::default(),
        }
    }
}

impl Demo for Viewer {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let mut lines = Vec::new();
        if !self.headers.is_empty() {
            lines.push(section("response headers", t));
            for (name, value) in self.headers {
                lines.push(Line::from(vec![
                    Span::styled(format!("{name}: "), t.accent()),
                    Span::styled(*value, t.text()),
                ]));
            }
            lines.push(Line::default());
            lines.push(section("response body", t));
        }
        lines.extend(body_lines(self.body, t));
        JsonView::new(&lines)
            .theme(*t)
            .render(area, buf, &mut self.state);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        self.state.on_key(key)
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "json-view",
    name: "JSON view",
    ty: "twolab_tui::json_view::JsonView",
    summary: "This JSON, coloured, wrapped to the width and scrollable, with where you are in it.",
    description: "A request or response body as it went over the wire: one JSON document is re-indented (keys keep their wire order) and coloured — keys in accent, strings in done, numbers in warn, true/false/null in run, punctuation dim; SSE data: lines are coloured inline; anything else stays plain. Long strings wrap at the width in cells. ↑↓ PgUp PgDn Home End scroll, a scrollbar appears only when it overflows, and the footer says lines N–M of T.",
    used_in: "llmux raw viewer: the client and upstream request and response bodies of one exchange, up to megabytes each.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L4291-L4428",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "body_lines(body, &theme)",
            ty: "Vec<Line<'static>>",
            default: None,
            description: "A captured body as coloured lines: pretty JSON, SSE with inline JSON, or plain text. Build once per body.",
        },
        PropDoc {
            name: "json_line(line, &theme) / pretty(src)",
            ty: "Line<'static> / Option<String>",
            default: None,
            description: "Colour one line of JSON; re-indent one JSON document (None when it is not exactly one).",
        },
        PropDoc {
            name: "JsonView::new(&lines)",
            ty: "&[Line<'static>]",
            default: None,
            description: "The lines to show; render with a JsonViewState.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Scrollbar thumb in accent, track and legend dim, position secondary.",
        },
        PropDoc {
            name: ".hints(on)",
            ty: "bool",
            default: Some("true"),
            description: "The key legend at the left of the footer (dropped first when narrow).",
        },
        PropDoc {
            name: "state.on_key(key)",
            ty: "bool",
            default: None,
            description: "Scroll; returns whether the view moved. The render clamps, so End can be pressed before the first frame.",
        },
        PropDoc {
            name: "state.shown()",
            ty: "Option<(first, last, total)>",
            default: None,
            description: "The 1-based source lines on screen at the last render.",
        },
    ],
    keyboard: &[
        ("↑ / k, ↓ / j", "one row"),
        ("PgUp / PgDn", "a page (the view minus one row)"),
        ("Home / End", "top / bottom"),
    ],
    a11y: &[
        "Keys are bold and punctuation dim under NO_COLOR, so structure still reads without colour.",
        "The footer always says what part is on screen (lines 12–40 of 213, or 9 lines when it all fits).",
        "Wrapping never splits a grapheme, and a wide glyph moves to the next row instead of straddling the edge.",
        "The scrollbar thumb is a heavier glyph than its track, visible without colour.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "request",
            title: "A /v1/messages request",
            description: "What Claude Code sent through llmux: ↑↓ PgUp PgDn Home End scroll it.",
            cols: 80,
            rows: 16,
            animated: false,
            interactive: true,
            make: || Box::new(Viewer::new(REQUEST, &[])),
        },
        DemoDoc {
            id: "response",
            title: "Long strings wrap",
            description: "A reply whose thinking and text blocks wrap at the width in cells.",
            cols: 64,
            rows: 14,
            animated: false,
            interactive: true,
            make: || Box::new(Viewer::new(RESPONSE, &[])),
        },
        DemoDoc {
            id: "error",
            title: "An error body",
            description: "A 429 with its rate-limit headers above the body.",
            cols: 76,
            rows: 14,
            animated: false,
            interactive: true,
            make: || Box::new(Viewer::new(ERROR, ERROR_HEADERS)),
        },
        DemoDoc {
            id: "stream",
            title: "A streamed reply",
            description: "SSE lines kept as sent; each data: payload coloured inline.",
            cols: 96,
            rows: 12,
            animated: false,
            interactive: true,
            make: || Box::new(Viewer::new(STREAM, &[])),
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::json_view::pretty;

    #[test]
    fn demo_bodies_are_the_json_they_claim_to_be() {
        for body in [REQUEST, RESPONSE, ERROR] {
            assert!(pretty(body).is_some(), "{body}");
        }
        assert!(pretty(STREAM).is_none(), "SSE is not one document");
        let lines = body_lines(STREAM, &Theme::ansi());
        assert!(lines.iter().filter(|l| l.spans.len() > 2).count() >= 4);
    }
}
