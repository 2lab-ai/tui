use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::{Field, PackedGrid};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::spinner::{Spinner, SpinnerKind};
use crate::text;
use crate::theme::{Status, Theme};

const USAGE: &str = r#"use twolab_tui::packed_grid::{Field, PackedGrid};

let entries = models.iter().map(|m| vec![
    Field::new(m.name.as_str()),
    Field::new(human_count(m.tokens)).right().widest("999.9M"),
    // Measured on its widest form: the grid never re-packs on a tick.
    Field::new(last_used(m, now)).widest("99 in-flight"),
]);
let grid = PackedGrid::new(entries)
    .headers(["model", "tok", "last"])
    .min_per_line(2)       // fewer than two a line: show the table instead
    .theme(theme);

match grid.plan(area.width) {
    Some(_) => app.strip_hits = grid.render_hits(area, frame.buffer_mut()),
    None => draw_table(frame, area),
}"#;

/// `1.2k`, `3.4M` — llmux `format::human_count`.
fn human(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// llmux `format::gauge_bar`: filled ▰, empty ▱ (both East-Asian Neutral,
/// one cell everywhere).
fn bar(share: f64, width: usize, theme: &Theme) -> Vec<Span<'static>> {
    let filled = ((share.clamp(0.0, 1.0) * width as f64).round() as usize).min(width);
    vec![
        Span::styled("▰".repeat(filled), theme.accent()),
        Span::styled("▱".repeat(width - filled), theme.dim()),
    ]
}

/// (model, tokens, in flight now, idle seconds) — the in-flight count and
/// the idle age move with the demo clock.
const MODELS: [(&str, u64, bool, u64); 8] = [
    ("opus-4-8[1m]", 48_213_904, true, 0),
    ("gpt-5.6-sol", 31_702_118, false, 12),
    ("fable-5", 22_946_310, true, 0),
    ("grok-5", 9_402_771, false, 184),
    ("opus-4-8", 7_118_045, false, 2_460),
    ("gpt-5.6-sol-fast", 3_301_590, false, 7_310),
    ("fable-5-mini", 820_417, false, 21_900),
    ("grok-5-code", 96_233, false, 176_400),
];

fn age(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3_600 => format!("{}m", secs / 60),
        3_600..86_400 => format!("{}h", secs / 3_600),
        _ => format!("{}d", secs / 86_400),
    }
}

fn model_strip(cx: &DemoCx) -> PackedGrid<'static> {
    let t = &cx.theme;
    let s = cx.t.as_secs();
    let top = MODELS.iter().map(|m| m.1).max().unwrap_or(1) as f64;
    let entries = MODELS
        .iter()
        .enumerate()
        .map(|(i, &(name, tok, busy, idle))| {
            let (marker, last) = if busy {
                // The count runs 1–12 with the clock; `99 in-flight` is reserved.
                let n = 1 + (s / 2 + i as u64) % 12;
                (
                    Spinner::new(SpinnerKind::Braille)
                        .style(t.status(Status::Running))
                        .span_at(cx.t, t),
                    Span::styled(format!("{n} in-flight"), t.status(Status::Running)),
                )
            } else {
                (Span::raw(" "), Span::styled(age(idle + s), t.dim()))
            };
            let name_style = if busy {
                t.text().add_modifier(Modifier::BOLD)
            } else {
                t.text()
            };
            vec![
                Field::new(marker).widest(" "),
                Field::new(Span::styled(name, name_style)),
                Field::new(bar(tok as f64 / top, 8, t)),
                Field::new(human(tok)).right().widest("999.9M"),
                Field::new(last).widest("99 in-flight"),
            ]
        });
    PackedGrid::new(entries.collect::<Vec<_>>())
        .headers(["", "model", "share", "tok", "last"])
        .theme(*t)
}

fn models(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    model_strip(cx).render_layout(area, buf);
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let w = area.width.min(50);
    model_strip(cx).render_layout(Rect { width: w, ..area }, buf);
}

/// (group, #, account, 7d reset, in flight)
const SPENT: [(&str, u32, &str, &str, u32); 9] = [
    ("CLAUDE", 2, "ai3@iq.io", "2d 4h", 0),
    ("CLAUDE", 3, "ai2@iq.io", "5h 12m", 1),
    ("CLAUDE", 5, "dev1@2lab.ai", "6d 1h", 0),
    ("CODEX", 7, "dev2@2lab.ai", "41m", 0),
    ("CLAUDE", 8, "zhuge@2lab.ai", "3d 20h", 2),
    ("CODEX", 11, "simayi@2lab.ai", "1d 9h", 0),
    ("GROK", 12, "guanyu@2lab.ai", "4d", 0),
    ("CLAUDE", 14, "ops@iq.io", "17h 3m", 0),
    ("CODEX", 16, "ai7@iq.io", "6d 23h", 0),
];

/// The 7d-spent accounts folded off the table, a cursor on one: ←/→ move
/// along a line, ↑/↓ between lines.
struct Fold {
    cursor: usize,
    per_line: usize,
}

