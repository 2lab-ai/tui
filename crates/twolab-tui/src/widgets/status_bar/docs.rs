use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{Segment, StatusBar, context_label, model_label};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::theme::{Level, Theme};

const USAGE: &str = r#"use twolab_tui::status_bar::{Segment, StatusBar, context_label, model_label};

// Higher priority stays longer; the rightmost of equals goes first.
let bar = StatusBar::new(theme)
    .segment(Segment::new("auto").style(theme.text()).priority(4))
    .segment(Segment::new(model_label(Some("openai/gpt-5.6-sol"))).priority(5))
    .segment(Segment::new(context_label(Some(126_000), Some(200_000))).priority(3));

// After one esc while a turn runs: the cue is reserved, the left gives way.
let armed = bar
    .cue(["press esc again to interrupt", "esc again to interrupt", "esc esc"])
    .cue_alone("esc esc to interrupt");

frame.render_widget(&armed, footer_row);"#;

const INTERRUPT: [&str; 4] = [
    "press esc again to interrupt",
    "esc again to interrupt",
    "esc esc interrupt",
    "esc esc",
];

/// The agent footer every demo starts from.
fn footer(t: &Theme) -> StatusBar {
    StatusBar::new(*t)
        .segment(Segment::new("auto").style(t.text()).priority(4))
        .segment(Segment::new(model_label(Some("openai/gpt-5.6-sol"))).priority(5))
        .segment(Segment::new("high").priority(2))
        .segment(Segment::new(context_label(Some(126_000), Some(200_000))).priority(3))
        .segment(Segment::new("quota poller backoff").priority(1))
        .segment(Segment::new("~/src/llmux (main)"))
}

/// Captioned bars, each drawn at `min(area width, its width)`.
fn stack(area: Rect, buf: &mut Buffer, t: &Theme, rows: &[(&str, u16, StatusBar)]) {
    let mut y = area.y;
    for (caption, w, bar) in rows {
        if y >= area.bottom() {
            break;
        }
        buf.set_line(
            area.x,
            y,
            &Line::from(Span::styled(caption.to_string(), t.dim())),
            area.width,
        );
        y += 1;
        if y >= area.bottom() {
            break;
        }
        let width = (*w).min(area.width);
        bar.render(Rect::new(area.x, y, width, 1), buf);
        y += 1;
    }
}

fn widths(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    stack(
        area,
        buf,
        t,
        &[
            ("120 columns: everything", 120, footer(t)),
            ("80 columns: the workspace goes first", 80, footer(t)),
            ("40 columns: then the title and the effort", 40, footer(t)),
        ],
    );
}

fn armed(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let cue = |b: StatusBar| b.cue(INTERRUPT).cue_alone("esc esc to interrupt");
    stack(
        area,
        buf,
        t,
        &[
            ("esc pressed once while a turn runs", 100, cue(footer(t))),
            (
                "46 columns: the left gives way, the cue keeps its words",
                46,
                cue(footer(t)),
            ),
            ("18 columns: the cue owns the row", 18, cue(footer(t))),
            (
                "a draft, esc pressed once",
                100,
                footer(t).cue(["esc again to clear"]),
            ),
        ],
    );
}

