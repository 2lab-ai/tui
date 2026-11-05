// Derived from vercel-labs/fx `src/ui/footer/approval_ui.zig` and
// `src/ui/footer/interaction_state.zig` at commit
// 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025 Vercel,
// Inc.): the permission band's row grammar (`buildApprovalPanelLineWithLabel`),
// its compact and spacious heights, the question and reason per kind
// (`approvalQuestion`, `approvalReasonLine`), the `❯` choice rows
// (`approvalChoiceLine`), the header that drops its right half whole
// (`composeApprovalHeaderRow`) and the widest-first hint ladder
// (`approvalHint`). Translated from Zig to Rust and re-expressed as ratatui
// text; checked against 2lab's xfx `src/tui/approval_view.rs` (Apache-2.0) at
// 93f516b. Modified by 2lab.

//! What the agent wants to do, how risky it is, and your three options.
//!
//! Ported from Vercel fx's permission band (`src/ui/footer/approval_ui.zig`,
//! Apache-2.0): a header naming the kind of request, the question, a reason,
//! the command or path itself, three numbered choices — allow once, allow for
//! the rest of the session, deny — and a hint row that degrades widest-first
//! and always keeps Enter and Esc. Added here: a risk level in the header and
//! a recommended choice, both said in words so they survive `NO_COLOR`.
//!
//! The command or path is shown verbatim: control characters are spelled
//! (`\x1b`, `\n`) rather than obeyed, and a row too long for the area is
//! clipped by cells and ends in `…` (a path keeps its file name, a command its
//! start). Keys go through [`ApprovalState::key`], which answers with an
//! [`ApprovalEvent`]; the widget never acts on a decision itself.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::{Level, Theme};
use crate::widgets::diff_view::{inert, truncate_left};

/// Rows of the compact card with a reason row (fx `approval_panel_rows_compact`).
pub const COMPACT_ROWS: u16 = 8;
/// Rows of the spacious card with a reason row (fx `approval_panel_rows_spacious`).
pub const SPACIOUS_ROWS: u16 = 11;

/// Cells every row is indented by (fx `inset_width`).
const INSET: usize = 2;
/// Cells the header keeps clear at its right edge (fx `right_margin`).
const RIGHT_MARGIN: usize = 2;

const TITLE: &str = "Permission needed \u{2219} Choose one";
const SHORT_TITLE: &str = "Permission needed";
const SEPARATOR: &str = " \u{2219} ";

/// Widest first; every rung keeps Enter and Esc so a narrow card never loses
/// how to answer (fx `approval_hint_variants`, with the arrows and the dash
/// spelled in ASCII so the row is the same width on a CJK terminal).
const HINTS: [&str; 3] = [
    "1-3 choose now    up/down options    enter confirm    esc cancel",
    "1-3 choose now    enter confirm    esc cancel",
    "enter confirm    esc cancel",
];

/// What the agent is asking to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ApprovalKind {
    /// Run a shell command.
    Command,
    /// Create or overwrite a file.
    Write,
    /// Edit part of a file.
    Edit,
    /// Call any other tool.
    Tool,
}

impl ApprovalKind {
    /// The header's name for it (fx `approvalKind`).
    pub const fn label(self) -> &'static str {
        match self {
            ApprovalKind::Command => "Command",
            ApprovalKind::Write => "Write file",
            ApprovalKind::Edit => "Edit file",
            ApprovalKind::Tool => "Tool",
        }
    }

    /// The question (fx `approvalQuestion`).
    pub const fn question(self) -> &'static str {
        match self {
            ApprovalKind::Command => "Would you like to run the following command?",
            ApprovalKind::Write => "Would you like to create or update this file?",
            ApprovalKind::Edit => "Would you like to edit this file?",
            ApprovalKind::Tool => "Would you like to allow this action?",
        }
    }

    /// The reason when the host gives none (fx `approvalReasonLine`; a
    /// command has none of its own).
    pub const fn default_reason(self) -> Option<&'static str> {
        match self {
            ApprovalKind::Command => None,
            ApprovalKind::Write | ApprovalKind::Edit => {
                Some("This action changes files in your workspace.")
            }
            ApprovalKind::Tool => Some("This action needs approval before the agent can continue."),
        }
    }

    /// The label of each choice (fx `approval_once_label`,
    /// `approvalAlwaysChoice`, `approval_deny_label`), the second naming
    /// what "for this session" covers.
    pub const fn labels(self) -> [&'static str; 3] {
        let session = match self {
            ApprovalKind::Command => "Yes, and allow this command for the session",
            ApprovalKind::Write => "Yes, and allow writes to this file for the session",
            ApprovalKind::Edit => "Yes, and allow edits to this file for the session",
            ApprovalKind::Tool => "Yes, and allow this tool for the session",
        };
        ["Yes", session, "No"]
    }
}

