use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::{Button, Modal, ModalOutcome, ModalState, PopupSize, Tone};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Status, Theme};
use crate::widgets::key_hints::{KeyHint, KeyHints};

const USAGE: &str = r#"use twolab_tui::key_hints::{KeyHint, KeyHints};
use twolab_tui::modal::{Button, Modal, ModalOutcome, ModalState, PopupSize, Tone};

let modal = Modal::new("Delete account ai3@iq.io?")
    .text("Its API key and 30 days of usage history are removed.")
    .actions([
        Button::new("cancel").key("n"),
        Button::new("delete").key("y").destructive(true),
    ])
    .tone(Tone::Danger)                // red border and the word "danger" on it
    .footer(KeyHints::new([KeyHint::new("tab", "switch"), KeyHint::new("esc", "cancel")]))
    .theme(theme);                     // sized to its content, at most 80% x 80%

// Over the screen: dims what is under it, returns the rects to route clicks by.
let layout = modal.render(frame.area(), frame.buffer_mut(), &mut app.confirm);

match app.confirm.key(key) {           // or .press(col, row) / .drag(col, row) / .scroll_by(n)
    ModalOutcome::Pressed(1) => delete_account(),
    ModalOutcome::Pressed(_) | ModalOutcome::Closed => app.dialog = None,
    _ => {}
}

// Release notes: fixed size, a body that scrolls with a scrollbar.
let notes = Modal::new("v0.9.5")
    .subtitle("what's new in this release")
    .body(lines)
    .size(PopupSize::Cells(80), PopupSize::Percent(80));"#;

const ACCOUNTS: [(&str, &str, Status, &str, &str); 7] = [
    ("zhuge@2lab.ai", "active", Status::Running, "41%", "Max 20x"),
    ("ai3@iq.io", "ready", Status::Done, "63%", "Max 5x"),
    ("simayi@2lab.ai", "ready", Status::Done, "12%", "Max 20x"),
    ("dev1@2lab.ai", "cooldown", Status::Waiting, "88%", "Pro"),
    ("ai2@iq.io", "7d spent", Status::Failed, "99%", "Max 5x"),
    ("luxun@2lab.ai", "ready", Status::Done, "27%", "Max 20x"),
    ("zhaoyun@2lab.ai", "ready", Status::Done, "54%", "Max 5x"),
];

fn put(buf: &mut Buffer, x: u16, y: u16, line: Line<'static>, w: u16) {
    if w == 0 || !buf.area.contains((x, y).into()) {
        return;
    }
    buf.set_line(x, y, &text::truncate_line(line, w as usize), w);
}

/// The llmux dashboard under the modal: what the scrim dims.
fn backdrop(area: Rect, buf: &mut Buffer, t: &Theme) {
    put(
        buf,
        area.x,
        area.y,
        Line::from(vec![
            Span::styled("llmux", t.accent().add_modifier(Modifier::BOLD)),
            Span::styled("  accounts · 7 · rotation on", t.dim()),
        ]),
        area.width,
    );
    if area.height > 1 {
        put(
            buf,
            area.x,
            area.y + 1,
            Line::styled("  account          status     7d    plan", t.dim()),
            area.width,
        );
    }
    let rows = area.height.saturating_sub(2) as usize;
    for (i, (name, status, st, pct, plan)) in ACCOUNTS.iter().cycle().take(rows).enumerate() {
        put(
            buf,
            area.x,
            area.y + 2 + i as u16,
            Line::from(vec![
                Span::raw("  "),
                Span::styled(text::pad(name, 17, text::Align::Left), t.text()),
                Span::styled(text::pad(status, 11, text::Align::Left), t.status(*st)),
                Span::styled(format!("{pct:<6}"), t.secondary()),
                Span::styled(*plan, t.dim()),
            ]),
            area.width,
        );
    }
}

fn status_line(area: Rect, buf: &mut Buffer, line: Line<'static>) {
    if area.height > 0 {
        put(buf, area.x, area.bottom() - 1, line, area.width);
    }
}

