use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{GradientText, Rgb};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::format;
use crate::spinner::{Spinner, SpinnerKind};
use crate::text::{self, Align};
use crate::theme::{Level, Motion, Status, Theme};

const USAGE: &str = r#"use twolab_tui::gradient_text::{parse_hex, GradientText};

// A headline model breathes in its group's colour.
let base = parse_hex(&config.claude).unwrap_or((255, 121, 198));
let name = GradientText::solid("fable-5", base)
    .at(elapsed)            // the host's animation clock
    .theme(theme)           // reduced motion: still, bold, base colour
    .line();

// The max effort badge cycles through a rainbow, twice as fast.
let badge = GradientText::rainbow("max").speed(2.0).at(elapsed).theme(theme).spans();"#;

/// Group anchors from llmux's config defaults (claude magenta, codex cyan).
const CLAUDE: Rgb = (255, 121, 198);
const CODEX: Rgb = (86, 220, 220);

fn fit(lines: Vec<Line<'static>>, area: Rect) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(area.width)))
        .collect()
}

/// A model name: a gradient for headline models, plain for the rest.
fn name_spans(model: &str, base: Option<Rgb>, t: &Theme, cx: &DemoCx) -> Vec<Span<'static>> {
    let mut spans = match base {
        Some(b) => GradientText::solid(model.to_string(), b)
            .at(cx.t)
            .theme(*t)
            .spans(),
        None => vec![Span::styled(model.to_string(), t.text())],
    };
    let pad = 14usize.saturating_sub(text::width(model));
    spans.push(Span::raw(" ".repeat(pad)));
    spans
}

/// An effort badge: `max` shimmers, `xhigh` is a still warning, the rest
/// plain.
fn effort_spans(effort: &str, t: &Theme, cx: &DemoCx) -> Vec<Span<'static>> {
    match effort {
        "max" => GradientText::rainbow("max").at(cx.t).theme(*t).spans(),
        "xhigh" => vec![Span::styled(
            "xhigh",
            t.level(Level::Crit).add_modifier(Modifier::BOLD),
        )],
        other => vec![Span::styled(other.to_string(), t.secondary())],
    }
}

fn model_rows(t: &Theme, cx: &DemoCx) -> Vec<Line<'static>> {
    let rows: [(&str, &str, Option<Rgb>, &str); 5] = [
        ("claude", "fable-5", Some(CLAUDE), "max"),
        ("codex", "gpt-5.6-sol", Some(CODEX), "xhigh"),
        ("claude", "opus-4-8[1m]", None, "high"),
        ("codex", "gpt-5.5", None, "max"),
        ("grok", "grok-5", None, "medium"),
    ];
    rows.into_iter()
        .map(|(group, model, base, effort)| {
            let mut spans = vec![Span::styled(
                format!(" {}", text::pad(group, 8, Align::Left)),
                t.secondary(),
            )];
            spans.extend(name_spans(model, base, t, cx));
            spans.extend(effort_spans(effort, t, cx));
            Line::from(spans)
        })
        .collect()
}

