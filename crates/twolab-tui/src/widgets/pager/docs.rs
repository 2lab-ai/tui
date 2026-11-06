use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::{Outcome, Pager, Selection, notes};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};
use crate::osc52;
use crate::render::{FlexRenderable, Renderable, RenderableItem};
use crate::text;
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::pager::{Outcome, Pager};

let mut pager = Pager::from_text("Request body", &pretty_json).theme(theme);

match pager.key(key) {
    Outcome::Closed => close_overlay(),
    Outcome::Scrolled => redraw(),
    Outcome::Ignored => {}
}
frame.render_widget(&pager, frame.area());

// Mouse: drag to select (it stays on its rows while scrolling), copy on release.
match mouse.kind {
    Down(Left) => { pager.mouse_down(mouse.column, mouse.row); }
    Drag(Left) => { pager.mouse_drag(mouse.column, mouse.row); }
    Up(Left) if pager.mouse_up(mouse.column, mouse.row) => {
        if let Some(seq) = pager.copy_sequence(osc52::Mux::detect(tmux, screen)) {
            stdout.write_all(seq.as_bytes())?;
        }
    }
    _ => {}
}
// Release notes as pager lines.
let pager = Pager::new("v0.5.0", twolab_tui::pager::notes(changelog, &theme));"#;

#[derive(Clone, Copy)]
enum Kind {
    User,
    Agent,
    Tool,
    Out,
    Blank,
}

/// Lines styled for a theme; rebuilt when the page switches themes.
type Build = fn(&Theme) -> Vec<Line<'static>>;

struct PagerDemo {
    pager: Pager,
    build: Build,
    theme: Option<Theme>,
    closed: bool,
}

impl PagerDemo {
    fn boxed(title: &str, build: Build) -> Box<dyn Demo> {
        Box::new(PagerDemo {
            pager: Pager::new(title, Vec::new()),
            build,
            theme: None,
            closed: false,
        })
    }
}

impl Demo for PagerDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if self.theme != Some(cx.theme) {
            self.pager.set_lines((self.build)(&cx.theme));
            self.pager.set_theme(cx.theme);
            self.theme = Some(cx.theme);
        }
        let mut col = FlexRenderable::new();
        col.push(1, RenderableItem::borrowed(&self.pager));
        if self.closed {
            col.push(
                0,
                RenderableItem::owned(Line::styled(
                    " closed (a host would drop the pager); any scroll key reopens it",
                    cx.theme.dim(),
                )),
            );
        }
        col.render(area, buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.pager.key(key) {
            Outcome::Closed => {
                self.closed = true;
                true
            }
            Outcome::Scrolled => {
                self.closed = false;
                true
            }
            Outcome::Ignored => false,
        }
    }
}

