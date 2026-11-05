// Derived from vercel-labs/fx `src/ui/footer/question_ui.zig`,
// `src/ui/footer/question_freeform_layout.zig` and the question hints in
// `src/ui/footer/input_presentation.zig` at commit
// 412d841d73dd6e1b8de551022728d9c929caae17 (Apache-2.0, Copyright 2025 Vercel,
// Inc.): the panel's rows (`composeQuestionPanelText`), the `N) ` ordinals and
// their seven-cell prefix (`optionPrefixWidth`), the description column by
// width (`questionDescriptionColumn`), the freeform slot that becomes a field
// with a reverse-video caret (`writeSelectedFreeformOptionLines`), the hint
// ladder and the right-aligned `Question i of n` (`questionInteractionHint`),
// and the resolution rows (`writeQuestionAnswerResolution`). Translated from
// Zig to Rust and re-expressed as ratatui text; key rules checked against
// 2lab's xfx `src/tui/question.rs` and `src/tui/freeform.rs` (Apache-2.0) at
// 93f516b. Modified by 2lab.

//! The agent needs you to decide: a question, numbered options with the
//! recommended one marked, and an "Other" row you can type into.
//!
//! Ported from Vercel fx's question prompt (`src/ui/footer/question_ui.zig`,
//! Apache-2.0). The question is bold and wraps; options are `N) label`, with
//! the description in a column of its own on a wide area (from 72 cells it
//! starts at column 38, from 52 at 30) or under the label on a narrow one.
//! The marked option is bold with a `❯`; the rest are quieter. The last row,
//! "Other", becomes a one-line field when marked — typing goes into it, the
//! caret is a reversed cell, and Enter answers with what was typed.
//!
//! A digit answers at once (the "Other" digit opens the field instead);
//! ↑↓ move the mark and wrap; Enter answers; Esc cancels. When the area is
//! short the blank rows go first, then the hint, then wrapping, and then the
//! options are windowed around the mark with a `⋯ N more` row on each side
//! that has hidden options — never cut silently.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

use crate::input::{Key, KeyInput};
use crate::text;
use crate::theme::Theme;
use crate::widgets::diff_view::inert;

/// Cells in front of a question row (fx `"  "`).
const QUESTION_INSET: usize = 2;
/// Cells in front of an option's label: two of inset, the mark, its space and
/// `N) ` (fx `optionPrefixWidth`: four of indent and `N) `).
const PREFIX: usize = 7;

/// What the free-text row is called until something is typed into it.
pub const DEFAULT_OTHER: &str = "Other";
/// What the free-text row says it is for.
pub const DEFAULT_PLACEHOLDER: &str = "type your answer";

/// Widest first; every rung keeps Enter and Esc (fx
/// `predefined_question_hints`, ASCII arrows so the row is one width on a
/// CJK terminal). `{n}` is the option count.
const CHOOSE_HINTS: [&str; 3] = [
    "1-{n} choose now    up/down options    enter answer    esc cancel",
    "up/down options    enter answer    esc cancel",
    "enter answer    esc cancel",
];
/// While the free-text row is open (fx `freeform_question_hints`).
const TYPE_HINTS: [&str; 3] = [
    "type answer    left/right cursor    up/down options    enter answer    esc cancel",
    "up/down options    enter answer    esc cancel",
    "enter answer    esc cancel",
];

/// One option: a label and, optionally, what it means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption<'a> {
    pub label: Cow<'a, str>,
    pub description: Option<Cow<'a, str>>,
}

impl<'a> ChoiceOption<'a> {
    pub fn new(label: impl Into<Cow<'a, str>>) -> ChoiceOption<'a> {
        ChoiceOption {
            label: label.into(),
            description: None,
        }
    }

    pub fn description(mut self, d: impl Into<Cow<'a, str>>) -> Self {
        self.description = Some(d.into());
        self
    }
}

impl<'a> From<&'a str> for ChoiceOption<'a> {
    fn from(label: &'a str) -> Self {
        ChoiceOption::new(label)
    }
}

impl<'a> From<(&'a str, &'a str)> for ChoiceOption<'a> {
    fn from((label, description): (&'a str, &'a str)) -> Self {
        ChoiceOption::new(label).description(description)
    }
}

/// What the person answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceAnswer {
    /// One of the options, by index.
    Option(usize),
    /// What they typed into the free-text row (may be empty, as fx allows).
    Other(String),
}

/// What a key did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChoiceEvent {
    Ignored,
    /// The mark moved (or the caret moved inside the field).
    Moved,
    /// The free-text answer changed.
    Edited,
    Answered(ChoiceAnswer),
    /// Esc or Ctrl+C.
    Cancelled,
}

/// The marked row and the free-text draft with its caret.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChoiceState {
    selected: usize,
    draft: String,
    /// A byte offset into `draft`, always on a grapheme boundary.
    cursor: usize,
}

impl ChoiceState {
    pub fn new() -> ChoiceState {
        ChoiceState::default()
    }

    /// Marked on row `i` (start on the recommended option).
    pub fn at(i: usize) -> ChoiceState {
        ChoiceState {
            selected: i,
            ..ChoiceState::default()
        }
    }

