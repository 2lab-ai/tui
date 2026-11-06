use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::{Column, Outcome, RowDisplay, Search, SelectItem, SelectList};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::render::{FlexRenderable, Renderable, RenderableItem};
use crate::theme::Status;

const USAGE: &str = r#"use twolab_tui::select_list::{Outcome, Search, SelectItem, SelectList};

let mut list = SelectList::new(vec![
    SelectItem::new("claude-opus-5-5").description("1M context, $4 / $20 per Mtok").current(),
    SelectItem::new("claude-haiku-4-5").description("200k context, $1 / $5 per Mtok"),
    SelectItem::new("claude-fable-5-1").disabled("not on this plan"),
])
.title("Select model")
.search(Search::OnDemand)
.theme(theme);

// In the event loop:
match list.key(key) {
    Outcome::Accepted(i) => use_model(&list.items()[i].name),
    Outcome::Cancelled => close(),
    _ => {}
}
frame.render_widget(&list, area);

// A tree (herdr's navigator): depth, folding groups, meta columns, a detail line.
let nav = SelectList::new(vec![
    SelectItem::new("local").group(),
    SelectItem::new("llmux").depth(1).group().detail("~/2lab.ai/llmux · main ↑2"),
    SelectItem::new("claude").depth(2).meta(["claude", "working"]),
    SelectItem::new("tui").depth(1).collapsed(),          // ▸ tui +3
    SelectItem::new("shell").depth(2).meta(["terminal", "shell"]),
])
.columns([Column::new(11).min_width(36), Column::new(11).min_width(64)])
.numbered(false);
// ←/→ fold, unfold and walk; nav.toggle(i) for a click on a toggle."#;

/// A list and what the last pick or cancel said.
struct ListDemo {
    list: SelectList,
    status: Option<(String, bool)>,
}

impl Demo for ListDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        self.list.set_theme(cx.theme);
        let mut col = FlexRenderable::new();
        col.push(1, RenderableItem::borrowed(&self.list));
        if let Some((msg, picked)) = &self.status {
            let style = if *picked {
                cx.theme.status(Status::Done)
            } else {
                cx.theme.dim()
            };
            col.push(
                0,
                RenderableItem::owned(Line::styled(format!("  {msg}"), style)),
            );
        }
        col.render(area, buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.list.key(key) {
            Outcome::Accepted(i) => {
                let name = self.list.items()[i].name.clone();
                self.status = Some((format!("picked {name}"), true));
                true
            }
            Outcome::Cancelled => {
                self.status = Some(("went back without picking".into(), false));
                true
            }
            Outcome::Changed => {
                self.status = None;
                true
            }
            Outcome::Ignored => false,
        }
    }
}

fn model_items() -> Vec<SelectItem> {
    vec![
        SelectItem::new("claude-opus-5-5")
            .description("1M context, $4 in / $20 out per Mtok. The default.")
            .current(),
        SelectItem::new("claude-sonnet-5-5")
            .description("1M context, $2 / $10 per Mtok. Everyday coding at speed."),
        SelectItem::new("claude-haiku-4-5")
            .description("200k context, $1 / $5 per Mtok. Fastest; good for sub-agents."),
        SelectItem::new("claude-fable-5-1")
            .description("1M context, $10 / $50 per Mtok. Most capable.")
            .disabled("not on this plan"),
        SelectItem::new("claude-opus-4-8")
            .description("1M context, $5 / $25 per Mtok. Previous Opus."),
        SelectItem::new("claude-sonnet-4-6")
            .description("1M context, $3 / $15 per Mtok. Previous Sonnet."),
    ]
}

fn models() -> Box<dyn Demo> {
    Box::new(ListDemo {
        list: SelectList::new(model_items())
            .title("Select model")
            .subtitle("Used for new requests from zhuge@2lab.ai")
            .search(Search::OnDemand),
        status: None,
    })
}