/// How much harm saying yes could do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Risk {
    Low,
    Medium,
    High,
}

impl Risk {
    pub const fn label(self) -> &'static str {
        match self {
            Risk::Low => "low risk",
            Risk::Medium => "medium risk",
            Risk::High => "high risk",
        }
    }

    /// The theme level it is drawn in (high risk is bold and reversed in mono).
    pub const fn level(self) -> Level {
        match self {
            Risk::Low => Level::Ok,
            Risk::Medium => Level::Warn,
            Risk::High => Level::Crit,
        }
    }
}

/// The three answers, in the order they are numbered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Decision {
    /// 1 — allow this one request.
    Once,
    /// 2 — allow it, and the same request for the rest of the session.
    Session,
    /// 3 — deny it.
    Deny,
}

impl Decision {
    pub const ALL: [Decision; 3] = [Decision::Once, Decision::Session, Decision::Deny];

    pub const fn index(self) -> usize {
        match self {
            Decision::Once => 0,
            Decision::Session => 1,
            Decision::Deny => 2,
        }
    }

    pub const fn from_index(i: usize) -> Option<Decision> {
        match i {
            0 => Some(Decision::Once),
            1 => Some(Decision::Session),
            2 => Some(Decision::Deny),
            _ => None,
        }
    }

    pub const fn allows(self) -> bool {
        !matches!(self, Decision::Deny)
    }
}

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalEvent {
    /// Not a key the card uses.
    Ignored,
    /// The marked choice moved.
    Moved,
    /// The person answered: a digit, or Enter on the marked choice.
    Decided(Decision),
    /// Esc or Ctrl+C: no answer (hosts usually treat it as a denial).
    Cancelled,
}

/// Which choice is marked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApprovalState {
    selected: usize,
}

impl ApprovalState {
    /// Marked on "allow once", as fx starts.
    pub const fn new() -> ApprovalState {
        ApprovalState { selected: 0 }
    }

    /// Marked on `d` — start on [`ApprovalCard::recommended`] so Enter takes
    /// the safe answer for a risky request.
    pub const fn at(d: Decision) -> ApprovalState {
        ApprovalState {
            selected: d.index(),
        }
    }

    pub fn selected(&self) -> Decision {
        Decision::from_index(self.selected).unwrap_or(Decision::Once)
    }

    pub fn select(&mut self, d: Decision) {
        self.selected = d.index();
    }

    /// 1–3 answer at once; ↑↓ (and Tab / Shift+Tab) move the mark, wrapping;
    /// Enter answers with the marked choice; Esc or Ctrl+C cancels.
    pub fn key(&mut self, key: KeyInput) -> ApprovalEvent {
        if key.mods.ctrl {
            return match key.key {
                Key::Char('c') => ApprovalEvent::Cancelled,
                _ => ApprovalEvent::Ignored,
            };
        }
        match key.key {
            Key::Char(c @ '1'..='3') if !key.mods.alt => {
                let d = Decision::from_index(c as usize - '1' as usize).unwrap_or(Decision::Deny);
                self.selected = d.index();
                ApprovalEvent::Decided(d)
            }
            Key::Up | Key::BackTab => {
                self.selected = (self.selected + 2) % 3;
                ApprovalEvent::Moved
            }
            Key::Down | Key::Tab => {
                self.selected = (self.selected + 1) % 3;
                ApprovalEvent::Moved
            }
            Key::Enter => ApprovalEvent::Decided(self.selected()),
            Key::Esc => ApprovalEvent::Cancelled,
            _ => ApprovalEvent::Ignored,
        }
    }
}