    /// With `draft` already typed into the free-text row, the caret at its
    /// end — to restore a draft the person left.
    pub fn with_draft(mut self, draft: impl Into<String>) -> ChoiceState {
        self.draft = draft.into();
        self.cursor = self.draft.len();
        self
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// What has been typed into the free-text row.
    pub fn draft(&self) -> &str {
        &self.draft
    }

    /// The caret's byte offset into [`ChoiceState::draft`].
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    fn insert(&mut self, s: &str) {
        self.draft.insert_str(self.cursor, s);
        self.cursor = snap_to_grapheme(&self.draft, self.cursor + s.len());
    }
}

/// The grapheme boundary before `at` (or 0).
pub(crate) fn prev_grapheme(s: &str, at: usize) -> usize {
    s[..at]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(i, _)| i)
}

/// The grapheme boundary after `at` (or `at` at the end).
pub(crate) fn next_grapheme(s: &str, at: usize) -> usize {
    s[at..].graphemes(true).next().map_or(at, |g| at + g.len())
}

/// `at` if it is a grapheme boundary, else the next one — after an insert
/// that joined two clusters (a ZWJ, a combining mark) the caret is never left
/// inside one.
pub(crate) fn snap_to_grapheme(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    let mut c = GraphemeCursor::new(at, s.len(), true);
    if c.is_boundary(s, 0) == Ok(true) {
        return at;
    }
    c.next_boundary(s, 0).ok().flatten().unwrap_or(s.len())
}

/// Text cut into rows of at most `width` cells, word by word: a word that
/// does not fit moves down whole, a word wider than a row is split by
/// grapheme, and no row starts or ends with the space it broke at (fx
/// `nextWrappedTextLine`). Each span keeps its style. Hard line breaks start
/// a new row.
pub(crate) fn wrap_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut col = 0usize;
    fn push(rows: &mut [Vec<Span<'static>>], s: &str, style: Style) {
        let row = rows.last_mut().expect("a row");
        match row.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(s),
            _ => row.push(Span::styled(s.to_string(), style)),
        }
    }
    fn trim_end(row: &mut Vec<Span<'static>>) {
        while let Some(last) = row.last_mut() {
            let t = last.content.trim_end_matches(' ').len();
            if t == 0 {
                row.pop();
                continue;
            }
            last.content.to_mut().truncate(t);
            break;
        }
    }
    for span in spans {
        let style = span.style;
        for (pi, para) in span.content.split('\n').enumerate() {
            if pi > 0 {
                trim_end(rows.last_mut().expect("a row"));
                rows.push(Vec::new());
                col = 0;
            }
            for token in para.split_inclusive(' ') {
                let (word, space) = match token.strip_suffix(' ') {
                    Some(w) => (w, true),
                    None => (token, false),
                };
                let w = text::width(word);
                if col > 0 && col + w > width {
                    trim_end(rows.last_mut().expect("a row"));
                    rows.push(Vec::new());
                    col = 0;
                }
                let mut rest = word;
                while text::width(rest) > width - col {
                    let (head, _) = text::take_cells(rest, width - col);
                    let head = if head.is_empty() && col == 0 {
                        // One grapheme wider than the whole row: it gets a row.
                        rest.graphemes(true).next().unwrap_or(rest)
                    } else {
                        head
                    };
                    if !head.is_empty() {
                        push(&mut rows, head, style);
                    }
                    rows.push(Vec::new());
                    col = 0;
                    rest = &rest[head.len()..];
                }
                if !rest.is_empty() {
                    push(&mut rows, rest, style);
                    col += text::width(rest);
                }
                if space && col > 0 {
                    if col < width {
                        push(&mut rows, " ", style);
                        col += 1;
                    } else {
                        rows.push(Vec::new());
                        col = 0;
                    }
                }
            }
        }
    }
    for row in &mut rows {
        trim_end(row);
    }
    if rows.len() > 1 && rows.last().is_some_and(Vec::is_empty) {
        rows.pop();
    }
    rows
}

/// The question, its options and a free-text row, as a panel.
#[derive(Debug, Clone)]
pub struct ChoiceForm<'a> {
    question: Cow<'a, str>,
    options: Vec<ChoiceOption<'a>>,
    recommended: Option<usize>,
    other: Option<Cow<'a, str>>,
    placeholder: Cow<'a, str>,
    progress: Option<(usize, usize)>,
    hints: bool,
    theme: Theme,
}

/// How much of the panel fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fit {
    blanks: bool,
    hint: bool,
    wrap: bool,
}

