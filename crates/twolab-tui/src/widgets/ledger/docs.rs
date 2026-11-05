use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Amount, Tier};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::format;
use crate::text::{self, Align};
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::ledger::{Amount, Tier};

// A per-model row: dimmer than the total above it.
let cell = Amount::new(row.cost_usd).tier(Tier::Detail).theme(theme).line();

// A bucket total that leaves out an unpriced model says so.
let total = Amount::new(Some(sum))
    .marker(if any_unpriced { "+?" } else { "" })
    .theme(theme)
    .line();

// Size the column on its widest row.
let col = Amount::new(None).marker("+?").width();"#;

fn fit(lines: Vec<Line<'static>>, area: Rect) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(area.width)))
        .collect()
}

fn row(label: Vec<Span<'static>>, label_w: usize, amount: Amount) -> Line<'static> {
    let mut spans = text::pad_spans(label, label_w);
    spans.extend(amount.line().spans);
    Line::from(spans)
}

fn per_model(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t: &Theme = &cx.theme;
    let rows: [(&str, u64, Option<f64>); 6] = [
        ("claude/fable-5", 61_204_000, Some(1_204.5)),
        ("claude/opus-4-8[1m]", 41_237_880, Some(412.38)),
        ("codex/gpt-5.6-sol", 9_802_114, Some(87.12)),
        ("claude/sonnet-4-6", 1_022_500, Some(3.07)),
        ("codex/gpt-5.5", 88_014, Some(0.421)),
        ("grok/grok-5", 310_220, None),
    ];
    let mut lines = vec![Line::from(Span::styled(
        format!(
            " {}{:>7}  {:>9}",
            text::pad("model", 22, Align::Left),
            "tokens",
            "cost"
        ),
        t.dim(),
    ))];
    for (model, tokens, cost) in rows {
        lines.push(row(
            vec![
                Span::styled(format!(" {}", text::pad(model, 22, Align::Left)), t.text()),
                Span::styled(
                    format!("{:>7}  ", format::human_count(tokens)),
                    t.secondary(),
                ),
            ],
            32,
            Amount::new(cost).tier(Tier::Detail).theme(*t),
        ));
    }
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn totals(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t: &Theme = &cx.theme;
    let w = 30;
    let total = |day: &str, models: usize, sum: f64, marker: &str| {
        row(
            vec![
                Span::styled(format!(" {day}"), t.text()),
                Span::styled(format!("  {models} models"), t.secondary()),
            ],
            w,
            Amount::new(Some(sum)).marker(marker).theme(*t),
        )
    };
    let detail = |model: &str, cost: Option<f64>| {
        row(
            vec![Span::styled(format!("   {model}"), t.secondary())],
            w,
            Amount::new(cost).tier(Tier::Detail).theme(*t),
        )
    };
    let lines = vec![
        total("2026-11-05", 3, 1_706.07, ""),
        detail("claude/fable-5", Some(1_204.5)),
        detail("claude/opus-4-8[1m]", Some(412.38)),
        detail("codex/gpt-5.6-sol", Some(89.19)),
        total("2026-11-04", 2, 96.19, "+?"),
        detail("claude/sonnet-4-6", Some(96.19)),
        detail("grok/grok-5", None),
        Line::from(Span::styled(
            " +? — a model without a price is left out of the total",
            t.dim(),
        )),
    ];
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn sub_dollar(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t: &Theme = &cx.theme;
    let rows: [(&str, &str, Option<f64>, &str); 6] = [
        ("09:41:07", "opus-4-8[1m]", Some(0.0123), ""),
        ("09:41:12", "gpt-5.6-sol", Some(0.0076), ""),
        (
            "09:41:30",
            "fable-5",
            Some(0.99999),
            "rounds up, never $1.0000",
        ),
        ("09:42:02", "sonnet-4-6", Some(0.0), "free, and known to be"),
        ("09:42:15", "grok-5", None, "no price known"),
        ("09:43:51", "fable-5", Some(12.5), ""),
    ];
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(at, model, cost, note)| {
            let mut l = row(
                vec![
                    Span::styled(format!(" {at}  "), t.dim()),
                    Span::styled(text::pad(model, 13, Align::Left), t.text()),
                ],
                24,
                Amount::new(cost).int_width(3).tier(Tier::Detail).theme(*t),
            );
            let used = text::line_width(&l);
            l.spans
                .push(Span::raw(" ".repeat(35usize.saturating_sub(used))));
            l.spans.push(Span::styled(note, t.secondary()));
            l
        })
        .collect();
    Paragraph::new(fit(lines, area)).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "ledger",
    name: "Ledger amount",
    ty: "twolab_tui::ledger::Amount",
    summary: "How much it cost, lined up on the decimal point so a column reads like a ledger.",
    description: "The integer part is right-aligned in a fixed width with thousands separators, so every row's decimal point lands in one column; sub-dollar amounts keep four digits, larger ones two. Dollars carry the weight, cents sit a tier dimmer and the sign quietest; detail rows sit a tier below totals. An unknown or invalid amount reads — in the ones column, never a credible $0.",
    used_in: "llmux usage overlay: the cost column, with day and model-group totals over per-model rows, and +? on totals that leave out an unpriced model.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L1211-L1307",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "Amount::new(value) / Amount::unknown()",
            ty: "Option<f64>",
            default: None,
            description: "The amount; None, negative or non-finite reads —.",
        },
        PropDoc {
            name: ".tier(Tier)",
            ty: "Tier::Total | Tier::Detail",
            default: Some("Tier::Total"),
            description: "Totals at full strength, detail rows one tier dimmer.",
        },
        PropDoc {
            name: ".int_width(n)",
            ty: "usize",
            default: Some("7"),
            description: "Cells for the integer part (`999,999`); the decimal point sits after them.",
        },
        PropDoc {
            name: ".currency(sign)",
            ty: "&'static str",
            default: Some("\"$\""),
            description: "The sign before the integer part, measured in cells.",
        },
        PropDoc {
            name: ".marker(s)",
            ty: "impl Into<String>",
            default: Some("\"\""),
            description: "A qualifier after the fraction, e.g. `+?` on a total with an unpriced part.",
        },
        PropDoc {
            name: ".line() / .width()",
            ty: "Line<'static> / usize",
            default: None,
            description: "The amount for a table cell; the cells of its widest form, for sizing the column.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Tiers are weight, not just colour: in mono the sign and cents are dim and detail dollars dim, total dollars normal.",
        "Unknown is the — dash, never $0 — a screen reader and a skimming eye both read “no price”, not “free”.",
        "The decimal point holds one column, so magnitudes compare by length at a glance.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "per-model",
            title: "Per-model cost",
            description: "A day's models, detail tier; the unpriced one reads —.",
            cols: 46,
            rows: 7,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(per_model)),
        },
        DemoDoc {
            id: "totals",
            title: "Totals over detail",
            description: "Day totals at full strength over their models, +? where a model has no price.",
            cols: 56,
            rows: 8,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(totals)),
        },
        DemoDoc {
            id: "sub-dollar",
            title: "Sub-dollar amounts",
            description: "Per-request costs keep four digits; a carry rounds up cleanly; zero and unknown differ.",
            cols: 62,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(sub_dollar)),
        },
    ],
};
