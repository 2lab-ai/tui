use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{Fact, FactTable, compact, grouped, money};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::theme::{Level, Theme};
use crate::widgets::rule::Rule;

const USAGE: &str = r#"use twolab_tui::fact_table::{Fact, FactTable, compact, grouped, money};

let table = FactTable::new(theme)
    .max_columns(3) // group by meaning; omit for "as many as fit"
    .fact(Fact::new("requests", grouped(3_665)))
    .fact(Fact::new("tokens in", compact(41_200_000)))
    .fact(Fact::new("cache hit", "95.4").unit("%"))
    .fact(Fact::new("cost", money(14.10)))
    .fact(Fact::maybe("ttft", ttft_ms.map(|ms| format!("{:.1}", ms / 1000.0))).unit("s"));

let rows = table.height(area.width); // rows it needs at this width
frame.render_widget(&table, area);"#;

/// A month of gateway usage, grouped down the columns: counts and cost,
/// tokens, cache and latency.
fn month(t: &Theme) -> Vec<Fact> {
    vec![
        Fact::new("requests", grouped(3_665)),
        Fact::new("failed", grouped(14)),
        Fact::new("cost", money(14.10)),
        Fact::new("tokens in", compact(41_200_000)),
        Fact::new("tokens out", compact(6_900_000)),
        Fact::new("reasoning", compact(2_300_000)),
        Fact::new("cache hit", "95.4")
            .unit("%")
            .style(t.level(Level::Ok)),
        Fact::new("ttft", "2.1").unit("s"),
        Fact::new("throughput", "58").unit("tok/s"),
    ]
}

/// A heading rule, then the table under it.
fn titled(area: Rect, buf: &mut Buffer, t: &Theme, title: &str, table: &FactTable) {
    if area.is_empty() {
        return;
    }
    let heading = Rule::new()
        .style(t.border())
        .label(Span::styled(title.to_string(), t.secondary()));
    (&heading).render(Rect { height: 1, ..area }, buf);
    let body = Rect {
        y: area.y + 1,
        height: area.height - 1,
        ..area
    };
    table.render(body, buf);
}

fn dashboard(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let table = FactTable::new(*t).max_columns(3).facts(month(t));
    titled(area, buf, t, "usage · this month", &table);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let table = FactTable::new(*t).facts(month(t));
    titled(area, buf, t, "usage", &table);
}

fn unknown(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let table = FactTable::new(*t).max_columns(2).facts([
        Fact::new("5h window", "42").unit("%"),
        Fact::new("7d window", "87")
            .unit("%")
            .style(t.level(Level::Warn)),
        Fact::new("resets", "in 1h 12m"),
        Fact::new("tokens in", compact(12_400_000)),
        Fact::maybe("requests", None::<String>),
        Fact::maybe("cache hit", None::<String>).unit("%"),
        Fact::unknown("cost"),
    ]);
    titled(area, buf, t, "subscription plan · this week", &table);
    if area.height > 6 {
        let note = Line::from(Span::styled(
            "  a plan reports limits, not requests, cache or spend: — rather than 0",
            t.dim(),
        ));
        buf.set_line(area.x, area.y + 6, &note, area.width);
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "fact-table",
    name: "Fact table",
    ty: "twolab_tui::fact_table::FactTable",
    summary: "The numbers, aligned, in as many columns as fit.",
    description: "Label/value facts packed down then across into as many columns as the width holds, the way df prints: labels dim, values right-aligned so their last digits line up, units in a column of their own. The table is as wide as its content, never stretched; a narrow width drops to fewer columns before any label is cut. Unknown values read as —, and a live value can reserve its widest form so nothing jitters.",
    used_in: "fx's /usage dashboard and info panels; llmux's usage overlay and per-account quota summary.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/footer/usage_presentation.zig#L316-L361",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "FactTable::new(theme)",
            ty: "Theme",
            default: None,
            description: "Labels and units in theme.dim(), values in theme.text(), unknowns dim.",
        },
        PropDoc {
            name: ".fact(Fact::new(label, value))",
            ty: "Fact",
            default: None,
            description: "One fact. Fact::unknown(label) and Fact::maybe(label, Option) read as —.",
        },
        PropDoc {
            name: "Fact::unit(u) / ::style(s) / ::reserve(cells)",
            ty: "String / Style / usize",
            default: Some("none / theme.text() / 0"),
            description: "Unit column after the value; the value's style (e.g. theme.level(..)); the widest form a live value can take.",
        },
        PropDoc {
            name: ".max_columns(n)",
            ty: "usize",
            default: Some("as many as fit"),
            description: "Cap the columns, e.g. to keep related facts in one column.",
        },
        PropDoc {
            name: ".columns(width) / .height(width) / .lines(width)",
            ty: "usize / u16 / Vec<Line<'static>>",
            default: None,
            description: "The layout at a width, and the rows as lines for a host's own paragraph.",
        },
        PropDoc {
            name: "grouped(n) / compact(n) / money(x)",
            ty: "fn -> String",
            default: None,
            description: "fx's number formats: 3,665 · 6.9M · $1,234.56 ($0.5000 under a dollar).",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Every number has a word beside it; colour on a value (a quota level) only repeats what the number says.",
        "Unknown is —, never 0, and drops its unit.",
        "Never cut silently: a short area ends in `+N more`; a too-narrow single column cuts labels with …, never values.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "dashboard",
            title: "Usage dashboard",
            description: "Requests, tokens in/out, cache hit and cost, three columns grouped by meaning.",
            cols: 100,
            rows: 4,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(dashboard)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow: one column",
            description: "The same facts at 36 columns: one column, nothing cut. Widen it to see them re-flow.",
            cols: 36,
            rows: 10,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
        DemoDoc {
            id: "unknown",
            title: "Unknown values",
            description: "A plan that reports limits but not requests, cache or cost.",
            cols: 76,
            rows: 7,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(unknown)),
        },
    ],
};
