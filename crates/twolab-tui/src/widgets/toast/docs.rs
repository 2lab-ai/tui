use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Anchor, Toast, ToastQueue, ToastStack};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::text;
use crate::theme::Theme;

const USAGE: &str = r#"use std::time::Duration;
use twolab_tui::toast::{Anchor, Toast, ToastQueue};

// Once: the queue lives with the app. `now` is time since the app started.
let mut toasts = ToastQueue::new().max_visible(3);

// When something happens. A newer toast with the same key replaces it.
toasts.push(
    Toast::fail("usage refresh: 3 of 5 failed")
        .lines(failures.iter().map(|f| format!("{}  {}", f.account, f.error)))
        .key("refresh"),
    now,
);

// Every frame: retire what is done, draw the rest over the dashboard.
toasts.tick(now);
let layout = toasts
    .stack(now)
    .anchor(Anchor::TopRight)
    .offset(1) // keep the header row clear
    .theme(theme)
    .render(frame.area(), frame.buffer_mut());
// A click on a toast: layout.hit_test(col, row) -> index of the toast drawn.
// Wake up for the next redraw when a toast goes down.
let wake_at: Option<Duration> = toasts.next_deadline();"#;

/// A dashboard to float the toasts over.
fn backdrop(area: Rect, buf: &mut Buffer, t: &Theme) {
    let rows = [
        " llmux 0.2.26 · 29 accounts · 3 in flight · ↻ 2m",
        "",
        "   group   #  account                 status    7d left   reset",
        "   claude  1  ai1@insightquest.io     ok          62%     3d 4h",
        "   claude  2  ai2@insightquest.io     cooling      0%     1h 12m",
        "   claude  3  ai3@insightquest.io     ok          88%     5d 1h",
        "   codex   4  codex:icedac@gmail.com  ok          41%     2d 9h",
        "   codex   5  codex:ai5@iq.io         error        —      —",
        "   grok    6  grok:ai6@iq.io          ok          97%     6d 2h",
        "",
        "   model                    req      tok       $",
        "   claude-opus-5-5          698.3k   243.6G    $112.3k",
        "   gpt-6-astra              120.9k    41.2G     $18.0k",
        "   grok-5                    12.4k     3.1G      $1.2k",
        "",
        "   activity",
        "   09:31 ai3  claude-opus-5-5   1.2k tok   2.1s",
        "   09:31 ai1  claude-opus-5-5   8.9k tok  11.4s",
        "   09:30 ai5  gpt-6-astra        401 token expired",
    ];
    let lines: Vec<Line> = rows
        .iter()
        .map(|r| {
            let style = if r.starts_with(" llmux") {
                t.secondary()
            } else {
                t.dim()
            };
            text::truncate_line(Line::from(Span::styled(*r, style)), usize::from(area.width))
        })
        .collect();
    Paragraph::new(lines).render(area, buf);
}

fn failures() -> [&'static str; 5] {
    [
        "ai15@insightquest.io     401 token expired",
        "ai7@insightquest.io      timeout after 10s",
        "codex:icedac@gmail.com   connection refused",
        "grok:ai6@iq.io           429 rate limited",
        "ai9@insightquest.io      500 upstream error",
    ]
}

fn stack(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    backdrop(area, buf, &t);
    let toasts = [
        Toast::attention("claude needs attention").body("repo tui · tab 2 · pane 1"),
        Toast::ok("copied to clipboard").body("ai15@insightquest.io"),
        Toast::fail("usage refresh: 5 of 29 failed").lines(failures()),
    ];
    ToastStack::new(&toasts)
        .anchor(Anchor::TopRight)
        .offset(1)
        .theme(t)
        .render(area, buf);
}

/// When each toast arrives in the queue demo, in seconds of a 24 s loop.
fn script() -> Vec<(Duration, Toast)> {
    let ms = Duration::from_millis;
    vec![
        (
            ms(0),
            Toast::progress("refreshing 29 accounts").key("refresh"),
        ),
        (
            ms(1_500),
            Toast::fail("usage refresh: 5 of 29 failed")
                .lines(failures())
                .key("refresh"),
        ),
        (
            ms(2_000),
            Toast::ok("copied ai15@insightquest.io").ttl(ms(3_000)),
        ),
        (
            ms(2_400),
            Toast::ok("codex finished").body("repo llmux · tab 1"),
        ),
        (ms(2_600), Toast::info("config reloaded")),
        (ms(2_800), Toast::warn("ai2 cooling until 10:42")),
        (ms(3_000), Toast::info("update ready").body("llmux 0.2.27")),
        (
            ms(3_200),
            Toast::attention("claude needs attention").body("repo tui · tab 2"),
        ),
        (
            ms(12_000),
            Toast::ok("usage refresh: 29 of 29 ok").key("refresh"),
        ),
    ]
}

const LOOP: Duration = Duration::from_secs(24);