/// Release notes as lines: upper-case headings, `- ` bullets, code in the
/// accent colour.
fn notes_lines(t: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let heading = |s: &str| Line::styled(s.to_uppercase(), t.accent().add_modifier(Modifier::BOLD));
    let bullet = |parts: &[(&str, bool)]| {
        let mut spans = vec![Span::styled("- ", t.dim())];
        for (s, code) in parts {
            spans.push(if *code {
                Span::styled(s.to_string(), t.accent())
            } else {
                Span::styled(s.to_string(), t.text())
            });
        }
        Line::from(spans)
    };
    out.push(heading("Added"));
    out.push(bullet(&[
        (
            "Accounts rotate on the seven-day window as well as the five-hour one; set ",
            false,
        ),
        ("rotation.weekly_headroom", true),
        (" to keep some of the week in reserve.", false),
    ]));
    out.push(bullet(&[(
        "A scrollbar on the keys, sessions and usage tables; drag it or click the track.",
        false,
    )]));
    out.push(bullet(&[(
        "Popups size to what they show: a two-line notice no longer takes 80% of the screen.",
        false,
    )]));
    out.push(bullet(&[
        ("", false),
        ("llmux accounts export", true),
        (
            " writes the account list without tokens, for a second machine.",
            false,
        ),
    ]));
    out.push(Line::default());
    out.push(heading("Changed"));
    out.push(bullet(&[(
        "Status messages stay until read when a refresh fails for more than one account.",
        false,
    )]));
    out.push(bullet(&[(
        "The raw viewer opens at the request body, not the headers.",
        false,
    )]));
    out.push(bullet(&[
        ("Cooldowns count down in the accounts table (", false),
        ("cooldown 12m", true),
        (") instead of showing the end time.", false),
    ]));
    out.push(Line::default());
    out.push(heading("Fixed"));
    out.push(bullet(&[(
        "A 429 from one account no longer marks every account on the same plan as cooling down.",
        false,
    )]));
    out.push(bullet(&[(
        "Korean and Japanese account labels line up in the table at any width.",
        false,
    )]));
    out.push(bullet(&[
        ("The logs overlay in attach mode says when the daemon has no logs instead of showing an empty pane.", false),
    ]));
    out.push(bullet(&[(
        "End followed by a resize no longer leaves the pager past the last line.",
        false,
    )]));
    out.push(Line::default());
    out.push(heading("Upgrade"));
    out.push(Line::from(vec![
        Span::styled("Detach, run ", t.text()),
        Span::styled("cargo install llmux --locked", t.accent()),
        Span::styled(
            ", then run llmux again to reconnect. Accounts and settings are kept.",
            t.text(),
        ),
    ]));
    out
}

/// Release notes over the dashboard: scroll, Esc or Enter close.
struct NotesDemo {
    st: ModalState,
    open: bool,
}

impl Demo for NotesDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        backdrop(area, buf, t);
        if !self.open {
            status_line(
                area,
                buf,
                Line::styled(" release notes closed · Enter reopens them", t.secondary()),
            );
            return;
        }
        Modal::new("v0.9.5")
            .subtitle("what's new in this release")
            .body(notes_lines(t))
            .footer(
                KeyHints::new([
                    KeyHint::new("↑↓", "scroll"),
                    KeyHint::new("pgup/pgdn", "page"),
                    KeyHint::new("esc", "close").priority(1),
                ])
                .no_more(),
            )
            .size(PopupSize::Cells(68), PopupSize::Percent(90))
            .theme(*t)
            .render(area, buf, &mut self.st);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        if !self.open {
            if key.key == Key::Enter {
                self.open = true;
                self.st = ModalState::new();
                return true;
            }
            return false;
        }
        match self.st.key(key) {
            ModalOutcome::Scrolled | ModalOutcome::Focused(_) => true,
            ModalOutcome::Closed | ModalOutcome::Pressed(_) => {
                self.open = false;
                true
            }
            ModalOutcome::Ignored | ModalOutcome::Outside => false,
        }
    }
}

fn notes() -> Box<dyn Demo> {
    Box::new(NotesDemo {
        st: ModalState::new(),
        open: true,
    })
}

/// Delete an account: focus starts on cancel; Tab, Enter, y/n, Esc.
struct ConfirmDemo {
    st: ModalState,
    open: bool,
    said: Option<&'static str>,
}

