use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use super::{HintGroup, KeyHelp, KeyHint, KeyHints};
use crate::catalog::{ComponentDoc, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
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
frame.render_widget(&footer, footer_area);"#;

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

fn help(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    KeyHelp::new([
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
    ])
    .theme(cx.theme)
    .render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "key-hints",
    name: "Key hints",
    ty: "twolab_tui::key_hints::KeyHints",
    summary: "Which keys do something here, and what each one does.",
    description: "A footer of key hints that fits its width: when it runs out of room it drops whole hints, lowest priority first, and ends with … ? more so the person knows where the rest are — never half a hint clipped mid-word. KeyHelp lays every shortcut out by group in three, two or one aligned columns and counts what does not fit. Keys are plain text (ctrl+c, esc, ↑↓) with an ASCII fallback for the arrows, which are East-Asian Ambiguous width; key matching smooths over terminals that send raw control bytes.",
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
            name: "is_press(binding, event) / normalize(key)",
            ty: "KeyInput",
            default: None,
            description: "Match keys across terminals: raw C0 bytes as Ctrl chords, uppercase as Shift, Shift+Tab as BackTab.",
        },
    ],
    keyboard: &[(
        "? (host-bound)",
        "The key the overflow marker names; the host opens KeyHelp with it.",
    )],
    a11y: &[
        "Whole hints only: a key is never shown without its action, or cut mid-word.",
        "Dropped hints are announced (… ? more, or … +N more), and KeyHelp counts shortcuts below the fold.",
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
    ],
};