const TRANSCRIPT: [(Kind, &str); 62] = [
    (
        Kind::User,
        "the pager scroll test in llmux fails about one run in forty. can you find out why?",
    ),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "I'll run it in a loop first to see how often it fails and what the failure looks like.",
    ),
    (
        Kind::Tool,
        "$ cargo test -p llmux pager::scroll -- --test-threads=1 (x200)",
    ),
    (
        Kind::Out,
        "test pager::tests::scrolls_to_bottom_after_resize ... FAILED (5 of 200)",
    ),
    (
        Kind::Out,
        "assertion `left == right` failed: left: 37, right: 41",
    ),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "It fails 5 times in 200. The offset is 4 rows short, which is exactly the height of the hint area. Let me read the test.",
    ),
    (Kind::Tool, "$ read src/tui/pager.rs:212-260"),
    (Kind::Out, "fn scrolls_to_bottom_after_resize() {"),
    (
        Kind::Out,
        "    let mut p = Pager::from_text(\"log\", &LOG);",
    ),
    (Kind::Out, "    p.key(End);"),
    (Kind::Out, "    thread::sleep(Duration::from_millis(50));"),
    (Kind::Out, "    term.resize(80, 20);"),
    (Kind::Out, "    assert_eq!(p.scroll_offset(), 41);"),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "The test presses End before the first frame is drawn, so the pager has no page height yet. It then sleeps 50 ms and hopes the render loop has drawn a frame. When the loop is slower than 50 ms, the offset is computed from the default page height, which leaves out the hints.",
    ),
    (Kind::Blank, ""),
    (Kind::Agent, "Two changes:"),
    (
        Kind::Agent,
        "  - remove the sleep and draw one frame explicitly before pressing End;",
    ),
    (
        Kind::Agent,
        "  - clamp the offset from the last drawn frame, so End means the last page at any size.",
    ),
    (Kind::Tool, "$ edit src/tui/pager.rs"),
    (Kind::Out, "-    thread::sleep(Duration::from_millis(50));"),
    (
        Kind::Out,
        "+    term.draw(|f| f.render_widget(&p, f.area()))?;",
    ),
    (
        Kind::Out,
        "-        self.scroll = self.scroll.saturating_add(page);",
    ),
    (
        Kind::Out,
        "+        self.scroll = self.scroll.saturating_add(page).min(self.last_max.get());",
    ),
    (Kind::Blank, ""),
    (
        Kind::Tool,
        "$ cargo test -p llmux pager::scroll -- --test-threads=1 (x500)",
    ),
    (Kind::Out, "test result: ok. 500 passed; 0 failed"),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "500 runs, no failures. The flake came from timing, not from the scroll arithmetic; the clamp also fixes a real bug where End followed by a resize left the view past the end.",
    ),
    (Kind::Blank, ""),
    (
        Kind::User,
        "does the same problem exist in the logs overlay?",
    ),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "Checking. The logs overlay keeps its own offset and follows new lines when it is at the bottom.",
    ),
    (Kind::Tool, "$ grep -n \"scroll\" src/tui/logs.rs"),
    (Kind::Out, "48:    scroll: usize,"),
    (
        Kind::Out,
        "91:        if self.follow { self.scroll = usize::MAX; }",
    ),
    (
        Kind::Out,
        "133:       let offset = self.scroll.min(total.saturating_sub(view));",
    ),
    (Kind::Blank, ""),
    (
        Kind::Agent,
        "It already clamps at render time, so the offset is never past the end on screen. But scrolling up from the bottom starts from usize::MAX, so the first Up press does nothing visible. Same fix: start from the clamped offset.",
    ),
    (Kind::Tool, "$ edit src/tui/logs.rs"),
    (
        Kind::Out,
        "-            Key::Up => self.scroll = self.scroll.saturating_sub(1),",
    ),
    (
        Kind::Out,
        "+            Key::Up => self.scroll = self.offset().saturating_sub(1),",
    ),
    (Kind::Tool, "$ cargo test -p llmux logs::"),
    (Kind::Out, "test result: ok. 14 passed; 0 failed"),
    (Kind::Blank, ""),
    (Kind::Agent, "Done. Summary:"),
    (
        Kind::Agent,
        "  - pager: the test draws a frame instead of sleeping; End and paging clamp to the last frame.",
    ),
    (
        Kind::Agent,
        "  - logs: the first Up after following the tail now moves one line.",
    ),
    (
        Kind::Agent,
        "  - 500 consecutive runs of the pager test pass.",
    ),
    (Kind::Blank, ""),
    (Kind::User, "commit it"),
    (Kind::Blank, ""),
    (
        Kind::Tool,
        "$ git commit -am \"fix(pager): clamp scrolling to the last frame\"",
    ),
    (
        Kind::Out,
        "[main 4c1e9a2] fix(pager): clamp scrolling to the last frame",
    ),
    (
        Kind::Out,
        " 2 files changed, 9 insertions(+), 5 deletions(-)",
    ),
    (Kind::Blank, ""),
    (Kind::Agent, "Committed as 4c1e9a2 on main. I did not push."),
    (Kind::Blank, ""),
    (
        Kind::Out,
        "18.2k tokens in, 3.4k out, 2 min 41 s, claude-opus-5-5 via zhuge@2lab.ai",
    ),
    (Kind::Blank, ""),
];

