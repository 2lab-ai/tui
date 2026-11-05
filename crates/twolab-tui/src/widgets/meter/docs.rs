use std::time::{Duration, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Fill, Meter};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::text::{self, Align};

const USAGE: &str = r#"use std::time::Duration;
use twolab_tui::meter::{Fill, Meter};

// 81 % of the weekly window used, resets in 3d 4h; drain toward the reset.
let cell = Meter::new(0.81)
    .fill(Fill::Left)
    .resets_in(Duration::from_secs(3 * 86_400 + 4 * 3_600))
    .theme(theme)
    .line(18); // a Line<'static> for a table Cell

// Or the wall-clock reset, at the host's own UTC offset.
let m = Meter::new(0.42).resets_at(resets_at, now, offset_secs).theme(theme);
frame.render_widget(&m, area);"#;

/// 2026-11-05 09:30:00 UTC — the demos' fixed "now".
const NOW_SECS: u64 = 1_793_871_000;
/// The host's zone: Seoul, UTC+9.
const OFFSET: i64 = 9 * 3_600;

const fn hms(h: u64, m: u64, s: u64) -> u64 {
    h * 3_600 + m * 60 + s
}

/// (account, 5h used, 5h resets in, 7d used, 7d resets in, parked)
type Row = (&'static str, Option<f64>, u64, Option<f64>, u64, bool);

const ACCOUNTS: [Row; 6] = [
    (
        "ai3@iq.io",
        Some(0.42),
        hms(2, 13, 5),
        Some(0.61),
        hms(76, 0, 40),
        false,
    ),
    (
        "zhuge@2lab.ai",
        Some(0.08),
        hms(4, 51, 12),
        Some(0.33),
        hms(135, 8, 0),
        false,
    ),
    (
        "simayi@2lab.ai",
        Some(0.77),
        hms(0, 38, 12),
        Some(0.71),
        hms(33, 2, 9),
        false,
    ),
    (
        "dev1@2lab.ai",
        Some(0.97),
        hms(0, 12, 40),
        Some(0.92),
        hms(22, 13, 0),
        true,
    ),
    ("ai2@iq.io", None, 0, Some(0.18), hms(146, 30, 0), false),
    ("lusu@2lab.ai", Some(0.55), hms(1, 2, 3), None, 0, false),
];

#[derive(Default)]
struct Accounts {
    absolute: bool,
    left: bool,
}

impl Accounts {
    fn meter(&self, used: Option<f64>, resets: u64, cx: &DemoCx) -> Meter {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS) + cx.t;
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(NOW_SECS + resets);
        let m = Meter::from_option(used)
            .fill(if self.left { Fill::Left } else { Fill::Used })
            .theme(cx.theme);
        match used {
            None => m.text("cold", 0),
            Some(_) if self.absolute => m.resets_at(at, now, OFFSET),
            Some(_) => m.resets_in(Duration::from_secs(resets).saturating_sub(cx.t)),
        }
    }
}

impl Demo for Accounts {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let name_w = 15usize;
        let gauge = (usize::from(area.width).saturating_sub(name_w + 3) / 2).clamp(1, 24);
        let gauge_u16 = u16::try_from(gauge).unwrap_or(1);
        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!(" {}", text::pad("account", name_w, Align::Left)),
                t.dim(),
            ),
            Span::styled(text::pad(" 5h", gauge + 1, Align::Left), t.dim()),
            Span::styled(" 7d", t.dim()),
        ])];
        for (name, u5, r5, u7, r7, parked) in ACCOUNTS {
            let mut spans = vec![
                Span::raw(" "),
                Span::styled(text::pad(name, name_w, Align::Left), t.text()),
                Span::raw(" "),
            ];
            spans.extend(self.meter(u5, r5, cx).over(parked).line(gauge_u16).spans);
            spans.push(Span::raw(" "));
            spans.extend(self.meter(u7, r7, cx).line(gauge_u16).spans);
            lines.push(Line::from(spans));
        }
        lines.push(Line::from(vec![
            Span::styled(" t ", t.accent()),
            Span::styled(
                if self.absolute {
                    "countdown"
                } else {
                    "wall clock"
                },
                t.secondary(),
            ),
            Span::styled(" · ", t.dim()),
            Span::styled("f ", t.accent()),
            Span::styled(
                if self.left { "fill used" } else { "fill left" },
                t.secondary(),
            ),
        ]));
        let lines: Vec<Line> = lines
            .into_iter()
            .map(|l| text::truncate_line(l, usize::from(area.width)))
            .collect();
        Paragraph::new(lines).render(area, buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match key.typed() {
            Some('t') => self.absolute = !self.absolute,
            Some('f') => self.left = !self.left,
            _ => return false,
        }
        true
    }
}

