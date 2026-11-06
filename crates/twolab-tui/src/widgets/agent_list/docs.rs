use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use super::{AgentItem, AgentList, AgentListState, AgentRail, AgentState, Tally};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::spinner;
use crate::text::{self, Align};
use crate::theme::Theme;

const USAGE: &str = r#"use std::time::Duration;
use twolab_tui::agent_list::{AgentItem, AgentList, AgentListState, AgentState, Tally};

// Data in: one item per agent, `changed_at` on the host's clock.
let items: Vec<AgentItem> = agents
    .iter()
    .map(|a| {
        AgentItem::new(a.state, &a.name)
            .line(format!("{} · tab {}", a.repo, a.tab))
            .changed_at(a.changed_at)
            .focused(a.id == focused_id)
    })
    .collect();

let list = AgentList::new(&items)
    .gap(1)
    .frame(twolab_tui::spinner::tick(elapsed, twolab_tui::spinner::DEFAULT_INTERVAL))
    .theme(theme);
frame.render_stateful_widget(&list, sidebar, &mut state); // state: AgentListState

// Clicks and keys.
if let Some(i) = state.hit_test(col, row) { focus(&agents[i]) }
if key == Key::Down { if let Some(i) = list.next_focus(1) { focus(&agents[i]) } }

// Collapsed sidebar and narrow header.
frame.render_stateful_widget(&list.rail(), rail_area, &mut state);
frame.render_widget(Tally::of(&items).theme(theme), header_row);"#;

fn agents() -> Vec<AgentItem> {
    let s = Duration::from_secs;
    vec![
        AgentItem::new(AgentState::Working, "claude")
            .line("tui · tab 2 · pane 1")
            .line("editing widgets/toast/mod.rs")
            .changed_at(s(40))
            .focused(true),
        AgentItem::new(AgentState::Waiting, "codex")
            .word("needs input")
            .line("llmux · tab 1")
            .line("approve: cargo publish --dry-run")
            .changed_at(s(55)),
        AgentItem::new(AgentState::Done, "pi")
            .line("herdr · tab 3")
            .line("review complete — 4 comments")
            .changed_at(s(30)),
        AgentItem::new(AgentState::Working, "gemini")
            .line("site · tab 1")
            .changed_at(s(20)),
        AgentItem::new(AgentState::Idle, "opencode")
            .line("notes · tab 1")
            .changed_at(s(5)),
        AgentItem::new(AgentState::Failed, "aider")
            .word("exited 1")
            .line("infra · tab 2")
            .line("error: 429 rate limited")
            .changed_at(s(50)),
        AgentItem::new(AgentState::Unknown, "zsh")
            .line("scratch · tab 4")
            .changed_at(s(0)),
    ]
}

fn frame(cx: &DemoCx) -> usize {
    spinner::tick(cx.t, spinner::DEFAULT_INTERVAL)
}

fn header(label: &str, right: &str, w: u16, t: &Theme) -> Line<'static> {
    let w = usize::from(w);
    let rw = text::width(right);
    let left = w.saturating_sub(rw + 1);
    let line = Line::from(vec![
        Span::styled(
            text::pad(label, left, Align::Left),
            t.dim().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(right.to_string(), t.dim()),
    ]);
    text::truncate_line(line, w)
}

fn agent_list(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    if area.is_empty() {
        return;
    }
    let items = agents();
    buf.set_line(
        area.x,
        area.y,
        &header(" agents", "priority", area.width, &t),
        area.width,
    );
    let body = Rect {
        y: area.y + 1,
        height: area.height.saturating_sub(2),
        ..area
    };
    let list = AgentList::new(&items).gap(1).frame(frame(cx)).theme(t);
    (&list).render(body, buf, &mut AgentListState::new());
    if area.height >= 2 {
        let tally = list.tally().line(area.width.saturating_sub(1));
        let mut spans = vec![Span::raw(" ")];
        spans.extend(tally.spans);
        buf.set_line(
            area.x,
            area.bottom() - 1,
            &text::truncate_line(Line::from(spans), usize::from(area.width)),
            area.width,
        );
    }
}

/// llmux's 29 accounts: ok, cooling or error.
fn accounts(focused: usize) -> Vec<AgentItem> {
    let s = Duration::from_secs;
    (0..29)
        .map(|i| {
            let n = i + 1;
            let provider = ["claude", "claude", "codex", "claude", "grok"][i % 5];
            let label = match provider {
                "codex" => format!("codex:ai{n}@insightquest.io"),
                "grok" => format!("grok:ai{n}@iq.io"),
                _ => format!("ai{n}@insightquest.io"),
            };
            let item = match i {
                5 | 17 => AgentItem::new(AgentState::Failed, label)
                    .word("error 401")
                    .line(format!("{provider} · token expired {}h ago", 1 + i % 4)),
                2 | 9 | 13 | 21 | 26 => {
                    let mins = 12 + (i * 17) % 140;
                    AgentItem::new(AgentState::Waiting, label)
                        .word(format!("cooling {}h{:02}m", mins / 60, mins % 60))
                        .line(format!("{provider} · 7d 0% left"))
                }
                _ => {
                    let left = 97 - (i * 23) % 80;
                    AgentItem::new(AgentState::Idle, label)
                        .word("ok")
                        .line(format!(
                            "{provider} · 7d {left}% left · reset {}d",
                            1 + i % 6
                        ))
                }
            };
            item.changed_at(s((i as u64 * 37) % 300))
                .focused(i == focused)
        })
        .collect()
}