/// A permission request as a card: header, question, reason, the action
/// itself, three numbered choices and a key hint.
#[derive(Debug, Clone)]
pub struct ApprovalCard<'a> {
    kind: ApprovalKind,
    action: Cow<'a, str>,
    tool: Option<Cow<'a, str>>,
    question: Option<Cow<'a, str>>,
    reason: Option<Cow<'a, str>>,
    risk: Option<Risk>,
    recommended: Option<Decision>,
    labels: Option<[Cow<'a, str>; 3]>,
    hints: bool,
    theme: Theme,
}

/// One row and how much it matters when the area is short.
struct CardRow {
    line: Line<'static>,
    keep: u8,
}

impl<'a> ApprovalCard<'a> {
    pub fn new(kind: ApprovalKind, action: impl Into<Cow<'a, str>>) -> ApprovalCard<'a> {
        ApprovalCard {
            kind,
            action: action.into(),
            tool: None,
            question: None,
            reason: None,
            risk: None,
            recommended: None,
            labels: None,
            hints: true,
            theme: Theme::default(),
        }
    }

    /// Run `command` in a shell.
    pub fn command(command: impl Into<Cow<'a, str>>) -> ApprovalCard<'a> {
        ApprovalCard::new(ApprovalKind::Command, command)
    }

    /// Create or overwrite the file at `path`.
    pub fn write(path: impl Into<Cow<'a, str>>) -> ApprovalCard<'a> {
        ApprovalCard::new(ApprovalKind::Write, path)
    }

    /// Edit the file at `path`.
    pub fn edit(path: impl Into<Cow<'a, str>>) -> ApprovalCard<'a> {
        ApprovalCard::new(ApprovalKind::Edit, path)
    }

    /// Call `tool` with `args` (shown as `tool args`).
    pub fn tool(tool: impl Into<Cow<'a, str>>, args: impl Into<Cow<'a, str>>) -> ApprovalCard<'a> {
        let mut c = ApprovalCard::new(ApprovalKind::Tool, args);
        c.tool = Some(tool.into());
        c
    }

    /// Replace the kind's question.
    pub fn question(mut self, q: impl Into<Cow<'a, str>>) -> Self {
        self.question = Some(q.into());
        self
    }

    /// Why it needs approval, after a dim `Reason:`.
    pub fn reason(mut self, r: impl Into<Cow<'a, str>>) -> Self {
        self.reason = Some(r.into());
        self
    }

    pub fn risk(mut self, risk: Risk) -> Self {
        self.risk = Some(risk);
        self
    }

    /// The choice marked `(recommended)`.
    pub fn recommended(mut self, d: Decision) -> Self {
        self.recommended = Some(d);
        self
    }

    /// Replace the three choice labels (numbers are added).
    pub fn labels(mut self, labels: [impl Into<Cow<'a, str>>; 3]) -> Self {
        self.labels = Some(labels.map(Into::into));
        self
    }

    /// Whether the key-hint row is drawn (default on).
    pub fn hints(mut self, on: bool) -> Self {
        self.hints = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn kind(&self) -> ApprovalKind {
        self.kind
    }

    pub fn recommended_choice(&self) -> Option<Decision> {
        self.recommended
    }

    fn reason_text(&self) -> Option<&str> {
        self.reason.as_deref().or(self.kind.default_reason())
    }

    fn label(&self, d: Decision) -> &str {
        match &self.labels {
            Some(l) => &l[d.index()],
            None => self.kind.labels()[d.index()],
        }
    }

    /// Rows the card takes: compact or spacious (one fewer with no reason,
    /// one fewer with no hint row).
    pub fn height(&self, spacious: bool) -> u16 {
        let base = if spacious {
            SPACIOUS_ROWS
        } else {
            COMPACT_ROWS
        };
        base - u16::from(self.reason_text().is_none()) - u16::from(!self.hints)
    }

    /// The header: the title on the left, the kind and the risk on the right,
    /// the right half dropped whole when it does not fit beside the title
    /// with a cell to spare (fx `composeApprovalHeaderRow`). The risk is kept
    /// longest: it is what the person most needs.
    fn header(&self, width: usize) -> Line<'static> {
        let t = &self.theme;
        let inner = width.saturating_sub(INSET + RIGHT_MARGIN);
        let title_style = t.text().add_modifier(Modifier::BOLD);
        let kind = Span::styled(self.kind.label(), t.secondary());
        let risk = self
            .risk
            .map(|r| Span::styled(r.label(), t.level(r.level())));
        let mut rights: Vec<Vec<Span<'static>>> = Vec::new();
        if let Some(r) = &risk {
            rights.push(vec![
                kind.clone(),
                Span::styled(SEPARATOR, t.dim()),
                r.clone(),
            ]);
            rights.push(vec![r.clone()]);
        } else {
            rights.push(vec![kind.clone()]);
        }
        let mut options: Vec<(&str, Vec<Span<'static>>)> = Vec::new();
        for title in [TITLE, SHORT_TITLE] {
            for right in &rights {
                options.push((title, right.clone()));
            }
        }
        let fallback = match &risk {
            Some(r) => vec![r.clone()],
            None => vec![Span::styled(SHORT_TITLE, title_style)],
        };
        let mut spans = vec![Span::raw(" ".repeat(INSET))];
        match options
            .into_iter()
            .find(|(title, right)| text::width(title) + 1 + text::spans_width(right) <= inner)
        {
            Some((title, right)) => {
                let gap = inner - text::width(title) - text::spans_width(&right);
                spans.push(Span::styled(title, title_style));
                spans.push(Span::raw(" ".repeat(gap)));
                spans.extend(right);
            }
            None => spans.extend(fallback),
        }
        Line::from(spans)
    }

    /// The action row: `$ command` for a command, the path for a file,
    /// `tool args` for a tool — inert, and clipped to `width`.
    fn action_line(&self, width: usize) -> Line<'static> {
        let t = &self.theme;
        let strong = t.accent().add_modifier(Modifier::BOLD);
        let action = inert(&self.action);
        let room = width.saturating_sub(INSET);
        let mut spans = vec![Span::raw(" ".repeat(INSET))];
        match self.kind {
            ApprovalKind::Command => {
                spans.push(Span::styled("$ ", t.dim()));
                spans.push(Span::styled(action.into_owned(), strong));
            }
            ApprovalKind::Write | ApprovalKind::Edit => {
                // A path keeps its file name: shortened from the left.
                spans.push(Span::styled(
                    truncate_left(&action, room).into_owned(),
                    strong,
                ));
            }
            ApprovalKind::Tool => {
                if let Some(tool) = &self.tool {
                    spans.push(Span::styled(inert(tool).into_owned(), strong));
                    if !action.is_empty() {
                        spans.push(Span::raw(" "));
                    }
                }
                spans.push(Span::styled(action.into_owned(), t.text()));
            }
        }
        Line::from(spans)
    }

    fn choice_line(&self, d: Decision, selected: bool) -> Line<'static> {
        let t = &self.theme;
        let label = format!("{}. {}", d.index() + 1, inert(self.label(d)));
        let mut spans = vec![Span::raw(" ".repeat(INSET))];
        if selected {
            spans.push(Span::styled("\u{276f} ", t.accent()));
            spans.push(Span::styled(label, t.accent().add_modifier(Modifier::BOLD)));
        } else {
            spans.push(Span::raw("  "));
            spans.push(Span::styled(label, t.text()));
        }
        if self.recommended == Some(d) {
            spans.push(Span::styled("  (recommended)", t.secondary()));
        }
        Line::from(spans)
    }

    fn hint_line(&self, width: usize) -> Line<'static> {
        let room = width.saturating_sub(INSET);
        let hint = HINTS
            .iter()
            .find(|h| text::width(h) <= room)
            .unwrap_or(&HINTS[HINTS.len() - 1]);
        Line::from(vec![
            Span::raw(" ".repeat(INSET)),
            Span::styled(*hint, self.theme.dim()),
        ])
    }

    /// Every row in order, each with how long it survives a short area: the
    /// marked choice first, then the action, the other choices, the header,
    /// the question, the hint, the reason, and blank rows last.
    fn rows(&self, state: &ApprovalState, width: usize, spacious: bool) -> Vec<CardRow> {
        let t = &self.theme;
        let blank = || CardRow {
            line: Line::default(),
            keep: 0,
        };
        let mut rows = vec![CardRow {
            line: self.header(width),
            keep: 4,
        }];
        if spacious {
            rows.push(blank());
        }
        let question = self.question.as_deref().unwrap_or(self.kind.question());
        rows.push(CardRow {
            line: Line::from(vec![
                Span::raw(" ".repeat(INSET)),
                Span::styled(
                    inert(question).into_owned(),
                    t.text().add_modifier(Modifier::BOLD),
                ),
            ]),
            keep: 3,
        });
        if let Some(reason) = self.reason_text() {
            rows.push(CardRow {
                line: Line::from(vec![
                    Span::raw(" ".repeat(INSET)),
                    Span::styled("Reason: ", t.dim()),
                    Span::styled(inert(reason).into_owned(), t.text()),
                ]),
                keep: 1,
            });
        }
        rows.push(CardRow {
            line: self.action_line(width),
            keep: 6,
        });
        if spacious {
            rows.push(blank());
        }
        let selected = state.selected();
        for d in Decision::ALL {
            rows.push(CardRow {
                line: self.choice_line(d, d == selected),
                keep: if d == selected { 7 } else { 5 },
            });
        }
        if self.hints {
            if spacious {
                rows.push(blank());
            }
            rows.push(CardRow {
                line: self.hint_line(width),
                keep: 2,
            });
        }
        rows
    }

    /// The compact card as ratatui text, each row clipped to `width` — for a
    /// host that places it inside its own layout.
    pub fn lines(&self, state: &ApprovalState, width: u16) -> Vec<Line<'static>> {
        self.rows(state, width as usize, false)
            .into_iter()
            .map(|r| text::truncate_line(r.line, width as usize))
            .collect()
    }

    /// A one-line receipt of the answer, for the transcript: `None` is a
    /// cancel.
    pub fn receipt(&self, decision: Option<Decision>) -> Line<'static> {
        let t = &self.theme;
        let (mark, words, style) = match decision {
            Some(Decision::Once) => (
                "\u{2713}",
                "Allowed once",
                t.status(crate::theme::Status::Done),
            ),
            Some(Decision::Session) => (
                "\u{2713}",
                "Allowed for this session",
                t.status(crate::theme::Status::Done),
            ),
            Some(Decision::Deny) => ("\u{2717}", "Denied", t.status(crate::theme::Status::Failed)),
            None => (
                "\u{2717}",
                "Cancelled",
                t.status(crate::theme::Status::Failed),
            ),
        };
        let action = match (self.kind, &self.tool) {
            (ApprovalKind::Command, _) => format!("$ {}", inert(&self.action)),
            (ApprovalKind::Tool, Some(tool)) => format!("{} {}", inert(tool), inert(&self.action)),
            _ => inert(&self.action).into_owned(),
        };
        Line::from(vec![
            Span::styled(mark, style),
            Span::raw(" "),
            Span::styled(words, style.add_modifier(Modifier::BOLD)),
            Span::styled("  ", t.dim()),
            Span::styled(action, t.secondary()),
        ])
    }
}

