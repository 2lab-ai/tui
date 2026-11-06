use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{
    ScrollPos, Scrollbar, ScrollbarDrag, ScrollbarGlyphs, list_pos, reveal, split, thumb, track,
};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Status, Theme};

const USAGE: &str = r#"use twolab_tui::scrollbar::{self, ScrollPos, Scrollbar, ScrollbarDrag};

// Draw: the rows in all, the rows in view, how far down.
let pos = ScrollPos::of(lines.len(), body.height as usize, app.top);
let (text_area, track) = scrollbar::split(body, pos);
frame.render_widget(Paragraph::new(lines).scroll((pos.top as u16, 0)), text_area);
frame.render_widget(Scrollbar::new(pos).theme(theme), body);

// Mouse: a press on the track jumps (or grabs the thumb), a drag follows.
if let Some(track) = track {
    match ev.kind {
        Down(Left) => if let Some(top) = app.drag.press(pos, track, ev.column, ev.row) { app.top = top },
        Drag(Left) => if let Some(top) = app.drag.drag(pos, track, ev.row) { app.top = top },
        Up(Left) => app.drag.release(),
        _ => {}
    }
}
// Keys: ↑ ↓ PgUp PgDn Home End.
if let Some(top) = pos.on_key(key) { app.top = top }

// A list whose rows differ in height: positions count list rows.
app.start = scrollbar::reveal(&heights, &gaps, body.height, app.start, app.selected);
let pos = scrollbar::list_pos(&heights, &gaps, body.height, app.start);"#;

/// One request in llmux's log: time, id, model, account, status, latency,
/// tokens in and out.
const LOG: [(&str, &str, &str, u16, &str, &str); 16] = [
    (
        "req_01JD8Q4M",
        "claude-opus-5-5",
        "zhuge@2lab.ai",
        200,
        "1.8s",
        "12.4k 2.1k",
    ),
    (
        "req_01JD8Q5A",
        "claude-sonnet-5",
        "ai3@iq.io",
        200,
        "0.9s",
        "3.2k 610",
    ),
    (
        "req_01JD8Q5K",
        "claude-opus-5-5",
        "zhuge@2lab.ai",
        429,
        "0.2s",
        "18.1k —",
    ),
    (
        "req_01JD8Q61",
        "claude-opus-5-5",
        "simayi@2lab.ai",
        200,
        "2.4s",
        "18.1k 3.9k",
    ),
    (
        "req_01JD8Q6H",
        "claude-haiku-5",
        "dev1@2lab.ai",
        200,
        "0.4s",
        "1.1k 220",
    ),
    (
        "req_01JD8Q7C",
        "claude-sonnet-5",
        "ai3@iq.io",
        200,
        "1.1s",
        "6.8k 1.4k",
    ),
    (
        "req_01JD8Q7W",
        "claude-opus-5-5",
        "simayi@2lab.ai",
        529,
        "8.0s",
        "22.0k —",
    ),
    (
        "req_01JD8Q8B",
        "claude-opus-5-5",
        "zhuge@2lab.ai",
        200,
        "3.1s",
        "22.0k 4.6k",
    ),
    (
        "req_01JD8Q8T",
        "claude-haiku-5",
        "dev1@2lab.ai",
        200,
        "0.3s",
        "840 95",
    ),
    (
        "req_01JD8Q9E",
        "claude-sonnet-5",
        "ai2@iq.io",
        401,
        "0.1s",
        "2.0k —",
    ),
    (
        "req_01JD8Q9X",
        "claude-sonnet-5",
        "ai3@iq.io",
        200,
        "1.6s",
        "9.9k 2.2k",
    ),
    (
        "req_01JD8QA4",
        "claude-opus-5-5",
        "zhuge@2lab.ai",
        200,
        "2.0s",
        "14.7k 2.8k",
    ),
    (
        "req_01JD8QAQ",
        "claude-opus-5-5",
        "simayi@2lab.ai",
        200,
        "4.2s",
        "31.5k 6.0k",
    ),
    (
        "req_01JD8QB2",
        "claude-haiku-5",
        "dev1@2lab.ai",
        200,
        "0.5s",
        "1.6k 310",
    ),
    (
        "req_01JD8QBM",
        "claude-sonnet-5",
        "ai3@iq.io",
        429,
        "0.2s",
        "7.3k —",
    ),
    (
        "req_01JD8QC0",
        "claude-opus-5-5",
        "zhuge@2lab.ai",
        200,
        "1.4s",
        "8.8k 1.9k",
    ),
];