fn account_tally(items: &[AgentItem], t: Theme) -> Tally {
    Tally::of(items)
        .word(AgentState::Failed, "error")
        .word(AgentState::Waiting, "cooling")
        .word(AgentState::Idle, "ok")
        .empty("no accounts")
        .theme(t)
}

/// The account list, with the host's focus moved by ↑/↓.
struct Accounts {
    items: Vec<AgentItem>,
    state: AgentListState,
}

impl Accounts {
    fn list(&self, t: Theme) -> AgentList<'_> {
        AgentList::new(&self.items).empty("no accounts").theme(t)
    }

    fn focus(&mut self, i: usize) {
        for (n, it) in self.items.iter_mut().enumerate() {
            it.focused = n == i;
        }
    }
}

impl Demo for Accounts {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = cx.theme;
        if area.is_empty() {
            return;
        }
        let tally = account_tally(&self.items, t).line(area.width.saturating_sub(1));
        let mut spans = vec![Span::raw(" ")];
        spans.extend(tally.spans);
        buf.set_line(
            area.x,
            area.y,
            &text::truncate_line(Line::from(spans), usize::from(area.width)),
            area.width,
        );
        let body = Rect {
            y: area.y + 1,
            height: area.height - 1,
            ..area
        };
        let list = AgentList::new(&self.items)
            .empty("no accounts")
            .frame(frame(cx))
            .theme(t);
        (&list).render(body, buf, &mut self.state);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let t = Theme::default();
        let next = match key.key {
            Key::Up => self.list(t).next_focus(-1),
            Key::Down => self.list(t).next_focus(1),
            Key::Home => self.list(t).next_focus(isize::MIN / 2),
            Key::End => self.list(t).next_focus(isize::MAX / 2),
            Key::PageDown => {
                self.state.scroll_by(5);
                return true;
            }
            Key::PageUp => {
                self.state.scroll_by(-5);
                return true;
            }
            _ => None,
        };
        match next {
            Some(i) => {
                self.focus(i);
                true
            }
            None => false,
        }
    }
}

