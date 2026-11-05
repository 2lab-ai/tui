use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Hints, Tab, Tabs, TabsLayout};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::text;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::tabs::{hit_test, Tab, Tabs};

let tabs = Tabs::new([
    Tab::new("dashboard").key("Esc"),
    Tab::new("accounts").key("a"),
    Tab::new("logs").key("l"),
])
.active(app.view)
.tail([Span::styled(" cfg ~/.config/llmux.json ", theme.dim())])
.theme(theme);

// Keys: ←/→ step, a tab's own key opens it.
if let Some(i) = tabs.on_key(key) { app.view = i; }

// Draw, keep the rects, map a click later.
app.tab_hits = tabs.render_hits(row, frame.buffer_mut());
if let Some(i) = hit_test(&app.tab_hits, mouse.column, mouse.row) { app.view = i; }"#;

fn llmux_tabs(all_keys: bool) -> Vec<Tab<'static>> {
    [
        ("dashboard", "Esc"),
        ("accounts", "a"),
        ("stats", "g"),
        ("usage", "U"),
        ("keys", "K"),
        ("perf", "p"),
        ("logs", "l"),
        ("sessions", "s"),
        ("misc", "?"),
        ("config", "c"),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (label, key))| {
        if all_keys || i == 0 {
            Tab::new(label).key(key)
        } else {
            Tab::new(label)
        }
    })
    .collect()
}

/// Carets under every click rect, one per cell: the rects are what a
/// mouse hits, so they must be exactly as wide as the labels.
fn ruler(layout: &TabsLayout, active: usize, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    for h in &layout.hits {
        let style = if h.index == active {
            theme.accent()
        } else {
            theme.dim()
        };
        let w = h.rect.width.min(area.right().saturating_sub(h.rect.x));
        buf.set_stringn(h.rect.x, area.y, "^".repeat(w as usize), w as usize, style);
    }
}

fn rows_below(area: Rect, skip: u16) -> Rect {
    Rect {
        y: area.y.saturating_add(skip).min(area.bottom()),
        height: area.height.saturating_sub(skip),
        ..area
    }
}

/// ←/→ (or a key letter) moves the active tab.
struct Switch {
    active: usize,
}

const APP: [(&str, &str); 5] = [
    ("overview", "o"),
    ("accounts", "a"),
    ("models", "m"),
    ("logs", "l"),
    ("settings", "s"),
];

fn app_tabs(active: usize, theme: Theme) -> Tabs<'static> {
    Tabs::new(APP.map(|(l, k)| Tab::new(l).key(k)))
        .active(active)
        .tail([Span::styled(" ai3@iq.io ", theme.dim())])
        .theme(theme)
}

impl Demo for Switch {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let tabs = app_tabs(self.active, *t);
        let layout = tabs.render_layout(Rect { height: 1, ..area }, buf);
        ruler(&layout, self.active, rows_below(area, 1), buf, t);
        let label = APP[self.active.min(APP.len() - 1)].0;
        let cells = layout
            .hits
            .iter()
            .find(|h| h.index == self.active)
            .map(|h| format!(" (cells {}–{})", h.rect.x, h.rect.right() - 1))
            .unwrap_or_default();
        let line = Line::from(vec![
            Span::styled(" on ", t.dim()),
            Span::styled(label, t.text()),
            Span::styled(cells, t.secondary()),
            Span::styled(" · ←/→ or the underlined letter switches", t.dim()),
        ]);
        Paragraph::new(text::truncate_line(line, area.width as usize))
            .render(rows_below(area, 3), buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match app_tabs(self.active, Theme::default()).on_key(key) {
            Some(i) if i != self.active => {
                self.active = i;
                true
            }
            _ => false,
        }
    }
}

fn llmux(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let row = |i: u16| Rect {
        y: area.y.saturating_add(i),
        height: 1,
        ..area
    };
    let tail = [Span::styled(" cfg ~/.config/llmux.json ", t.dim())];
    let rows: [(&str, Tabs); 2] = [
        (
            " on the dashboard: every tab shows its key",
            Tabs::new(llmux_tabs(true))
                .hints(Hints::Bracket)
                .active(0)
                .tail(tail.clone())
                .theme(*t),
        ),
        (
            " inside the usage overlay: only the way back",
            Tabs::new(llmux_tabs(false))
                .hints(Hints::Bracket)
                .active(3)
                .tail(tail)
                .theme(*t),
        ),
    ];
    for (i, (note, tabs)) in rows.into_iter().enumerate() {
        let y = i as u16 * 3;
        if y >= area.height {
            break;
        }
        Paragraph::new(Line::from(Span::styled(note, t.dim()))).render(row(y), buf);
        if y + 1 < area.height {
            tabs.render_hits(row(y + 1), buf);
        }
    }
}