const SESSIONS: [(&str, &str); 30] = [
    ("fix flaky pager scroll test", "llmux, 12 min ago, 14 turns"),
    ("port codex select list", "tui, 40 min ago, 31 turns"),
    (
        "why does the 7d window reset early",
        "llmux, 2 h ago, 6 turns",
    ),
    (
        "add cooldown clock to accounts table",
        "llmux, 3 h ago, 22 turns",
    ),
    (
        "review PR 412: catalog wasm exports",
        "tui, 5 h ago, 9 turns",
    ),
    (
        "quota bar colours in light theme",
        "ui, yesterday, 11 turns",
    ),
    ("migrate theme tokens to OKLCH", "ui, yesterday, 27 turns"),
    ("raw request viewer: curl tab", "llmux, yesterday, 18 turns"),
    (
        "debug SSE stall on large tool input",
        "llmux, 2 days ago, 40 turns",
    ),
    ("write release notes for 0.4", "llmux, 2 days ago, 5 turns"),
    (
        "CJK width bug in session table",
        "tui, 3 days ago, 13 turns",
    ),
    ("spinner reduced-motion audit", "tui, 3 days ago, 7 turns"),
    ("rotate expired OAuth tokens", "llmux, 4 days ago, 16 turns"),
    (
        "proxy: retry 529 with jitter",
        "llmux, 4 days ago, 21 turns",
    ),
    ("table sort by reset time", "llmux, 5 days ago, 8 turns"),
    ("site: dark mode flash on load", "ui, 5 days ago, 4 turns"),
    ("bench render_offset_content", "tui, 6 days ago, 10 turns"),
    (
        "account modal layout with flex",
        "llmux, 6 days ago, 19 turns",
    ),
    ("logs overlay: follow mode", "llmux, last week, 12 turns"),
    ("keys picker: require one key", "llmux, last week, 6 turns"),
    ("NO_COLOR snapshot tests", "tui, last week, 9 turns"),
    ("deploy catalog to vercel", "tui, last week, 3 turns"),
    ("usage export to CSV", "llmux, last week, 15 turns"),
    ("fx diff renderer study", "tui, 2 weeks ago, 24 turns"),
    (
        "pricing table for new models",
        "llmux, 2 weeks ago, 7 turns",
    ),
    ("attach mode stdin race", "llmux, 2 weeks ago, 33 turns"),
    ("shimmer timing at 32 ms", "tui, 3 weeks ago, 8 turns"),
    ("headless mode for CI", "llmux, 3 weeks ago, 12 turns"),
    (
        "first commit: dashboard skeleton",
        "llmux, last month, 52 turns",
    ),
    ("pick a name for the catalog", "ui, last month, 2 turns"),
];

fn sessions() -> Box<dyn Demo> {
    let items = SESSIONS
        .iter()
        .map(|(name, desc)| SelectItem::new(*name).description(*desc))
        .collect();
    Box::new(ListDemo {
        list: SelectList::new(items)
            .title("Resume a session")
            .row_display(RowDisplay::SingleLine)
            .search(Search::OnDemand),
        status: None,
    })
}

fn no_match() -> Box<dyn Demo> {
    Box::new(ListDemo {
        list: SelectList::new(model_items())
            .title("Select model")
            .search(Search::Immediate)
            .placeholder("Type a model name")
            .with_query("gemini"),
        status: None,
    })
}

/// herdr's "Go to" navigator: machines, workspaces, panes.
fn navigator_items() -> Vec<SelectItem> {
    let pane = |name: &str, agent: &str, state: &str, detail: &str| {
        SelectItem::new(name)
            .depth(2)
            .meta([agent, state])
            .detail(detail)
    };
    vec![
        SelectItem::new("local")
            .group()
            .detail("this machine · 3 workspaces"),
        SelectItem::new("llmux")
            .depth(1)
            .group()
            .detail("~/2lab.ai/llmux · main ↑2"),
        pane(
            "fix flaky pager test",
            "claude",
            "working",
            "pane 1 · tab 2 · claude-opus-5-5 · 14 turns",
        ),
        pane(
            "rotate oauth tokens",
            "codex",
            "blocked",
            "pane 2 · tab 2 · waiting for approval",
        ),
        pane(
            "zsh",
            "terminal",
            "shell",
            "pane 3 · tab 3 · ~/2lab.ai/llmux",
        ),
        SelectItem::new("tui")
            .depth(1)
            .group()
            .detail("~/2lab.ai/tui · port/herdr-merge-b"),
        pane(
            "port herdr tabs",
            "claude",
            "done",
            "pane 1 · tab 1 · 31 turns",
        ),
        pane("catalog wasm build", "claude", "idle", "pane 2 · tab 1"),
        SelectItem::new("herdr")
            .depth(1)
            .collapsed()
            .detail("~/src/herdr · main"),
        pane("read selection.rs", "claude", "done", "pane 1 · tab 1"),
        pane("cargo test", "terminal", "shell", "pane 2 · tab 1"),
        SelectItem::new("studio-mac").group().detail("ssh · online"),
        SelectItem::new("site")
            .depth(1)
            .group()
            .detail("~/site · deploy ↓1"),
        pane(
            "deploy preview",
            "fx",
            "working",
            "pane 1 · tab 1 · vercel preview",
        ),
        pane("lighthouse run", "terminal", "shell", "pane 2 · tab 1"),
    ]
}