impl<'a> ChoiceForm<'a> {
    pub fn new<O: Into<ChoiceOption<'a>>>(
        question: impl Into<Cow<'a, str>>,
        options: impl IntoIterator<Item = O>,
    ) -> ChoiceForm<'a> {
        ChoiceForm {
            question: question.into(),
            options: options.into_iter().map(Into::into).collect(),
            recommended: None,
            other: Some(Cow::Borrowed(DEFAULT_OTHER)),
            placeholder: Cow::Borrowed(DEFAULT_PLACEHOLDER),
            progress: None,
            hints: true,
            theme: Theme::default(),
        }
    }

    /// The option marked `(recommended)`.
    pub fn recommended(mut self, i: usize) -> Self {
        self.recommended = Some(i);
        self
    }

    /// The free-text row's label (default `Other`).
    pub fn other(mut self, label: impl Into<Cow<'a, str>>) -> Self {
        self.other = Some(label.into());
        self
    }

    /// No free-text row: the options are the only answers.
    pub fn no_other(mut self) -> Self {
        self.other = None;
        self
    }

    /// What the free-text row says it is for (default `type your answer`).
    pub fn placeholder(mut self, p: impl Into<Cow<'a, str>>) -> Self {
        self.placeholder = p.into();
        self
    }

    /// `Question i of n`, right-aligned on the hint row (1-based `i`).
    pub fn progress(mut self, i: usize, n: usize) -> Self {
        self.progress = Some((i, n));
        self
    }

    pub fn hints(mut self, on: bool) -> Self {
        self.hints = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Rows that can be marked: the options and the free-text row.
    pub fn len(&self) -> usize {
        self.options.len() + usize::from(self.other.is_some())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The free-text row's index, when there is one.
    pub fn other_index(&self) -> Option<usize> {
        self.other.as_ref().map(|_| self.options.len())
    }

    fn typing(&self, state: &ChoiceState) -> bool {
        self.other_index() == Some(state.selected)
    }

    /// Apply a key. Digits answer at once (the free-text digit opens the
    /// field); ↑↓ and Tab move the mark, wrapping; Enter answers with the
    /// marked row; Esc or Ctrl+C cancels. While the free-text row is marked,
    /// printable keys, Backspace, Delete, ←→, Home and End edit it.
    pub fn key(&self, state: &mut ChoiceState, key: KeyInput) -> ChoiceEvent {
        let n = self.len();
        if n == 0 {
            return match key.key {
                Key::Esc => ChoiceEvent::Cancelled,
                _ => ChoiceEvent::Ignored,
            };
        }
        state.selected = state.selected.min(n - 1);
        state.cursor = snap_to_grapheme(&state.draft, state.cursor);
        if key.mods.ctrl {
            return match key.key {
                Key::Char('c') => ChoiceEvent::Cancelled,
                Key::Char('u') if self.typing(state) && state.cursor > 0 => {
                    state.draft.replace_range(..state.cursor, "");
                    state.cursor = 0;
                    ChoiceEvent::Edited
                }
                _ => ChoiceEvent::Ignored,
            };
        }
        let mv = |state: &mut ChoiceState, down: bool| {
            state.selected = if down {
                (state.selected + 1) % n
            } else {
                (state.selected + n - 1) % n
            };
            ChoiceEvent::Moved
        };
        match key.key {
            Key::Up | Key::BackTab => return mv(state, false),
            Key::Down | Key::Tab => return mv(state, true),
            Key::Esc => return ChoiceEvent::Cancelled,
            _ => {}
        }
        if self.typing(state) {
            let d = &state.draft;
            let at = state.cursor;
            return match key.key {
                Key::Enter => ChoiceEvent::Answered(ChoiceAnswer::Other(state.draft.clone())),
                Key::Char(c) if !key.mods.alt && !c.is_control() => {
                    let mut b = [0u8; 4];
                    state.insert(c.encode_utf8(&mut b));
                    ChoiceEvent::Edited
                }
                Key::Backspace if at > 0 => {
                    let p = prev_grapheme(d, at);
                    state.draft.replace_range(p..at, "");
                    state.cursor = p;
                    ChoiceEvent::Edited
                }
                Key::Delete if at < d.len() => {
                    let e = next_grapheme(d, at);
                    state.draft.replace_range(at..e, "");
                    ChoiceEvent::Edited
                }
                Key::Left if at > 0 => {
                    state.cursor = prev_grapheme(d, at);
                    ChoiceEvent::Moved
                }
                Key::Right if at < d.len() => {
                    state.cursor = next_grapheme(d, at);
                    ChoiceEvent::Moved
                }
                Key::Home if at > 0 => {
                    state.cursor = 0;
                    ChoiceEvent::Moved
                }
                Key::End if at < d.len() => {
                    state.cursor = d.len();
                    ChoiceEvent::Moved
                }
                _ => ChoiceEvent::Ignored,
            };
        }
        match key.key {
            Key::Enter => ChoiceEvent::Answered(ChoiceAnswer::Option(state.selected)),
            Key::Char(c @ '1'..='9') if !key.mods.alt => {
                let i = c as usize - '1' as usize;
                if i >= n {
                    ChoiceEvent::Ignored
                } else if Some(i) == self.other_index() {
                    // fx: the free-text ordinal opens the field, it does not answer.
                    state.selected = i;
                    state.cursor = state.draft.len();
                    ChoiceEvent::Moved
                } else {
                    state.selected = i;
                    ChoiceEvent::Answered(ChoiceAnswer::Option(i))
                }
            }
            _ => ChoiceEvent::Ignored,
        }
    }

    /// The column the description starts at, or `None` when it goes under
    /// the label (fx `questionDescriptionColumn`, as a 0-based cell).
    fn description_column(width: usize) -> Option<usize> {
        if width >= 72 {
            Some(37)
        } else if width >= 52 {
            Some(29)
        } else {
            None
        }
    }

    fn question_rows(&self, width: usize, wrap: bool) -> Vec<Line<'static>> {
        let style = self.theme.text().add_modifier(Modifier::BOLD);
        let q: String = self
            .question
            .split('\n')
            .map(|p| inert(p).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        let indent = || Span::raw(" ".repeat(QUESTION_INSET));
        let room = width.saturating_sub(QUESTION_INSET);
        if !wrap {
            let one = q.replace('\n', " ");
            return vec![text::truncate_line(
                Line::from(vec![indent(), Span::styled(one, style)]),
                width,
            )];
        }
        wrap_spans(vec![Span::styled(q, style)], room)
            .into_iter()
            .map(|mut r| {
                r.insert(0, indent());
                Line::from(r)
            })
            .collect()
    }

    /// The prefix of option `i`: inset, the mark, and `N) ` (no number past 9).
    fn prefix(&self, i: usize, selected: bool) -> Vec<Span<'static>> {
        let t = &self.theme;
        let ordinal = if i < 9 {
            format!("{}) ", i + 1)
        } else {
            "   ".to_string()
        };
        let mark = if selected {
            Span::styled("\u{276f} ", t.accent())
        } else {
            Span::raw("  ")
        };
        vec![
            Span::raw("  "),
            mark,
            Span::styled(ordinal, self.label_style(selected)),
        ]
    }

    fn label_style(&self, selected: bool) -> Style {
        if selected {
            self.theme.accent().add_modifier(Modifier::BOLD)
        } else {
            self.theme.secondary()
        }
    }

    /// The label words of option `i` (with `(recommended)`), and its
    /// description, both inert.
    fn option_text(
        &self,
        i: usize,
        selected: bool,
        state: &ChoiceState,
    ) -> (Vec<Span<'static>>, Option<String>) {
        let t = &self.theme;
        let mut label = Vec::new();
        let description;
        if Some(i) == self.other_index() {
            let other = self.other.as_deref().unwrap_or(DEFAULT_OTHER);
            if state.draft.is_empty() {
                label.push(Span::styled(
                    inert(other).into_owned(),
                    self.label_style(selected),
                ));
                description = Some(inert(&self.placeholder).into_owned());
            } else {
                label.push(Span::styled(
                    inert(&state.draft).into_owned(),
                    self.label_style(selected),
                ));
                description = None;
            }
        } else {
            let o = &self.options[i];
            label.push(Span::styled(
                inert(&o.label).into_owned(),
                self.label_style(selected),
            ));
            description = o.description.as_deref().map(|d| inert(d).into_owned());
        }
        if self.recommended == Some(i) {
            label.push(Span::styled(
                " (recommended)",
                t.dim().add_modifier(Modifier::ITALIC),
            ));
        }
        (label, description)
    }

    /// The open free-text row: the prefix, then the draft scrolled so the
    /// caret shows, with the caret as a reversed cell (fx
    /// `writeSelectedFreeformOptionLines`, one row). An empty draft shows the
    /// caret and the placeholder, dim.
    fn field_row(&self, i: usize, state: &ChoiceState, width: usize) -> Line<'static> {
        let t = &self.theme;
        let style = self.label_style(true);
        let caret = Style::new().add_modifier(Modifier::REVERSED);
        let mut spans = self.prefix(i, true);
        let room = width.saturating_sub(PREFIX).max(1);
        if state.draft.is_empty() {
            spans.push(Span::styled(" ", caret));
            spans.push(Span::styled(inert(&self.placeholder).into_owned(), t.dim()));
            return text::truncate_line(Line::from(spans), width);
        }
        let draft = &state.draft;
        let at = state.cursor.min(draft.len());
        let end = next_grapheme(draft, at);
        let under = if end > at {
            inert(&draft[at..end]).into_owned()
        } else {
            " ".into()
        };
        let mut before = inert(&draft[..at]).into_owned();
        let after = inert(&draft[end..]).into_owned();
        // Scroll: drop graphemes off the left until the caret fits, and say so.
        let under_w = text::width(&under);
        if text::width(&before) + under_w > room {
            let keep = room.saturating_sub(under_w + 1);
            let mut start = before.len();
            let mut used = 0;
            for (b, g) in before.grapheme_indices(true).rev() {
                let w = text::width(g);
                if used + w > keep {
                    break;
                }
                used += w;
                start = b;
            }
            before = format!("{}{}", text::ELLIPSIS, &before[start..]);
        }
        spans.push(Span::styled(before, style));
        spans.push(Span::styled(under, style.patch(caret)));
        spans.push(Span::styled(after, style));
        text::truncate_line(Line::from(spans), width)
    }

    /// Option `i` as rows: the label wrapped beside its description column,
    /// or the description under it, or one clipped row when `wrap` is off.
    fn option_rows(
        &self,
        i: usize,
        state: &ChoiceState,
        width: usize,
        wrap: bool,
    ) -> Vec<Line<'static>> {
        let selected = i == state.selected;
        if selected && Some(i) == self.other_index() {
            return vec![self.field_row(i, state, width)];
        }
        let t = &self.theme;
        let (label, description) = self.option_text(i, selected, state);
        // fx: only an option with a description gives up width to its column.
        let column = description
            .as_ref()
            .and_then(|_| Self::description_column(width));
        let pad = || Span::raw(" ".repeat(PREFIX));
        if !wrap {
            let mut spans = self.prefix(i, selected);
            spans.extend(label);
            if let (Some(d), Some(col)) = (description, column) {
                let used = text::spans_width(&spans);
                spans.push(Span::raw(" ".repeat(col.saturating_sub(used).max(1))));
                spans.push(Span::styled(d, t.dim()));
            }
            return vec![text::truncate_line(Line::from(spans), width)];
        }
        match column {
            Some(col) => {
                // Label inside its column, description beside it.
                let label_w = col.saturating_sub(1 + PREFIX).max(1);
                let lrows = wrap_spans(label, label_w);
                let drows = description
                    .map(|d| {
                        wrap_spans(
                            vec![Span::styled(d, t.dim())],
                            width.saturating_sub(col + 1).max(1),
                        )
                    })
                    .unwrap_or_default();
                let n = lrows.len().max(drows.len());
                (0..n)
                    .map(|r| {
                        let mut spans = if r == 0 {
                            self.prefix(i, selected)
                        } else {
                            vec![pad()]
                        };
                        if let Some(l) = lrows.get(r) {
                            spans.extend(l.iter().cloned());
                        }
                        if let Some(d) = drows.get(r) {
                            let used = text::spans_width(&spans);
                            spans.push(Span::raw(" ".repeat(col.saturating_sub(used).max(1))));
                            spans.extend(d.iter().cloned());
                        }
                        text::truncate_line(Line::from(spans), width)
                    })
                    .collect()
            }
            None => {
                let room = width.saturating_sub(PREFIX).max(1);
                let mut rows: Vec<Line<'static>> = wrap_spans(label, room)
                    .into_iter()
                    .enumerate()
                    .map(|(r, l)| {
                        let mut spans = if r == 0 {
                            self.prefix(i, selected)
                        } else {
                            vec![pad()]
                        };
                        spans.extend(l);
                        text::truncate_line(Line::from(spans), width)
                    })
                    .collect();
                if let Some(d) = description {
                    rows.extend(
                        wrap_spans(vec![Span::styled(d, t.dim())], room)
                            .into_iter()
                            .map(|l| {
                                let mut spans = vec![pad()];
                                spans.extend(l);
                                text::truncate_line(Line::from(spans), width)
                            }),
                    );
                }
                rows
            }
        }
    }

    fn hint_line(&self, state: &ChoiceState, width: usize) -> Line<'static> {
        let t = &self.theme;
        let room = width.saturating_sub(QUESTION_INSET);
        let n = self.len().min(9).to_string();
        let ladder = if self.typing(state) {
            TYPE_HINTS
        } else {
            CHOOSE_HINTS
        };
        let hint = ladder
            .iter()
            .map(|h| h.replace("{n}", &n))
            .find(|h| text::width(h) <= room)
            .unwrap_or_else(|| ladder[ladder.len() - 1].to_string());
        let mut spans = vec![
            Span::raw(" ".repeat(QUESTION_INSET)),
            Span::styled(hint, t.dim()),
        ];
        if let Some((i, n)) = self.progress {
            let p = format!("Question {i} of {n}");
            let used = text::spans_width(&spans);
            let pw = text::width(&p);
            if used + 2 + pw <= width {
                spans.push(Span::raw(" ".repeat(width - used - pw)));
                spans.push(Span::styled(p, t.secondary()));
            }
        }
        text::truncate_line(Line::from(spans), width)
    }

    fn layout(&self, state: &ChoiceState, width: usize, fit: Fit) -> Vec<Line<'static>> {
        let mut rows = self.question_rows(width, fit.wrap);
        if fit.blanks {
            rows.push(Line::default());
        }
        for i in 0..self.len() {
            rows.extend(self.option_rows(i, state, width, fit.wrap));
        }
        if fit.hint {
            if fit.blanks {
                rows.push(Line::default());
            }
            rows.push(self.hint_line(state, width));
        }
        rows
    }

    /// Every row of the full panel at `width` (question, blank, options,
    /// blank, hint) — for a host that places it in its own layout.
    pub fn lines(&self, state: &ChoiceState, width: u16) -> Vec<Line<'static>> {
        let mut st = state.clone();
        st.selected = st.selected.min(self.len().saturating_sub(1));
        self.layout(
            &st,
            width as usize,
            Fit {
                blanks: true,
                hint: self.hints,
                wrap: true,
            },
        )
    }

    /// Rows the full panel takes at `width`.
    pub fn height(&self, state: &ChoiceState, width: u16) -> u16 {
        self.lines(state, width).len().min(u16::MAX as usize) as u16
    }

    /// What stays in the transcript once answered (fx
    /// `writeQuestionAnswerResolution`): the question, and the answer hung
    /// dim beneath it — or `▪ Cancelled`.
    pub fn receipt(&self, answer: Option<&ChoiceAnswer>, width: u16) -> Vec<Line<'static>> {
        let t = &self.theme;
        let width = width as usize;
        let Some(answer) = answer else {
            return vec![Line::from(vec![
                Span::styled("\u{25aa}", t.status(crate::theme::Status::Failed)),
                Span::raw(" Cancelled"),
            ])];
        };
        let words = match answer {
            ChoiceAnswer::Option(i) => self
                .options
                .get(*i)
                .map_or_else(|| "\u{2014}".to_string(), |o| inert(&o.label).into_owned()),
            ChoiceAnswer::Other(s) => inert(s).into_owned(),
        };
        let mut rows: Vec<Line<'static>> = self.question_rows(width, true);
        let indent = || Span::raw(" ".repeat(QUESTION_INSET));
        for r in wrap_spans(
            vec![Span::styled(words, t.dim())],
            width.saturating_sub(QUESTION_INSET),
        ) {
            let mut spans = vec![indent()];
            spans.extend(r);
            rows.push(Line::from(spans));
        }
        rows
    }
}

