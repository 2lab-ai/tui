use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::Rule;
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::text::{Align, pad};
use crate::theme::Level;

const USAGE: &str = r#"use twolab_tui::rule::Rule;

// The band's top edge, in the divider grey.
frame.render_widget(Rule::new().style(theme.border()), area);

// A section heading; the label keeps its own style.
let heading = Rule::new()
    .style(theme.border())
    .label(Span::styled("accounts", theme.secondary()));

// Review screens use the dotted rule. On a terminal that draws East-Asian
// ambiguous glyphs two cells wide, fall back to ASCII.
let review = Rule::dashed().style(theme.border()).ambiguous_wide(cjk_wide);"#;

fn band(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let lines = vec![
        Line::from(Span::styled(
            "  Backoff now doubles from 1 s to 60 s on each 429, capped at 5 retries.",
            t.text(),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", t.accent()),
            Span::styled("also log the retry count", t.text()),
        ]),
        Rule::new().style(t.border()).line(area.width),
        Line::from(Span::styled(
            "auto · gpt-5.6-sol · 126k/200k 63%",
            t.secondary(),
        )),
    ];
    Paragraph::new(lines).render(area, buf);
}

fn sections(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let heading = |label: &'static str| {
        Rule::new()
            .style(t.border())
            .label(Span::styled(label, t.secondary()))
    };
    let row = |name: &str, five: &str, seven: &str, level: Level| {
        Line::from(vec![
            Span::styled(format!("  {}", pad(name, 18, Align::Left)), t.text()),
            Span::styled("5h ", t.dim()),
            Span::styled(pad(five, 4, Align::Right), t.level(level)),
            Span::styled("   7d ", t.dim()),
            Span::styled(pad(seven, 4, Align::Right), t.text()),
        ])
    };
    let lines = vec![
        heading("accounts").line(area.width),
        row("zhuge@2lab.ai", "42%", "18%", Level::Ok),
        row("simayi@2lab.ai", "87%", "61%", Level::Warn),
        heading("models").line(area.width),
        Line::from(vec![
            Span::styled(
                format!("  {}", pad("claude-opus-4-7", 18, Align::Left)),
                t.text(),
            ),
            Span::styled("1,204 requests", t.secondary()),
        ]),
        Line::from(vec![
            Span::styled(
                format!("  {}", pad("gpt-5.6-sol", 18, Align::Left)),
                t.text(),
            ),
            Span::styled("  318 requests", t.secondary()),
        ]),
    ];
    Paragraph::new(lines).render(area, buf);
}

fn review(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let dotted = Rule::dashed().style(t.border()).line(area.width);
    let added = |n: &str, code: &str| {
        Line::from(vec![
            Span::styled(format!("  {} ", pad(n, 3, Align::Right)), t.dim()),
            Span::styled("+ ", t.text()),
            Span::styled(code.to_string(), t.text()),
        ])
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(" src/quota.rs", t.text()),
            Span::styled("  +2 -1", t.dim()),
        ]),
        dotted.clone(),
        added("41", "let delay = base * 2u32.pow(attempt.min(6));"),
        added("42", "if attempt >= 5 { return Err(GaveUp(attempt)); }"),
        dotted,
        Line::from(vec![
            Span::styled("  1 ", t.dim()),
            Span::styled("yes", t.accent()),
            Span::styled("    2 ", t.dim()),
            Span::styled("yes, and don't ask again", t.text()),
            Span::styled("    3 ", t.dim()),
            Span::styled("no", t.text()),
        ]),
    ];
    Paragraph::new(lines).render(area, buf);
}

fn ascii(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let wide = true;
    let lines = vec![
        Rule::new()
            .style(t.border())
            .label(Span::styled("계정", t.secondary()))
            .ambiguous_wide(wide)
            .line(area.width),
        Line::from(vec![
            Span::styled("  zhuge@2lab.ai     ", t.text()),
            Span::styled("5h 42%", t.secondary()),
        ]),
        Rule::new()
            .style(t.border())
            .ambiguous_wide(wide)
            .line(area.width),
        Line::from(Span::styled(
            "  this terminal draws ─ two cells wide, so the rule is ASCII -",
            t.dim(),
        )),
    ];
    Paragraph::new(lines).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "rule",
    name: "Rule",
    ty: "twolab_tui::rule::Rule",
    summary: "Where a band or a section starts.",
    description: "One row of a box-drawing glyph across the whole width, in the divider grey: the edge of a footer band, a section heading with a left label, or the dotted rule a review screen puts between a diff and its choices. The glyphs are East-Asian Ambiguous, so a host on a terminal that draws them two cells wide switches to ASCII and the rule still fits.",
    used_in: "fx's footer band around an open menu or approval, and its file-review screen; llmux's borderless summary band.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/footer/row_text.zig#L12-L21",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "Rule::new() / Rule::dashed()",
            ty: "Rule",
            default: Some("─ (U+2500)"),
            description: "Solid ─, or the review screens' dotted ┄ (U+2504).",
        },
        PropDoc {
            name: ".style(s)",
            ty: "Style",
            default: Some("Style::new()"),
            description: "The glyph's style; usually theme.border().",
        },
        PropDoc {
            name: ".label(line)",
            ty: "impl Into<Line<'static>>",
            default: Some("none"),
            description: "Drawn after a two-cell lead in its own styles; clipped with … when the row is short, dropped below five cells.",
        },
        PropDoc {
            name: ".glyph(c) / .ascii()",
            ty: "char",
            default: Some("─"),
            description: "Any glyph; a two-cell glyph leaves a space on an odd width, a zero-width one draws as -.",
        },
        PropDoc {
            name: ".ambiguous_wide(wide)",
            ty: "bool",
            default: Some("false"),
            description: "When the terminal draws East-Asian Ambiguous glyphs two cells wide, swap an ambiguous glyph for ASCII -.",
        },
        PropDoc {
            name: ".line(width)",
            ty: "Line<'static>",
            default: None,
            description: "The rule as exactly width cells, for a host's own paragraph.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Decoration only: a screen reader reading the buffer meets a row of ─; the label is the only text, so put the meaning there.",
        "In mono the rule is the terminal's dim weight, and the label keeps its own.",
        "─ and ┄ are East-Asian Ambiguous: on a terminal that draws them wide, .ambiguous_wide(true) keeps the rule one row.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "band",
            title: "Band divider",
            description: "The rule between the composer and the footer row.",
            cols: 76,
            rows: 5,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(band)),
        },
        DemoDoc {
            id: "sections",
            title: "Labelled sections",
            description: "Section headings in a borderless panel.",
            cols: 48,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(sections)),
        },
        DemoDoc {
            id: "review",
            title: "Review, dotted",
            description: "The dotted rule around a diff under review.",
            cols: 64,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(review)),
        },
        DemoDoc {
            id: "ascii",
            title: "CJK terminal fallback",
            description: "Ambiguous glyphs drawn wide: the same rule in ASCII.",
            cols: 66,
            rows: 4,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(ascii)),
        },
    ],
};