const LOG_ROWS: usize = 64;

fn log_line(i: usize, t: &Theme) -> Line<'static> {
    let (id, model, account, status, latency, tokens) = LOG[i % LOG.len()];
    let secs = 14 * 60 + 2 + i * 3;
    let (word, st) = match status {
        200 => ("ok", Status::Done),
        429 => ("busy", Status::Waiting),
        _ => ("fail", Status::Failed),
    };
    Line::from(vec![
        Span::styled(format!("09:{:02}:{:02} ", secs / 60, secs % 60), t.dim()),
        Span::styled(format!("{id} "), t.secondary()),
        Span::styled(text::pad(model, 16, text::Align::Left), t.text()),
        Span::styled(text::pad(account, 15, text::Align::Left), t.secondary()),
        Span::styled(format!("{status} {word:<4} "), t.status(st)),
        Span::styled(format!("{latency:>4} "), t.text()),
        Span::styled(tokens, t.dim()),
    ])
}

fn put(buf: &mut Buffer, x: u16, y: u16, line: Line<'static>, w: u16) {
    if w == 0 || !buf.area.contains((x, y).into()) {
        return;
    }
    buf.set_line(x, y, &text::truncate_line(line, w as usize), w);
}

/// Draw the log scrolled to `top` inside `body`, with its scrollbar.
/// Returns the position drawn.
fn draw_log(body: Rect, buf: &mut Buffer, t: &Theme, top: usize, focused: bool) -> ScrollPos {
    let pos = ScrollPos::of(LOG_ROWS, body.height as usize, top);
    let (text_area, _) = split(body, pos);
    for (row, i) in (pos.top..LOG_ROWS).take(body.height as usize).enumerate() {
        put(
            buf,
            text_area.x,
            text_area.y + row as u16,
            log_line(i, t),
            text_area.width,
        );
    }
    Scrollbar::new(pos)
        .focused(focused)
        .theme(*t)
        .render(body, buf);
    pos
}

fn rows_label(pos: ScrollPos, total: usize, unit: &str) -> String {
    if total == 0 {
        return format!("no {unit}");
    }
    let last = (pos.top + pos.viewport).min(total);
    format!("{unit} {}-{last} of {total}", pos.top + 1)
}

/// llmux's request log, scrolled with the keys.
#[derive(Default)]
struct LogDemo {
    top: usize,
    last: Option<ScrollPos>,
}

impl Demo for LogDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        if area.is_empty() {
            return;
        }
        let header = area.height >= 3;
        let footer = area.height >= 2;
        let mut y = area.y;
        if header {
            put(
                buf,
                area.x,
                y,
                Line::from(vec![
                    Span::styled("requests", t.text().add_modifier(Modifier::BOLD)),
                    Span::styled("  llmux · last 3 min", t.dim()),
                ]),
                area.width,
            );
            y += 1;
        }
        let body_h = area.height - u16::from(header) - u16::from(footer);
        let body = Rect::new(area.x, y, area.width, body_h);
        let pos = draw_log(body, buf, t, self.top, true);
        self.top = pos.top;
        self.last = Some(pos);
        if footer {
            put(
                buf,
                area.x,
                area.bottom() - 1,
                Line::from(vec![
                    Span::styled(rows_label(pos, LOG_ROWS, "rows"), t.secondary()),
                    Span::styled("  ↑↓ PgUp PgDn Home End", t.dim()),
                ]),
                area.width,
            );
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let pos = self
            .last
            .unwrap_or_else(|| ScrollPos::of(LOG_ROWS, 10, self.top));
        match pos.on_key(key) {
            Some(top) if top != self.top => {
                self.top = top;
                true
            }
            _ => false,
        }
    }
}

fn log() -> Box<dyn Demo> {
    Box::new(LogDemo::default())
}

/// An agent in the sidebar: workspace, name, state, then detail rows.
struct Agent {
    workspace: &'static str,
    name: &'static str,
    state: Status,
    details: &'static [&'static str],
}