fn transcript_lines(t: &Theme) -> Vec<Line<'static>> {
    TRANSCRIPT
        .iter()
        .map(|(kind, text)| match kind {
            Kind::User => Line::from(vec![
                Span::styled("you  ", t.accent().add_modifier(Modifier::BOLD)),
                Span::styled(*text, t.text().add_modifier(Modifier::BOLD)),
            ]),
            Kind::Agent => Line::styled(*text, t.text()),
            Kind::Tool => Line::styled(*text, t.secondary()),
            Kind::Out => Line::styled(format!("  {text}"), t.dim()),
            Kind::Blank => Line::default(),
        })
        .collect()
}

fn transcript() -> Box<dyn Demo> {
    PagerDemo::boxed("Session: fix flaky pager scroll test", transcript_lines)
}

fn short_lines(t: &Theme) -> Vec<Line<'static>> {
    [
        ("content-type", "application/json"),
        ("anthropic-version", "2023-06-01"),
        ("anthropic-beta", "fast-mode-2026-02-01"),
        ("x-llmux-account", "zhuge@2lab.ai"),
        ("x-llmux-request-id", "req_01JD8Q4M2X"),
    ]
    .into_iter()
    .map(|(k, v)| {
        Line::from(vec![
            Span::styled(format!("{k}: "), t.secondary()),
            Span::styled(v, t.text()),
        ])
    })
    .collect()
}

fn short() -> Box<dyn Demo> {
    PagerDemo::boxed("Request headers", short_lines)
}

const CODE: [&str; 27] = [
    "/// Pick the account for the next request: the first in rotation order whose",
    "/// five-hour and seven-day windows both have room, skipping accounts that are cooling down.",
    "pub fn pick_account<'a>(accounts: &'a [Account], now: Instant, model: &ModelId) -> Option<&'a Account> {",
    "    accounts",
    "        .iter()",
    "        .filter(|a| a.enabled && a.plan.allows(model) && a.cooldown_until.is_none_or(|t| t <= now))",
    "        .find(|a| {",
    "            let five_hour = a.usage.five_hour.used_fraction();",
    "            let seven_day = a.usage.seven_day.used_fraction();",
    "            // Leave headroom on the 7-day window: it resets weekly, and an account that",
    "            // hits 100% mid-week is gone until Monday 09:00 even if its 5-hour window is empty.",
    "            five_hour < FIVE_HOUR_LIMIT && seven_day < SEVEN_DAY_LIMIT - a.plan.weekly_headroom()",
    "        })",
    "}",
    "",
    "#[cfg(test)]",
    "mod tests {",
    "    use super::*;",
    "",
    "    #[test]",
    "    fn skips_cooling_down_and_full_accounts_in_rotation_order() {",
    "        let now = Instant::now();",
    "        let accounts = [cooling(\"dev1@2lab.ai\", now + MINUTE * 72), full_week(\"ai2@iq.io\"), ok(\"zhuge@2lab.ai\")];",
    "        let picked = pick_account(&accounts, now, &ModelId::from(\"claude-opus-5-5\")).map(|a| a.email.as_str());",
    "        assert_eq!(picked, Some(\"zhuge@2lab.ai\"));",
    "    }",
    "}",
];

fn code_lines(t: &Theme) -> Vec<Line<'static>> {
    CODE.iter()
        .map(|l| {
            let style = if l.trim_start().starts_with("//") {
                t.dim()
            } else {
                t.text()
            };
            Line::styled(*l, style)
        })
        .collect()
}

fn code() -> Box<dyn Demo> {
    PagerDemo::boxed("src/rotation.rs", code_lines)
}

