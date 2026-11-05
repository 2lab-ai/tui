use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, StatefulWidget, Widget};

use super::{Menu, MenuAction, MenuItem, MenuState};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Status, Theme};

const USAGE: &str = r#"use twolab_tui::menu::{Menu, MenuAction, MenuItem, MenuState};

let menu = Menu::new([
    MenuItem::new("switch now").key("s"),
    MenuItem::new("pause").key("p"),
    MenuItem::new("set limit").key("L"),
    MenuItem::separator(),
    MenuItem::new("delete").key("r").destructive(true),
])
.title(account)
.anchor(mouse.column, mouse.row)
.theme(theme);

// Over the whole frame: the menu places itself and records its rects.
frame.render_stateful_widget(&menu, frame.area(), &mut app.menu);

match app.menu.on_key(&menu, key) {
    MenuAction::Run(i) => run(i),
    MenuAction::Close => app.menu_open = false,
    MenuAction::Moved | MenuAction::Ignored => {}
}
// A click: an item runs it, outside the popup closes it.
match app.menu.hit(mouse.column, mouse.row) {
    Some(i) => run(i),
    None if !app.menu.layout.contains(mouse.column, mouse.row) => app.menu_open = false,
    None => {}
}"#;

const ACCOUNTS: [(&str, &str, Status, &str); 5] = [
    ("zhuge@2lab.ai", "active", Status::Done, "41%"),
    ("ai3@iq.io", "ready", Status::Done, "63%"),
    ("simayi@2lab.ai", "ready", Status::Done, "12%"),
    ("dev1@2lab.ai", "cooldown", Status::Waiting, "88%"),
    ("ai2@iq.io", "7d spent", Status::Failed, "99%"),
];

fn table(area: Rect, buf: &mut Buffer, theme: &Theme) {
    let mut lines = vec![Line::from(Span::styled(
        "  #  account          status     7d",
        theme.dim(),
    ))];
    for (i, (name, status, st, pct)) in ACCOUNTS.iter().enumerate() {
        lines.push(Line::from(vec![
            Span::styled(format!("  {}  ", i + 1), theme.dim()),
            Span::styled(text::pad(name, 17, text::Align::Left), theme.text()),
            Span::styled(text::pad(status, 11, text::Align::Left), theme.status(*st)),
            Span::styled(*pct, theme.secondary()),
        ]));
    }
    Paragraph::new(lines).render(area, buf);
}

fn footer(area: Rect, buf: &mut Buffer, line: Line<'static>) {
    if area.height == 0 {
        return;
    }
    let y = area.bottom() - 1;
    buf.set_line(
        area.x,
        y,
        &text::truncate_line(line, area.width as usize),
        area.width,
    );
}

fn account_menu(theme: Theme) -> Menu<'static> {
    Menu::new([
        MenuItem::new("switch now").key("s"),
        MenuItem::new("pause").key("p"),
        MenuItem::new("resume").disabled(true),
        MenuItem::new("set limit").key("L"),
        MenuItem::separator(),
        MenuItem::new("delete").key("r").destructive(true),
    ])
    .title("ai3@iq.io")
    .theme(theme)
}

/// Right-clicked `ai3@iq.io`: ↑/↓, Enter, Esc; Enter reopens a closed menu.
struct Actions {
    state: MenuState,
    open: bool,
    said: Option<String>,
}

impl Demo for Actions {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        table(area, buf, t);
        let note = match (&self.said, self.open) {
            (_, true) => Line::from(Span::styled(
                " ↑↓ move · Enter run · s/p/L/r run directly · Esc close",
                t.dim(),
            )),
            (Some(said), false) => Line::from(vec![
                Span::styled(format!(" {said}"), t.secondary()),
                Span::styled(" · Enter reopens", t.dim()),
            ]),
            (None, false) => Line::from(Span::styled(" Enter reopens the menu", t.dim())),
        };
        footer(area, buf, note);
        if self.open {
            let menu = account_menu(*t).anchor(area.x + 5, area.y + 2);
            let bounds = Rect {
                height: area.height.saturating_sub(1),
                ..area
            };
            (&menu).render(bounds, buf, &mut self.state);
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let menu = account_menu(Theme::default());
        if !self.open {
            if key.key == Key::Enter {
                self.open = true;
                self.state = MenuState::new(&menu);
                return true;
            }
            return false;
        }
        match self.state.on_key(&menu, key) {
            MenuAction::Moved => true,
            MenuAction::Ignored => false,
            MenuAction::Run(i) => {
                self.open = false;
                self.said = Some(format!("ran {} on ai3@iq.io", menu.items()[i].label));
                true
            }
            MenuAction::Close => {
                self.open = false;
                self.said = Some("closed without running anything".to_string());
                true
            }
        }
    }
}

fn edge(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    if area.is_empty() {
        return;
    }
    let anchor = (
        area.right().saturating_sub(4).max(area.x),
        area.bottom().saturating_sub(2).max(area.y),
    );
    let menu = Menu::new([
        MenuItem::new("copy model id").key("c"),
        MenuItem::new("pin to strip").key("p"),
        MenuItem::new("hide for today").key("h"),
        MenuItem::separator(),
        MenuItem::new("open raw request").key("o"),
    ])
    .title("gpt-5.6-sol")
    .anchor(anchor.0, anchor.1)
    .theme(*t);
    let at = menu.area(area);
    let where_ = format!(
        " clicked col {}, row {}: opens {} and {}",
        anchor.0 - area.x,
        anchor.1 - area.y,
        if at.y < anchor.1 { "above" } else { "below" },
        if at.x < anchor.0 {
            "to the left"
        } else {
            "to the right"
        },
    );
    Paragraph::new(text::truncate_line(
        Line::from(Span::styled(where_, t.dim())),
        area.width as usize,
    ))
    .render(area, buf);
    buf.set_stringn(anchor.0, anchor.1, "+", 1, t.accent());
    let mut state = MenuState::new(&menu);
    (&menu).render(area, buf, &mut state);
}