fn navigator() -> Box<dyn Demo> {
    Box::new(ListDemo {
        list: SelectList::new(navigator_items())
            .title("Go to")
            .numbered(false)
            .max_rows(10)
            .columns([Column::new(11).min_width(36), Column::new(11).min_width(64)])
            .search(Search::OnDemand)
            .placeholder("search agents and terminals"),
        status: None,
    })
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "select-list",
    name: "Select list",
    ty: "twolab_tui::select_list::SelectList",
    summary: "Which one of these to use — and which is in effect now, which cannot be picked and why.",
    description: "A pick-one list from Codex's selection popup: `› 1. name (current)` with a description column aligned across rows, disabled rows that say why and that the cursor skips, a substring search (always on, or opened with /), digits 1–9 to pick, ▴/▾ marks and an N of M position when rows are out of view, and an honest `no match for \"…\"` when the filter empties it. Trees from herdr's navigator opt in per item: depth with ├─ └─ │ connectors, ▾/▸ group rows that fold (←/→) and say how many rows they hide, meta columns at the right that appear by width, and a detail line for the selected row.",
    used_in: "Codex's /model, /approvals and other pickers. llmux's context menu, config rows and account switcher are the planned hosts.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/bottom_pane/list_selection_view.rs#L168-L1910",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "SelectList::new(items)",
            ty: "Vec<SelectItem>",
            default: None,
            description: "SelectItem::new(name).description(d).current().disabled(reason).search_value(s).",
        },
        PropDoc {
            name: ".title(s) / .subtitle(s)",
            ty: "impl Into<String>",
            default: Some("none"),
            description: "Header lines above the list.",
        },
        PropDoc {
            name: ".search(mode)",
            ty: "Search::{Off, Immediate, OnDemand}",
            default: Some("Off"),
            description: "Immediate types every key into the filter; OnDemand opens it with /, so digits still pick until then.",
        },
        PropDoc {
            name: ".numbered(bool)",
            ty: "bool",
            default: Some("true"),
            description: "Number enabled rows and let digits 1–9 pick them (not while the search line takes keys).",
        },
        PropDoc {
            name: ".row_display(d)",
            ty: "RowDisplay::{Wrapped, SingleLine}",
            default: Some("Wrapped"),
            description: "Wrap descriptions under their column, or keep one line per row and end long ones in …",
        },
        PropDoc {
            name: ".max_rows(n)",
            ty: "usize",
            default: Some("8"),
            description: "Rows shown at once; PgUp/PgDn move by the rows actually shown.",
        },
        PropDoc {
            name: ".glyphs(g)",
            ty: "SelectGlyphs",
            default: Some("› ▴ ▾ (Narrow)"),
            description: "SelectGlyphs::ascii() for > ^ v; SelectGlyphs::arrows() opts into Codex's ↑ ↓ (Ambiguous width).",
        },
        PropDoc {
            name: "SelectItem .depth(n) / .group() / .collapsed()",
            ty: "u8 / — / —",
            default: Some("0, not a group"),
            description: "Tree rows: indent under the nearest shallower row with ├─ └─ │ connectors; a ▾ group whose deeper rows fold away (collapsed() starts folded: ▸ and +N hidden).",
        },
        PropDoc {
            name: ".columns([Column::new(w).min_width(n)])",
            ty: "impl IntoIterator<Item = Column>",
            default: Some("none"),
            description: "Meta columns at the right, filled from SelectItem::meta([..]); each shows on rows at least min_width wide (herdr: agent from 36, state from 64). Long values end in …, missing ones say —.",
        },
        PropDoc {
            name: "SelectItem::detail(s)",
            ty: "impl Into<String>",
            default: Some("none"),
            description: "A line under the list while the row is selected; the row is reserved whenever any item has one, so the footer holds still.",
        },
        PropDoc {
            name: ".toggle(i) / .set_collapsed(i, b) / .is_collapsed(i)",
            ty: "usize, bool",
            default: None,
            description: "Fold or unfold a group from code (a click on its toggle); .tree_glyphs(TreeGlyphs::ascii()) swaps the connectors for |- `- |.",
        },
        PropDoc {
            name: ".key(key) -> Outcome",
            ty: "KeyInput",
            default: None,
            description: "Ignored, Changed, Accepted(index into items) or Cancelled.",
        },
        PropDoc {
            name: ".footer(line) / .placeholder(s) / .empty_message(s) / .theme(t)",
            ty: "Line / String / Theme",
            default: Some("key hints / \"Type to search\""),
            description: "Replace the key-hint footer, the search placeholder, the empty-list text; set the theme.",
        },
    ],
    keyboard: &[
        (
            "↑ ↓ / Ctrl-P Ctrl-N",
            "Move, wrapping at the ends; disabled rows are skipped",
        ),
        (
            "PgUp PgDn / Ctrl-B Ctrl-F",
            "Move a page (the rows shown), stopping at the ends",
        ),
        ("Home End", "First or last enabled row"),
        (
            "Enter",
            "Pick the highlighted row (on an empty list: go back)",
        ),
        ("Esc", "Close an on-demand search, else go back"),
        (
            "1–9",
            "Pick the Nth enabled row (when the search line is not taking keys)",
        ),
        ("/", "Open the search line (Search::OnDemand)"),
        ("typing, Backspace", "Edit the search text"),
        (
            "← →",
            "Tree lists: fold the open group or go to the parent; unfold the folded group or go to its first row",
        ),
    ],
    a11y: &[
        "The highlighted row is the theme's selection (reversed in mono and ANSI) and also carries the › cursor, so it reads without colour.",
        "Disabled rows say why in words (disabled: reason) and are never only dimmed.",
        "Rows out of view are announced: ▴/▾ marks and an N of M position; an empty filter says no match instead of drawing nothing.",
        "Every default glyph (› ▴ ▾) is East-Asian Narrow, so the gutter stays one cell on CJK terminals.",
        "Open and folded groups differ by glyph (▾ / ▸, both Narrow), and a folded group says how many rows it hides (+3), so nothing disappears silently.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "models",
            title: "Model picker",
            description: "Context window and price per row, the current model marked, one model disabled with its reason. Digits pick; / searches.",
            cols: 84,
            rows: 15,
            animated: false,
            interactive: true,
            make: models,
        },
        DemoDoc {
            id: "sessions",
            title: "A long list that scrolls",
            description: "Thirty sessions, eight at a time, one line each: ▴/▾ and N of M say where you are; PgUp/PgDn move by a page.",
            cols: 72,
            rows: 13,
            animated: false,
            interactive: true,
            make: sessions,
        },
        DemoDoc {
            id: "no-match",
            title: "No match",
            description: "The filter matches nothing and says so. Backspace brings the models back; Enter on the empty list goes back.",
            cols: 64,
            rows: 9,
            animated: false,
            interactive: true,
            make: no_match,
        },
        DemoDoc {
            id: "navigator",
            title: "Tree: machines, workspaces, panes",
            description: "herdr's Go to: ←/→ fold and walk, ↑/↓ move, / searches (matches keep their parents). Agent and state columns appear by width; the selected row's detail sits under the list.",
            cols: 84,
            rows: 18,
            animated: false,
            interactive: true,
            make: navigator,
        },
    ],
};
