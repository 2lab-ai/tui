use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::{Phase, Working};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};

const USAGE: &str = r#"use twolab_tui::working::{Working, WorkTimer};

// The host owns the clock: `now` is its monotonic time since any origin.
let row = Working::new("Working")
    .timer(&timer, now)                 // elapsed, paused while it waits
    .header_age(now - header_changed)   // the shimmer restarts with the header
    .inline("2 background tasks")
    .detail("Reading crates/twolab-tui/src/text.rs");
let h = row.height(area.width);
frame.render_widget(row.at(now, theme), area);"#;

/// Draw `w` into `rows` rows starting at `y`, clipped to the area.
fn place(buf: &mut Buffer, area: Rect, y: u16, rows: u16, w: Working<'_>, cx: &DemoCx) {
    if y >= area.bottom() {
        return;
    }
    let h = rows.min(area.bottom() - y);
    let r = Rect::new(area.x, y, area.width, h);
    w.at(cx.t, cx.theme).render(r, buf);
}

fn running(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    // The turn began 12 s before the demo opened; the clock runs with it.
    let w = Working::new("Working")
        .elapsed(Duration::from_secs(12) + cx.t)
        .inline("2 background tasks")
        .detail("Reading crates/twolab-tui/src/widgets/working/mod.rs and its tests");
    place(buf, area, area.y, area.height, w, cx);
}

fn paused(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let w = Working::new("Waiting for approval")
        .elapsed(Duration::from_secs(65))
        .paused(true)
        .detail("cargo test --workspace wants to run outside the sandbox");
    place(buf, area, area.y, area.height, w, cx);
}

fn done(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let ok = Working::new("Done")
        .phase(Phase::Done)
        .elapsed(Duration::from_secs(4 * 60 + 12))
        .inline("14 files changed, 3 commits");
    let failed = Working::new("Failed")
        .phase(Phase::Failed)
        .elapsed(Duration::from_secs(38))
        .detail("cargo clippy: 3 warnings treated as errors");
    place(buf, area, area.y, 1, ok, cx);
    place(buf, area, area.y.saturating_add(1), 2, failed, cx);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let w = Working::new("Preparing bootstrap diagnostics")
        .elapsed(Duration::from_secs(3 * 60 + 41) + cx.t)
        .detail("Checking 한국어 경로와 emoji 🚀 names in /srv/llmux/accounts");
    place(buf, area, area.y, area.height, w, cx);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "working",
    name: "Working",
    ty: "twolab_tui::working::Working",
    summary: "What the agent is doing right now, for how long, and which key stops it.",
    description: "The status row above an agent's composer: a braille spinner, the header as a shimmer, the elapsed time with the interrupt key, an optional inline note and a detail wrapped beneath (three lines at most, the last ending in … when cut). The host passes elapsed time and a paused flag, or a pause-aware WorkTimer read at its own clock; paused rows stop the clock and say so, done and failed rows close the turn. Clipped by cells; narrow glyphs by default.",
    used_in: "Agent composers (the turn in progress), llmux in-flight requests and the connecting screen, cooldown waits with a paused clock.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/status_indicator_widget.rs#L220-L322",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "Working::new(header)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "What it is doing: Working, Thinking, Reading files.",
        },
        PropDoc {
            name: ".elapsed(d)",
            ty: "Duration",
            default: Some("0s"),
            description: "Time the turn has run, pauses excluded; shown as 12s, 1m 05s, 1h 00m 00s.",
        },
        PropDoc {
            name: ".paused(b)",
            ty: "bool",
            default: Some("false"),
            description: "Waiting on the person: a slow drift, the clock still, \"paused\".",
        },
        PropDoc {
            name: ".timer(&timer, now)",
            ty: "&WorkTimer, Duration",
            default: None,
            description: "Elapsed and paused from a pause-aware timer read at the host's clock.",
        },
        PropDoc {
            name: ".phase(p)",
            ty: "Phase",
            default: Some("Running"),
            description: "Running, Paused, Done (✓ Done in 4m 12s) or Failed (✗ Failed after 38s).",
        },
        PropDoc {
            name: ".header_age(d)",
            ty: "Duration",
            default: Some("t"),
            description: "Time since the header changed; the shimmer restarts with it.",
        },
        PropDoc {
            name: ".interrupt(keys) / .no_interrupt()",
            ty: "impl Into<Cow<str>>",
            default: Some("\"esc\""),
            description: "The key that stops the turn, shown while running or paused.",
        },
        PropDoc {
            name: ".inline(note)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "Short context after the clock; it never shifts while the clock ticks.",
        },
        PropDoc {
            name: ".detail(text) / .detail_lines(n)",
            ty: "impl Into<Cow<str>>, usize",
            default: Some("3 lines"),
            description: "Wrapped by words beneath the row; the last kept line ends in ….",
        },
        PropDoc {
            name: ".glyphs(g)",
            ty: "WorkingGlyphs",
            default: Some("Narrow"),
            description: "Narrow ( - , ↳) or Unicode (codex's •, ·, └ — Ambiguous width).",
        },
        PropDoc {
            name: ".at(t, theme) / .lines_at(t, &theme, width)",
            ty: "WorkingFrame / Vec<Line>",
            default: None,
            description: "A widget for one frame, or the fitted lines for your own layout; .height(width) sizes it.",
        },
    ],
    keyboard: &[(
        "esc (host-bound)",
        "Shown as the interrupt key; the host handles the press.",
    )],
    a11y: &[
        "Reduced motion holds the spinner on its first glyph and shows the header in plain bold; the clock still counts.",
        "State is in words, not only colour: \"paused\", ✓ Done, ✗ Failed.",
        "Default glyphs are East-Asian Narrow (braille,  - , ↳, ✓, ✗), so the row keeps its width on CJK terminals.",
        "Mono (NO_COLOR) keeps the key bold and the hint dim, with no colour.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "running",
            title: "Running",
            description: "The clock ticks with the demo; an inline note and a detail line.",
            cols: 76,
            rows: 2,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(running)),
        },
        DemoDoc {
            id: "paused",
            title: "Waiting for approval",
            description: "Paused: the clock stops at 1m 05s and the row says so.",
            cols: 76,
            rows: 2,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(paused)),
        },
        DemoDoc {
            id: "done",
            title: "Done and failed",
            description: "The turn closed: how long it took, and what it left.",
            cols: 60,
            rows: 3,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(done)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow",
            description: "28 cells: the row clips with …, the detail wraps by words in cells.",
            cols: 28,
            rows: 4,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