const AGENTS: [Agent; 12] = [
    Agent {
        workspace: "llmux",
        name: "claude · pager",
        state: Status::Running,
        details: &["fix flaky pager scroll test", "src/tui/pager.rs · 2 min"],
    },
    Agent {
        workspace: "llmux",
        name: "codex · release",
        state: Status::Waiting,
        details: &["needs approval: cargo publish"],
    },
    Agent {
        workspace: "llmux",
        name: "claude · usage",
        state: Status::Done,
        details: &[],
    },
    Agent {
        workspace: "tui",
        name: "claude · modal",
        state: Status::Running,
        details: &["port herdr popup sizing", "widgets/modal · 6 min"],
    },
    Agent {
        workspace: "tui",
        name: "claude · toast",
        state: Status::Running,
        details: &["toast queue with TTL per kind"],
    },
    Agent {
        workspace: "tui",
        name: "codex · review",
        state: Status::Failed,
        details: &["clippy: 3 warnings as errors", "widgets/tabs/mod.rs:212"],
    },
    Agent {
        workspace: "tui",
        name: "claude · docs",
        state: Status::Done,
        details: &[],
    },
    Agent {
        workspace: "herdr",
        name: "claude · audit",
        state: Status::Done,
        details: &["14 candidates, 12 defects"],
    },
    Agent {
        workspace: "herdr",
        name: "codex · probe",
        state: Status::Waiting,
        details: &["needs approval: rm -rf scratch/"],
    },
    Agent {
        workspace: "site",
        name: "claude · catalog",
        state: Status::Running,
        details: &["wasm build 2.1 MB to 1.4 MB", "vite.config.ts · 1 min"],
    },
    Agent {
        workspace: "site",
        name: "claude · a11y",
        state: Status::Done,
        details: &[],
    },
    Agent {
        workspace: "site",
        name: "codex · deploy",
        state: Status::Waiting,
        details: &["needs approval: vercel --prod"],
    },
];

fn state_word(s: Status) -> (&'static str, &'static str) {
    // A glyph and a word: never colour alone. All East-Asian Narrow.
    match s {
        Status::Running => ("▸", "working"),
        Status::Waiting => ("!", "waiting"),
        Status::Done => ("✓", "done"),
        Status::Failed => ("x", "failed"),
    }
}

fn agent_heights() -> (Vec<u16>, Vec<u16>) {
    let heights = AGENTS.iter().map(|a| 1 + a.details.len() as u16).collect();
    // The caller's gap policy: one blank row after each workspace's last agent.
    let gaps = AGENTS
        .iter()
        .enumerate()
        .map(|(i, a)| {
            u16::from(
                AGENTS
                    .get(i + 1)
                    .is_some_and(|n| n.workspace != a.workspace),
            )
        })
        .collect();
    (heights, gaps)
}

fn agent_rows(a: &Agent, selected: bool, w: u16, t: &Theme) -> Vec<Line<'static>> {
    let (glyph, word) = state_word(a.state);
    let base = if selected { t.selected() } else { t.text() };
    let mark = if selected { "› " } else { "  " };
    let left = format!("{mark}{glyph} {}", a.name);
    let right = format!(" {word} ");
    let gap = (w as usize).saturating_sub(text::width(&left) + text::width(&right));
    let mut rows = vec![Line::from(vec![
        Span::styled(left, base.add_modifier(Modifier::BOLD)),
        Span::styled(" ".repeat(gap), base),
        Span::styled(right, base.patch(t.status(a.state))),
    ])];
    for d in a.details {
        rows.push(Line::from(vec![
            Span::styled("    ", base),
            Span::styled(*d, if selected { base } else { t.dim() }),
        ]));
    }
    rows
}

/// A sidebar of agents whose rows differ in height, with gaps between
/// workspaces: the selection stays in view; the scrollbar counts agents.
#[derive(Default)]
struct AgentsDemo {
    selected: usize,
    start: usize,
}

