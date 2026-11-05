use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::MiniBar;
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::format;
use crate::text::{self, Align};

const USAGE: &str = r#"use twolab_tui::mini_bar::MiniBar;

// Each model's tokens against the busiest model's.
let busiest = rows.iter().map(|r| r.tokens).max().unwrap_or(0) as f64;
let cell = MiniBar::new(row.tokens as f64, busiest)
    .width(10)
    .theme(theme)
    .line(); // ▰▰▰▱▱▱▱▱▱▱

// A heat list: colour by how hot the share is.
let heat = MiniBar::new(share, 1.0).levels(0.7, 0.9).width(12).theme(theme);"#;

fn fit(lines: Vec<Line<'static>>, area: Rect) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(area.width)))
        .collect()
}

fn models(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let rows: [(&str, Option<u64>); 6] = [
        ("opus-4-8[1m]", Some(1_204_311)),
        ("gpt-5.6-sol", Some(830_402)),
        ("fable-5", Some(412_977)),
        ("sonnet-4-6", Some(96_120)),
        ("grok-5", Some(12_408)),
        ("gemini-3-pro", None),
    ];
    let max = rows.iter().filter_map(|r| r.1).max().unwrap_or(0) as f64;
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {}{:>6}  share of the busiest",
            text::pad("model", 14, Align::Left),
            "tokens"
        ),
        t.dim(),
    ))];
    for (model, tokens) in rows {
        let value = tokens.map_or(f64::NAN, |n| n as f64);
        let mut spans = vec![
            Span::styled(format!(" {}", text::pad(model, 14, Align::Left)), t.text()),
            Span::styled(format!("{:>6}  ", format::opt_count(tokens)), t.secondary()),
        ];
        spans.extend(MiniBar::new(value, max).width(10).theme(*t).spans());
        lines.push(Line::from(spans));
    }
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn heat(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let cells: [(&str, &str, &str, u64); 8] = [
        ("claude", "opus-4-8[1m]", "ai3@iq.io", 1_212_400),
        ("claude", "opus-4-8[1m]", "zhuge@2lab.ai", 944_120),
        ("codex", "gpt-5.6-sol", "dev1@2lab.ai", 611_090),
        ("claude", "fable-5", "simayi@2lab.ai", 402_300),
        ("codex", "gpt-5.6-sol", "ai2@iq.io", 88_700),
        ("grok", "grok-5", "lusu@2lab.ai", 30_100),
        ("claude", "sonnet-4-6", "ai3@iq.io", 12_800),
        ("codex", "gpt-5.5", "zhuge@2lab.ai", 4_020),
    ];
    let shown = 5;
    let max = cells[0].3 as f64;
    let mut lines = vec![
        Line::from(Span::styled(
            " approximate — best effort over the last 24h",
            t.dim(),
        )),
        Line::from(Span::styled(
            format!(
                " {}{}{}{:>7}  load",
                text::pad("group", 7, Align::Left),
                text::pad("model", 13, Align::Left),
                text::pad("account", 15, Align::Left),
                "tokens"
            ),
            t.dim(),
        )),
    ];
    for (group, model, account, tokens) in cells.iter().take(shown) {
        let mut spans = vec![
            Span::styled(
                format!(" {}", text::pad(group, 7, Align::Left)),
                t.secondary(),
            ),
            Span::styled(text::pad(model, 13, Align::Left), t.text()),
            Span::styled(text::pad(account, 15, Align::Left), t.text()),
            Span::styled(
                format!("{:>7}  ", format::human_count(*tokens)),
                t.secondary(),
            ),
        ];
        spans.extend(
            MiniBar::new(*tokens as f64, max)
                .levels(0.7, 0.9)
                .width(12)
                .theme(*t)
                .spans(),
        );
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(Span::styled(
        format!(" +{} more cells", cells.len() - shown),
        t.dim(),
    )));
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn glyphs(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let share = 0.62;
    let mut lines = Vec::new();
    for w in [4u16, 8, 12, 20] {
        let mut spans = vec![Span::styled(format!(" {w:>2} cells  "), t.dim())];
        spans.extend(MiniBar::ratio(share).width(w).theme(*t).spans());
        lines.push(Line::from(spans));
    }
    for (name, f, e) in [("ascii", '#', '.'), ("braille", '⣿', '⣀')] {
        let mut spans = vec![Span::styled(
            format!(" {}", text::pad(name, 10, Align::Left)),
            t.dim(),
        )];
        spans.extend(
            MiniBar::ratio(share)
                .width(12)
                .glyphs(f, e)
                .theme(*t)
                .spans(),
        );
        lines.push(Line::from(spans));
    }
    Paragraph::new(fit(lines, area)).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "mini-bar",
    name: "Mini bar",
    ty: "twolab_tui::mini_bar::MiniBar",
    summary: "This row's share of the biggest one, as a short bar beside the number.",
    description: "A fixed-width run of ▰ and ▱ glyphs: round(value ÷ max × width) cells filled, so a column of rows reads as a ranking at a glance. The filled part takes the accent, a series colour, or a level colour by how hot the share is; the empty part is dim. A share that cannot be computed reads `—`, never an empty bar that looks like zero.",
    used_in: "llmux models strip and models table (token share, 10 cells) and the stats heatmap (load per group × model × account, 12 cells, coloured by level).",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/format.rs#L74-L88",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "MiniBar::new(value, max) / MiniBar::ratio(r)",
            ty: "(f64, f64) / f64",
            default: None,
            description: "The row's value and the biggest row's; or a ratio already in 0–1.",
        },
        PropDoc {
            name: ".width(n)",
            ty: "u16",
            default: Some("10"),
            description: "Cells the bar takes. A narrower area draws a shorter bar of the same share.",
        },
        PropDoc {
            name: ".glyphs(filled, empty)",
            ty: "(char, char)",
            default: Some("'▰', '▱'"),
            description: "One-cell glyphs; a wider glyph is refused.",
        },
        PropDoc {
            name: ".style(s)",
            ty: "Style",
            default: Some("theme.accent()"),
            description: "Style of the filled part — a series or group colour (weight only, in mono).",
        },
        PropDoc {
            name: ".levels(low, high)",
            ty: "(f64, f64)",
            default: None,
            description: "Colour the filled part by the share's level instead.",
        },
        PropDoc {
            name: ".line() / .spans() / .text()",
            ty: "Line<'static> / Vec<Span> / String",
            default: None,
            description: "The bar for a table cell, a span run, or plain text.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Put the number beside the bar: the bar ranks, the number says how much.",
        "In mono the filled glyph is solid and the empty one outlined (and dim), so the share reads without colour.",
        "▰ ▱, braille and ASCII are one cell on every terminal, CJK included; block shades (█) are not, so they are the host's opt-in.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "models",
            title: "Model token shares",
            description: "Tokens per model against the busiest; a model with no count reads —.",
            cols: 46,
            rows: 7,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(models)),
        },
        DemoDoc {
            id: "heat",
            title: "Heat list",
            description: "The busiest group × model × account cells, coloured by load, with the rest counted.",
            cols: 66,
            rows: 8,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(heat)),
        },
        DemoDoc {
            id: "glyphs",
            title: "Widths and glyphs",
            description: "The same 62 % share at four widths and in two other glyph sets.",
            cols: 36,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(glyphs)),
        },
    ],
};