fn unknown(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    stack(
        area,
        buf,
        t,
        &[
            (
                "before the model catalog loads",
                90,
                StatusBar::new(*t)
                    .segment(Segment::new("ask").priority(4))
                    .segment(Segment::new(model_label(None)).style(t.dim()).priority(5))
                    .segment(
                        Segment::new(context_label(None, None))
                            .style(t.dim())
                            .priority(3),
                    )
                    .note("connecting to the gateway"),
            ),
            (
                "a model that publishes no context window",
                90,
                StatusBar::new(*t)
                    .segment(Segment::new("auto").style(t.text()).priority(4))
                    .segment(Segment::new(model_label(Some("zai/glm-5.2"))).priority(5))
                    .segment(Segment::new(context_label(Some(12_400), None)).priority(3)),
            ),
            (
                "full access: a standing warning, reserved like a cue",
                90,
                StatusBar::new(*t)
                    .segment(Segment::new("full access").style(t.text()).priority(4))
                    .segment(
                        Segment::new(model_label(Some("anthropic/claude-opus-4-7"))).priority(5),
                    )
                    .cue([
                        "Full access enabled: permission checks disabled",
                        "Full access",
                    ])
                    .cue_style(t.level(Level::Warn)),
            ),
        ],
    );
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "status-bar",
    name: "Status bar",
    ty: "twolab_tui::status_bar::StatusBar",
    summary: "What mode, which model, how full the context is, and what you just armed.",
    description: "One row of facts joined by ` · `. Each segment has a priority; as the row narrows, whole segments go — lowest priority first — and only the last one standing is cut. The right end holds a cue about the next keystroke, reserved before the left is laid out and shortened through its variants, or a note that shows only when it fits. Unknown values read as —, never 0.",
    used_in: "fx's hint row under the composer (mode · model · effort · context · workspace, esc cues on the right); llmux's footer (keybar or mode on the left, a transient status on the right).",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/render.zig#L194-L470",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "StatusBar::new(theme)",
            ty: "Theme",
            default: None,
            description: "Row in theme.secondary(), cue in theme.accent(), note in theme.dim().",
        },
        PropDoc {
            name: ".segment(Segment::new(text).priority(p).style(s))",
            ty: "Segment",
            default: Some("priority 0, row style"),
            description: "A fact on the left. Higher priority stays longer; empty text takes no separator.",
        },
        PropDoc {
            name: ".cue(variants) / .cue_alone(text)",
            ty: "IntoIterator<Item = Into<String>>",
            default: Some("none"),
            description: "Widest first. Reserved before the left; the widest variant that leaves room for the most important segment wins, else the cue takes the row: `alone` if it fits, else its widest variant that does.",
        },
        PropDoc {
            name: ".note(text)",
            ty: "Into<String>",
            default: Some("none"),
            description: "Right-aligned status, shown only beside what the left kept. A cue outranks it.",
        },
        PropDoc {
            name: ".separator(s) / .style(s) / .cue_style(s) / .note_style(s)",
            ty: "String / Style",
            default: Some("\" · \""),
            description: "· is East-Asian Ambiguous: pass \" | \" on a terminal that draws it wide.",
        },
        PropDoc {
            name: "model_label(id) / context_label(used, window) / compact_model(id)",
            ty: "fn -> String",
            default: None,
            description: "fx's compact model (`opus 4-7`), `126k/200k 63%`; `model —`, `context —`, `12k/—` for what is unknown.",
        },
        PropDoc {
            name: ".layout(width) / .line(width)",
            ty: "BarLayout / Line<'static>",
            default: None,
            description: "What stays at a width, or the row as a line for a host's own paragraph.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Never cut silently: segments go whole, and the one that cannot fit alone ends in ….",
        "A cue about a destructive key is reserved first and never dropped; on the narrowest row it takes the whole row.",
        "In mono the cue is bold and the note dim, so the right end still reads apart from the facts.",
        "Unknowns say — with a noun (`model —`), never a made-up 0.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "widths",
            title: "Agent footer at 120, 80, 40",
            description: "The same facts as the row narrows: lowest priority goes first.",
            cols: 120,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(widths)),
        },
        DemoDoc {
            id: "armed",
            title: "Armed",
            description: "One esc pressed: the cue is reserved, then shortened, then owns the row.",
            cols: 100,
            rows: 8,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(armed)),
        },
        DemoDoc {
            id: "unknown",
            title: "Unknown model, unknown window",
            description: "What nobody measured reads as —; a standing warning on the right.",
            cols: 90,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(unknown)),
        },
    ],
};
