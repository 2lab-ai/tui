use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::{Hints, Reveal, Tab, TabTarget, Tabs, TabsLayout};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
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
if let Some(i) = hit_test(&app.tab_hits, mouse.column, mouse.row) { app.view = i; }

// herdr's workspace strip: < > buttons, a + action, drag to reorder.
let reveal = if app.switched { Reveal::Centre } else { Reveal::From(app.first) };
let layout = Tabs::new(tabs).active(app.tab).scroll_buttons(true).action("+")
    .reveal(reveal).drop_marker(app.drag.map(|_| app.drop))
    .render_layout(row, frame.buffer_mut());
app.first = layout.hidden_before; // hold the scroll position
match layout.target_at(mouse.column, mouse.row) {
    Some(TabTarget::ScrollLeft) => app.first = app.first.saturating_sub(1),
    Some(TabTarget::ScrollRight) => app.first += 1, // clamped at the next render
    Some(TabTarget::Action) => new_tab(),
    Some(TabTarget::Tab(i)) => app.tab = i,
    None => {}
}
// While dragging: the insert index under the pointer, by label midpoints.
app.drop = layout.drop_index(mouse.column, mouse.row).unwrap_or(app.drop);"#;

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

const WORKSPACE: [&str; 12] = [
    "main", "claude", "codex", "logs", "review", "deploy", "notes", "db", "tests", "docs", "ci",
    "scratch",
];

/// Marks under the strip: `<` `>` under the buttons, `+` under the action,
/// `^` under each tab — the cells a click lands on.
fn targets(layout: &TabsLayout, area: Rect, buf: &mut Buffer, theme: &Theme) {
    if area.is_empty() {
        return;
    }
    let mut put = |r: Option<Rect>, glyph: &str| {
        if let Some(r) = r
            && r.x < area.right()
        {
            let w = r.width.min(area.right() - r.x) as usize;
            buf.set_stringn(r.x, area.y, glyph.repeat(w), w, theme.dim());
        }
    };
    put(layout.scroll_left, "<");
    put(layout.scroll_right, ">");
    put(layout.action, "+");
    for h in &layout.hits {
        put(Some(h.rect), "^");
    }
}

/// herdr's workspace strip, overflowing: the buttons scroll it, the action
/// adds a tab, a switch centres the new tab.
struct Overflow {
    tabs: Vec<String>,
    active: usize,
    first: usize,
    max_start: usize,
    centre: bool,
    said: &'static str,
}

impl Overflow {
    fn tabs(&self, theme: Theme) -> Tabs<'static> {
        Tabs::new(self.tabs.iter().map(|l| Tab::new(l.clone())))
            .active(self.active)
            .scroll_buttons(true)
            .action("+")
            .reveal(if self.centre {
                Reveal::Centre
            } else {
                Reveal::From(self.first)
            })
            .theme(theme)
    }
}

impl Demo for Overflow {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let layout = self.tabs(*t).render_layout(Rect { height: 1, ..area }, buf);
        self.first = layout.hidden_before;
        self.max_start = layout.max_start;
        self.centre = false;
        targets(&layout, rows_below(area, 1), buf, t);
        let hidden = layout.hidden_before + layout.hidden_after;
        let line = Line::from(vec![
            Span::styled(format!(" {} ", self.said), t.secondary()),
            Span::styled(
                format!("· {hidden} hidden · PgUp/PgDn < > · Enter + · ←/→ switch"),
                t.dim(),
            ),
        ]);
        Paragraph::new(text::truncate_line(line, area.width as usize))
            .render(rows_below(area, 3), buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        // A key stands in for the click a host maps with `target_at`.
        let target = match key.key {
            Key::PageUp => Some(TabTarget::ScrollLeft),
            Key::PageDown => Some(TabTarget::ScrollRight),
            Key::Enter => Some(TabTarget::Action),
            _ => None,
        };
        match target {
            Some(TabTarget::ScrollLeft) if self.first > 0 => {
                self.first -= 1;
                self.said = "pressed <";
            }
            Some(TabTarget::ScrollRight) if self.first < self.max_start => {
                self.first += 1;
                self.said = "pressed >";
            }
            Some(TabTarget::ScrollLeft | TabTarget::ScrollRight) => return false,
            Some(TabTarget::Action) => {
                if self.tabs.len() >= 20 {
                    return false;
                }
                self.tabs.push(format!("new {}", self.tabs.len() - 11));
                self.active = self.tabs.len() - 1;
                self.centre = true;
                self.said = "pressed +";
            }
            _ => match self.tabs(Theme::default()).on_key(key) {
                Some(i) if i != self.active => {
                    self.active = i;
                    self.centre = true;
                    self.said = "switched";
                }
                _ => return false,
            },
        }
        true
    }
}

fn overflow() -> Box<dyn Demo> {
    Box::new(Overflow {
        tabs: WORKSPACE.map(String::from).to_vec(),
        active: 5,
        first: 0,
        max_start: 0,
        centre: true,
        said: "deploy centred",
    })
}

/// Dragging a tab: a key moves a stand-in pointer; the marker shows where
/// the tab would land.
struct Drag {
    tabs: Vec<&'static str>,
    active: usize,
    /// The tab picked up, and the pointer's column.
    held: Option<usize>,
    pointer: u16,
    said: String,
}

impl Drag {
    fn tabs(&self, theme: Theme) -> Tabs<'static> {
        Tabs::new(self.tabs.iter().map(|l| Tab::new(*l)))
            .active(self.active)
            .theme(theme)
    }
}