/// The queue as it stands `t` into the loop: replayed from the script, so
/// any frame is the same however the page steps time.
fn queue_at(t: Duration) -> (ToastQueue, Duration) {
    let now = Duration::from_nanos((t.as_nanos() % LOOP.as_nanos()) as u64);
    let mut q = ToastQueue::new().max_visible(2).cap(3);
    for (at, toast) in script() {
        if at > now {
            break;
        }
        q.push(toast, at);
    }
    q.tick(now);
    (q, now)
}

fn queue(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    let (q, now) = queue_at(cx.t);
    let side = area.width.min(30);
    let left = Rect {
        width: side,
        ..area
    };
    let right = Rect {
        x: area.x + side,
        width: area.width - side,
        ..area
    };
    let mut lines = vec![
        Line::from(Span::styled(
            format!(" t = {:>4.1} s", now.as_secs_f32()),
            t.accent(),
        )),
        Line::from(Span::styled(" 2 up at once · 3 may wait", t.dim())),
        Line::from(vec![
            Span::styled(" waiting ", t.secondary()),
            Span::styled(q.waiting().to_string(), t.text()),
            Span::styled(" · dropped ", t.secondary()),
            Span::styled(q.dropped().to_string(), t.text()),
        ]),
        Line::default(),
        Line::from(Span::styled(" up now        left", t.dim())),
    ];
    let mut any = false;
    for (_, toast, left_for) in q.entries(now) {
        any = true;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", toast.kind_of().glyph()),
                toast.kind_of().style(&t),
            ),
            Span::styled(text::pad(toast.title(), 13, text::Align::Left), t.text()),
            Span::styled(format!("{:>4.1}s", left_for.as_secs_f32()), t.secondary()),
        ]));
    }
    if !any {
        lines.push(Line::from(Span::styled(" nothing up", t.dim())));
    }
    let lines: Vec<Line> = lines
        .into_iter()
        .map(|l| text::truncate_line(l, usize::from(left.width)))
        .collect();
    Paragraph::new(lines).render(left, buf);
    backdrop(right, buf, &t);
    q.stack(now)
        .anchor(Anchor::TopRight)
        .offset(1)
        .narrow_width(0)
        .theme(t)
        .render(right, buf);
}

