use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use super::{Composer, ComposerEvent, ComposerState};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::text;

const USAGE: &str = r#"use twolab_tui::composer::{Composer, ComposerEvent, ComposerState};

let composer = Composer::new()
    .placeholder("Ask the agent to change something")
    .max_height(6)
    .theme(theme);

// Give it the rows it wants, up to its max height.
let h = composer.height(&state, area.width);
let input = Rect { y: area.bottom() - h, height: h, ..area };
frame.render_stateful_widget(&composer, input, &mut state);

// The same width it was drawn at: ↑↓ move by the rows it wraps to.
match state.key(key, input.width) {
    ComposerEvent::Submitted(text) => send(text),
    ComposerEvent::Ignored => history_or_other(key), // e.g. ↑ on the first row
    ComposerEvent::Moved | ComposerEvent::Edited => {}
}"#;

const HINT: &str = "enter send    shift+enter newline    up/down move";

/// A composer at the bottom of its area, what was sent above it, and a hint.
struct Compose {
    state: ComposerState,
    sent: Vec<String>,
    placeholder: &'static str,
    max_height: u16,
    width: u16,
}

impl Compose {
    fn new(text: &str, placeholder: &'static str, max_height: u16, width: u16) -> Compose {
        Compose {
            state: ComposerState::with_text(text),
            sent: Vec::new(),
            placeholder,
            max_height,
            width,
        }
    }
}

impl Demo for Compose {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if area.is_empty() {
            return;
        }
        let t = &cx.theme;
        self.width = area.width;
        let composer = Composer::new()
            .placeholder(self.placeholder)
            .max_height(self.max_height)
            .theme(*t);
        let hint_rows = u16::from(area.height > 1);
        let h = composer
            .height(&self.state, area.width)
            .min(area.height - hint_rows);
        let input = Rect::new(area.x, area.bottom() - hint_rows - h, area.width, h);
        StatefulWidget::render(&composer, input, buf, &mut self.state);
        if hint_rows == 1 {
            let hint =
                text::truncate_line(Line::from(Span::styled(HINT, t.dim())), area.width as usize);
            buf.set_line(area.x, area.bottom() - 1, &hint, area.width);
        }
        // What was sent, newest just above the composer.
        let room = (input.y - area.y) as usize;
        for (i, msg) in self.sent.iter().rev().take(room).enumerate() {
            let first = msg.lines().next().unwrap_or("");
            let more = msg.lines().count().saturating_sub(1);
            let mut spans = vec![
                Span::styled("sent  ", t.dim()),
                Span::styled(first.to_string(), t.secondary()),
            ];
            if more > 0 {
                spans.push(Span::styled(
                    format!(
                        "  +{more} more {}",
                        if more == 1 { "line" } else { "lines" }
                    ),
                    t.dim(),
                ));
            }
            let line = text::truncate_line(Line::from(spans), area.width as usize);
            buf.set_line(area.x, input.y - 1 - i as u16, &line, area.width);
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.state.key(key, self.width) {
            ComposerEvent::Ignored => false,
            ComposerEvent::Moved | ComposerEvent::Edited => true,
            ComposerEvent::Submitted(text) => {
                self.sent.push(text);
                true
            }
        }
    }
}

const REVIEW: &str = "리뷰 부탁해요 🙏\n- retry.rs: 429와 5xx에서 지수 백오프, 최대 8초\n- send_with_retry가 Retry-After 헤더를 따르게 해 주세요\n테스트도 같이 돌려 주세요 👩\u{200d}💻";

const PASTE: &str = r#"Here is the panic from CI, can you find the cause?

thread 'proxy::retry::tests::backoff_caps_at_max_delay' panicked at crates/proxy/src/retry.rs:88:9:
assertion `left == right` failed
  left: 12.8s
 right: 8s
stack backtrace:
   0: rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::assert_failed_inner
   3: core::panicking::assert_failed
   4: proxy::retry::tests::backoff_caps_at_max_delay
             at ./crates/proxy/src/retry.rs:88:9
   5: proxy::retry::tests::backoff_caps_at_max_delay::{{closure}}
             at ./crates/proxy/src/retry.rs:80:36
   6: core::ops::function::FnOnce::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
failures:
    proxy::retry::tests::backoff_caps_at_max_delay