impl Demo for AgentsDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        if area.is_empty() {
            return;
        }
        let header = area.height >= 3;
        let footer = area.height >= 2;
        let mut y = area.y;
        if header {
            put(
                buf,
                area.x,
                y,
                Line::from(vec![
                    Span::styled("agents", t.text().add_modifier(Modifier::BOLD)),
                    Span::styled("  4 workspaces", t.dim()),
                ]),
                area.width,
            );
            y += 1;
        }
        let body_h = area.height - u16::from(header) - u16::from(footer);
        let body = Rect::new(area.x, y, area.width, body_h);
        let (heights, gaps) = agent_heights();
        self.start = reveal(&heights, &gaps, body_h, self.start, self.selected);
        let pos = list_pos(&heights, &gaps, body_h, self.start);
        let (list, _) = split(body, pos);
        let mut y = list.y;
        'agents: for i in pos.top..AGENTS.len() {
            for line in agent_rows(&AGENTS[i], i == self.selected, list.width, t) {
                if y >= list.bottom() {
                    break 'agents;
                }
                put(buf, list.x, y, line, list.width);
                y += 1;
            }
            y = y.saturating_add(gaps[i]);
        }
        Scrollbar::new(pos).theme(*t).render(body, buf);
        if footer {
            put(
                buf,
                area.x,
                area.bottom() - 1,
                Line::from(vec![
                    Span::styled(
                        format!("agent {} of {}", self.selected + 1, AGENTS.len()),
                        t.secondary(),
                    ),
                    Span::styled(
                        format!(
                            "  {}  ↑↓ PgUp PgDn Home End",
                            rows_label(pos, AGENTS.len(), "in view")
                        ),
                        t.dim(),
                    ),
                ]),
                area.width,
            );
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let last = AGENTS.len() - 1;
        let to = match key.key {
            Key::Up => self.selected.saturating_sub(1),
            Key::Down => (self.selected + 1).min(last),
            Key::PageUp => self.selected.saturating_sub(4),
            Key::PageDown => (self.selected + 4).min(last),
            Key::Home => 0,
            Key::End => last,
            _ => return false,
        };
        let moved = to != self.selected;
        self.selected = to;
        moved
    }
}

fn agents() -> Box<dyn Demo> {
    Box::new(AgentsDemo::default())
}

/// One step of the replayed pointer.
struct Step {
    row: u16,
    top: usize,
    pressed: bool,
    note: String,
}

/// Replay a press on the thumb, a drag past the end, a release and a click
/// on the track, at time `t` into an 8-second loop.
fn replay(t: Duration, body: Rect) -> Step {
    let ms = (t.as_millis() % 8_000) as u64;
    let start = ScrollPos::of(LOG_ROWS, body.height as usize, 6);
    let tr = track(body);
    let Some(th) = thumb(start, tr) else {
        return Step {
            row: tr.y,
            top: start.top,
            pressed: false,
            note: "everything fits: no thumb".into(),
        };
    };
    let grab_at = th.top + (th.len - 1).min(1);
    let mut drag = ScrollbarDrag::new();
    let top = drag.press(start, tr, tr.x, grab_at).unwrap_or(start.top);
    let g = drag.grab_offset().unwrap_or(0);
    if ms < 1_500 {
        return Step {
            row: grab_at,
            top,
            pressed: true,
            note: format!("press on the thumb: grabbed {g} row(s) down, view stays"),
        };
    }
    let end_row = tr.bottom().saturating_add(2);
    if ms < 4_500 {
        let span = u64::from(end_row - grab_at);
        let row = grab_at + ((ms - 1_500) * span / 3_000) as u16;
        let top = drag.drag(start, tr, row).unwrap_or(top);
        let past = if row >= tr.bottom() {
            " (past the end)"
        } else {
            ""
        };
        return Step {
            row,
            top,
            pressed: true,
            note: format!("drag to row {}{past}: top {top}", row.saturating_sub(tr.y)),
        };
    }
    let at_end = drag.drag(start, tr, end_row).unwrap_or(top);
    drag.release();
    if ms < 5_500 {
        return Step {
            row: end_row.min(tr.bottom().saturating_sub(1)),
            top: at_end,
            pressed: false,
            note: format!("released: top {at_end} of {}", start.max),
        };
    }
    let end = start.with_top(at_end);
    let click = tr.y + tr.height / 4;
    let top = drag.press(end, tr, tr.x, click).unwrap_or(at_end);
    Step {
        row: click,
        top,
        pressed: true,
        note: format!(
            "click on the track at row {}: the thumb centres there, top {top}",
            click - tr.y
        ),
    }
}

