use std::time::{Duration, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Banner, Event, Flash, active, flash_or};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::format;
use crate::text::{self, Align};

const USAGE: &str = r#"use std::time::Duration;
use twolab_tui::banner::{Banner, Event, Flash};

// Read the clock once per frame; the banner takes no row when nothing is on.
if let Some(b) = Banner::for_events(&events, now) {
    frame.render_widget(b.offset(utc_offset_secs).theme(theme), banner_row);
}

// The last action's result, for three seconds; then the key legend again.
let flash = Flash::ok("copied 4,132 bytes → pbcopy")
    .elapsed(since_copy)
    .ttl(Duration::from_secs(3))
    .theme(theme);
let footer = twolab_tui::banner::flash_or(Some(&flash), legend, width);"#;

/// 2026-11-05 09:30:00 UTC — the demos' fixed "now".
const NOW: u64 = 1_793_871_000;
/// Seoul, UTC+9: the host's zone.
const OFFSET: i64 = 9 * 3_600;

fn instant(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn events() -> [Event; 4] {
    [
        Event::new(
            "Fable 5 available on every plan",
            instant(1_793_664_000), // 11/03 00:00 UTC
            instant(1_794_441_600), // 11/12 00:00 UTC
        ),
        Event::new(
            "Codex maintenance — requests queue, nothing is lost",
            instant(1_793_869_200), // 11/05 09:00 UTC
            instant(1_793_873_700), // 11/05 10:15 UTC
        ),
        Event::new(
            "Double limits for the weekend",
            instant(1_794_150_000), // 11/08 15:00 UTC
            instant(1_794_322_800),
        ),
        Event::new(
            "gpt-5.6-sol preview",
            instant(1_793_491_200),
            instant(1_793_836_800), // 11/05 00:00 UTC
        ),
    ]
}

fn fit(lines: Vec<Line<'static>>, area: Rect) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(area.width)))
        .collect()
}

fn event(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let now = instant(NOW) + cx.t;
    let all = events();
    let mut lines = vec![match Banner::for_events(&all, now) {
        Some(b) => b.offset(OFFSET).theme(*t).line(area.width),
        None => Line::default(),
    }];
    lines.push(Line::from(vec![
        Span::styled(" llmux", t.accent()),
        Span::styled("  6 accounts · 2 in flight · ↻3m", t.secondary()),
    ]));
    lines.push(Line::from(Span::styled(
        " events — the one on that ends first takes the strip",
        t.dim(),
    )));
    let shown = active(&all, now).map(|e| e.text.clone());
    for e in &all {
        let (state, when) = if Some(&e.text) == shown.as_ref() {
            (
                "shown",
                format!("until {}", format::month_day_hm(e.until, OFFSET)),
            )
        } else if e.is_active(now) {
            (
                "on",
                format!("until {}", format::month_day_hm(e.until, OFFSET)),
            )
        } else if now < e.from {
            (
                "later",
                format!("from {}", format::month_day_hm(e.from, OFFSET)),
            )
        } else {
            (
                "ended",
                format!("at {}", format::month_day_hm(e.until, OFFSET)),
            )
        };
        let style = if state == "shown" {
            t.accent()
        } else {
            t.secondary()
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {}", text::pad(state, 6, Align::Left)), style),
            Span::styled(text::pad(&e.text, 34, Align::Left), t.text()),
            Span::styled(format!(" {when}"), t.dim()),
        ]));
    }
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn flash(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let cycle = Duration::from_secs(4);
    let n = (cx.t.as_millis() / cycle.as_millis()) as usize;
    let since = Duration::from_millis((cx.t.as_millis() % cycle.as_millis()) as u64);
    let samples = [
        Flash::ok("copied 4,132 bytes → pbcopy"),
        Flash::fail("save failed: ~/Downloads is read-only"),
        Flash::warn("no clipboard tool — sent with OSC 52"),
    ];
    let live = samples[n % samples.len()]
        .clone()
        .elapsed(since)
        .ttl(Duration::from_secs(3))
        .theme(*t);
    let legend = Line::from(Span::styled(
        "←→ switch · c copy · s save · esc close",
        t.dim(),
    ));
    let w = area.width.saturating_sub(1);
    let mut lines = vec![
        Line::from(Span::styled(
            " raw · request 0f3a · claude/opus-4-8[1m]",
            t.secondary(),
        )),
        Line::from(
            [Span::raw(" ")]
                .into_iter()
                .chain(flash_or(Some(&live), legend, w).spans)
                .collect::<Vec<_>>(),
        ),
        Line::from(Span::styled(" each outcome, fresh:", t.dim())),
    ];
    for f in samples {
        let mut spans = vec![Span::raw(" ")];
        spans.extend(f.theme(*t).line().map(|l| l.spans).unwrap_or_default());
        lines.push(Line::from(spans));
    }
    Paragraph::new(fit(lines, area)).render(area, buf);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let now = instant(NOW) + cx.t;
    let b = Banner::new(
        "Fable 5 available on every plan",
        instant(1_794_441_600),
        now,
    )
    .offset(OFFSET)
    .theme(*t);
    let lines: Vec<Line> = [76u16, 56, 40, 23, 12]
        .into_iter()
        .map(|w| {
            let mut spans = vec![Span::styled(format!("{w:>3} "), t.dim())];
            spans.extend(b.line(w).spans);
            Line::from(spans)
        })
        .collect();
    Paragraph::new(fit(lines, area)).render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "banner",
    name: "Banner and flash",
    ty: "twolab_tui::banner::Banner",
    summary: "Something is on until a deadline and how long is left — and whether the last action worked.",
    description: "Banner is a one-row strip for a timed event: the event in bold, until when on the host's wall clock, and a ticking countdown; it is shown only while an event is on, the earliest deadline winning. Flash is a short ok / warn / fail message about the last action that expires after a few seconds, handing the row back to the key legend. Neither reads a clock — the host passes now and the elapsed time.",
    used_in: "llmux dashboard: the event banner over the header (`event_banners` in the config); the raw viewer's copy / save flash and the footer status line.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L330-L366",
        license: "MIT",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "Banner::new(text, until, now) / Banner::for_events(&events, now)",
            ty: "(impl Into<String>, SystemTime, SystemTime)",
            default: None,
            description: "One event, or the active one of many (from ≤ now < until, earliest until wins); None when nothing is on.",
        },
        PropDoc {
            name: ".offset(secs)",
            ty: "i64",
            default: Some("0 (UTC)"),
            description: "The host's UTC offset for the `until 11/12 09:00` wall clock.",
        },
        PropDoc {
            name: ".line(width)",
            ty: "Line<'static>",
            default: None,
            description: "The strip in at most `width` cells: the deadline gives way first, then the event clips, then the countdown shortens.",
        },
        PropDoc {
            name: "Flash::ok / warn / fail(text)",
            ty: "impl Into<String>",
            default: None,
            description: "A message led by its outcome word.",
        },
        PropDoc {
            name: ".elapsed(d) / .ttl(d)",
            ty: "Duration",
            default: Some("0 / 5 s"),
            description: "How long ago the action happened; how long the flash stays.",
        },
        PropDoc {
            name: "flash_or(flash, fallback, width)",
            ty: "Line<'static>",
            default: None,
            description: "The flash while it lives, else the line it stands in for.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "The flash leads with a word (ok / warn / fail), so the outcome reads without colour; in mono fail is reversed.",
        "The countdown keeps counting under reduced motion — it is a clock, not an animation.",
        "Nothing is cut silently: a clipped event ends in …, and the countdown is the last thing to go.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "event",
            title: "Event banner",
            description: "Four configured events: the active one ending first takes the strip and counts down (UTC+9).",
            cols: 76,
            rows: 7,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(event)),
        },
        DemoDoc {
            id: "flash",
            title: "Flash",
            description: "The raw viewer's footer: a flash for three seconds, then the key legend; each outcome below.",
            cols: 56,
            rows: 6,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(flash)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow widths",
            description: "The same banner from 76 cells down to 12.",
            cols: 80,
            rows: 5,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
