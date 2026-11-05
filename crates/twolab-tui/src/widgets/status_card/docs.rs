use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::{Limit, StatusCard};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};

const USAGE: &str = r#"use twolab_tui::status_card::{Limit, StatusCard};

let card = StatusCard::new()
    .field("Account", "ai3@iq.io")
    .field("Plan", "Max 20x")
    .gap()
    .limit("5h limit", Limit::used(45.0).resets("09:25"))
    .limit("7d limit", Limit::unknown())   // shown as —, never 0%
    .theme(theme);
frame.render_widget(&card, area);"#;

/// An llmux account in its detail pane.
fn account_card() -> StatusCard<'static> {
    StatusCard::new()
        .field("Account", "ai3@iq.io")
        .field("Plan", "Max 20x")
        .field("Model", "opus-4.5 (thinking high)")
        .field("Session", "2 in flight, last request 4s ago")
        .gap()
        .limit("5h limit", Limit::used(45.0).resets("09:25"))
        .limit("7d limit", Limit::used(82.0).resets("Nov 9 09:55"))
        .limit("7d opus", Limit::used(97.0).resets("Nov 9 09:55"))
}

fn account(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    account_card().theme(cx.theme).render(area, buf);
}

fn unknown(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    StatusCard::new()
        .field("Account", "dev1@2lab.ai")
        .unknown("Plan")
        .field("Model", "opus-4.5")
        .gap()
        .limit("5h limit", Limit::used(12.0).resets("14:05"))
        .limit("7d limit", Limit::unknown().resets("Nov 11 02:00"))
        .unknown("Credits")
        .theme(cx.theme)
        .render(area, buf);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    account_card().theme(cx.theme).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "status-card",
    name: "Status card",
    ty: "twolab_tui::status_card::StatusCard",
    summary: "An account or session at a glance: its facts in aligned rows and how much of each limit is left.",
    description: "Label: value rows aligned on the widest label in cells, and a compact bar per usage limit showing the part left, coloured by how much is used, with when it resets. As the card narrows the bar goes first and the percentage stays; the reset time moves to its own line instead of being cut, and long values wrap by words under their column. Unknown values read —, never a made-up 0%. Braille bar glyphs by default (codex's █ is East-Asian Ambiguous).",
    used_in: "llmux account detail pane and account modal (5h / 7d windows, resets); an agent's /status card.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/status/format.rs#L7-L93",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "StatusCard::new()",
            ty: "StatusCard",
            default: None,
            description: "An empty card; add rows in display order.",
        },
        PropDoc {
            name: ".field(label, value)",
            ty: "impl Into<Cow<str>> ×2",
            default: None,
            description: "A text row; blank text shows —. Wraps by words under the value column.",
        },
        PropDoc {
            name: ".unknown(label)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "A row whose value is not known: —.",
        },
        PropDoc {
            name: ".limit(label, Limit)",
            ty: "Limit",
            default: None,
            description: "A usage row: [bar] 55% left (resets 09:25).",
        },
        PropDoc {
            name: "Limit::left(p) / ::used(p) / ::unknown()",
            ty: "f64 (0–100)",
            default: None,
            description: "How much is left; .resets(text) when it renews; .thresholds(warn, crit) on the used fraction (0.8, 0.95).",
        },
        PropDoc {
            name: ".gap()",
            ty: "—",
            default: None,
            description: "A blank row between sections.",
        },
        PropDoc {
            name: ".segments(n) / .glyphs(g)",
            ty: "usize / BarGlyphs",
            default: Some("20, BarGlyphs::NARROW"),
            description: "Bar length and glyphs: NARROW ⣿⣀, BLOCK █░ (codex, Ambiguous width), ASCII #-.",
        },
        PropDoc {
            name: ".indent(n) / .theme(t)",
            ty: "usize / Theme",
            default: Some("2, Theme::ansi()"),
            description: "Cells before labels; colours.",
        },
        PropDoc {
            name: ".lines(width) / .height(width)",
            ty: "Vec<Line> / u16",
            default: None,
            description: "The laid-out card for your own layout, and its height.",
        },
        PropDoc {
            name: "limit_bar(percent_left, segments, glyphs)",
            ty: "String",
            default: None,
            description: "The bar alone, as text.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Every limit says its number in words (55% left); the bar and its colour only repeat it.",
        "Mono (NO_COLOR) keeps levels in the percentage's weight: normal, bold, bold reversed.",
        "Unknown is —, never 0%: an empty bar would claim the limit is spent.",
        "Default bar glyphs are braille (East-Asian Narrow), so the column keeps its width on CJK terminals.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "account",
            title: "Account status",
            description: "Account, plan, model, and the 5h / 7d windows — one of them nearly spent.",
            cols: 76,
            rows: 8,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(account)),
        },
        DemoDoc {
            id: "unknown",
            title: "Unknown values",
            description: "No plan or weekly figure yet: —, with the reset time still shown.",
            cols: 64,
            rows: 7,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(unknown)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow",
            description: "36 cells: bars give way to the percentages, resets and long values wrap.",
            cols: 36,
            rows: 13,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