const REFRESH_LOG: [&str; 9] = [
    "refresh 5 accounts at 14:02:11",
    "  zhuge@2lab.ai     ok      7d 41%",
    "  ai3@iq.io         ok      7d 63%",
    "  dev1@2lab.ai      failed  error sending request for url (http://localhost:3456/llmux/refresh): connection refused",
    "  simayi@2lab.ai    ok      7d 12%",
    "  ai2@iq.io         failed  401 Unauthorized: refresh token revoked; run `llmux login ai2@iq.io`",
    "",
    "메모: 서울 리전 키는 주말에 교체 (담당 zhuge@2lab.ai)",
    "4 of 5 accounts usable · next refresh 14:07:11",
];

/// Selecting with a stand-in pointer: the catalog's frames take keys, so
/// arrows move the pointer and Enter presses and releases the button.
struct Select {
    pager: Pager,
    theme: Option<Theme>,
    pointer: (u16, u16),
    pressed: bool,
    said: String,
}

impl Demo for Select {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        if self.theme != Some(cx.theme) {
            let t = cx.theme;
            let lines = REFRESH_LOG
                .iter()
                .map(|l| {
                    let style = if l.contains("failed") {
                        t.text()
                    } else {
                        t.secondary()
                    };
                    Line::styled(*l, style)
                })
                .collect();
            self.pager.set_lines(lines);
            self.pager.set_theme(t);
            self.theme = Some(t);
        }
        let t = &cx.theme;
        let body = Rect {
            height: area.height.saturating_sub(1),
            ..area
        };
        Renderable::render(&self.pager, body, buf);
        // The pointer: underlined and bold (the selection is reversed).
        let (x, y) = (area.x + self.pointer.0, area.y + self.pointer.1);
        if x < body.right() && y < body.bottom() {
            let c = &mut buf[(x, y)];
            c.modifier.insert(Modifier::UNDERLINED | Modifier::BOLD);
        }
        if area.height > 1 {
            let line = Line::from(Span::styled(format!(" {}", self.said), t.dim()));
            let row = Rect::new(area.x, area.bottom() - 1, area.width, 1);
            Renderable::render(&text::truncate_line(line, area.width as usize), row, buf);
        }
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let (x, y) = self.pointer;
        self.pointer = match key.key {
            Key::Left => (x.saturating_sub(1), y),
            Key::Right => (x.saturating_add(1).min(139), y),
            Key::Up => (x, y.saturating_sub(1)),
            Key::Down => (x, y.saturating_add(1).min(39)),
            _ => (x, y),
        };
        let (x, y) = self.pointer;
        match key.key {
            Key::Left | Key::Right | Key::Up | Key::Down => {
                if self.pressed {
                    self.pager.mouse_drag(x, y);
                }
                true
            }
            Key::Enter if !self.pressed => {
                self.pressed = true;
                self.pager.mouse_down(x, y);
                self.said = "pressed · arrows drag · Enter releases".into();
                true
            }
            Key::Enter => {
                self.pressed = false;
                self.said = if self.pager.mouse_up(x, y) {
                    self.copied()
                } else {
                    "a click, not a selection".into()
                };
                true
            }
            Key::Tab => {
                self.said = if self.pager.select_word(x, y) {
                    self.copied()
                } else {
                    "no word under the pointer".into()
                };
                true
            }
            Key::Esc => {
                self.pressed = false;
                self.said = "cleared".into();
                self.pager.clear_selection()
            }
            Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                self.pager.key(key) == Outcome::Scrolled
            }
            _ => false,
        }
    }
}

impl Select {
    fn copied(&self) -> String {
        let text = self.pager.selected_text().unwrap_or_default();
        let bytes = self
            .pager
            .copy_sequence(osc52::Mux::None)
            .map_or(0, |s| s.len());
        let first = text.lines().next().unwrap_or("");
        format!(
            "copied {} chars, {} lines (OSC 52, {bytes} bytes): {first}",
            text.chars().count(),
            text.lines().count()
        )
    }
}

