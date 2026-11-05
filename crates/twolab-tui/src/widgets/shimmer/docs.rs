use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{Cadence, Shimmer};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::color::{self, ColorLevel, Rgb};
use crate::text::{self, Align};
use crate::theme::Motion;

const USAGE: &str = r#"use twolab_tui::shimmer::Shimmer;

// `since` is the time since the label last changed: the sweep restarts with it.
let label = Shimmer::new("Working").style(theme.text());
frame.render_widget(label.at(since, theme), area);

// Or as spans inside your own status row.
let mut spans = vec![spinner_span, Span::raw(" ")];
spans.extend(Shimmer::new(&summary).spans_at(since, &theme));"#;

/// One row: the shimmer, clipped to the row.
fn row(buf: &mut Buffer, area: Rect, y: u16, x: u16, s: Shimmer<'_>, cx: &DemoCx) {
    if y >= area.bottom() || x >= area.right() {
        return;
    }
    let r = Rect::new(x, y, area.right() - x, 1);
    s.at(cx.t, cx.theme).render(r, buf);
}

fn headline(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    row(
        buf,
        area,
        area.y,
        area.x.saturating_add(1),
        Shimmer::new("Working").style(t.text().add_modifier(Modifier::BOLD)),
        cx,
    );
    if area.height > 1 {
        let note = Line::from(Span::styled(
            " waits 600 ms, then one pass every 4 s",
            t.dim(),
        ));
        buf.set_line(
            area.x,
            area.y + 1,
            &text::truncate_line(note, area.width as usize),
            area.width,
        );
    }
}

fn summary(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let lines = [
        "Preparing bootstrap diagnostics for crates/twolab-tui (14 files, 3 warnings)",
        "Summarising the 5h window: ai3@iq.io at 82%, zhuge@2lab.ai at 41%",
        "변경 사항 요약: 위젯 네 개와 색상 모듈, 셀 단위로 흐릅니다",
    ];
    for (i, s) in lines.iter().enumerate() {
        // Each line restarts its sweep as if it appeared 1.2 s after the last.
        let since =
            cx.t.saturating_sub(std::time::Duration::from_millis(1_200 * i as u64));
        let y = area.y.saturating_add(i as u16);
        if y >= area.bottom() {
            break;
        }
        let r = Rect::new(area.x.saturating_add(1), y, area.width.saturating_sub(1), 1);
        if r.is_empty() {
            continue;
        }
        Shimmer::new(*s)
            .style(t.secondary())
            .at(since, *t)
            .render(r, buf);
    }
}

/// A dark terminal's replies to OSC 10 / OSC 11, as a host would read them.
const PROBED_FG: &[u8] = b"\x1b]10;rgb:e4e4/e4e4/e7e7\x1b\\";
const PROBED_BG: &[u8] = b"\x1b]11;rgb:1818/1818/1b1b\x07";