/// The options in `0..n` a window of `slots` rows shows around `sel`, with
/// room left for a `⋯ N more` row on each side that hides any.
fn window(n: usize, sel: usize, slots: usize) -> (usize, usize) {
    if n <= slots {
        return (0, n);
    }
    if slots <= 2 {
        let k = slots.max(1);
        let start = sel.saturating_sub(k - 1).min(n - k);
        return (start, start + k);
    }
    for k in (1..=slots).rev() {
        let start = sel.saturating_sub(k / 2).min(n - k);
        let marks = usize::from(start > 0) + usize::from(start + k < n);
        if k + marks <= slots {
            return (start, start + k);
        }
    }
    (sel, sel + 1)
}

impl StatefulWidget for ChoiceForm<'_> {
    type State = ChoiceState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ChoiceState) {
        StatefulWidget::render(&self, area, buf, state);
    }
}

impl StatefulWidget for &ChoiceForm<'_> {
    type State = ChoiceState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut ChoiceState) {
        if area.is_empty() {
            return;
        }
        let width = area.width as usize;
        let height = area.height as usize;
        if self.is_empty() {
            let q = self.question_rows(width, false);
            buf.set_line(area.x, area.y, &q[0], area.width);
            return;
        }
        state.selected = state.selected.min(self.len() - 1);
        state.cursor = snap_to_grapheme(&state.draft, state.cursor);
        let fits = [
            Fit {
                blanks: true,
                hint: self.hints,
                wrap: true,
            },
            Fit {
                blanks: false,
                hint: self.hints,
                wrap: true,
            },
            Fit {
                blanks: false,
                hint: false,
                wrap: true,
            },
            Fit {
                blanks: false,
                hint: false,
                wrap: false,
            },
        ];
        let rows = fits
            .iter()
            .map(|f| self.layout(state, width, *f))
            .find(|rows| rows.len() <= height)
            .unwrap_or_else(|| {
                // Window the one-row options around the mark.
                let t = &self.theme;
                let mut rows = Vec::new();
                let with_question = height >= 3;
                if with_question {
                    rows.extend(self.question_rows(width, false));
                }
                let slots = height - rows.len();
                let n = self.len();
                let (start, end) = window(n, state.selected, slots);
                let more = |k: usize| {
                    text::truncate_line(
                        Line::from(vec![
                            Span::raw(" ".repeat(PREFIX)),
                            Span::styled(format!("\u{22ef} {k} more"), t.dim()),
                        ]),
                        width,
                    )
                };
                if start > 0 && slots > end - start {
                    rows.push(more(start));
                }
                for i in start..end {
                    rows.extend(self.option_rows(i, state, width, false));
                }
                if end < n && rows.len() < height {
                    rows.push(more(n - end));
                }
                rows
            });
        for (dy, line) in rows.iter().take(height).enumerate() {
            buf.set_line(area.x, area.y + dy as u16, line, area.width);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use ratatui::style::Color;

    fn db() -> ChoiceForm<'static> {
        ChoiceForm::new(
            "Which database should I use?",
            [
                ChoiceOption::new("Postgres").description("Relational, strong consistency"),
                ChoiceOption::new("SQLite").description("Single file, embedded"),
                ChoiceOption::new("DynamoDB"),
            ],
        )
        .hints(false)
    }

    fn render(form: &ChoiceForm, st: &mut ChoiceState, w: u16, h: u16) -> Vec<String> {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        StatefulWidget::render(form, buf.area, &mut buf, st);
        snapshot::lines(&buf)
    }

    fn typed(form: &ChoiceForm, st: &mut ChoiceState, s: &str) {
        for c in s.chars() {
            form.key(st, c.into());
        }
    }

    // xfx `the_panel_is_upstreams_rows_at_120_columns`, with the ❯ mark.
    #[test]
    fn the_panel_is_fxs_rows_at_120_columns() {
        let mut st = ChoiceState::new();
        let rows = render(&db(), &mut st, 120, 8);
        assert_eq!(
            rows,
            [
                "  Which database should I use?",
                "",
                "  \u{276f} 1) Postgres                      Relational, strong consistency",
                "    2) SQLite                        Single file, embedded",
                "    3) DynamoDB",
                "    4) Other                         type your answer",
                "",
                "",
            ]
        );
        assert_eq!(db().height(&st, 120), 6);
    }

    // xfx `a_description_takes_its_column_by_the_screens_width`.
    #[test]
    fn the_description_column_follows_the_width() {
        let mut st = ChoiceState::new();
        let at = |w: u16, st: &mut ChoiceState| render(&db(), st, w, 12)[2].clone();
        assert_eq!(at(80, &mut st).find("Relational"), Some(37 + 2));
        assert_eq!(at(72, &mut st).find("Relational"), Some(37 + 2));
        assert_eq!(at(71, &mut st).find("Relational"), Some(29 + 2));
        assert_eq!(at(52, &mut st).find("Relational"), Some(29 + 2));
        let narrow = render(&db(), &mut st, 51, 12);
        assert_eq!(narrow[2], "  \u{276f} 1) Postgres");
        assert_eq!(narrow[3], "       Relational, strong consistency");
        assert_eq!(narrow[4], "    2) SQLite");
    }

    // xfx `a_long_label_and_its_description_wrap_side_by_side`.
    #[test]
    fn a_long_label_and_its_description_wrap_side_by_side() {
        let form = ChoiceForm::new(
            "Which?",
            [
                ChoiceOption::new("Run the complete verification suite before pushing")
                    .description("Keep the entire explanation visible even when it wraps"),
                ChoiceOption::new("Skip"),
            ],
        )
        .hints(false);
        let mut st = ChoiceState::at(1);
        let rows = render(&form, &mut st, 80, 10);
        assert_eq!(
            rows[2],
            "    1) Run the complete verification Keep the entire explanation visible even"
        );
        assert_eq!(
            rows[3],
            "       suite before pushing          when it wraps"
        );
        assert_eq!(rows[4], "  \u{276f} 2) Skip");
        assert!(rows.iter().all(|r| text::width(r) <= 80));
    }

    #[test]
    fn a_label_without_a_description_takes_the_whole_row() {
        let form = ChoiceForm::new(
            "Run it?",
            [
                ChoiceOption::new("No, schedule it for 02:00 UTC tonight"),
                ChoiceOption::new("Yes").description("Holds a write lock for 2 s"),
            ],
        )
        .recommended(0)
        .no_other()
        .hints(false);
        let rows = render(&form, &mut ChoiceState::new(), 80, 4);
        assert_eq!(
            rows[2],
            "  \u{276f} 1) No, schedule it for 02:00 UTC tonight (recommended)"
        );
        assert_eq!(rows[3].find("Holds"), Some(37));
    }

    #[test]
    fn a_digit_answers_at_once_and_arrows_wrap() {
        let form = db();
        let mut st = ChoiceState::new();
        assert_eq!(
            form.key(&mut st, '2'.into()),
            ChoiceEvent::Answered(ChoiceAnswer::Option(1))
        );
        assert_eq!(form.key(&mut st, '9'.into()), ChoiceEvent::Ignored);
        assert_eq!(form.key(&mut st, Key::Up.into()), ChoiceEvent::Moved);
        assert_eq!(st.selected(), 0);
        form.key(&mut st, Key::Up.into());
        assert_eq!(st.selected(), 3, "wraps to Other");
        form.key(&mut st, Key::Down.into());
        assert_eq!(st.selected(), 0, "and back");
        assert_eq!(
            form.key(&mut st, Key::Enter.into()),
            ChoiceEvent::Answered(ChoiceAnswer::Option(0))
        );
        assert_eq!(form.key(&mut st, Key::Esc.into()), ChoiceEvent::Cancelled);
    }

    // xfx `the_freeform_ordinal_only_opens_editing_and_later_digits_are_text`.
    #[test]
    fn the_other_digit_opens_the_field_and_later_digits_are_text() {
        let form = db();
        let mut st = ChoiceState::new();
        assert_eq!(form.key(&mut st, '4'.into()), ChoiceEvent::Moved);
        assert_eq!(form.key(&mut st, '1'.into()), ChoiceEvent::Edited);
        assert_eq!(st.draft(), "1");
        form.key(&mut st, Key::Up.into());
        assert_eq!(st.draft(), "1", "leaving keeps the draft");
        assert_eq!(
            form.key(&mut st, '1'.into()),
            ChoiceEvent::Answered(ChoiceAnswer::Option(0)),
            "and a digit is a choice again"
        );
    }

    #[test]
    fn the_field_edits_by_grapheme_and_answers_with_what_was_typed() {
        let form = db();
        let mut st = ChoiceState::new();
        form.key(&mut st, '4'.into());
        typed(&form, &mut st, "네—ok 👨\u{200d}👩\u{200d}👧");
        form.key(&mut st, Key::Backspace.into());
        assert_eq!(st.draft(), "네—ok ", "the family is one grapheme");
        form.key(&mut st, Key::Home.into());
        form.key(&mut st, Key::Delete.into());
        assert_eq!(st.draft(), "—ok ");
        form.key(&mut st, Key::Right.into());
        form.key(&mut st, 'X'.into());
        assert_eq!(st.draft(), "—Xok ");
        assert_eq!(
            form.key(&mut st, Key::Enter.into()),
            ChoiceEvent::Answered(ChoiceAnswer::Other("—Xok ".into()))
        );
        // fx accepts an empty answer.
        let mut st = ChoiceState::at(3);
        assert_eq!(
            form.key(&mut st, Key::Enter.into()),
            ChoiceEvent::Answered(ChoiceAnswer::Other(String::new()))
        );
    }

    #[test]
    fn the_open_field_is_one_row_with_a_reversed_caret() {
        let form = db().theme(Theme::mono());
        let mut st = ChoiceState::at(3);
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 6));
        StatefulWidget::render(&form, buf.area, &mut buf, &mut st);
        assert_eq!(snapshot::lines(&buf)[5], "  \u{276f} 4)  type your answer");
        assert!(buf[(7, 5)].modifier.contains(Modifier::REVERSED));
        typed(&form, &mut st, "MariaDB please");
        form.key(&mut st, Key::Home.into());
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 6));
        StatefulWidget::render(&form, buf.area, &mut buf, &mut st);
        assert_eq!(snapshot::lines(&buf)[5], "  \u{276f} 4) MariaDB please");
        assert!(
            buf[(7, 5)].modifier.contains(Modifier::REVERSED),
            "the caret is on M"
        );
        assert!(!buf[(8, 5)].modifier.contains(Modifier::REVERSED));
        // An unmarked slot reads as what was typed into it (fx `questionOptionLabel`).
        form.key(&mut st, Key::Up.into());
        assert_eq!(render(&form, &mut st, 60, 6)[5], "    4) MariaDB please");
    }

    #[test]
    fn a_long_answer_scrolls_to_keep_the_caret_and_says_so() {
        let form = db();
        let mut st = ChoiceState::at(3);
        typed(
            &form,
            &mut st,
            "please use the managed Postgres in eu-west-1 with 3 replicas",
        );
        let row = render(&form, &mut st, 40, 6)
            .into_iter()
            .find(|r| r.starts_with("  \u{276f} 4)"))
            .expect("the open field");
        assert!(row.starts_with("  \u{276f} 4) …"), "{row:?}");
        assert!(row.ends_with("3 replicas"), "{row:?}");
        assert!(text::width(&row) <= 40);
    }

    #[test]
    fn recommended_and_progress_are_words() {
        let form = db()
            .recommended(1)
            .progress(2, 3)
            .hints(true)
            .theme(Theme::mono());
        let mut st = ChoiceState::at(1);
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 8));
        StatefulWidget::render(&form, buf.area, &mut buf, &mut st);
        let rows = snapshot::lines(&buf);
        assert!(
            rows[3].starts_with("  \u{276f} 2) SQLite (recommended)"),
            "{rows:?}"
        );
        assert!(
            rows[7].starts_with("  1-4 choose now    up/down options"),
            "{rows:?}"
        );
        assert!(rows[7].ends_with("Question 2 of 3"), "{rows:?}");
        for cell in buf.content() {
            assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset));
        }
        assert!(buf[(7, 3)].modifier.contains(Modifier::BOLD));
        assert!(!buf[(7, 2)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn the_hint_changes_while_typing_and_keeps_enter_and_esc() {
        let form = db().hints(true);
        let mut st = ChoiceState::at(3);
        let rows = render(&form, &mut st, 100, 8);
        assert!(
            rows[7].starts_with("  type answer    left/right cursor"),
            "{rows:?}"
        );
        let rows = render(&form, &mut st, 34, 10);
        assert_eq!(
            rows.iter().rev().find(|r| !r.is_empty()).unwrap(),
            "  enter answer    esc cancel"
        );
    }

    #[test]
    fn a_short_area_drops_blanks_then_hint_then_windows_with_counts() {
        let form = ChoiceForm::new(
            "Which region?",
            [
                "us-east-1",
                "us-west-2",
                "eu-west-1",
                "eu-central-1",
                "ap-northeast-2",
                "ap-southeast-1",
            ],
        );
        let mut st = ChoiceState::at(4);
        let rows = render(&form, &mut st, 60, 9);
        assert_eq!(rows[0], "  Which region?");
        assert_eq!(rows[1], "    1) us-east-1", "blank rows went first");
        assert_eq!(rows.len(), 9);
        let rows = render(&form, &mut st, 60, 5);
        assert_eq!(
            rows,
            [
                "  Which region?",
                "       \u{22ef} 3 more",
                "    4) eu-central-1",
                "  \u{276f} 5) ap-northeast-2",
                "       \u{22ef} 2 more",
            ]
        );
        let rows = render(&form, &mut st, 60, 1);
        assert_eq!(rows, ["  \u{276f} 5) ap-northeast-2"]);
    }

    #[test]
    fn wrapping_keeps_words_whole_and_splits_only_what_cannot_fit() {
        let rows = |s: &str, w| -> Vec<String> {
            wrap_spans(vec![Span::raw(s.to_string())], w)
                .into_iter()
                .map(|r| r.iter().map(|s| s.content.to_string()).collect())
                .collect()
        };
        assert_eq!(rows("hello brave world", 10), ["hello", "brave", "world"]);
        assert_eq!(rows("aaaa bbbbbb cc", 10), ["aaaa", "bbbbbb cc"]);
        assert_eq!(rows("abcdefghijkl", 5), ["abcde", "fghij", "kl"]);
        assert_eq!(rows("한국어 문장도 셀로", 7), ["한국어", "문장도", "셀로"]);
        assert_eq!(rows("one\ntwo", 20), ["one", "two"]);
        assert_eq!(rows("가", 1), ["가"]);
    }

    #[test]
    fn a_receipt_hangs_the_answer_under_the_question() {
        let form = db();
        let rows: Vec<String> = form
            .receipt(Some(&ChoiceAnswer::Option(1)), 60)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert_eq!(rows, ["  Which database should I use?", "  SQLite"]);
        let c = form.receipt(None, 60);
        assert_eq!(c[0].spans[1].content, " Cancelled");
    }

    #[test]
    fn tiny_areas_and_empty_forms_do_not_panic() {
        let mut st = ChoiceState::at(99);
        for (w, h) in [(1, 1), (12, 2), (3, 9), (7, 3)] {
            render(&db(), &mut st, w, h);
        }
        let empty = ChoiceForm::new("Nothing to pick?", Vec::<&str>::new()).no_other();
        assert!(empty.is_empty());
        assert_eq!(empty.key(&mut st, Key::Enter.into()), ChoiceEvent::Ignored);
        render(&empty, &mut st, 20, 3);
    }
}
