use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use super::{ApprovalCard, ApprovalEvent, ApprovalState, Decision, Risk};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::approval_card::{
    ApprovalCard, ApprovalEvent, ApprovalState, Decision, Risk,
};

let card = ApprovalCard::command("rm -rf build/")
    .reason("Deletes the build/ directory and everything in it.")
    .risk(Risk::High)
    .recommended(Decision::Deny)
    .theme(theme);

// Start on the recommended answer, so Enter is the safe key.
let mut state = ApprovalState::at(Decision::Deny);
frame.render_stateful_widget(&card, area, &mut state);

match state.key(key) {
    ApprovalEvent::Decided(d) if d.allows() => run_it(d == Decision::Session),
    ApprovalEvent::Decided(_) | ApprovalEvent::Cancelled => deny(),
    ApprovalEvent::Moved | ApprovalEvent::Ignored => {}
}"#;

/// An interactive request: the card until it is answered, then the receipt.
struct Ask {
    card: fn(Theme) -> ApprovalCard<'static>,
    state: ApprovalState,
    answer: Option<Option<Decision>>,
}

impl Ask {
    fn new(card: fn(Theme) -> ApprovalCard<'static>) -> Ask {
        let start = card(Theme::default())
            .recommended_choice()
            .unwrap_or(Decision::Once);
        Ask {
            card,
            state: ApprovalState::at(start),
            answer: None,
        }
    }
}

impl Demo for Ask {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let card = (self.card)(cx.theme);
        match self.answer {
            None => StatefulWidget::render(&card, area, buf, &mut self.state),
            Some(answer) => {
                let lines = vec![
                    card.receipt(answer),
                    Line::from(Span::styled("press any key to ask again", cx.theme.dim())),
                ];
                for (dy, line) in lines.into_iter().enumerate().take(area.height as usize) {
                    let line = crate::text::truncate_line(line, area.width as usize);
                    Widget::render(
                        line,
                        Rect::new(area.x, area.y + dy as u16, area.width, 1),
                        buf,
                    );
                }
            }
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        if self.answer.is_some() {
            *self = Ask::new(self.card);
            return true;
        }
        match self.state.key(key) {
            ApprovalEvent::Ignored => false,
            ApprovalEvent::Moved => true,
            ApprovalEvent::Decided(d) => {
                self.answer = Some(Some(d));
                true
            }
            ApprovalEvent::Cancelled => {
                self.answer = Some(None);
                true
            }
        }
    }
}

fn shell_command(theme: Theme) -> ApprovalCard<'static> {
    ApprovalCard::command("cargo test --workspace -- --nocapture")
        .reason("Builds and runs the test suite; writes only to target/.")
        .risk(Risk::Low)
        .recommended(Decision::Session)
        .theme(theme)
}

fn file_write(theme: Theme) -> ApprovalCard<'static> {
    ApprovalCard::write("crates/proxy/src/retry/backoff.rs")
        .risk(Risk::Medium)
        .recommended(Decision::Once)
        .theme(theme)
}

fn risky(theme: Theme) -> ApprovalCard<'static> {
    ApprovalCard::command("rm -rf build/")
        .reason("Deletes the build/ directory and everything in it.")
        .risk(Risk::High)
        .recommended(Decision::Deny)
        .theme(theme)
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "approval-card",
    name: "Approval card",
    ty: "twolab_tui::approval_card::ApprovalCard",
    summary: "What the agent wants to do, how risky it is, and your options — allow once, for this session, or deny.",
    description: "A permission request as one card: a header with the kind of request and its risk, the question, a reason, the command or path shown verbatim (controls spelled, clipped by cells with an ellipsis), three numbered choices with the recommended one marked, and a key hint that shrinks widest-first but always keeps Enter and Esc. On a short screen it keeps the marked choice and the action longest.",
    used_in: "Agent shells asking before a command runs or a file is written; destructive dashboard actions (remove an account, spend a reset) that need a yes.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/footer/approval_ui.zig#L1500-L1540",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Agent,
    props: &[
        PropDoc {
            name: "ApprovalCard::command(cmd) / ::write(path) / ::edit(path) / ::tool(name, args)",
            ty: "impl Into<Cow<str>>",
            default: None,
            description: "What the agent wants to do; sets the question, the reason and the choice labels.",
        },
        PropDoc {
            name: ".risk(r)",
            ty: "Risk",
            default: Some("none"),
            description: "Low, Medium or High, said in words in the header (bold and reversed in mono when high).",
        },
        PropDoc {
            name: ".recommended(d)",
            ty: "Decision",
            default: Some("none"),
            description: "The choice marked (recommended). Start the state there with ApprovalState::at(d).",
        },
        PropDoc {
            name: ".reason(text) / .question(text)",
            ty: "impl Into<Cow<str>>",
            default: Some("per kind"),
            description: "Why it needs approval; replace the kind's question.",
        },
        PropDoc {
            name: ".labels([a, b, c])",
            ty: "[impl Into<Cow<str>>; 3]",
            default: Some("per kind"),
            description: "Replace the three choice labels; the numbers are added.",
        },
        PropDoc {
            name: ".hints(on)",
            ty: "bool",
            default: Some("true"),
            description: "The key-hint row.",
        },
        PropDoc {
            name: "ApprovalState::key(k)",
            ty: "fn(KeyInput) -> ApprovalEvent",
            default: None,
            description: "Ignored, Moved, Decided(Decision) or Cancelled.",
        },
        PropDoc {
            name: ".lines(&state, width) / .receipt(answer)",
            ty: "Vec<Line> / Line",
            default: None,
            description: "The compact card as text; a one-line record of the answer.",
        },
    ],
    keyboard: &[
        ("1 / 2 / 3", "Answer at once"),
        ("↑ / ↓, Tab / Shift+Tab", "Move the mark (wraps)"),
        ("Enter", "Answer with the marked choice"),
        ("Esc, Ctrl+C", "Cancel"),
    ],
    a11y: &[
        "The marked choice has a ❯ and is bold; risk and the recommendation are words, so nothing depends on colour.",
        "The command or path is verbatim: control and bidirectional characters are spelled, never obeyed; a clipped row ends in ….",
        "The hint row is ASCII (1-3, up/down) so it is the same width on a CJK terminal.",
        "Start the state on the recommended choice so a reflexive Enter takes the safe answer for a risky request.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "command",
            title: "Run a command",
            description: "A low-risk test run; 1–3, ↑↓, Enter, Esc.",
            cols: 72,
            rows: 11,
            animated: false,
            interactive: true,
            make: || Box::new(Ask::new(shell_command)),
        },
        DemoDoc {
            id: "file-write",
            title: "Write a file",
            description: "A file write in the compact layout; the path keeps its name when narrow.",
            cols: 64,
            rows: 8,
            animated: false,
            interactive: true,
            make: || Box::new(Ask::new(file_write)),
        },
        DemoDoc {
            id: "risky",
            title: "A risky command",
            description: "High risk, deny recommended and marked first — Enter is the safe key.",
            cols: 72,
            rows: 11,
            animated: false,
            interactive: true,
            make: || Box::new(Ask::new(risky)),
        },
    ],
};