impl Demo for Fold {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let entries = SPENT.iter().map(|&(group, n, name, reset, busy)| {
            vec![
                Field::new(Span::styled(
                    group,
                    t.secondary().add_modifier(Modifier::BOLD),
                )),
                Field::new(Span::styled(n.to_string(), t.dim())).right(),
                Field::new(name),
                Field::new(Span::styled(reset, t.secondary())).widest("99d 23h"),
                Field::new(Span::styled("100%", t.status(Status::Failed))).right(),
                Field::new(if busy > 0 {
                    Span::styled(busy.to_string(), t.status(Status::Running))
                } else {
                    Span::styled("0", t.dim())
                })
                .right()
                .widest("99"),
            ]
        });
        let grid = PackedGrid::new(entries.collect::<Vec<_>>())
            .headers(["group", "#", "account", "7d reset", "7d", "if"])
            .cursor(Some(self.cursor))
            .theme(*t);
        let rows = area.height.saturating_sub(2);
        let layout = grid.render_layout(
            Rect {
                height: rows,
                ..area
            },
            buf,
        );
        self.per_line = layout.per_line.max(1);
        if area.height >= 2 {
            let note = Line::from(vec![
                Span::styled(" 9 accounts 7d spent, folded · cursor on ", t.dim()),
                Span::styled(SPENT[self.cursor].2, t.text()),
                Span::styled(" · ←/→ ↑/↓ move", t.dim()),
            ]);
            buf.set_line(
                area.x,
                area.bottom() - 1,
                &text::truncate_line(note, area.width as usize),
                area.width,
            );
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let n = SPENT.len();
        let k = self.per_line.max(1);
        let to = match key.key {
            Key::Left => self.cursor.saturating_sub(1),
            Key::Right => (self.cursor + 1).min(n - 1),
            Key::Up => self.cursor.checked_sub(k).unwrap_or(self.cursor),
            Key::Down if self.cursor + k < n => self.cursor + k,
            Key::Home => 0,
            Key::End => n - 1,
            _ => self.cursor,
        };
        let moved = to != self.cursor;
        self.cursor = to;
        moved
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "packed-grid",
    name: "Packed grid",
    ty: "twolab_tui::packed_grid::PackedGrid",
    summary: "Many short records on few lines, each field lined up in its column.",
    description: "Entries go row-major, as many to a line as fit; every field is as wide as the widest value in its column, measured in cells and on the widest form a ticking value can take (99 in-flight, 99d 23h), so the grid never re-packs or shifts on a clock tick. Right-aligned numbers, a dim header over each entry column, a selected cursor entry, a click rect per entry, and +N more when the rows run out.",
    used_in: "llmux: the models strip (model · share · tokens · last used, packed two or three to a line) and the fold of 7d-spent accounts off the accounts table.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L5802-L6037",
        license: "MIT",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "PackedGrid::new(entries)",
            ty: "impl IntoIterator<Item = Vec<Field>>",
            default: None,
            description: "One Vec of fields per record, the same fields in the same order.",
        },
        PropDoc {
            name: "Field::new(content).right().widest(sample)",
            ty: "Field",
            default: Some("left, live width"),
            description: "A field's text or spans; right-align it; reserve the width of the widest text it can show.",
        },
        PropDoc {
            name: ".headers(words)",
            ty: "impl IntoIterator<Item = &str>",
            default: Some("none"),
            description: "A dim word over each field of every entry column.",
        },
        PropDoc {
            name: ".cursor(Some(i))",
            ty: "Option<usize>",
            default: Some("None"),
            description: "The entry drawn selected, edge to edge.",
        },
        PropDoc {
            name: ".min_per_line(n)",
            ty: "usize",
            default: Some("1"),
            description: "plan() says None below this, so the host can show its table instead.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Header and +N more dim; the cursor entry selected.",
        },
        PropDoc {
            name: ".plan(width) / .height(width)",
            ty: "Option<GridPlan> / u16",
            default: None,
            description: "Entries per line and column widths; rows the whole grid needs, for the host's layout.",
        },
        PropDoc {
            name: ".render_layout(area, buf)",
            ty: "GridLayout { hits, per_line, hidden, clipped }",
            default: None,
            description: "Draw; one GridHit { rect, index } per drawn entry. hit_test(&hits, col, row) maps a click.",
        },
    ],
    keyboard: &[],
    a11y: &[
        "Columns are fixed by the widest form of each value, so nothing moves while you read it.",
        "Entries that do not fit the rows are counted (+N more); entries wider than the screen end in …",
        "The cursor entry is selected across its whole width (reversed under NO_COLOR).",
        "Every value stays as text in its column; the share bar's ▰/▱ read without colour.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "models",
            title: "Models strip",
            description: "Model · share · tokens · last used, two to a line; in-flight counts tick and no column moves.",
            cols: 120,
            rows: 5,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(models)),
        },
        DemoDoc {
            id: "fold",
            title: "Account fold",
            description: "Nine 7d-spent accounts folded off the table, three to a line, a cursor on one.",
            cols: 132,
            rows: 6,
            animated: false,
            interactive: true,
            make: || {
                Box::new(Fold {
                    cursor: 4,
                    per_line: 3,
                })
            },
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow",
            description: "Fifty cells: one entry a line, and the rows that do not fit say +N more.",
            cols: 50,
            rows: 5,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
    ],
};