fn levels(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    // True-colour themes carry their own RGB; on ANSI the demo plays a host
    // that probed its terminal.
    let probed: Option<(Rgb, Rgb)> = match (
        color::rgb_of(t.text),
        color::parse_osc_color(PROBED_FG),
        color::parse_osc_color(PROBED_BG),
    ) {
        (Some(_), _, _) => None,
        (None, Some(fg), Some(bg)) => Some((fg, bg)),
        _ => None,
    };
    let reduced = t.with_motion(Motion::Reduced);
    let rows: [(&str, Option<ColorLevel>, bool); 4] = [
        ("true colour", Some(ColorLevel::TrueColor), false),
        ("256 colours", Some(ColorLevel::Ansi256), false),
        ("16 colours", Some(ColorLevel::Ansi16), false),
        ("reduced", None, true),
    ];
    for (i, (label, level, still)) in rows.iter().enumerate() {
        let y = area.y.saturating_add(i as u16);
        if y >= area.bottom() {
            break;
        }
        let head = Line::from(Span::styled(
            format!(" {}", text::pad(label, 13, Align::Left)),
            t.dim(),
        ));
        buf.set_line(
            area.x,
            y,
            &text::truncate_line(head, area.width as usize),
            area.width,
        );
        let mut s =
            Shimmer::new("Thinking through the migration plan").cadence(Cadence::CONTINUOUS);
        if let Some(l) = level {
            s = s.level(*l);
        }
        if let Some((fg, bg)) = probed {
            s = s.colors(fg, bg);
        }
        let x = area.x.saturating_add(15);
        if x >= area.right() {
            continue;
        }
        let r = Rect::new(x, y, area.right() - x, 1);
        let theme = if *still { reduced } else { *t };
        s.at(cx.t, theme).render(r, buf);
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "shimmer",
    name: "Shimmer",
    ty: "twolab_tui::shimmer::Shimmer",
    summary: "That this label is live — work behind it is still in progress.",
    description: "A soft band of light passes across a label: wait 600 ms, then one 1 s pass every 4 s, so a status line looks alive without flashing. Only brightness changes and whole graphemes move together (CJK, emoji, combining marks), measured in cells. True colour blends the text toward the background and back; 256-colour terminals get the nearest fixed xterm colour; ANSI and mono get dim/normal/bold steps; reduced motion is plain bold.",
    used_in: "Agent status rows (the Working header above a composer), llmux in-flight request labels and the connecting screen.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/summary_shimmer.rs#L24-L60",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "Shimmer::new(text)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "The label.",
        },
        PropDoc {
            name: ".style(s)",
            ty: "Style",
            default: Some("theme.text()"),
            description: "Base style; a true-colour foreground becomes the band's crest.",
        },
        PropDoc {
            name: ".colors(fg, bg)",
            ty: "(Rgb, Rgb)",
            default: Some("theme text over surface_raised"),
            description: "The terminal's own colours, read with color::parse_osc_color — true-colour blending on an ANSI theme.",
        },
        PropDoc {
            name: ".level(l)",
            ty: "ColorLevel",
            default: Some("TrueColor"),
            description: "Ansi256 maps each step to the fixed xterm palette; Ansi16 uses weight steps.",
        },
        PropDoc {
            name: ".cadence(c)",
            ty: "Cadence",
            default: Some("Cadence::CALM"),
            description: "Delay, pass length and period. CONTINUOUS sweeps back to back.",
        },
        PropDoc {
            name: ".spans_at(t, &theme)",
            ty: "Vec<Span<'static>>",
            default: None,
            description: "The label at time t (since it last changed), for your own row.",
        },
        PropDoc {
            name: ".at(t, theme)",
            ty: "ShimmerFrame",
            default: None,
            description: "A one-row widget, clipped with … by cells.",
        },
        PropDoc {
            name: "sweep(text, t, cadence)",
            ty: "Vec<(&str, f64)>",
            default: None,
            description: "Each grapheme with the band's strength over it, 0.0–1.0.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Reduced motion (Theme::with_motion(Motion::Reduced)) shows the label in plain bold, still.",
        "Only brightness changes; the text never moves or changes, so copy and screen readers see the plain label.",
        "No flashing: one pass every 4 s, and no glyph changes by more than 25/255 between 32 ms frames (tested).",
        "Mono (NO_COLOR) keeps the band in weight — dim, normal, bold — with no colour.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "headline",
            title: "Working headline",
            description: "The status header above an agent's composer.",
            cols: 42,
            rows: 2,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(headline)),
        },
        DemoDoc {
            id: "summary",
            title: "Long summary lines",
            description: "Whole graphemes sweep together, CJK included; each line starts its own sweep.",
            cols: 84,
            rows: 3,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(summary)),
        },
        DemoDoc {
            id: "levels",
            title: "True colour, 256, 16, reduced",
            description: "The same label at each colour level, and under reduced motion.",
            cols: 56,
            rows: 4,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(levels)),
        },
    ],
};
