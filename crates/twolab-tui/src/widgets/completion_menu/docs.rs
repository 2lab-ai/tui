use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use super::{CompletionMenu, Item, MenuState};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::theme::Theme;
use crate::widgets::rule::Rule;

const USAGE: &str = r#"use twolab_tui::completion_menu::{CompletionMenu, Item, MenuState};

let items = vec![
    Item::new("/model", "choose the model and reasoning effort").meta("Model"),
    Item::new("/usage", "show token usage and spend").meta("Usage"),
];
let mut state = MenuState::new(); // keep it across frames

// Each frame: what the composer holds is the query.
let menu = CompletionMenu::new(&items, theme).query(&draft);
let height = menu.height().min(area.height);
frame.render_stateful_widget(&menu, area, &mut state);

// Keys while it is open.
match key.key {
    Key::Up => menu.previous(&mut state),
    Key::Down => menu.next(&mut state),
    Key::Tab => draft = menu.selected(&state).map(|i| i.label.clone()).unwrap_or(draft),
    _ => {}
}"#;

const COMMANDS: [(&str, &str, &str); 12] = [
    ("/help", "show available slash commands", "General"),
    (
        "/clear",
        "clear the conversation and free its context",
        "General",
    ),
    ("/new", "start a fresh session", "Session"),
    ("/resume", "resume a saved session", "Session"),
    ("/rename", "rename the current session", "Session"),
    (
        "/compact",
        "summarise the conversation to free context",
        "Session",
    ),
    ("/model", "choose the model and reasoning effort", "Model"),
    (
        "/permissions",
        "choose what runs without asking",
        "Settings",
    ),
    (
        "/statusline",
        "choose what the status line shows",
        "Settings",
    ),
    ("/usage", "show token usage and spend", "Usage"),
    ("/review", "review the current changes", "Tools"),
    ("/quit", "exit the interactive shell", "General"),
];

const SKILLS: [(&str, &str, &str); 7] = [
    (
        "$review-pr",
        "review a pull request against its issue",
        "project",
    ),
    (
        "$release-notes",
        "write release notes from two git refs",
        "user",
    ),
    (
        "$refactor",
        "restructure code without changing behaviour",
        "plugin",
    ),
    (
        "$code-review",
        "review the current diff for bugs",
        "project",
    ),
    (
        "$explore",
        "map an unfamiliar codebase before changing it",
        "user",
    ),
    ("$simplify", "cut what the change does not need", "plugin"),
    (
        "$debug",
        "trace a bug from its symptom to its cause",
        "user",
    ),
];

fn items(rows: &[(&str, &str, &str)]) -> Vec<Item> {
    rows.iter()
        .map(|(l, d, m)| Item::new(*l, *d).meta(*m))
        .collect()
}

/// The composer row above the menu: a prompt, the draft, a block cursor.
fn composer(draft: &str, t: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled("> ", t.accent()),
        Span::styled(draft.to_string(), t.text()),
        Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
    ])
}

/// Composer, rule, menu, rule — the band fx draws while the menu is open —
/// and an optional last line.
fn band(
    area: Rect,
    buf: &mut Buffer,
    t: &Theme,
    draft: &str,
    menu: Option<(&CompletionMenu, &mut MenuState)>,
    last: Option<Line<'static>>,
) {
    fn row(area: Rect, buf: &mut Buffer, y: &mut u16, line: &Line) {
        if *y < area.bottom() {
            buf.set_line(area.x, *y, line, area.width);
            *y += 1;
        }
    }
    let mut y = area.y;
    row(area, buf, &mut y, &composer(draft, t));
    let rule = Rule::new().style(t.border()).line(area.width);
    if let Some((menu, state)) = menu {
        row(area, buf, &mut y, &rule);
        let reserve = 1 + u16::from(last.is_some());
        let room = area.bottom().saturating_sub(y).saturating_sub(reserve);
        let h = menu.height().min(room);
        StatefulWidget::render(menu, Rect::new(area.x, y, area.width, h), buf, state);
        y += h;
    }
    row(area, buf, &mut y, &rule);
    if let Some(last) = last {
        row(area, buf, &mut y, &last);
    }
}

/// Type to filter, ↑/↓ to move, Tab or Enter to complete, Esc to close.
struct Slash {
    items: Vec<Item>,
    draft: String,
    state: MenuState,
    open: bool,
    used: Option<String>,
}

impl Slash {
    fn new() -> Slash {
        Slash {
            items: items(&COMMANDS),
            draft: "/".to_string(),
            state: MenuState::new(),
            open: true,
            used: None,
        }
    }
}

