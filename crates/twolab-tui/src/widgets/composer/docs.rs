use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::StatefulWidget;

use super::{Composer, ComposerEvent, ComposerState, MASK_GLYPH, Mask};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::color::{ColorLevel, Rgb};
use crate::input::KeyInput;
use crate::text;
use crate::theme::{Palette, Theme};

const USAGE: &str = r#"use twolab_tui::color::{self, ColorLevel};
use twolab_tui::composer::{Composer, ComposerEvent, ComposerState, Mask};

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
    ComposerEvent::SubmittedSecret => {} // only a masked state answers this
}

// A one-line field with a preset the first key replaces, drawn in a
// selection colour made from the terminal's background (OSC 11), or
// reversed when that is unknown.
let mut rename = ComposerState::with_text("feat/quota-backoff")
    .single_line()
    .replace_on_type(true);
let field = Composer::new()
    .selection_style(theme.selection_on(color::parse_osc_color(&reply), ColorLevel::TrueColor));

// A secret: drawn as ∙ per grapheme, inside the host's own prompt line.
let mut key = ComposerState::new().masked(Mask::default());
let mut spans = vec![Span::raw(" add account — key: ")];
spans.extend(Composer::new().prompt("").theme(theme).line(&mut key, 40).spans);
if let ComposerEvent::SubmittedSecret = key.key(k, 40) {
    add_account(key.take_secret()); // the only way the text comes out
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
            ComposerEvent::Moved | ComposerEvent::Edited | ComposerEvent::SubmittedSecret => true,
            ComposerEvent::Submitted(text) => {
                self.sent.push(text);
                true
            }
        }
    }
}

fn dim_line(buf: &mut Buffer, area: Rect, y: u16, s: &str, t: &Theme) {
    if y < area.bottom() {
        let l = text::truncate_line(
            Line::from(Span::styled(s.to_string(), t.dim())),
            area.width as usize,
        );
        buf.set_line(area.x, y, &l, area.width);
    }
}

/// The terminal background the catalog draws each palette on — what a host
/// learns from its OSC 11 query. ANSI: the user's theme, so unknown.
fn host_background(t: &Theme) -> Option<Rgb> {
    match t.palette {
        Palette::Dark => Some((0x14, 0x14, 0x16)),
        Palette::Light => Some((0xfb, 0xfb, 0xfa)),
        Palette::Ansi | Palette::Mono => None,
    }
}

/// A one-line rename field: the suggested name is a preset the first key
/// replaces, longer than the field, so the window and its `‹ ›` show. The
/// preset is drawn in the selection colour made from the host background.
struct Rename {
    state: ComposerState,
    done: Option<String>,
    width: u16,
}

impl Rename {
    fn new() -> Rename {
        Rename {
            state: ComposerState::with_text("feat/quota-poller-backoff-honours-retry-after")
                .single_line()
                .replace_on_type(true),
            done: None,
            width: 36,
        }
    }
}

impl Demo for Rename {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if area.is_empty() {
            return;
        }
        let t = &cx.theme;
        self.width = area.width.min(36);
        dim_line(buf, area, area.y, "rename workspace", t);
        let row = if area.height > 1 { area.y + 1 } else { area.y };
        let composer = Composer::new()
            .placeholder("a name")
            .selection_style(t.selection_on(host_background(t), ColorLevel::TrueColor))
            .theme(*t);
        StatefulWidget::render(
            &composer,
            Rect::new(area.x, row, self.width, 1),
            buf,
            &mut self.state,
        );
        let hint = if self.state.is_preset() {
            "type to replace    \u{2190}\u{2192} keep and edit    enter rename"
        } else {
            "ctrl+w/u/k cut    ctrl+y put back    enter rename"
        };
        dim_line(buf, area, row + 1, hint, t);
        if let Some(name) = &self.done {
            let l = text::truncate_line(
                Line::from(vec![
                    Span::styled("renamed to ", t.dim()),
                    Span::styled(name.clone(), t.text()),
                ]),
                area.width as usize,
            );
            if row + 2 < area.bottom() {
                buf.set_line(area.x, row + 2, &l, area.width);
            }
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.state.key(key, self.width) {
            ComposerEvent::Ignored => false,
            ComposerEvent::Submitted(name) => {
                self.done = Some(name);
                true
            }
            _ => true,
        }
    }
}

