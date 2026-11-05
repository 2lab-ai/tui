use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{StatefulWidget, Widget};

use super::{ChoiceAnswer, ChoiceEvent, ChoiceForm, ChoiceOption, ChoiceState};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::choice_form::{
    ChoiceAnswer, ChoiceEvent, ChoiceForm, ChoiceOption, ChoiceState,
};

let form = ChoiceForm::new(
    "Where should I deploy this build?",
    [
        ChoiceOption::new("staging").description("Fresh data, safe to break"),
        ChoiceOption::new("production").description("Live traffic, needs a release tag"),
    ],
)
.recommended(0)
.theme(theme);

let mut state = ChoiceState::at(0); // start on the recommended option
frame.render_stateful_widget(&form, area, &mut state);

match form.key(&mut state, key) {
    ChoiceEvent::Answered(ChoiceAnswer::Option(i)) => deploy_to(i),
    ChoiceEvent::Answered(ChoiceAnswer::Other(text)) => reply(text),
    ChoiceEvent::Cancelled => cancel(),
    ChoiceEvent::Moved | ChoiceEvent::Edited | ChoiceEvent::Ignored => {}
}"#;

/// An interactive question: the form until it is answered, then the receipt.
struct Ask {
    form: fn(Theme) -> ChoiceForm<'static>,
    start: fn() -> ChoiceState,
    state: ChoiceState,
    answer: Option<Option<ChoiceAnswer>>,
}

impl Ask {
    fn new(form: fn(Theme) -> ChoiceForm<'static>, start: fn() -> ChoiceState) -> Ask {
        Ask {
            form,
            start,
            state: start(),
            answer: None,
        }
    }
}

impl Demo for Ask {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let form = (self.form)(cx.theme);
        match &self.answer {
            None => StatefulWidget::render(&form, area, buf, &mut self.state),
            Some(answer) => {
                let mut lines = form.receipt(answer.as_ref(), area.width);
                lines.push(Line::default());
                lines.push(Line::from(Span::styled(
                    "press any key to ask again",
                    cx.theme.dim(),
                )));
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
            *self = Ask::new(self.form, self.start);
            return true;
        }
        let form = (self.form)(Theme::default());
        match form.key(&mut self.state, key) {
            ChoiceEvent::Ignored => false,
            ChoiceEvent::Moved | ChoiceEvent::Edited => true,
            ChoiceEvent::Answered(a) => {
                self.answer = Some(Some(a));
                true
            }
            ChoiceEvent::Cancelled => {
                self.answer = Some(None);
                true
            }
        }
    }
}

fn deploy(theme: Theme) -> ChoiceForm<'static> {
    ChoiceForm::new(
        "Where should I deploy build 2026.11.05-3 (retry backoff, 14 files changed)?",
        [
            ChoiceOption::new("staging").description("Fresh data every night, safe to break"),
            ChoiceOption::new("production").description("Live traffic; needs the v1.8.0 tag"),
            ChoiceOption::new("preview").description("A throwaway URL for this branch"),
        ],
    )
    .recommended(0)
    .theme(theme)
}

fn yes_no(theme: Theme) -> ChoiceForm<'static> {
    ChoiceForm::new(
        "Run the users-table migration now? It holds a write lock for about 2 seconds.",
        [
            ChoiceOption::new("Yes, run it now"),
            ChoiceOption::new("No, schedule it for 02:00 UTC"),
        ],
    )
    .no_other()
    .recommended(1)
    .progress(2, 2)
    .theme(theme)
}