fn models(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {}{}effort",
            text::pad("group", 8, Align::Left),
            text::pad("model", 14, Align::Left)
        ),
        t.dim(),
    ))];
    lines.extend(model_rows(t, cx));
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn status(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let lines: Vec<Line> = [0.5f32, 1.0, 2.0]
        .into_iter()
        .map(|speed| {
            let mut spans = vec![
                Span::raw(" "),
                Spinner::new(SpinnerKind::Braille)
                    .style(t.status(Status::Running))
                    .span_at(cx.t, t),
                Span::raw(" "),
            ];
            spans.extend(
                GradientText::rainbow("Thinking")
                    .speed(speed)
                    .at(cx.t)
                    .theme(*t)
                    .spans(),
            );
            spans.push(Span::styled(
                format!("  {} · esc to interrupt", format::elapsed(cx.t)),
                t.dim(),
            ));
            spans.push(Span::styled(format!("   speed {speed}"), t.secondary()));
            Line::from(spans)
        })
        .collect();
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn reduced(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let still = cx.theme.with_motion(Motion::Reduced);
    let mono = Theme::mono().with_motion(cx.theme.motion);
    let row = |label: &str, t: &Theme| {
        let mut spans = vec![Span::styled(
            format!(" {}", text::pad(label, 16, Align::Left)),
            cx.theme.dim(),
        )];
        spans.extend(name_spans("fable-5", Some(CLAUDE), t, cx));
        spans.extend(name_spans("gpt-5.6-sol", Some(CODEX), t, cx));
        spans.extend(effort_spans("max", t, cx));
        Line::from(spans)
    };
    let lines = vec![
        row("reduced motion", &still),
        row("no colour", &mono),
        Line::from(Span::styled(
            " still bold, in the base colour — the names stand out, nothing moves",
            cx.theme.dim(),
        )),
    ];
    Paragraph::new(fit(lines, area)).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "gradient-text",
    name: "Gradient text",
    ty: "twolab_tui::gradient_text::GradientText",
    summary: "This name is special — a slow colour shimmer that drifts along the text.",
    description: "Each grapheme takes its colour from a sine phase that drifts with time: a rainbow turns the hue, a solid base only breathes its brightness, and a luma band keeps every letter readable. Time comes in as a Duration, never from a clock. Reduced motion holds the text still in bold and its base colour; mono draws it bold with no colour at all.",
    used_in: "llmux models strip and activity badges: headline models (fable-5 in the claude magenta, gpt-5.6-sol in the codex cyan) and the max effort badge.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L9385-L9487",
        license: "MIT",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "GradientText::rainbow(text) / ::solid(text, base)",
            ty: "(impl Into<Cow<str>>[, Rgb])",
            default: None,
            description: "A turning rainbow, or one colour whose brightness breathes.",
        },
        PropDoc {
            name: ".at(t)",
            ty: "Duration",
            default: Some("0"),
            description: "Time since the host's animation started; a 120 ms tick is one llmux frame.",
        },
        PropDoc {
            name: ".speed(s)",
            ty: "f32",
            default: Some("1.0"),
            description: "Drift multiplier; non-finite or non-positive reads 1.0.",
        },
        PropDoc {
            name: ".luma(min, max)",
            ty: "(f32, f32)",
            default: Some("0.45–1.00 (0.25–0.70 on light)"),
            description: "The brightness band every letter stays inside.",
        },
        PropDoc {
            name: ".still(style)",
            ty: "Style",
            default: Some("bold, base colour"),
            description: "The still style under reduced motion (weight only, in mono).",
        },
        PropDoc {
            name: ".spans() / .line()",
            ty: "Vec<Span<'static>> / Line<'static>",
            default: None,
            description: "One span per grapheme cluster; their text is exactly the input.",
        },
        PropDoc {
            name: "parse_hex(s)",
            ty: "fn(&str) -> Option<Rgb>",
            default: None,
            description: "`#rrggbb` from a config file; anything else is None.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Reduced motion (Theme::with_motion(Motion::Reduced)) holds the text still: bold, in its base colour.",
        "Mono draws bold with no colour, so the emphasis survives NO_COLOR.",
        "The luma band keeps every letter legible; on a light palette it moves darker.",
        "Coloured per grapheme cluster, so emoji and Hangul are never split into differently styled halves.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "models",
            title: "Model names",
            description: "Headline models in their group's colour, the max effort badge as a rainbow, xhigh still.",
            cols: 40,
            rows: 6,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(models)),
        },
        DemoDoc {
            id: "status",
            title: "A status word",
            description: "The word a working agent shows, at half, normal and double speed.",
            cols: 56,
            rows: 3,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(status)),
        },
        DemoDoc {
            id: "reduced",
            title: "Reduced motion and no colour",
            description: "The same names held still, and in mono.",
            cols: 72,
            rows: 3,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(reduced)),
        },
    ],
};