fn drag_demo(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    if area.is_empty() {
        return;
    }
    let caption = area.height >= 2;
    let body = Rect {
        height: area.height - u16::from(caption),
        ..area
    };
    let at = if t.reduced_motion() {
        Duration::from_millis(3_000)
    } else {
        cx.t
    };
    // Leave a column for the pointer left of the track.
    let content = Rect {
        width: body.width.saturating_sub(2),
        ..body
    };
    let step = replay(at, body);
    let pos = ScrollPos::of(LOG_ROWS, body.height as usize, step.top);
    for (row, i) in (pos.top..LOG_ROWS).take(body.height as usize).enumerate() {
        put(
            buf,
            content.x,
            content.y + row as u16,
            log_line(i, t),
            content.width,
        );
    }
    Scrollbar::new(pos).theme(*t).render(body, buf);
    if body.width >= 2 {
        let x = body.right() - 2;
        let y = step.row.min(body.bottom().saturating_sub(1));
        let style = if step.pressed {
            t.accent().add_modifier(Modifier::BOLD)
        } else {
            t.dim()
        };
        put(buf, x, y, Line::styled("<", style), 1);
    }
    if caption {
        put(
            buf,
            area.x,
            area.bottom() - 1,
            Line::styled(step.note, t.secondary()),
            area.width,
        );
    }
}

fn drag() -> Box<dyn Demo> {
    Box::new(FnDemo(drag_demo))
}

fn glyphs_demo(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let sets: [(&str, ScrollbarGlyphs, bool); 4] = [
        ("focused", ScrollbarGlyphs::default(), true),
        ("unfocused", ScrollbarGlyphs::default(), false),
        ("ascii", ScrollbarGlyphs::ascii(), true),
        ("herdr (opt-in)", ScrollbarGlyphs::herdr(), true),
    ];
    let col_w = (area.width / sets.len() as u16).max(1);
    for (i, (label, g, focused)) in sets.into_iter().enumerate() {
        let x = area.x + col_w * i as u16;
        if x >= area.right() {
            break;
        }
        let w = col_w.min(area.right() - x);
        put(buf, x, area.y, Line::styled(label, t.secondary()), w);
        if area.height < 2 {
            continue;
        }
        let bar = Rect::new(x, area.y + 1, 1, area.height - 1);
        let pos = ScrollPos::of(40, bar.height as usize, 12);
        Scrollbar::new(pos)
            .glyphs(g)
            .focused(focused)
            .theme(*t)
            .render(bar, buf);
        if w > 3 && bar.height > 0 {
            let note = if g == ScrollbarGlyphs::herdr() {
                "▕ is Ambiguous"
            } else {
                "one cell in CJK"
            };
            put(
                buf,
                x + 2,
                bar.y + bar.height / 2,
                Line::styled(note, t.dim()),
                w - 3,
            );
        }
    }
}

