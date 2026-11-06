use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use super::{HintGroup, KeyHelp, KeyHelpState, KeyHint, KeyHints};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::composer::{Composer, ComposerEvent, ComposerState};
use crate::input::KeyInput;
use crate::text;

const USAGE: &str = r#"use twolab_tui::key_hints::{KeyHint, KeyHints};

let footer = KeyHints::new([
    KeyHint::new("q", "quit").priority(9),
    KeyHint::new("esc", "back").priority(8),
    KeyHint::new("↑↓", "scroll").priority(6),
    KeyHint::new("o", "sort"),
])
.title("accounts")
.theme(theme);
// Drops `o sort` first when it runs out of room, then says `… ? more`.
frame.render_widget(&footer, footer_area);

// `?` help: filtered by what the person typed, scrolling when it is tall.
let help = KeyHelp::new(groups).filter(query.text()).theme(theme);
frame.render_stateful_widget(&help, body, &mut help_state); // lines 8–14 of 31
help_state.on_key(key); // ↑↓ PgUp PgDn Home End"#;

/// llmux's accounts keybar (`draw_footer`, `Overlay::Accounts`), with the
/// priorities a person would keep longest.
fn llmux_accounts() -> KeyHints<'static> {
    KeyHints::new([
        KeyHint::new("s", "switch").priority(7),
        KeyHint::new("f", "refresh").priority(4),
        KeyHint::new("R", "reset").priority(3),
        KeyHint::new("a", "add").priority(5),
        KeyHint::new("n", "login").priority(3),
        KeyHint::new("r", "remove").priority(5),
        KeyHint::new("u", "used/left").priority(2),
        KeyHint::new("t", "eta/utc").priority(1),
        KeyHint::new("o", "sort").priority(1),
        KeyHint::new("esc", "back").priority(8),
        KeyHint::new("q", "quit").priority(9),
    ])
    .title("accounts")
}

fn label(buf: &mut Buffer, area: Rect, y: u16, s: &str, cx: &DemoCx) {
    if y < area.bottom() {
        let l = text::truncate_line(
            Line::from(Span::styled(s.to_string(), cx.theme.dim())),
            area.width as usize,
        );
        buf.set_line(area.x, y, &l, area.width);
    }
}

fn footer_at(buf: &mut Buffer, area: Rect, y: u16, cols: u16, h: &KeyHints<'_>) {
    if y < area.bottom() {
        let r = Rect::new(area.x, y, cols.min(area.width), 1);
        h.render(r, buf);
    }
}

fn llmux_footer(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let h = llmux_accounts().theme(cx.theme);
    for (i, cols) in [120u16, 80, 40].into_iter().enumerate() {
        let y = area.y.saturating_add(2 * i as u16);
        label(buf, area, y, &format!("{cols} columns"), cx);
        footer_at(buf, area, y.saturating_add(1), cols, &h);
    }
}

fn composer(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let h = KeyHints::new([
        KeyHint::new("enter", "send").priority(9),
        KeyHint::new("shift+enter", "newline").priority(6),
        KeyHint::new("esc", "interrupt").priority(8),
        KeyHint::new("@", "files").priority(4),
        KeyHint::new("/", "commands").priority(5),
        KeyHint::new("ctrl+t", "transcript").priority(3),
        KeyHint::new("↑↓", "history").priority(2),
        KeyHint::new("ctrl+c", "quit").priority(7),
    ])
    .more("?")
    .theme(cx.theme);
    let full = area.width;
    label(buf, area, area.y, "composer, full width", cx);
    footer_at(buf, area, area.y.saturating_add(1), full, &h);
    label(
        buf,
        area,
        area.y.saturating_add(2),
        "composer, 44 columns",
        cx,
    );
    footer_at(buf, area, area.y.saturating_add(3), 44, &h);
}

/// llmux's `?` help, by group.
fn llmux_groups() -> Vec<HintGroup<'static>> {
    vec![
        HintGroup::new(
            "Navigate",
            [
                KeyHint::new("↑↓", "scroll"),
                KeyHint::new("pgup/pgdn", "page"),
                KeyHint::new("home/end", "top / bottom"),
                KeyHint::new("tab", "next panel"),
                KeyHint::new("esc", "back"),
            ],
        ),
        HintGroup::new(
            "Accounts",
            [
                KeyHint::new("s", "switch"),
                KeyHint::new("a", "add"),
                KeyHint::new("r", "remove"),
                KeyHint::new("f", "refresh usage"),
                KeyHint::new("R", "redeem a reset"),
                KeyHint::new("n", "new login"),
            ],
        ),
        HintGroup::new(
            "View",
            [
                KeyHint::new("u", "used / left"),
                KeyHint::new("t", "eta / utc"),
                KeyHint::new("o", "sort"),
                KeyHint::new("S", "schedule"),
                KeyHint::new("z", "fold exhausted"),
                KeyHint::new("?", "this help"),
                KeyHint::new("q", "quit"),
            ],
        ),
    ]
}