fn narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let tabs = Tabs::new(llmux_tabs(false))
        .active(6)
        .tail([Span::styled(" cfg ~/.config/llmux.json ", t.dim())])
        .theme(*t);
    let full = area.width;
    for (i, w) in [full, full * 3 / 4, full / 2].into_iter().enumerate() {
        let y = area.y.saturating_add(i as u16 * 2);
        if y >= area.bottom() {
            break;
        }
        let layout = tabs.render_layout(Rect::new(area.x, y, w, 1), buf);
        if y + 1 < area.bottom() {
            let mut what = Vec::new();
            what.push(if layout.tail {
                "tail shown"
            } else {
                "tail gave way"
            });
            let hidden = layout.hidden_before + layout.hidden_after;
            let counted = format!("{hidden} tabs counted off-screen");
            if hidden > 0 {
                what.push(&counted);
            }
            let note = format!(" ↑ {w} cells: {}", what.join(", "));
            Paragraph::new(Line::from(Span::styled(note, t.dim()))).render(
                Rect {
                    y: y + 1,
                    height: 1,
                    ..area
                },
                buf,
            );
        }
    }
}

fn korean(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let labels = ["대시보드", "계정", "통계", "사용량", "로그", "설정"];
    let tabs = Tabs::new(labels.map(Tab::new)).active(2).theme(*t);
    let layout = tabs.render_layout(Rect { height: 1, ..area }, buf);
    ruler(&layout, 2, rows_below(area, 1), buf, t);
    let note = Line::from(Span::styled(
        " ^ marks each click rect: two cells per Hangul syllable",
        t.dim(),
    ));
    Paragraph::new(text::truncate_line(note, area.width as usize)).render(rows_below(area, 3), buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "tabs",
    name: "Tabs",
    ty: "twolab_tui::tabs::Tabs",
    summary: "Which view you are on, and how to get to the others.",
    description: "One row of view names. The active one is reversed; each label's click rect comes back from the render, measured in cells, so a click on a wide Korean label lands on it. Key hints stay while they fit; a right-aligned tail takes the room the labels leave and gives way first; when even the plain labels do not fit, the strip scrolls to the active tab and counts the hidden ones.",
    used_in: "llmux: the tab row under the header (dashboard, accounts, stats, usage, keys, perf, logs, sessions, misc, config), live over every overlay, with the header's cfg path moved into its free end.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L651-L699",
        license: "MIT",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "Tabs::new(tabs)",
            ty: "impl IntoIterator<Item = Tab>",
            default: None,
            description: "The views, in order. Tab::new(label).key(\"a\") adds a shortcut.",
        },
        PropDoc {
            name: ".active(i)",
            ty: "usize",
            default: Some("0"),
            description: "The view you are on.",
        },
        PropDoc {
            name: ".hints(h)",
            ty: "Hints",
            default: Some("Hints::Mnemonic"),
            description: "Mnemonic underlines the key letter in its label (a `[Esc] ` prefix when the label lacks it); Bracket is llmux's `[a] accounts`; Off.",
        },
        PropDoc {
            name: ".tail(spans)",
            ty: "impl IntoIterator<Item = Span>",
            default: Some("none"),
            description: "Right-aligned extras, drawn only when they fit after the labels with a one-cell gap.",
        },
        PropDoc {
            name: ".divider(s) / .indent(n)",
            ty: "&str / u16",
            default: Some("\" │ \" / 1"),
            description: "Between labels; blank cells before the first.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Labels in accent, the active one selected + bold, dividers dim.",
        },
        PropDoc {
            name: ".render_hits(area, buf)",
            ty: "Vec<TabHit { rect, index }>",
            default: None,
            description: "Draw and return one rect per drawn label; render_layout also says whether hints and the tail fit and how many tabs are off-screen.",
        },
        PropDoc {
            name: "hit_test(&hits, col, row)",
            ty: "Option<usize>",
            default: None,
            description: "The tab under a click.",
        },
        PropDoc {
            name: ".on_key(key)",
            ty: "Option<usize>",
            default: None,
            description: "The tab a key selects (arrows, Home/End, a tab's own key).",
        },
    ],
    keyboard: &[
        ("← / →", "previous / next tab, wrapping"),
        ("Shift-Tab / Tab", "previous / next tab"),
        ("Home / End", "first / last tab"),
        ("a tab's key", "open that tab (Esc, a, l, …)"),
    ],
    a11y: &[
        "The active tab is reversed and bold, so it reads without colour (NO_COLOR).",
        "Key letters are underlined in their label: the shortcut is visible without colour and costs no width.",
        "Tabs that do not fit are counted at the ends (‹2 … 3›), never dropped silently.",
        "Click rects are cell widths, so wide CJK labels are clickable across their whole width.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "switch",
            title: "Switch views",
            description: "←/→ or an underlined letter moves the active tab; the hit cells follow.",
            cols: 76,
            rows: 4,
            animated: false,
            interactive: true,
            make: || Box::new(Switch { active: 1 }),
        },
        DemoDoc {
            id: "llmux",
            title: "llmux tab row",
            description: "On the dashboard every tab shows its key; inside an overlay only the way back does.",
            cols: 140,
            rows: 5,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(llmux)),
        },
        DemoDoc {
            id: "narrow",
            title: "Narrow: the tail gives way",
            description: "The same row at three widths: the cfg tail drops first, then the strip scrolls to the active tab.",
            cols: 120,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(narrow)),
        },
        DemoDoc {
            id: "korean",
            title: "Korean labels",
            description: "Wide labels get click rects as wide as they are drawn.",
            cols: 64,
            rows: 4,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(korean)),
        },
    ],
};