impl ConfirmDemo {
    fn modal(t: Theme) -> Modal<'static> {
        Modal::new("Delete account ai3@iq.io?")
            .body([
                "Its API key and 30 days of usage history are removed from this machine.",
                "",
                "Requests routed to it fail over to the next account in rotation.",
            ])
            .actions([
                Button::new("cancel").key("n"),
                Button::new("delete").key("y").destructive(true),
            ])
            .footer(
                KeyHints::new([
                    KeyHint::new("tab", "switch"),
                    KeyHint::new("enter", "press"),
                    KeyHint::new("esc", "cancel").priority(1),
                ])
                .no_more(),
            )
            .tone(Tone::Danger)
            .tone_word("cannot be undone")
            .close_button(false)
            .fit_content(PopupSize::Cells(56), PopupSize::Percent(90))
            .theme(t)
    }
}

impl Demo for ConfirmDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let t = &cx.theme;
        backdrop(area, buf, t);
        if !self.open {
            let said = self.said.unwrap_or("closed");
            status_line(
                area,
                buf,
                Line::from(vec![
                    Span::styled(format!(" {said}"), t.secondary()),
                    Span::styled(" · Enter asks again", t.dim()),
                ]),
            );
            return;
        }
        ConfirmDemo::modal(*t).render(area, buf, &mut self.st);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        if !self.open {
            if key.key == Key::Enter {
                self.open = true;
                self.st = ModalState::with_focus(0);
                return true;
            }
            return false;
        }
        match self.st.key(key) {
            ModalOutcome::Focused(_) | ModalOutcome::Scrolled => true,
            ModalOutcome::Pressed(1) => {
                self.open = false;
                self.said = Some("deleted ai3@iq.io (a demo: nothing was removed)");
                true
            }
            ModalOutcome::Pressed(_) | ModalOutcome::Closed => {
                self.open = false;
                self.said = Some("kept ai3@iq.io");
                true
            }
            ModalOutcome::Ignored | ModalOutcome::Outside => false,
        }
    }
}

fn confirm() -> Box<dyn Demo> {
    Box::new(ConfirmDemo {
        st: ModalState::with_focus(0),
        open: true,
        said: None,
    })
}

fn notice_demo(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let t = &cx.theme;
    backdrop(area, buf, t);
    let modal = Modal::new("Copied to clipboard")
        .body(vec![
            Line::from(vec![
                Span::styled("sk-ant-api03-…4f2a", t.accent()),
                Span::styled(" for ai3@iq.io", t.text()),
            ]),
            Line::styled(
                "It stays there until you copy something else.",
                t.secondary(),
            ),
        ])
        .footer(KeyHints::new([KeyHint::new("enter", "close")]).no_more())
        .theme(*t);
    let layout = modal.render(area, buf, &mut ModalState::new());
    let o = layout.outer;
    if o.bottom() < area.bottom() {
        let fixed = (
            PopupSize::Percent(80).resolve(area.width),
            PopupSize::Percent(80).resolve(area.height),
        );
        status_line(
            area,
            buf,
            Line::from(vec![
                Span::styled(
                    format!(" fits its content: {}×{}", o.width, o.height),
                    t.secondary(),
                ),
                Span::styled(
                    format!(" · a fixed 80%×80% popup here: {}×{}", fixed.0, fixed.1),
                    t.dim(),
                ),
            ]),
        );
    }
}