fn help(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    Widget::render(KeyHelp::new(llmux_groups()).theme(cx.theme), area, buf);
}

/// The help grid under a one-line filter field: type to filter, ↑↓ PgUp
/// PgDn to scroll.
struct FilterHelp {
    query: ComposerState,
    scroll: KeyHelpState,
}

impl FilterHelp {
    fn new() -> FilterHelp {
        FilterHelp {
            query: ComposerState::new().single_line(),
            scroll: KeyHelpState::new(),
        }
    }
}

impl Demo for FilterHelp {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if area.is_empty() {
            return;
        }
        let t = &cx.theme;
        let field = Composer::new()
            .prompt("/")
            .placeholder("filter by key or description")
            .theme(*t);
        let w = area.width.min(40);
        StatefulWidget::render(
            &field,
            Rect::new(area.x, area.y, w, 1),
            buf,
            &mut self.query,
        );
        let help = KeyHelp::new(llmux_groups())
            .filter(self.query.text())
            .columns(2)
            .theme(*t);
        if area.height > 1 {
            let count = format!("{} shortcuts", help.matches());
            let cw = text::width(&count) as u16;
            if area.width >= w + cw + 2 {
                buf.set_string(area.right() - cw, area.y, &count, t.dim());
            }
            let body = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
            StatefulWidget::render(&help, body, buf, &mut self.scroll);
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.query.key(key, 40) {
            ComposerEvent::Ignored => self.scroll.on_key(key),
            ComposerEvent::Edited => {
                self.scroll.scroll = 0;
                true
            }
            _ => true,
        }
    }
}

