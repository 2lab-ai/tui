use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

use super::{Outcome, Pager};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::render::{FlexRenderable, Renderable, RenderableItem};
use crate::theme::Theme;

const USAGE: &str = r#"use twolab_tui::pager::{Outcome, Pager};

let mut pager = Pager::from_text("Request body", &pretty_json).theme(theme);

match pager.key(key) {
    Outcome::Closed => close_overlay(),
    Outcome::Scrolled => redraw(),
    Outcome::Ignored => {}
}
frame.render_widget(&pager, frame.area());"#;

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

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "pager",
    name: "Pager",
    ty: "twolab_tui::pager::Pager",
    summary: "The whole text, and where you are in it.",
    description: "Codex's pager overlay: a title rule, the content with long lines wrapped (nothing cut), ~ on rows past the end, and a bottom rule that says how far down you are as a percentage. Line heights are measured exactly once per width and cached, so a long transcript scrolls without re-wrapping every frame; any stack of Renderables can be paged too.",
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
    ],
};