fn branch_name(theme: Theme) -> ChoiceForm<'static> {
    ChoiceForm::new(
        "What should the new branch be called?",
        [
            ChoiceOption::new("feat/retry-backoff").description("From the task title"),
            ChoiceOption::new("fix/429-storm").description("From the linked issue"),
        ],
    )
    .other("Something else")
    .placeholder("type a branch name")
    .theme(theme)
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "choice-form",
    name: "Choice form",
    ty: "twolab_tui::choice_form::ChoiceForm",
    summary: "The agent needs you to decide: a question, numbered options with the recommended one marked, and a row to type your own answer.",
    description: "A question in bold, then numbered options, each with an optional description in a column of its own on a wide area or under the label on a narrow one. The marked option is bold with a ❯; the recommended one says so. The last row turns into a one-line field when marked, with a reversed-cell caret that scrolls with long answers. A digit answers at once, Enter answers the marked row, Esc cancels; a short area drops blank rows, the hint and wrapping before it windows the options with ⋯ N more rows.",
    used_in: "Agent shells asking a clarifying question mid-task; confirmations in a dashboard (spend a reset, switch scheduler) that need one clear answer.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/footer/question_ui.zig#L40-L210",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "ChoiceForm::new(question, options)",
            ty: "impl Into<Cow<str>>, impl IntoIterator<Item: Into<ChoiceOption>>",
            default: None,
            description: "Options are labels (&str), (label, description) pairs or ChoiceOption values.",
        },
        PropDoc {
            name: ".recommended(i)",
            ty: "usize",
            default: Some("none"),
            description: "The option marked (recommended); start the state there with ChoiceState::at(i).",
        },
        PropDoc {
            name: ".other(label) / .no_other() / .placeholder(text)",
            ty: "impl Into<Cow<str>>",
            default: Some("\"Other\", \"type your answer\""),
            description: "The free-text row, or none.",
        },
        PropDoc {
            name: ".progress(i, n)",
            ty: "(usize, usize)",
            default: Some("none"),
            description: "Question i of n, right-aligned on the hint row.",
        },
        PropDoc {
            name: ".hints(on)",
            ty: "bool",
            default: Some("true"),
            description: "The key-hint row.",
        },
        PropDoc {
            name: ".key(&mut state, k)",
            ty: "fn(&mut ChoiceState, KeyInput) -> ChoiceEvent",
            default: None,
            description: "Ignored, Moved, Edited, Answered(Option(i) | Other(text)) or Cancelled.",
        },
        PropDoc {
            name: ".lines(&state, width) / .height(&state, width) / .receipt(answer, width)",
            ty: "Vec<Line> / u16 / Vec<Line>",
            default: None,
            description: "The whole panel as text and its height; the question with its answer for the transcript.",
        },
    ],
    keyboard: &[
        (
            "1–9",
            "Answer with that option (the Other digit opens the field)",
        ),
        ("↑ / ↓, Tab / Shift+Tab", "Move the mark (wraps)"),
        ("Enter", "Answer with the marked row"),
        ("Esc, Ctrl+C", "Cancel"),
        ("In the field: ← → Home End", "Move the caret by grapheme"),
        (
            "In the field: Backspace, Delete, Ctrl+U",
            "Delete before, after, or to the start",
        ),
    ],
    a11y: &[
        "The marked row has a ❯ and is bold; the recommended option says (recommended) in words.",
        "The free-text caret is a reversed cell, drawn by the widget — the host keeps its terminal cursor hidden.",
        "Editing is by grapheme: a Korean syllable or an emoji family is one Backspace.",
        "Nothing is cut silently: a clipped row ends in …, a scrolled answer starts with …, and hidden options are counted.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "deploy",
            title: "Pick a deploy target",
            description: "Three targets with descriptions, staging recommended; 1–4, ↑↓, Enter, Esc.",
            cols: 80,
            rows: 10,
            animated: false,
            interactive: true,
            make: || Box::new(Ask::new(deploy, || ChoiceState::at(0))),
        },
        DemoDoc {
            id: "yes-no",
            title: "Yes or no",
            description: "Two answers, no free text, the second of two questions.",
            cols: 84,
            rows: 7,
            animated: false,
            interactive: true,
            make: || Box::new(Ask::new(yes_no, || ChoiceState::at(1))),
        },
        DemoDoc {
            id: "free-text",
            title: "Type your own answer",
            description: "The free-text row is open with a draft; type, move the caret, Enter to answer.",
            cols: 64,
            rows: 8,
            animated: false,
            interactive: true,
            make: || {
                Box::new(Ask::new(branch_name, || {
                    ChoiceState::at(2).with_draft("feat/retry-with-jitter")
                }))
            },
        },
    ],
};