test result: FAILED. 41 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s"#;

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "composer",
    name: "Composer",
    ty: "twolab_tui::composer::Composer",
    summary: "What you will send next — a multi-line input that grows with its text and scrolls once it is tall.",
    description: "A prompt glyph, then your text soft-wrapped by cells: words move down whole, a word wider than a row breaks where it starts, and spaces hang past the margin so no row starts with one. The caret is a reversed cell. Enter submits and Shift+Enter, Alt+Enter, Ctrl+J or a backslash then Enter add a line. Editing is by grapheme, so Korean and emoji are one step each. It grows to a max height, then follows the caret with ⋮ marking hidden rows.",
    used_in: "The input band of an agent shell; a multi-line note or prompt field in a dashboard.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/ui/input/visual_layout.zig#L39-L723",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "Composer::new()",
            ty: "Composer",
            default: None,
            description: "Drawn with a ComposerState (text, caret, scroll).",
        },
        PropDoc {
            name: ".placeholder(text)",
            ty: "impl Into<Cow<str>>",
            default: Some("\"\""),
            description: "Dim text behind the caret while empty.",
        },
        PropDoc {
            name: ".prompt(glyph)",
            ty: "impl Into<Cow<str>>",
            default: Some("\"❯\""),
            description: "One cell on the first row; ⋮ replaces it on a side with hidden rows.",
        },
        PropDoc {
            name: ".max_height(rows)",
            ty: "u16",
            default: Some("6"),
            description: "Rows it grows to before it scrolls.",
        },
        PropDoc {
            name: ".height(&state, width) / .cursor_position(&state, area)",
            ty: "u16 / Option<Position>",
            default: None,
            description: "Rows to give it; where a host would put the terminal cursor instead of the drawn caret.",
        },
        PropDoc {
            name: "ComposerState::key(k, width)",
            ty: "fn(KeyInput, u16) -> ComposerEvent",
            default: None,
            description: "Ignored, Moved, Edited or Submitted(text); width is the area it is drawn in.",
        },
        PropDoc {
            name: "ComposerState::with_text / insert_str / set_text / take",
            ty: "&str / String",
            default: None,
            description: "Fill it, paste into it (CR LF to LF, tabs to spaces, controls dropped), or empty it.",
        },
    ],
    keyboard: &[
        ("Enter", "Submit (ignored while blank)"),
        ("Shift+Enter, Alt+Enter, Ctrl+J, \\ then Enter", "New line"),
        ("← / →, Alt or Ctrl + ← / →", "By grapheme; by word"),
        (
            "↑ / ↓, PgUp / PgDn",
            "By visual row, keeping the column; at the edge, to the start or end",
        ),
        ("Home / End, Ctrl+A / Ctrl+E", "Start or end of the line"),
        ("Backspace / Delete", "One grapheme"),
        (
            "Ctrl+W, Alt+Backspace / Ctrl+U / Ctrl+K",
            "Word back / to line start / to line end",
        ),
    ],
    a11y: &[
        "The caret is a reversed cell the widget draws, so it shows in every theme including NO_COLOR; cursor_position lets a host use the real terminal cursor instead.",
        "Editing and caret moves are by grapheme cluster: a Korean syllable or a ZWJ emoji is one step and one Backspace.",
        "Hidden rows are marked with ⋮ in the prompt column, never cut silently.",
        "Up on the first row and Down on the last first go to the start or end of the text, so history keys never jump away mid-draft.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "empty",
            title: "Empty, with a placeholder",
            description: "Type, Shift+Enter for a new line, Enter to send.",
            cols: 64,
            rows: 6,
            animated: false,
            interactive: true,
            make: || Box::new(Compose::new("", "Ask the agent to change something", 6, 64)),
        },
        DemoDoc {
            id: "multi-line",
            title: "A multi-line message",
            description: "Korean and emoji, wrapped by cells; ↑↓ keep their column.",
            cols: 56,
            rows: 8,
            animated: false,
            interactive: true,
            make: || Box::new(Compose::new(REVIEW, "", 6, 56)),
        },
        DemoDoc {
            id: "long-paste",
            title: "A long paste",
            description: "Twenty lines of backtrace in a six-row composer; it follows the caret.",
            cols: 72,
            rows: 8,
            animated: false,
            interactive: true,
            make: || Box::new(Compose::new(PASTE, "", 6, 72)),
        },
    ],
};