fn glyphs() -> Box<dyn Demo> {
    Box::new(FnDemo(glyphs_demo))
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "scrollbar",
    name: "Scrollbar",
    ty: "twolab_tui::scrollbar::Scrollbar",
    summary: "How much there is, where you are in it, and a thumb the mouse can drag.",
    description: "herdr's one-column scrollbar: the thumb's length is the share in view and its place the place in the whole. The maths are free functions shared by drawing and hit tests: a click on the track centres the thumb there, a drag keeps the grabbed cell under the pointer, and list_pos / reveal handle lists whose rows differ in height with gaps between groups. Nothing is drawn when everything fits.",
    used_in: "herdr scrolls panes, the agent and workspace sidebars, the navigator, help and release notes with it. Planned for llmux's keys, sessions and usage tables and its raw viewer, which scroll with no scrollbar or a display-only one; the modal widget scrolls its body with it.",
    source: Source {
        project: "herdr",
        url: "https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/ui/scrollbar.rs#L37-L184",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "ScrollPos { top, max, viewport }",
            ty: "ScrollPos",
            default: None,
            description: "Where the view is. ScrollPos::of(total, viewport, top) builds one from a row count; from_bottom / offset_from_bottom convert to a terminal's scrollback convention; on_key(key) gives the top for ↑ ↓ PgUp PgDn Home End.",
        },
        PropDoc {
            name: "Scrollbar::new(pos)",
            ty: "ScrollPos",
            default: None,
            description: "Draws in the rightmost column of the area it is given; nothing when pos does not overflow.",
        },
        PropDoc {
            name: ".focused(bool)",
            ty: "bool",
            default: Some("true"),
            description: "Thumb in the accent colour when the surface has focus, secondary otherwise; bold in mono either way.",
        },
        PropDoc {
            name: ".glyphs(g)",
            ty: "ScrollbarGlyphs",
            default: Some("╎ track, ▐ thumb (Narrow)"),
            description: "ScrollbarGlyphs::ascii() for | and #; ScrollbarGlyphs::herdr() opts into herdr's ▕ track (Ambiguous width).",
        },
        PropDoc {
            name: "scrollbar::thumb(pos, track) / grab(pos, track, row)",
            ty: "Option<Thumb> / Option<u16>",
            default: None,
            description: "The thumb's first row and length; the row offset into the thumb a press landed on.",
        },
        PropDoc {
            name: "scrollbar::offset_at(pos, track, row) / offset_dragged(pos, track, row, grab)",
            ty: "usize",
            default: None,
            description: "The new top for a click (thumb centres on the row) and for a drag (the grabbed row stays under the pointer; past either end reaches the end).",
        },
        PropDoc {
            name: "ScrollbarDrag::press / drag / release",
            ty: "ScrollbarDrag",
            default: None,
            description: "A host's mouse handler in three calls: press returns Some(top) when it hit the track, drag follows while held.",
        },
        PropDoc {
            name: "scrollbar::list_pos(heights, gaps_after, body_h, start) / reveal(.., target)",
            ty: "ScrollPos / usize",
            default: None,
            description: "Lists whose rows differ in height, with caller-chosen gaps: the position in list rows, and the start that brings a row into view moving least.",
        },
        PropDoc {
            name: "scrollbar::split(area, pos) / track(area)",
            ty: "(Rect, Option<Rect>) / Rect",
            default: None,
            description: "The content area and the track column when pos overflows; the column the bar draws in and clicks are tested against.",
        },
    ],
    keyboard: &[
        ("↑ ↓", "One row (ScrollPos::on_key)"),
        ("PgUp PgDn", "One viewport"),
        ("Home End", "Top or bottom"),
        (
            "Mouse on the track",
            "Centre the thumb there, keep dragging",
        ),
        ("Mouse on the thumb", "Grab it where pressed and drag"),
    ],
    a11y: &[
        "Thumb and track differ by glyph (▐ and ╎) and by weight: the track is dim, the thumb bold in mono, so NO_COLOR and colour-blind readers still see where they are.",
        "Both default glyphs are East-Asian Narrow, one cell on CJK terminals; herdr's Ambiguous ▕ is opt-in.",
        "Hosts say the position in words too (rows 12-25 of 64), as every demo here does; the bar is the glance, not the only cue.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "log",
            title: "A request log",
            description: "64 requests through a short pane: ↑↓, PgUp/PgDn and Home/End move; the thumb's length is the share in view.",
            cols: 92,
            rows: 14,
            animated: false,
            interactive: true,
            make: log,
        },
        DemoDoc {
            id: "agents",
            title: "Rows of different heights",
            description: "Agents with one to three rows each and a gap after each workspace: ↑↓ moves the selection, reveal keeps it in view, and the scrollbar counts agents, not screen lines.",
            cols: 56,
            rows: 14,
            animated: false,
            interactive: true,
            make: agents,
        },
        DemoDoc {
            id: "drag",
            title: "Click and drag, replayed",
            description: "A pointer presses the thumb one row down, drags past the end, lets go, then clicks the track: the grabbed row stays under the pointer and a click centres the thumb.",
            cols: 92,
            rows: 14,
            animated: true,
            interactive: false,
            make: drag,
        },
        DemoDoc {
            id: "glyphs",
            title: "Glyphs and focus",
            description: "The default bar focused and unfocused, the ASCII set and herdr's own glyphs; switch to mono to see thumb and track kept apart by weight.",
            cols: 72,
            rows: 10,
            animated: false,
            interactive: false,
            make: glyphs,
        },
    ],
};