fn positions(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    backdrop(area, buf, &t);
    let toasts = [
        (
            Anchor::TopLeft,
            Toast::info("update ready").body("llmux 0.2.27"),
        ),
        (Anchor::TopCenter, Toast::progress("host · reconnecting")),
        (Anchor::TopRight, Toast::attention("claude waiting")),
        (Anchor::BottomLeft, Toast::warn("ai2 cooling")),
        (Anchor::BottomCenter, Toast::ok("copied").word("copied")),
        (
            Anchor::BottomRight,
            Toast::fail("refresh failed").body("ai7 timeout"),
        ),
    ];
    for (anchor, toast) in toasts {
        ToastStack::new([&toast])
            .anchor(anchor)
            .offset(1)
            .narrow_width(0)
            .theme(t)
            .render(area, buf);
    }
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    let phone = Rect {
        width: area.width.min(44),
        ..area
    };
    backdrop(phone, buf, &t);
    // Footer row the banners stay above.
    if phone.height > 0 {
        let footer = text::pad_spans(
            vec![Span::styled(" q quit · a accounts · ? help", t.dim())],
            usize::from(phone.width),
        );
        buf.set_line(
            phone.x,
            phone.bottom() - 1,
            &Line::from(footer),
            phone.width,
        );
    }
    let toasts = [
        Toast::attention("claude waiting").body("repo tui · tab 2"),
        Toast::ok("copied to clipboard"),
        Toast::fail("usage refresh: 5 of 29 failed").lines(failures()),
    ];
    ToastStack::new(&toasts)
        .anchor(Anchor::BottomCenter)
        .offset(1)
        .theme(t)
        .render(phone, buf);
    // herdr's panic: a 4-row terminal, a diagnostic on the first row, a
    // toast at the top. Here it becomes a banner inside the pane.
    if area.width > 46 {
        let pane = Rect {
            x: area.x + 46,
            width: area.width - 46,
            height: area.height.min(4),
            ..area
        };
        let rows = [
            Line::from(Span::styled(" ! config: unknown key `toast`", t.dim())),
            Line::from(Span::styled(" $ cargo test", t.secondary())),
            Line::from(Span::styled(" running 412 tests", t.secondary())),
            Line::from(Span::styled(" test result: ok", t.secondary())),
        ];
        let rows: Vec<Line> = rows
            .into_iter()
            .map(|l| text::truncate_line(l, usize::from(pane.width)))
            .collect();
        Paragraph::new(rows).render(pane, buf);
        Toast::ok("copied to clipboard")
            .theme(t)
            .render_at(pane, Anchor::TopRight, 1, buf);
        if area.height > 5 {
            let note = Line::from(Span::styled(" 4 rows, offset 1: a banner, inside", t.dim()));
            buf.set_line(
                pane.x,
                area.y + 5,
                &text::truncate_line(note, usize::from(pane.width)),
                pane.width,
            );
        }
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "toast",
    name: "Toast",
    ty: "twolab_tui::toast::Toast",
    summary: "A message that arrives over the screen, says what kind it is, and goes away on its own.",
    description: "A card anchored to a corner or the middle of an edge: the kind's word in the border, its glyph before the title, then one body line per fact, cut with … and ending in +N more. Several stack at one anchor without overlapping, and the last says how many did not fit or were dropped. ToastQueue shows them first in, first out, a few at a time, each for its own time to live, from a clock the host passes in. On a narrow or short area each toast is a one-row banner. Unlike Flash (one outcome line in the footer, replaced by the next), toasts float, carry several lines and queue.",
    used_in: "llmux dashboard (next): status messages that today are one row cut at 80 columns and gone after 5 s — a refresh with several failures becomes one card with a line per account. herdr: agent notifications, copy feedback and machine notices.",
    source: Source {
        project: "herdr",
        url: "https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/client/shell/notifications.rs#L10-L206",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "Toast::new(kind, title) / ::info / ::progress / ::attention / ::ok / ::warn / ::fail",
            ty: "(ToastKind, impl Into<String>)",
            default: None,
            description: "One message. Each kind has a glyph (▸ ⋯ ◉ ✓ ! ✗) and a word (note, working, waiting, ok, warn, fail); From<Outcome> and From<Status> convert.",
        },
        PropDoc {
            name: ".body(text) / .lines(iter)",
            ty: "&str / impl IntoIterator<Item: Into<String>>",
            default: Some("none"),
            description: "Body lines under the title, one fact each; `\\n` splits.",
        },
        PropDoc {
            name: ".max_body(n) / .max_width(cells)",
            ty: "usize / u16",
            default: Some("4 / 56"),
            description: "Body lines before +N more; widest card. Never wider or taller than the bounds.",
        },
        PropDoc {
            name: ".word(w) / .key(k) / .ttl(d)",
            ty: "impl Into<String> / impl Into<String> / Duration",
            default: Some("kind's word / none / 8 s attention and fail, else 5 s"),
            description: "The border word; a key a newer toast replaces in place; how long it stays once shown.",
        },
        PropDoc {
            name: ".render_at(bounds, anchor, offset, buf) -> Rect",
            ty: "(Rect, Anchor, u16, &mut Buffer)",
            default: None,
            description: "Draw alone, `offset` rows in from the anchored edge; always inside the bounds. Returns the click target.",
        },
        PropDoc {
            name: ".banner_line(width) / .offset_clear_of(bounds, anchor, base, &taken)",
            ty: "Line<'static> / Option<u16>",
            default: None,
            description: "The one-row form for a host's own status row; the offset that clears other rects (herdr's copy-toast offset).",
        },
        PropDoc {
            name: "ToastStack::new(&toasts).anchor(a).offset(n).gap(n).narrow_width(cols)",
            ty: "Anchor, u16, u16, u16",
            default: Some("TopRight, 0, 0, 64"),
            description: "Several at one anchor, first nearest the edge; banners at or below `narrow_width` columns. render() returns rects per toast and the hidden count.",
        },
        PropDoc {
            name: "ToastQueue::new().cap(n).max_visible(n)",
            ty: "usize, usize",
            default: Some("8, 3"),
            description: "push(toast, now), tick(now), visible(now), dismiss(id, now), next_deadline(), stack(now). The oldest waiting toast goes when the cap is hit; dropped() counts it.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "The kind reads without colour: its own glyph before the title and its word in the border; in mono fail is reversed and warn bold.",
        "Every kind glyph is one cell on a terminal that draws East-Asian Ambiguous glyphs wide (width_cjk = 1).",
        "Nothing is cut silently: titles and lines end in …, dropped lines say +N more, and the last toast counts the ones that did not fit or were dropped.",
        "Nothing moves under reduced motion — toasts do not animate; their time to live is a clock.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "stack",
            title: "Three kinds, stacked",
            description: "Waiting, ok and fail at the top right under the header row; the failure lists five accounts and says +2 more.",
            cols: 92,
            rows: 19,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(stack)),
        },
        DemoDoc {
            id: "queue",
            title: "Queue over time",
            description: "A burst of eight messages, two up at once and three allowed to wait: the refresh toast is replaced by its result, the oldest waiting ones are dropped and counted, each goes when its time is up.",
            cols: 110,
            rows: 18,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(queue)),
        },
        DemoDoc {
            id: "positions",
            title: "Six positions",
            description: "Top and bottom, left, centre and right, one row in from the edge.",
            cols: 96,
            rows: 16,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(positions)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow screens and herdr's panic",
            description: "At 44 columns a stack is banners above the footer. Right: a 4-row pane with a diagnostic row — the input that made herdr's copy toast panic — gets a banner inside the pane.",
            cols: 84,
            rows: 12,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