fn notice() -> Box<dyn Demo> {
    Box::new(FnDemo(notice_demo))
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "modal",
    name: "Modal",
    ty: "twolab_tui::modal::Modal",
    summary: "Something needs an answer before you go on: what it is, and what each answer does.",
    description: "herdr's dialog frame: centred over a dimmed screen, sized in cells or percent or to its content up to a cap, clamped to the area. A title and subtitle, a body that wraps and scrolls with a scrollbar and says which rows are in view, a key-hint footer, and buttons that take focus. A destructive tone colours the border and writes a word on it. render returns the rects a host routes the mouse by; ModalState turns keys and clicks into outcomes.",
    used_in: "herdr's release notes, product announcements, rename box, onboarding and close confirmation. Planned for llmux's input excerpt, keys picker, account editor and raw viewer, which today are fixed 80% x 80% to 94% x 92% popups whatever they hold.",
    source: Source {
        project: "herdr",
        url: "https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/popup_size.rs#L7-L90",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "Modal::new(title)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "Bold on the first row; cut with … when the popup is narrow.",
        },
        PropDoc {
            name: ".subtitle(s) / .body(lines) / .text(str)",
            ty: "Cow<str> / IntoIterator<Item: Into<Line>> / &str",
            default: None,
            description: "A secondary line under the title; the body, which wraps and scrolls with a scrollbar when it overflows.",
        },
        PropDoc {
            name: ".footer(hints)",
            ty: "KeyHints",
            default: None,
            description: "A key-hint line that drops whole hints when narrow (… ? more).",
        },
        PropDoc {
            name: ".actions(buttons)",
            ty: "IntoIterator<Item = Button>",
            default: None,
            description: "Button::new(label).key(k).destructive(bool). Centred; focus shows as › label ‹ in the selected style; a one-character key presses it.",
        },
        PropDoc {
            name: ".size(w, h) / .fit_content(max_w, max_h) / .width(s) / .height(s)",
            ty: "PopupSize::{Cells, Percent} / ModalSize::{Fixed, Fit}",
            default: Some("fit_content(80%, 80%)"),
            description: "Fixed sides, or as much as the content needs up to a cap. Clamped to the area either way.",
        },
        PropDoc {
            name: ".min_size(w, h) / .margin(h, v)",
            ty: "u16, u16",
            default: Some("6 × 4 / 2 and 1"),
            description: "herdr's minimum popup; cells kept free around it so the screen under it shows.",
        },
        PropDoc {
            name: ".tone(Tone::Danger) / .tone_word(s)",
            ty: "Tone / Cow<str>",
            default: Some("Normal / \"danger\""),
            description: "A destructive confirm: border in the failure style (bold in mono) and the word on it.",
        },
        PropDoc {
            name: ".scrim(bool) / .close_button(bool)",
            ty: "bool",
            default: Some("true / true"),
            description: "Dim what is under the popup; show esc close in the title row.",
        },
        PropDoc {
            name: ".render(area, buf, &mut state) -> ModalLayout",
            ty: "ModalLayout { outer, body, track, pos, buttons, close }",
            default: None,
            description: "Draws and returns the rects for the mouse; .layout(area, &state) computes them without drawing. Also a StatefulWidget.",
        },
        PropDoc {
            name: "ModalState::key / press / drag / release / scroll_by",
            ty: "-> ModalOutcome",
            default: None,
            description: "Ignored, Scrolled, Focused(i), Pressed(i), Closed or Outside. ModalState::with_focus(i) starts on the safe answer.",
        },
        PropDoc {
            name: "modal::popup_rect(area, w, h, min)",
            ty: "Rect",
            default: None,
            description: "herdr's resolve_popup_geometry for any popup: resolved, raised to min, clamped, centred.",
        },
    ],
    keyboard: &[
        ("↑ ↓ PgUp PgDn Home End", "Scroll the body"),
        ("Tab / →, Shift-Tab / ←", "Move between buttons"),
        (
            "Enter",
            "Press the focused button (close when there are none)",
        ),
        ("A button's key (y, n)", "Press that button"),
        ("Esc", "Close"),
    ],
    a11y: &[
        "The destructive tone is a word on the border as well as a colour; mono makes the border bold.",
        "The focused button is reversed and bracketed by › ‹ instead of [ ], so focus shows without colour.",
        "A body cut short says so twice: a scrollbar, and 12-21 of 40 on the bottom border.",
        "The scrim is a dim weight, not a colour, so NO_COLOR keeps the popup standing out.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "release-notes",
            title: "Release notes that scroll",
            description: "A fixed 68-column popup over the dimmed dashboard: the body wraps and scrolls with ↑↓, PgUp/PgDn and Home/End, the bottom border says which rows are in view, Esc closes.",
            cols: 84,
            rows: 24,
            animated: false,
            interactive: true,
            make: notes,
        },
        DemoDoc {
            id: "confirm",
            title: "A destructive confirm",
            description: "Delete an account: a red border with \"cannot be undone\" on it, focus on cancel first; Tab or ←/→ moves, Enter presses, y or n press directly, Esc cancels.",
            cols: 84,
            rows: 18,
            animated: false,
            interactive: true,
            make: confirm,
        },
        DemoDoc {
            id: "notice",
            title: "A notice that fits its content",
            description: "Two lines and a hint: the popup is as big as what it says, where a fixed 80% x 80% popup (llmux today) would cover the screen.",
            cols: 84,
            rows: 18,
            animated: false,
            interactive: false,
            make: notice,
        },
    ],
};