impl StatefulWidget for ApprovalCard<'_> {
    type State = ApprovalState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ApprovalState) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl StatefulWidget for &ApprovalCard<'_> {
    type State = ApprovalState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ApprovalState) {
        if area.is_empty() {
            return;
        }
        let width = area.width as usize;
        let spacious = area.height >= self.height(true);
        let rows = self.rows(state, width, spacious);
        // Keep the rows that matter most, in their own order.
        let mut order: Vec<usize> = (0..rows.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(rows[i].keep));
        order.truncate(area.height as usize);
        order.sort_unstable();
        for (dy, i) in order.into_iter().enumerate() {
            let line = text::truncate_line(rows[i].line.clone(), width);
            buf.set_line(area.x, area.y + dy as u16, &line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Mods;
    use crate::snapshot;
    use ratatui::style::Color;

    fn render(card: &ApprovalCard, state: &mut ApprovalState, w: u16, h: u16) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        StatefulWidget::render(card, buf.area, &mut buf, state);
        buf
    }

    #[test]
    fn the_compact_card_is_fxs_rows() {
        let card = ApprovalCard::command("echo hi > out.txt && cat out.txt");
        let mut st = ApprovalState::new();
        let rows = snapshot::lines(&render(&card, &mut st, 80, card.height(false)));
        assert_eq!(
            rows,
            [
                &format!(
                    "  Permission needed \u{2219} Choose one{}Command",
                    " ".repeat(39)
                ),
                "  Would you like to run the following command?",
                "  $ echo hi > out.txt && cat out.txt",
                "  \u{276f} 1. Yes",
                "    2. Yes, and allow this command for the session",
                "    3. No",
                "  1-3 choose now    up/down options    enter confirm    esc cancel",
            ]
        );
    }

    #[test]
    fn the_spacious_card_has_blank_rows_between_its_parts() {
        let card = ApprovalCard::write("src/main.rs");
        let mut st = ApprovalState::new();
        assert_eq!(card.height(true), SPACIOUS_ROWS);
        let rows = snapshot::lines(&render(&card, &mut st, 80, SPACIOUS_ROWS));
        assert_eq!(rows[1], "");
        assert_eq!(rows[2], "  Would you like to create or update this file?");
        assert_eq!(
            rows[3],
            "  Reason: This action changes files in your workspace."
        );
        assert_eq!(rows[4], "  src/main.rs");
        assert_eq!(rows[5], "");
        assert_eq!(rows[9], "");
    }

    #[test]
    fn digits_answer_at_once_and_arrows_wrap() {
        let mut st = ApprovalState::new();
        assert_eq!(
            st.key('2'.into()),
            ApprovalEvent::Decided(Decision::Session)
        );
        assert_eq!(st.selected(), Decision::Session);
        assert_eq!(st.key(Key::Down.into()), ApprovalEvent::Moved);
        assert_eq!(st.selected(), Decision::Deny);
        st.key(Key::Down.into());
        assert_eq!(st.selected(), Decision::Once, "wraps to the top");
        st.key(Key::Up.into());
        assert_eq!(st.selected(), Decision::Deny, "and to the bottom");
        assert_eq!(
            st.key(Key::Enter.into()),
            ApprovalEvent::Decided(Decision::Deny)
        );
        assert_eq!(st.key(Key::Esc.into()), ApprovalEvent::Cancelled);
        assert_eq!(
            st.key(KeyInput::ctrl(Key::Char('c'))),
            ApprovalEvent::Cancelled
        );
        assert_eq!(st.key('4'.into()), ApprovalEvent::Ignored);
        assert_eq!(st.key('x'.into()), ApprovalEvent::Ignored);
        let alt = KeyInput {
            key: Key::Char('1'),
            mods: Mods {
                alt: true,
                ..Mods::default()
            },
        };
        assert_eq!(st.key(alt), ApprovalEvent::Ignored);
    }

    #[test]
    fn the_marked_choice_moves_the_caret_and_the_bold() {
        let card = ApprovalCard::command("ls").theme(Theme::mono());
        let mut st = ApprovalState::at(Decision::Deny);
        let buf = render(&card, &mut st, 60, 7);
        let rows = snapshot::lines(&buf);
        assert_eq!(rows[5], "  \u{276f} 3. No");
        assert!(buf[(4, 5)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(4, 3)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn risk_and_recommendation_are_words_and_survive_mono() {
        let card = ApprovalCard::command("rm -rf build/")
            .risk(Risk::High)
            .recommended(Decision::Deny)
            .reason("Deletes build/ and everything in it.")
            .theme(Theme::mono());
        let mut st = ApprovalState::at(Decision::Deny);
        let buf = render(&card, &mut st, 80, 8);
        let rows = snapshot::lines(&buf);
        assert!(rows[0].ends_with("Command \u{2219} high risk"), "{rows:?}");
        assert_eq!(rows[6], "  \u{276f} 3. No  (recommended)");
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        let x = rows[0]
            .find("high")
            .map(|b| text::width(&rows[0][..b]))
            .unwrap() as u16;
        assert!(
            buf[(x, 0)].modifier.contains(Modifier::REVERSED),
            "high risk stands out in mono"
        );
        // In colour it is the crit colour.
        let t = Theme::dark();
        let buf = render(&card.clone().theme(t), &mut st, 80, 8);
        assert_eq!(buf[(x, 0)].fg, t.crit);
    }

    #[test]
    fn the_header_keeps_the_risk_longest_and_drops_the_right_half_whole() {
        let card = ApprovalCard::command("ls").risk(Risk::Medium);
        let st = ApprovalState::new();
        let at = |w: u16| snapshot::lines(&render(&card, &mut st.clone(), w, 7))[0].clone();
        assert!(at(80).ends_with("Command \u{2219} medium risk"));
        assert_eq!(
            at(40),
            format!("  Permission needed{}medium risk", " ".repeat(8))
        );
        assert_eq!(at(20), "  medium risk");
    }

    #[test]
    fn a_long_command_is_clipped_by_cells_and_a_path_keeps_its_name() {
        let card = ApprovalCard::command("echo 안녕하세요 && cargo build --release --all-features");
        let mut st = ApprovalState::new();
        let rows = snapshot::lines(&render(&card, &mut st, 30, 7));
        assert_eq!(rows[2], "  $ echo 안녕하세요 && cargo …");
        assert!(text::width(&rows[2]) <= 30);
        let card = ApprovalCard::edit("crates/twolab-tui/src/widgets/approval_card/mod.rs");
        let rows = snapshot::lines(&render(&card, &mut st, 30, 8));
        assert_eq!(rows[3], "  …idgets/approval_card/mod.rs");
    }

    #[test]
    fn a_command_is_shown_verbatim_and_never_obeyed() {
        let card = ApprovalCard::command("printf '\u{1b}[2J'\nrm -rf /tmp/x\r");
        let mut st = ApprovalState::new();
        let rows = snapshot::lines(&render(&card, &mut st, 80, 7));
        assert_eq!(rows[2], "  $ printf '\\x1b[2J'\\nrm -rf /tmp/x\\r");
    }

    #[test]
    fn the_hint_degrades_and_keeps_enter_and_esc() {
        let card = ApprovalCard::command("ls");
        let st = ApprovalState::new();
        let hint = |w: u16| snapshot::lines(&render(&card, &mut st.clone(), w, 7))[6].clone();
        assert_eq!(
            hint(80),
            "  1-3 choose now    up/down options    enter confirm    esc cancel"
        );
        assert_eq!(hint(50), "  1-3 choose now    enter confirm    esc cancel");
        assert_eq!(hint(32), "  enter confirm    esc cancel");
    }

    #[test]
    fn a_short_area_keeps_the_marked_choice_and_the_action_first() {
        let card = ApprovalCard::command("git push --force").risk(Risk::High);
        let mut st = ApprovalState::at(Decision::Session);
        let rows = snapshot::lines(&render(&card, &mut st, 60, 2));
        assert_eq!(rows[0], "  $ git push --force");
        assert!(rows[1].starts_with("  \u{276f} 2. Yes, and"), "{rows:?}");
        let rows = snapshot::lines(&render(&card, &mut st, 60, 4));
        assert_eq!(rows.len(), 4);
        assert!(
            rows[1].contains("1. Yes") && rows[3].contains("3. No"),
            "{rows:?}"
        );
        for (w, h) in [(1, 1), (12, 2), (3, 30)] {
            let _ = render(&card, &mut st, w, h);
        }
    }

    #[test]
    fn a_tool_request_names_the_tool_and_its_arguments() {
        let card = ApprovalCard::tool("web_fetch", "https://docs.rs/ratatui");
        let mut st = ApprovalState::new();
        let rows = snapshot::lines(&render(&card, &mut st, 80, 8));
        assert_eq!(rows[1], "  Would you like to allow this action?");
        assert_eq!(
            rows[2],
            "  Reason: This action needs approval before the agent can continue."
        );
        assert_eq!(rows[3], "  web_fetch https://docs.rs/ratatui");
    }

    #[test]
    fn a_receipt_says_what_was_answered() {
        let card = ApprovalCard::command("cargo test");
        let l = card.receipt(Some(Decision::Session));
        let s: String = l.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(s, "\u{2713} Allowed for this session  $ cargo test");
        let l = card.receipt(None);
        assert!(l.spans[2].content.contains("Cancelled"));
    }
}