fn rail(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = cx.theme;
    if area.is_empty() {
        return;
    }
    let a = agents();
    let accts = accounts(5);
    let f = frame(cx);
    // Agents rail, 4 columns; accounts rail, 6 columns.
    let r1 = Rect {
        width: area.width.min(4),
        ..area
    };
    (&AgentRail::new(&a).frame(f).theme(t)).render(r1, buf, &mut AgentListState::new());
    if area.width > 5 {
        let r2 = Rect {
            x: area.x + 5,
            width: (area.width - 5).min(6),
            ..area
        };
        (&AgentRail::new(&accts).frame(f).theme(t)).render(r2, buf, &mut AgentListState::new());
    }
    if area.width <= 13 {
        return;
    }
    let x = area.x + 13;
    let w = area.width - 13;
    let agents_tally = Tally::of(&a).theme(t);
    let acct_tally = account_tally(&accts, t).glyphs(true);
    let mut rows: Vec<Line<'static>> = vec![Line::from(Span::styled(
        "tally — whole counts drop, then …",
        t.dim(),
    ))];
    for cells in [52u16, 40, 30, 20, 12] {
        let mut spans = vec![Span::styled(format!("{cells:>3} "), t.dim())];
        spans.extend(agents_tally.line(cells).spans);
        rows.push(Line::from(spans));
    }
    rows.push(Line::default());
    rows.push(Line::from(Span::styled("accounts, with glyphs", t.dim())));
    for cells in [44u16, 30, 18] {
        let mut spans = vec![Span::styled(format!("{cells:>3} "), t.dim())];
        spans.extend(acct_tally.line(cells).spans);
        rows.push(Line::from(spans));
    }
    for (n, l) in rows.into_iter().enumerate() {
        let y = area.y + n as u16;
        if y >= area.bottom() {
            break;
        }
        buf.set_line(x, y, &text::truncate_line(l, usize::from(w)), w);
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "agent-list",
    name: "Agent list",
    ty: "twolab_tui::agent_list::AgentList",
    summary: "Who is doing what, and who needs the person first.",
    description: "Rows of agents or accounts, each with its own state glyph (✗ failed, ◉ waiting, ✓ done, a turning braille glyph for working, ◦ idle, ? unknown), a label, the state's word and secondary lines. Sorted by priority, then by the most recent change; gaps between rows, rows of different heights, scrolling that says ▴/▾ +N more, the focus highlighted and kept in view, a hit rect per item. Folds to a 4–6 column rail (index and glyph) and to a one-line tally that drops whole counts and ends in ….",
    used_in: "herdr: the agents panel of the sidebar, its collapsed rail and the narrow-screen header. llmux dashboard (next): a sidebar of all 29 accounts with ok / cooling / error, where the table shows four rows and +25 more at 80 columns.",
    source: Source {
        project: "herdr",
        url: "https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/client/shell/agent_sidebar.rs#L20-L374",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "AgentItem::new(state, label)",
            ty: "(AgentState, impl Into<String>)",
            default: None,
            description: "One agent or account. AgentState: Failed, Waiting, Done, Working, Idle, Unknown; From<Status> converts.",
        },
        PropDoc {
            name: ".word(w) / .line(l) / .changed_at(d) / .focused(b)",
            ty: "impl Into<String> / impl Into<Line> / Duration / bool",
            default: Some("state's word / none / 0 / false"),
            description: "The word at the right (`needs input`, `cooling 1h12m`); a secondary line (any number); when the state last changed on the host's clock; the focus.",
        },
        PropDoc {
            name: "AgentList::new(&items).sort(s)",
            ty: "Sort",
            default: Some("Sort::Priority"),
            description: "Priority: failed > waiting > done > working > idle > unknown, then the most recent change. Given: as passed.",
        },
        PropDoc {
            name: ".gap(rows) / .words(on) / .empty(msg)",
            ty: "u16 / bool / impl Into<String>",
            default: Some("0 / true / \"no agents\""),
            description: "Blank rows between items; the state word on each row; what an empty list says.",
        },
        PropDoc {
            name: ".frame(tick)",
            ty: "usize",
            default: Some("0"),
            description: "The shared spinner tick; the working glyph turns with it, held still under reduced motion.",
        },
        PropDoc {
            name: "render(area, buf, &mut AgentListState)",
            ty: "StatefulWidget",
            default: None,
            description: "state.hit_test(col, row) gives the item; state.scroll_by(n) for the wheel; layout() has the ▴/▾ rows and counts.",
        },
        PropDoc {
            name: ".next_focus(delta) / .rail() / .tally()",
            ty: "Option<usize> / AgentRail / Tally",
            default: None,
            description: "The item above or below the focus in the order shown; the folded rail; the one-line tally.",
        },
        PropDoc {
            name: "Tally::of(&items).word(state, w).glyphs(on).line(width)",
            ty: "Line<'static>",
            default: Some("state words, no glyphs"),
            description: "`2 waiting · 1 done · 3 working …`: zeros left out, whole counts dropped, … when it does.",
        },
    ],
    keyboard: &[
        (
            "↑ / ↓ (host)",
            "next_focus(∓1): the item above or below in the order shown",
        ),
        (
            "PgUp / PgDn, wheel (host)",
            "state.scroll_by(n): scroll without moving the focus",
        ),
    ],
    a11y: &[
        "Every state has its own glyph and word, so states read without colour; in mono failed and waiting are bold, unknown dim.",
        "Each state glyph is one cell on a terminal that draws East-Asian Ambiguous glyphs wide (width_cjk = 1); herdr's ● ○ ◐ are not.",
        "The focused item is marked with › and reversed (or raised), not by colour alone.",
        "Nothing is cut silently: ▴/▾ +N more for items out of view, labels end in …, the tally ends in … when it drops a count.",
        "Under reduced motion the working glyph holds still.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "agents",
            title: "Agents by priority",
            description: "Seven agents: the failed and waiting ones first, the working glyph turning, secondary lines of different lengths, the focused one marked; the tally below.",
            cols: 46,
            rows: 22,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(agent_list)),
        },
        DemoDoc {
            id: "accounts",
            title: "llmux accounts",
            description: "All 29 accounts, errors and cooling ones first: ↑/↓ moves the focus and the list follows it, PgUp/PgDn scroll, ▴/▾ say how many are out of view.",
            cols: 56,
            rows: 16,
            animated: false,
            interactive: true,
            make: || {
                Box::new(Accounts {
                    items: accounts(5),
                    state: AgentListState::new(),
                })
            },
        },
        DemoDoc {
            id: "rail",
            title: "Rail and tally",
            description: "The agents folded to a 4-column rail and the accounts to a 6-column one (index and glyph, ▾+N for the rest); the tally at shrinking widths.",
            cols: 72,
            rows: 14,
            animated: true,
            interactive: false,
            make: || Box::new(FnDemo(rail)),
        },
    ],
};