fn select() -> Box<dyn Demo> {
    let mut pager = Pager::new("llmux · refresh log", Vec::new()).hints(false);
    // The dev1 error, picked before any key: rows 3–4 of the content.
    pager.set_selection(Some(Selection::range((3, 28), (4, 40))));
    Box::new(Select {
        pager,
        theme: None,
        pointer: (30, 4),
        pressed: false,
        said: "arrows move the pointer · Enter press/release · Tab word · PgUp/PgDn scroll".into(),
    })
}

const CHANGELOG: &str = "### Added
- `tabs`: clickable < > scroll buttons and a `+` action, opt-in
- `select_list`: tree rows with folding groups; meta columns by width
- `pager`: drag to select, copied with OSC 52

### Changed
- the menu takes the taller side of a short screen instead of covering the row you clicked
- config: `new_tab = \"prefix+c\"` replaces `tab.new`

### Fixed
- 한국어 탭 이름의 클릭 영역이 셀 폭으로 맞춰졌습니다
- notes:

```bash
cargo install twolab-tui-catalog
tui-catalog --theme mono
```
";

fn notes_lines(t: &Theme) -> Vec<Line<'static>> {
    notes(CHANGELOG, t)
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "pager",
    name: "Pager",
    ty: "twolab_tui::pager::Pager",
    summary: "The whole text, and where you are in it.",
    description: "Codex's pager overlay: a title rule, the content with long lines wrapped (nothing cut), ~ on rows past the end, and a bottom rule that says how far down you are as a percentage. Line heights are measured exactly once per width and cached, so a long transcript scrolls without re-wrapping every frame; any stack of Renderables can be paged too. From herdr: a mouse drag selects text that stays on its rows while the pager scrolls (at most three highlight rects, read back by cells with wide glyphs whole, copied through OSC 52), a double-click selects a word, and notes() turns release-notes markdown into styled lines.",
    used_in: "Codex shows static text, diffs and approvals in it. llmux's raw request/response viewer, input excerpt and logs overlay are the planned hosts.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/pager_overlay.rs#L168-L488",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Primitives,
    props: &[
        PropDoc {
            name: "Pager::new(title, lines)",
            ty: "impl Into<String>, Vec<Line<'static>>",
            default: None,
            description: "Lines that wrap at the pager's width. Pager::from_text(title, &str) splits on newlines.",
        },
        PropDoc {
            name: "Pager::with_renderables(title, items)",
            ty: "Vec<Box<dyn Renderable>>",
            default: None,
            description: "Page any stack of renderables (a highlighted block, a diff); partly visible items scroll through render_offset_content.",
        },
        PropDoc {
            name: ".set_lines(lines)",
            ty: "Vec<Line<'static>>",
            default: None,
            description: "Replace the content and keep the position (clamped at the next paint).",
        },
        PropDoc {
            name: ".hints(bool)",
            ty: "bool",
            default: Some("true"),
            description: "The key-hint row under the bottom rule.",
        },
        PropDoc {
            name: ".glyphs(g)",
            ty: "PagerGlyphs",
            default: Some("/ ╌ ~ (Narrow)"),
            description: "PagerGlyphs::ascii() for a - rule; PagerGlyphs::box_drawing() opts into Codex's ─ (Ambiguous width).",
        },
        PropDoc {
            name: ".scroll_to(rows) / .scroll_offset()",
            ty: "usize",
            default: Some("0"),
            description: "Start position; the offset as of the last paint.",
        },
        PropDoc {
            name: ".key(key) -> Outcome",
            ty: "KeyInput",
            default: None,
            description: "Ignored, Scrolled or Closed.",
        },
        PropDoc {
            name: ".mouse_down / .mouse_drag / .mouse_up(col, row)",
            ty: "bool",
            default: None,
            description: "A drag selection: Anchored on press (nothing drawn), Dragging once the pointer moves, Done on release (mouse_up returns true: copy now). Past the top or bottom edge the pager scrolls 3–15 rows; rows are content rows, so the selection stays on its text while scrolling.",
        },
        PropDoc {
            name: ".select_word(col, row)",
            ty: "bool",
            default: None,
            description: "Double-click: the word under the cell (no spaces, brackets or quotes, so emails and paths select whole).",
        },
        PropDoc {
            name: ".selected_text() / .copy_sequence(mux)",
            ty: "Option<String>",
            default: None,
            description: "The selection read from the drawn cells, one line per row, a wide glyph whole; the OSC 52 escape for it (crate::osc52).",
        },
        PropDoc {
            name: ".selection_style(style) / .set_selection(sel) / .selection_rects()",
            ty: "Style / Option<Selection> / [Rect; 3]",
            default: Some("REVERSED"),
            description: "How selected cells are drawn; set or clear a Selection::range((row, col), (row, col)); the highlight rects of the last paint.",
        },
        PropDoc {
            name: "pager::notes(md, &theme)",
            ty: "Vec<Line<'static>>",
            default: None,
            description: "Release-notes markdown: ### headings upper-cased in bold accent, - bullets, `code` without backticks (non-breaking around =), fenced blocks behind a ▐ gutter. notes_with(.., NotesGlyphs::herdr()) for • and ▏.",
        },
        PropDoc {
            name: "WrappedLine(line)",
            ty: "Renderable",
            default: None,
            description: "One wrapping line with an exact, cacheable height and direct scrolling.",
        },
    ],
    keyboard: &[
        ("↑ ↓ / k j", "Scroll one row"),
        (
            "PgUp PgDn / Shift-Space Space / Ctrl-B Ctrl-F",
            "Scroll a page (the rows last shown)",
        ),
        ("Ctrl-U Ctrl-D", "Scroll half a page"),
        ("Home End", "Top or bottom"),
        ("q / Esc / Ctrl-C", "Close"),
    ],
    a11y: &[
        "Position is said in words (42%), not only by a scrollbar.",
        "Rows past the end show ~ so a short document never looks cut off; long lines wrap instead of being clipped.",
        "Default rule glyphs (/ ╌ ~) are East-Asian Narrow; the title is bold and the rules dim, so mono keeps the structure.",
        "The selection is reversed by default, so it shows without colour; the copied text keeps every grapheme whole (flags, skin tones, ZWJ sequences, CJK).",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "transcript",
            title: "A long transcript",
            description: "An agent session, 60-odd lines with long ones wrapping: scroll, page, jump; the bottom rule says how far down you are.",
            cols: 80,
            rows: 18,
            animated: false,
            interactive: true,
            make: transcript,
        },
        DemoDoc {
            id: "short",
            title: "Short text, no scroll",
            description: "Five request headers: everything fits, so the rows past the end show ~ and the rule says 100%.",
            cols: 60,
            rows: 10,
            animated: false,
            interactive: true,
            make: short,
        },
        DemoDoc {
            id: "code",
            title: "Wide code, wrapped",
            description: "Rust lines wider than the pager wrap onto the next row with their indentation kept; narrow the frame to see heights re-measured.",
            cols: 76,
            rows: 20,
            animated: false,
            interactive: true,
            make: code,
        },
        DemoDoc {
            id: "select",
            title: "Select and copy",
            description: "llmux's refresh log with an error picked. Arrows move a stand-in pointer, Enter presses and releases (a drag between), Tab selects the word under it, PgUp/PgDn scroll: the selection stays on its rows. The last line says what OSC 52 would copy.",
            cols: 72,
            rows: 12,
            animated: false,
            interactive: true,
            make: select,
        },
        DemoDoc {
            id: "notes",
            title: "Release notes",
            description: "A changelog through pager::notes: upper-case headings, bullets, inline code, a fenced block behind a gutter, Korean text wrapped by cells.",
            cols: 64,
            rows: 18,
            animated: false,
            interactive: true,
            make: || PagerDemo::boxed("v0.5.0 · what's new", notes_lines),
        },
    ],
};
