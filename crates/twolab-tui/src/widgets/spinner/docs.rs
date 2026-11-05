use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Spinner, SpinnerKind};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::text;
use crate::theme::{Motion, Status};

const USAGE: &str = r#"use twolab_tui::spinner::{Spinner, SpinnerKind};

// One shared tick keeps every spinner on screen in step.
let glyph = SpinnerKind::Braille.glyph(frame);

// Or drive it by time; reduced motion holds the first frame.
let span = Spinner::new(SpinnerKind::Block)
    .style(theme.status(Status::Running))
    .span_at(elapsed, &theme);"#;

fn families(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let rows: [(SpinnerKind, &str, Status); 6] = [
        (
            SpinnerKind::Braille,
            "working — first kind",
            Status::Running,
        ),
        (SpinnerKind::Block, "working — second kind", Status::Running),
        (SpinnerKind::Clock, "waiting for a reset", Status::Waiting),
        (SpinnerKind::Pulse, "active, idle", Status::Done),
        (SpinnerKind::Drift, "idle but ready", Status::Done),
        (SpinnerKind::Breathe, "filling up", Status::Waiting),
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|(k, what, st)| {
            let s = Spinner::new(*k).style(t.status(*st));
            Line::from(vec![
                Span::raw(" "),
                s.span_at(cx.t, t),
                Span::raw("  "),
                Span::styled(text::pad(k.name(), 8, text::Align::Left), t.accent()),
                Span::styled(*what, t.secondary()),
            ])
        })
        .collect();
    Paragraph::new(lines).render(area, buf);
}

fn in_rows(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let row = |name: &str, kind: SpinnerKind, st: Status, note: &str| {
        Line::from(vec![
            Span::styled(
                format!(" {}", text::pad(name, 16, text::Align::Left)),
                t.text(),
            ),
            Spinner::new(kind).style(t.status(st)).span_at(cx.t, t),
            Span::raw(" "),
            Span::styled(note.to_string(), t.status(st)),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(" account          status", t.dim())),
        row(
            "ai3@iq.io",
            SpinnerKind::Braille,
            Status::Running,
            "2 in flight",
        ),
        row("zhuge@2lab.ai", SpinnerKind::Pulse, Status::Done, "current"),
        row("simayi@2lab.ai", SpinnerKind::Drift, Status::Done, "ready"),
        row(
            "dev1@2lab.ai",
            SpinnerKind::Clock,
            Status::Waiting,
            "resets in 1h 12m",
        ),
        row("ai2@iq.io", SpinnerKind::Breathe, Status::Failed, "7d 96%"),
    ];
    Paragraph::new(lines).render(area, buf);
}

fn reduced(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme.with_motion(Motion::Reduced);
    let mut spans = vec![Span::styled(" reduced motion: ", t.secondary())];
    for k in SpinnerKind::ALL {
        spans.push(Spinner::new(k).style(t.accent()).span_at(cx.t, &t));
        spans.push(Span::raw(" "));
    }
    let lines = vec![
        Line::from(spans),
        Line::from(Span::styled(
            " each family holds its first glyph — the state still reads, nothing moves",
            t.dim(),
        )),
    ];
    Paragraph::new(lines).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "spinner",
    name: "Spinner",
    ty: "twolab_tui::spinner::Spinner",
    summary: "That something is in progress — and, by its family, which kind of progress.",
    description: "Six one-cell animations, each for one state, so a column of rows says working, waiting, idle and filling up at a glance. Glyphs are braille and the four quadrant blocks only — one cell wide even on a terminal that draws ambiguous-width glyphs wide (the CJK setting), so a spinner never pushes a table out of line there. Drive it with one shared tick or with elapsed time; reduced motion holds the first glyph.",
    used_in: "llmux dashboard: in-flight requests (braille for Claude, block for Codex), the current account's pulse, cooldown clocks, accounts filling up.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/anim.rs",
        license: "MIT",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "Spinner::new(kind)",
            ty: "SpinnerKind",
            default: None,
            description: "Braille, Block, Clock, Pulse, Drift or Breathe.",
        },
        PropDoc {
            name: ".interval(d)",
            ty: "Duration",
            default: Some("120 ms"),
            description: "Time per tick when driven by time.",
        },
        PropDoc {
            name: ".style(s)",
            ty: "Style",
            default: Some("Style::new()"),
            description: "Usually theme.status(…) for the row's state.",
        },
        PropDoc {
            name: "SpinnerKind::glyph(frame)",
            ty: "fn(usize) -> char",
            default: None,
            description: "The glyph at a shared tick counter.",
        },
        PropDoc {
            name: ".span_at(t, &theme)",
            ty: "Span<'static>",
            default: None,
            description: "The glyph at elapsed time t; the still glyph under reduced motion.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Reduced motion (Theme::with_motion(Motion::Reduced)) holds each family on its first glyph.",
        "Never the only signal: put a word beside it (\"2 in flight\", \"resets in 1h\").",
        "One cell wide on every terminal, including one that draws East-Asian Ambiguous glyphs wide (checked with width_cjk in the tests).",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "families",
            title: "Six families",
            description: "One family per state.",
            cols: 44,
            rows: 6,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(families)),
        },
        DemoDoc {
            id: "rows",
            title: "In a table",
            description: "A column of states in an accounts table.",
            cols: 52,
            rows: 6,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(in_rows)),
        },
        DemoDoc {
            id: "reduced",
            title: "Reduced motion",
            description: "Still frames that still say the state.",
            cols: 78,
            rows: 2,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(reduced)),
        },
    ],
};