fn destructive(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let menu = Menu::new([
        MenuItem::new("copy email").key("c"),
        MenuItem::new("re-login").key("l"),
        MenuItem::new("reset 7d window").disabled(true),
        MenuItem::separator(),
        MenuItem::new("revoke refresh token").destructive(true),
        MenuItem::new("delete account").key("r").destructive(true),
    ])
    .title("zhuge@2lab.ai")
    .anchor(area.x + 2, area.y)
    .theme(*t);
    let mut state = MenuState {
        selected: 5,
        ..MenuState::default()
    };
    Paragraph::new(Line::from(Span::styled(
        " zhuge@2lab.ai  active  7d 41%",
        t.secondary(),
    )))
    .render(area, buf);
    footer(
        area,
        buf,
        Line::from(vec![
            Span::styled(" Enter ", t.status(Status::Failed)),
            Span::styled("deletes zhuge@2lab.ai · ", t.dim()),
            Span::styled("Esc ", t.accent()),
            Span::styled("keeps it", t.dim()),
        ]),
    );
    let bounds = Rect {
        height: area.height.saturating_sub(1),
        ..area
    };
    (&menu).render(bounds, buf, &mut state);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "menu",
    name: "Menu",
    ty: "twolab_tui::menu::Menu",
    summary: "What you can do to this thing, right where you clicked.",
    description: "A popup list anchored at the clicked cell. It opens on the row below and flips above or to the left when the screen runs out, so the row you clicked stays visible. ↑/↓ move over runnable items only (separators and disabled items are skipped), Enter runs, an item's key runs it directly, Esc closes. Every runnable row's rect comes back from the render, measured in cells.",
    used_in: "llmux: right-click on an accounts row — switch now, pause or resume, set limit, delete — acting on the account pinned when the menu opened.",
    source: Source {
        project: "llmux",
        url: "https://github.com/2lab-ai/llmux/blob/524b6e4/src/tui/ui.rs#L3303-L3395",
        license: "MIT",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "Menu::new(items)",
            ty: "impl IntoIterator<Item = MenuItem>",
            default: None,
            description: "MenuItem::new(label) with .key(\"p\"), .disabled(true), .destructive(true); MenuItem::separator().",
        },
        PropDoc {
            name: ".title(s)",
            ty: "&str",
            default: Some("none"),
            description: "What the menu acts on, on the top border.",
        },
        PropDoc {
            name: ".anchor(col, row)",
            ty: "(u16, u16)",
            default: Some("(0, 0)"),
            description: "The clicked cell. The popup opens on the row below, or flips above / left to stay on screen.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Selected row selected + accent + bold; destructive in the failure colour (bold in mono); disabled dim.",
        },
        PropDoc {
            name: "MenuState::new(&menu)",
            ty: "MenuState { selected, offset, layout }",
            default: None,
            description: "Selection on the first runnable item. Rendering writes layout: the popup area and one MenuHit { rect, index } per runnable row.",
        },
        PropDoc {
            name: "state.on_key(&menu, key)",
            ty: "MenuAction",
            default: None,
            description: "Moved, Run(i), Close or Ignored.",
        },
        PropDoc {
            name: "state.hit(col, row) / state.layout.contains(col, row)",
            ty: "Option<usize> / bool",
            default: None,
            description: "The item under a click; whether the click is on the popup at all.",
        },
        PropDoc {
            name: "menu.area(bounds)",
            ty: "Rect",
            default: None,
            description: "Where the popup goes, without drawing it.",
        },
    ],
    keyboard: &[
        ("↑ / k, ↓ / j", "previous / next runnable item (no wrap)"),
        ("Home / End", "first / last runnable item"),
        ("Enter", "run the selected item"),
        ("an item's key", "run that item"),
        ("Esc, any other key", "close without running"),
    ],
    a11y: &[
        "The selected row is reversed and bold; disabled items are dim and destructive ones bold, so all three read without colour.",
        "Opens beside the clicked row, never over it, so you can see what the menu acts on.",
        "A menu taller than the screen scrolls to the selection and says how many items are out of view (+N more).",
        "Every action also has a key, shown at the right of its row.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "actions",
            title: "Account actions",
            description: "Right-clicked ai3@iq.io: ↑/↓ skip the disabled item and the separator, Enter runs, Esc closes.",
            cols: 56,
            rows: 12,
            animated: false,
            interactive: true,
            make: || {
                Box::new(Actions {
                    state: MenuState::new(&account_menu(Theme::default())),
                    open: true,
                    said: None,
                })
            },
        },
        DemoDoc {
            id: "edge",
            title: "Near the bottom-right edge",
            description: "Clicked two cells from the corner: the popup flips above and to the left.",
            cols: 60,
            rows: 12,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(edge)),
        },
        DemoDoc {
            id: "destructive",
            title: "A destructive item",
            description: "Delete and revoke in the failure colour; the selection on delete says what Enter will do.",
            cols: 56,
            rows: 11,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(destructive)),
        },
    ],
};