impl Demo for Drag {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let row = Rect { height: 1, ..area };
        self.pointer = self.pointer.min(area.width.saturating_sub(1));
        let x = area.x + self.pointer;
        // Where the drop would go, from a layout taken without drawing.
        let drop = self
            .held
            .and_then(|_| self.tabs(*t).layout(row).drop_index(x, row.y));
        self.tabs(*t).drop_marker(drop).render_layout(row, buf);
        let below = rows_below(area, 1);
        if !below.is_empty() && x < below.right() {
            buf.set_stringn(x, below.y, "^", 1, t.accent());
        }
        let note = match (self.held, drop) {
            (Some(h), Some(d)) => format!(
                " holding {} · drops at {d} · ←/→ move · Enter drops",
                self.tabs[h]
            ),
            _ => format!(" {} · Enter picks up {}", self.said, self.tabs[self.active]),
        };
        Paragraph::new(text::truncate_line(
            Line::from(Span::styled(note, t.dim())),
            area.width as usize,
        ))
        .render(rows_below(area, 3), buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match (key.key, self.held) {
            (Key::Left, Some(_)) => self.pointer = self.pointer.saturating_sub(2),
            (Key::Right, Some(_)) => self.pointer = self.pointer.saturating_add(2),
            (Key::Esc, Some(_)) => {
                self.held = None;
                self.said = "put back".into();
            }
            (Key::Enter, Some(h)) => {
                let row = Rect::new(0, 0, u16::MAX, 1);
                let drop = self
                    .tabs(Theme::default())
                    .layout(row)
                    .drop_index(self.pointer, 0);
                if let Some(d) = drop {
                    let tab = self.tabs.remove(h);
                    let to = if d > h { d - 1 } else { d }.min(self.tabs.len());
                    self.tabs.insert(to, tab);
                    self.active = to;
                    self.said = format!("moved {tab} to {to}");
                }
                self.held = None;
            }
            (Key::Enter, None) => {
                self.held = Some(self.active);
                let row = Rect::new(0, 0, u16::MAX, 1);
                if let Some(h) = self
                    .tabs(Theme::default())
                    .layout(row)
                    .hits
                    .iter()
                    .find(|h| h.index == self.active)
                {
                    self.pointer = h.rect.x + 1;
                }
            }
            (_, None) => match self.tabs(Theme::default()).on_key(key) {
                Some(i) if i != self.active => self.active = i,
                _ => return false,
            },
            _ => return false,
        }
        true
    }
}

fn drag() -> Box<dyn Demo> {
    Box::new(Drag {
        tabs: vec!["main", "claude", "codex", "logs", "review", "deploy"],
        active: 1,
        held: Some(1),
        pointer: 27,
        said: String::new(),
    })
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "tabs",
    name: "Tabs",
    ty: "twolab_tui::tabs::Tabs",
    summary: "Which view you are on, and how to get to the others.",
    description: "One row of view names. The active one is reversed; each label's click rect comes back from the render, measured in cells, so a click on a wide Korean label lands on it. Key hints stay while they fit; a right-aligned tail takes the room the labels leave and gives way first; when even the plain labels do not fit, the strip scrolls to the active tab and counts the hidden ones. Opt-in parts from herdr's workspace tab bar: clickable < > scroll buttons in place of the counts, a trailing + action with its own rect, centring the active tab on a switch, and a drop marker with a midpoint drop index for drag-to-reorder.",
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
            name: ".scroll_buttons(bool)",
            ty: "bool",
            default: Some("false"),
            description: "When the labels overflow: ` < ` and ` > ` at the ends of the row instead of the ‹N / N› counts, with rects in the layout (scroll_left, scroll_right). Dim when nothing is hidden on that side, bold when something is.",
        },
        PropDoc {
            name: ".action(label)",
            ty: "&str",
            default: Some("none"),
            description: "A trailing action (\"+\" for a new tab) drawn ` + ` after the last tab, or at the right end when the strip overflows; its rect is layout.action.",
        },
        PropDoc {
            name: ".reveal(r)",
            ty: "Reveal",
            default: Some("Reveal::Edge"),
            description: "Where an overflowing strip starts: Edge keeps the active tab on screen from the left; Centre puts it mid-strip (after a switch); From(i) holds the host's scroll position (layout.hidden_before), clamped to layout.max_start.",
        },
        PropDoc {
            name: ".drop_marker(Some(i))",
            ty: "Option<usize>",
            default: Some("None"),
            description: "While dragging: ▐ where the tab would be inserted (0..=len), on the divider before tab i or just outside the drawn labels.",
        },
        PropDoc {
            name: "layout.target_at(col, row) / layout.drop_index(col, row)",
            ty: "Option<TabTarget> / Option<usize>",
            default: None,
            description: "What a click lands on (Tab(i), ScrollLeft, ScrollRight, Action); the insert index under a dragged pointer, split at each label's midpoint (herdr's tab_drop_index_at).",
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
        "Scroll buttons are dim when nothing is hidden on their side and bold when something is, so mono shows which way there is more.",
        "The drop marker ▐ differs from the │ divider in shape and weight, and is one cell on a CJK terminal (East-Asian Narrow).",
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
        DemoDoc {
            id: "overflow",
            title: "Overflow: < > buttons and +",
            description: "herdr's workspace strip with twelve tabs: PgUp/PgDn press the scroll buttons, Enter presses +, ←/→ switch and centre the new tab. The marks below are the click rects.",
            cols: 52,
            rows: 4,
            animated: false,
            interactive: true,
            make: overflow,
        },
        DemoDoc {
            id: "drag",
            title: "Drag to reorder",
            description: "Holding claude: ←/→ move a stand-in pointer (^); the ▐ marker shows where it drops, split at each label's midpoint. Enter drops, Esc puts it back.",
            cols: 64,
            rows: 4,
            animated: false,
            interactive: true,
            make: drag,
        },
    ],
};