fn levels(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let w = area.width.saturating_sub(9).clamp(1, 26);
    let rest = |s: u64| Duration::from_secs(s).saturating_sub(cx.t);
    let rows: [(&str, Meter); 4] = [
        ("ok", Meter::new(0.42).resets_in(rest(hms(2, 13, 5)))),
        ("warn", Meter::new(0.78).resets_in(rest(hms(0, 38, 12)))),
        (
            "crit",
            Meter::new(0.95).over(true).resets_in(rest(hms(0, 12, 40))),
        ),
        ("unknown", Meter::unknown().text("cold", 0)),
    ];
    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|(name, m)| {
            let mut spans = vec![Span::styled(
                format!(" {}", text::pad(name, 8, Align::Left)),
                t.secondary(),
            )];
            spans.extend(m.theme(*t).line(w).spans);
            Line::from(spans)
        })
        .collect();
    lines.push(Line::from(Span::styled(
        " warn from 70 %, crit from 90 % used",
        t.dim(),
    )));
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(area.width)))
        .collect();
    Paragraph::new(lines).render(area, buf);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let m = Meter::new(0.64)
        .resets_in(Duration::from_secs(hms(2, 13, 5)).saturating_sub(cx.t))
        .theme(*t);
    let rows: [(u16, &str); 6] = [
        (26, "bar, reset, percent"),
        (13, "the narrowest full form"),
        (12, "the percent gives way first"),
        (6, "the bar is just the reset"),
        (3, "then the bold unit alone"),
        (1, "then only the fill"),
    ];
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(w, note)| {
            let mut spans = vec![Span::styled(format!("{w:>3} "), t.dim())];
            spans.extend(m.line(w).spans);
            spans.push(Span::raw(" ".repeat(usize::from(27 - w))));
            spans.push(Span::styled(note, t.secondary()));
            text::truncate_line(Line::from(spans), usize::from(area.width))
        })
        .collect();
    Paragraph::new(lines).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "meter",
    name: "Meter",
    ty: "twolab_tui::meter::Meter",
    summary: "How much of a quota window is used or left, when it resets, and whether that is fine.",
    description: "One row per window: a reverse-video fill shows the share, the reset time sits centred inside the bar with its larger unit bold, and a right-aligned percent label follows. Colour keys on the used share against low / high thresholds (0.70 / 0.90, like the web ui-meter) whichever way the bar fills. At narrow widths the label gives way first, then the reset's smaller unit — measured in cells, never on the ticking clock.",
    used_in: "llmux accounts table: the 5h and 7d (and 7d Fable) quota columns, filled toward the reset, `t` switching the countdown for the wall-clock reset.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L6143-L6385",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "Meter::new(used) / Meter::unknown()",
            ty: "f64 / —",
            default: None,
            description: "The used fraction (0.0–1.0); unknown draws no fill and `—`.",
        },
        PropDoc {
            name: ".thresholds(low, high)",
            ty: "(f64, f64)",
            default: Some("0.70, 0.90"),
            description: "Used fractions where the meter turns warn and crit.",
        },
        PropDoc {
            name: ".fill(Fill)",
            ty: "Fill::Used | Fill::Left",
            default: Some("Fill::Used"),
            description: "Fill with what is used, or drain what is left toward the reset. The label follows the fill; the colour keys on used.",
        },
        PropDoc {
            name: ".resets_in(d) / .resets_at(at, now, offset_secs)",
            ty: "Duration / (SystemTime, SystemTime, i64)",
            default: None,
            description: "The text in the bar: a countdown (`3d 4h`) or a wall-clock stamp (`11/08 15:00`) at the host's UTC offset.",
        },
        PropDoc {
            name: ".text(s, bold_cells)",
            ty: "(impl Into<String>, usize)",
            default: None,
            description: "Any other text in the bar (`cold`), its first cells bold.",
        },
        PropDoc {
            name: ".marker(c)",
            ty: "char",
            default: None,
            description: "A one-cell glyph pinned to the bar's last cell, so the centred time never moves.",
        },
        PropDoc {
            name: ".over(b) / .level(l)",
            ty: "bool / Level",
            default: Some("false / from thresholds"),
            description: "`!` after the percent (parked, past the host's limit); force a level when the host knows better.",
        },
        PropDoc {
            name: ".label(b)",
            ty: "bool",
            default: Some("true"),
            description: "Show the percent label beside the bar.",
        },
        PropDoc {
            name: ".line(width) / .full_width()",
            ty: "Line<'static> / u16",
            default: None,
            description: "The meter in exactly `width` cells; the narrowest width that shows everything.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "The share is said three ways: fill length, the percent number and the colour band — never by colour alone.",
        "In mono the fill is reversed video, warn is bold and crit is a bold reversed label; the empty part never inverts.",
        "Only spaces, the reset text and an optional marker are drawn — no bar glyphs that shift on a CJK terminal.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "accounts",
            title: "Accounts table",
            description: "5h and 7d windows for six accounts; t switches countdown and wall clock (UTC+9), f flips the fill.",
            cols: 66,
            rows: 8,
            animated: true,
            interactive: true,
            make: || Box::new(Accounts::default()),
        },
        DemoDoc {
            id: "levels",
            title: "Levels",
            description: "ok, warn and crit side by side, and a window nobody has measured.",
            cols: 44,
            rows: 5,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(levels)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow widths",
            description: "The same meter from 26 cells down to one: what gives way, in order.",
            cols: 60,
            rows: 6,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