/// llmux's add-account prompt: the key is typed into a masked field inside
/// the footer line, and only `take_secret` gives it back.
struct AddKey {
    state: ComposerState,
    added: usize,
}

/// A made-up key: the demo never draws it.
const FAKE_KEY: &str = "sk-ant-api03-demo-0000-not-a-real-key";

impl AddKey {
    fn new() -> AddKey {
        AddKey {
            state: ComposerState::with_text(FAKE_KEY).masked(Mask::default()),
            added: 0,
        }
    }
}

impl Demo for AddKey {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if area.is_empty() {
            return;
        }
        let t = &cx.theme;
        let lead = " add account \u{2014} key: ";
        let keys = [("enter", " add  "), ("esc", " cancel")];
        let tail: usize = keys
            .iter()
            .map(|(k, l)| text::width(k) + text::width(l))
            .sum::<usize>()
            + 2;
        let field_w = (area.width as usize)
            .saturating_sub(text::width(lead) + tail)
            .clamp(4, 40) as u16;
        let field = Composer::new()
            .prompt("")
            .placeholder("paste a key")
            .theme(*t)
            .line(&mut self.state, field_w);
        let mut spans = vec![Span::styled(lead, t.secondary())];
        spans.extend(field.spans);
        spans.push(Span::raw("  "));
        for (k, l) in keys {
            spans.push(Span::styled(k, t.accent()));
            spans.push(Span::styled(l, t.secondary()));
        }
        let l = text::truncate_line(Line::from(spans), area.width as usize);
        buf.set_line(area.x, area.y, &l, area.width);
        dim_line(
            buf,
            area,
            area.y + 1,
            " one \u{2219} per character; nothing here can draw the key itself",
            t,
        );
        if self.added > 0 {
            let msg = format!(
                " {} key{} handed to the daemon (never shown)",
                self.added,
                if self.added == 1 { "" } else { "s" }
            );
            dim_line(buf, area, area.y + 2, &msg, t);
        }
        // The same secret with a fixed mask: not even its length shows.
        let mut fixed = ComposerState::with_text(FAKE_KEY).masked(Mask::fixed(MASK_GLYPH, 8));
        let fixed_line = Composer::new()
            .prompt("")
            .theme(*t)
            .line(&mut fixed, area.width.saturating_sub(40).clamp(4, 16));
        if area.y + 4 < area.bottom() {
            let mut spans = vec![Span::styled(" Mask::fixed(\u{2219}, 8): ", t.dim())];
            spans.extend(fixed_line.spans);
            let l = text::truncate_line(Line::from(spans), area.width as usize);
            buf.set_line(area.x, area.y + 4, &l, area.width);
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.state.key(key, 40) {
            ComposerEvent::Ignored => false,
            ComposerEvent::SubmittedSecret => {
                let secret = self.state.take_secret();
                if !secret.is_empty() {
                    self.added += 1;
                }
                true
            }
            _ => true,
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
    description: "A prompt glyph, then your text soft-wrapped by cells: words move down whole, a word wider than a row breaks where it starts, and spaces hang past the margin so no row starts with one. The caret is a reversed cell. Enter submits and Shift+Enter, Alt+Enter, Ctrl+J or a backslash then Enter add a line. Editing is by grapheme, so Korean and emoji are one step each. It grows to a max height, then follows the caret with ⋮ marking hidden rows. Ctrl+U, Ctrl+K and Ctrl+W cut into a kill buffer and Ctrl+Y puts it back. As a single-line field (herdr's text editor) it never wraps: a window follows the caret, with ‹ and › where text is hidden; a preset is drawn as a selection and the first key replaces it; a masked field draws ∙ per character and never the secret.",
    used_in: "The input band of an agent shell; a multi-line note or prompt field in a dashboard; herdr's rename, search and help-filter fields; llmux's add-key and limits prompts (masked and single-line).",
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
        PropDoc {
            name: "ComposerState::single_line()",
            ty: "ComposerState",
            default: Some("multi-line"),
            description: "A field: no wrap, a window that moves only as far as the caret needs, ‹ and › where text is hidden; line breaks in a paste become spaces; ↑↓ are the host's.",
        },
        PropDoc {
            name: "ComposerState::replace_on_type(on)",
            ty: "bool",
            default: Some("false"),
            description: "The text is a preset: drawn as a selection, replaced whole by the first typed key or paste, emptied by Backspace, kept by a move. is_preset() says whether it still is.",
        },
        PropDoc {
            name: "ComposerState::masked(Mask::each(c) | Mask::fixed(c, n))",
            ty: "Mask",
            default: Some("Mask::each('∙')"),
            description: "A secret on one line: one glyph per grapheme, or always n glyphs. text() is empty, take() gives nothing, Debug says <masked>; Enter answers SubmittedSecret.",
        },
        PropDoc {
            name: "ComposerState::take_secret()",
            ty: "String",
            default: None,
            description: "The only way a masked text comes out; empties the field and its kill buffer.",
        },
        PropDoc {
            name: ".line(&mut state, width)",
            ty: "Line<'static>",
            default: None,
            description: "The one-line view as a line, for a field inside the host's own prompt or footer; moves the window like render.",
        },
        PropDoc {
            name: ".selection_style(style)",
            ty: "Style",
            default: Some("theme.selected()"),
            description: "How a preset is drawn; theme.selection_on(host_bg, level) makes it from the terminal's background (28 % toward white or black, foreground by contrast, reversed when unknown or NO_COLOR).",
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
            "Ctrl+W, Alt+Backspace / Ctrl+U / Ctrl+K / Alt+D",
            "Cut the word back / to line start / to line end / the word ahead, into the kill buffer",
        ),
        ("Ctrl+Y", "Put back what was cut last"),
        (
            "Ctrl+B / Ctrl+F, Alt+B / Alt+F, Ctrl+D, Ctrl+H",
            "readline: by grapheme; by word; delete ahead; Backspace",
        ),
    ],
    a11y: &[
        "The caret is a reversed cell the widget draws, so it shows in every theme including NO_COLOR; cursor_position lets a host use the real terminal cursor instead.",
        "Editing and caret moves are by grapheme cluster: a Korean syllable or a ZWJ emoji is one step and one Backspace.",
        "Hidden rows are marked with ⋮ in the prompt column, never cut silently.",
        "Up on the first row and Down on the last first go to the start or end of the text, so history keys never jump away mid-draft.",
        "A single-line field marks hidden text with ‹ and › (one cell on CJK terminals too) instead of clipping it silently.",
        "A masked field never draws, returns or logs the secret: the glyph ∙ is one cell everywhere, and a fixed mask hides even the length.",
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
        DemoDoc {
            id: "single-line",
            title: "A one-line field with a preset",
            description: "The suggested name is selected — in Theme::selection_on's colour, made from the terminal background (reversed on ANSI, where it is unknown): type to replace it, or ←→ to keep and edit it; ‹ › say the field shows part of it. Ctrl+W then Ctrl+Y cuts and puts back.",
            cols: 56,
            rows: 4,
            animated: false,
            interactive: true,
            make: || Box::new(Rename::new()),
        },
        DemoDoc {
            id: "masked",
            title: "A masked key inside a prompt",
            description: "llmux's add-account prompt: the key is ∙ per character inside the footer line (Composer::line); Enter hands it over with take_secret. Below, a fixed mask hides the length too.",
            cols: 72,
            rows: 5,
            animated: false,
            interactive: true,
            make: || Box::new(AddKey::new()),
        },
    ],
};