fn filtered(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    let mut y = area.y;
    for q in ["re", "zz"] {
        if y >= area.bottom() {
            break;
        }
        label(buf, area, y, &format!("filter(\"{q}\")"), cx);
        let help = KeyHelp::new(llmux_groups()).filter(q).theme(*t);
        let h = help
            .height(area.width)
            .min(area.bottom().saturating_sub(y + 1));
        Widget::render(&help, Rect::new(area.x, y + 1, area.width, h), buf);
        y += h + 2;
    }
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "key-hints",
    name: "Key hints",
    ty: "twolab_tui::key_hints::KeyHints",
    summary: "Which keys do something here, and what each one does.",
    description: "A footer of key hints that fits its width: when it runs out of room it drops whole hints, lowest priority first, and ends with … ? more so the person knows where the rest are — never half a hint clipped mid-word. KeyHelp lays every shortcut out by group in three, two or one aligned columns and counts what does not fit; filtered by a query (herdr's keybind help) it keeps the shortcuts whose key or description match, says no keys match \"q\" when none do, and scrolls with a lines a–b of n position when it is taller than its area. Keys are plain text (ctrl+c, esc, ↑↓) with an ASCII fallback for the arrows, which are East-Asian Ambiguous width; key matching smooths over terminals that send raw control bytes.",
    used_in: "llmux dashboard keybars (accounts, stats, logs) and its ? help; agent composer footers.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/footer_hint.rs#L1-L78",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "KeyHint::new(keys, label)",
            ty: "impl Into<Cow<str>> ×2",
            default: None,
            description: "A key as shown (ctrl+c, esc, ↑↓) and what it does.",
        },
        PropDoc {
            name: "KeyHint::priority(p)",
            ty: "u8",
            default: Some("0"),
            description: "Higher stays longer; equals drop from the right.",
        },
        PropDoc {
            name: "KeyHint::for_key(key, label, glyphs)",
            ty: "KeyInput",
            default: None,
            description: "Label a bound key as plain text (key_label).",
        },
        PropDoc {
            name: "KeyHints::new(hints)",
            ty: "impl IntoIterator<Item = KeyHint>",
            default: None,
            description: "The footer, in display order.",
        },
        PropDoc {
            name: ".title(t)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "Where the person is, before the hints (accounts).",
        },
        PropDoc {
            name: ".more(key) / .no_more()",
            ty: "impl Into<Cow<str>>",
            default: Some("\"?\""),
            description: "The help key named in the overflow marker (… ? more); without one it counts (… +3 more).",
        },
        PropDoc {
            name: ".separator(s)",
            ty: "impl Into<Cow<str>>",
            default: Some("two spaces"),
            description: "Between hints.",
        },
        PropDoc {
            name: ".glyphs(g)",
            ty: "KeyGlyphs",
            default: Some("Unicode"),
            description: "Ascii spells arrows out (up/down) for CJK terminals.",
        },
        PropDoc {
            name: ".theme(t)",
            ty: "Theme",
            default: Some("Theme::ansi()"),
            description: "Keys in accent bold, labels secondary.",
        },
        PropDoc {
            name: ".line(width) / .fit(width) / .rows(width)",
            ty: "Line / Fit / Vec<Line>",
            default: None,
            description: "The fitted footer, which hints it keeps, or every hint packed whole into rows.",
        },
        PropDoc {
            name: "KeyHelp::new(groups)",
            ty: "impl IntoIterator<Item = HintGroup>",
            default: None,
            description: "The help grid; .columns(n) (3), .gap(n) (4), .lines(width), .height(width).",
        },
        PropDoc {
            name: "KeyHelp::filter(query) / .matches()",
            ty: "impl Into<Cow<str>> / usize",
            default: Some("\"\""),
            description: "Only shortcuts whose keys (as written or drawn) or description contain the query, ignoring case; empty groups go; none: no keys match \"query\".",
        },
        PropDoc {
            name: "render_stateful_widget(&help, area, &mut KeyHelpState)",
            ty: "KeyHelpState",
            default: None,
            description: "Scrolls a grid taller than its area; the last row says lines a–b of n. on_key(↑↓ PgUp PgDn Home End), shown(), max_scroll().",
        },
        PropDoc {
            name: "is_press(binding, event) / normalize(key)",
            ty: "KeyInput",
            default: None,
            description: "Match keys across terminals: raw C0 bytes as Ctrl chords, uppercase as Shift, Shift+Tab as BackTab.",
        },
    ],
    keyboard: &[
        (
            "? (host-bound)",
            "The key the overflow marker names; the host opens KeyHelp with it.",
        ),
        (
            "↑ / ↓, PgUp / PgDn, Home / End (KeyHelpState)",
            "Scroll the help grid by a row, a page, or to an end",
        ),
    ],
    a11y: &[
        "Whole hints only: a key is never shown without its action, or cut mid-word.",
        "Dropped hints are announced (… ? more, or … +N more), and KeyHelp counts shortcuts below the fold or, scrolling, says lines a–b of n.",
        "A filter that matches nothing says so and repeats the query, rather than showing an empty box.",
        "Keys differ from labels by weight as well as colour, so mono (NO_COLOR) keeps them apart.",
        "Arrows are East-Asian Ambiguous; KeyGlyphs::Ascii keeps a footer's width exact on CJK terminals.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "llmux-footer",
            title: "llmux footer at 120, 80, 40",
            description: "The accounts keybar: everything at 120 columns, the least-used hints gone at 80 and 40, and the marker says so.",
            cols: 120,
            rows: 6,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(llmux_footer)),
        },
        DemoDoc {
            id: "composer",
            title: "Agent composer",
            description: "Send, newline and interrupt stay; history and transcript go first.",
            cols: 96,
            rows: 4,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(composer)),
        },
        DemoDoc {
            id: "help",
            title: "Help grid",
            description: "Every shortcut by group, in as many aligned columns as fit.",
            cols: 96,
            rows: 8,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(help)),
        },
        DemoDoc {
            id: "filter",
            title: "Filter and scroll the help",
            description: "Type to filter by key or description; ↑↓ PgUp PgDn scroll the grid, whose last row says where it is.",
            cols: 72,
            rows: 10,
            animated: false,
            interactive: true,
            make: || Box::new(FilterHelp::new()),
        },
        DemoDoc {
            id: "filtered",
            title: "Filtered: a match and no match",
            description: "filter(\"re\") keeps remove, refresh, redeem and their groups; filter(\"zz\") says no keys match.",
            cols: 96,
            rows: 10,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(filtered)),
        },
    ],
};