impl Demo for Slash {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        let menu = CompletionMenu::new(&self.items, *t).query(&self.draft);
        let last = match &self.used {
            Some(label) => Line::from(vec![
                Span::styled("enter: ran ", t.dim()),
                Span::styled(label.clone(), t.accent()),
            ]),
            None if self.open => Line::from(Span::styled(
                "type to filter   up/down move   tab complete   enter use   esc close",
                t.dim(),
            )),
            None => Line::from(Span::styled("menu closed: type to open it again", t.dim())),
        };
        let open = self.open.then_some((&menu, &mut self.state));
        band(area, buf, t, &self.draft, open, Some(last));
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let menu = CompletionMenu::new(&self.items, Theme::mono()).query(&self.draft);
        if let Some(c) = key.typed() {
            self.draft.push(c);
            self.open = true;
            self.used = None;
            return true;
        }
        match key.key {
            Key::Backspace => {
                if self.draft.chars().count() > 1 {
                    self.draft.pop();
                }
                self.open = true;
                self.used = None;
            }
            Key::Up if self.open => menu.previous(&mut self.state),
            Key::Down if self.open => menu.next(&mut self.state),
            Key::Tab | Key::Enter if self.open => {
                let Some(item) = menu.selected(&self.state) else {
                    return false;
                };
                let label = item.label.clone();
                if key.key == Key::Enter {
                    self.used = Some(label.clone());
                }
                self.draft = label;
            }
            Key::Esc => self.open = false,
            _ => return false,
        }
        true
    }
}

fn skills(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let list = items(&SKILLS);
    let draft = "$re";
    let menu = CompletionMenu::new(&list, *t).query(draft).title("Skills");
    band(
        area,
        buf,
        t,
        draft,
        Some((&menu, &mut MenuState::new())),
        None,
    );
}

fn no_match(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let list = items(&COMMANDS);
    let draft = "/deploy";
    let menu = CompletionMenu::new(&list, *t)
        .query(draft)
        .empty("no command matches /deploy; /help lists them all");
    band(
        area,
        buf,
        t,
        draft,
        Some((&menu, &mut MenuState::new())),
        None,
    );
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "completion-menu",
    name: "Completion menu",
    ty: "twolab_tui::completion_menu::CompletionMenu",
    summary: "The commands matching what you typed.",
    description: "An inline menu under the composer: a header with the count, then one row per match — label, description and an optional category column — with the matched part of each label in the accent colour and the marked row in bold. Matches rank exact, then prefix, then substring, ASCII case-insensitive. The window scrolls only at its edge and a last row counts what it hides; nothing matching says so.",
    used_in: "fx's slash menu (/) and skills menu ($) between the composer and the hint row; a command palette for llmux's single-letter bindings.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/footer/picker_presentation.zig#L1392-L1445",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "CompletionMenu::new(&items, theme)",
            ty: "&[Item], Theme",
            default: None,
            description: "Item::new(label, description).meta(category). Labels usually start with a sigil: / or $.",
        },
        PropDoc {
            name: ".query(q)",
            ty: "&str",
            default: Some("\"\""),
            description: "What was typed, sigil included. A new query moves the mark back to the first match.",
        },
        PropDoc {
            name: ".title(noun)",
            ty: "Into<String>",
            default: Some("\"Commands\""),
            description: "The header: `Commands 12 · type to filter`, then the visible range at the right while it overflows.",
        },
        PropDoc {
            name: ".max_rows(n)",
            ty: "u16",
            default: Some("6"),
            description: "Item rows at most; a `+N more` row follows when matches overflow.",
        },
        PropDoc {
            name: ".empty(text)",
            ty: "Into<String>",
            default: Some("nothing matches {query}"),
            description: "The one row shown when nothing matches.",
        },
        PropDoc {
            name: ".next(&mut state) / .previous(&mut state) / .selected(&state)",
            ty: "MenuState",
            default: None,
            description: "Move the mark (wrapping) and read the marked item for Tab or Enter.",
        },
        PropDoc {
            name: ".height() / .lines(w, h, &mut state)",
            ty: "u16 / Vec<Line<'static>>",
            default: None,
            description: "Rows the menu wants; the menu as lines for a host's own layout.",
        },
        PropDoc {
            name: "rank(label, query) / matches(items, query)",
            ty: "Option<(Rank, Range<usize>)> / Vec<Match>",
            default: None,
            description: "The matcher alone: tier and the matched bytes.",
        },
    ],
    keyboard: &[
        ("↑ / ↓", "mark the previous / next match, wrapping"),
        ("Tab", "complete the marked label into the composer"),
        ("Enter", "use the marked command"),
        ("Esc", "close the menu, keep the draft"),
    ],
    a11y: &[
        "The mark is weight, not only colour: bold on the marked row, so it reads in mono.",
        "In mono the matched part is underlined as well as bold, so it stays visible on the bold marked row.",
        "Never cut silently: hidden matches are counted (`+3 more`, the range in the header), long descriptions end in ….",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "slash",
            title: "Slash menu",
            description: "Type m, o: /mo leaves /model. ↑/↓ move, Tab or Enter completes, Esc closes.",
            cols: 76,
            rows: 13,
            animated: false,
            interactive: true,
            make: || Box::new(Slash::new()),
        },
        DemoDoc {
            id: "skills",
            title: "Skills menu",
            description: "$re: three prefix matches, then two that contain it, with their source.",
            cols: 76,
            rows: 10,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(skills)),
        },
        DemoDoc {
            id: "no-match",
            title: "Nothing matches",
            description: "An honest empty state instead of a closed menu.",
            cols: 64,
            rows: 4,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(no_match)),
        },
    ],
};
