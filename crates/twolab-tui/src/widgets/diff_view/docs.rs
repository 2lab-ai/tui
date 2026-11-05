use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use super::{DEFAULT_CONTEXT, DiffView, FileChange, Hunk, diff_lines, hunks};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, FnDemo, Group, PropDoc, Source};
use crate::input::{Key, KeyInput};

const USAGE: &str = r#"use twolab_tui::diff_view::{diff_lines, hunks, DiffView, DEFAULT_CONTEXT};

// Bring your own hunks, or compute them from two texts.
let hunks = hunks(&diff_lines(&before, &after), DEFAULT_CONTEXT);

let view = DiffView::new(&hunks)
    .path("src/proxy/retry.rs")
    .scroll(offset) // body rows scrolled off the top
    .theme(theme);
let max = view.max_scroll(area); // clamp your offset with this
frame.render_widget(view, area);"#;

const RETRY_OLD: &str = r#"use std::time::Duration;

use crate::upstream::{Upstream, UpstreamError};

/// How long to wait before the first retry.
const BASE_DELAY: Duration = Duration::from_millis(200);

/// Retries a request on a 429 or a 5xx.
pub struct Retry {
    attempts: u32,
    max_attempts: u32,
}

impl Retry {
    pub fn new(max_attempts: u32) -> Retry {
        Retry {
            attempts: 0,
            max_attempts,
        }
    }

    pub fn should_retry(&self, err: &UpstreamError) -> bool {
        if self.attempts >= self.max_attempts {
            return false;
        }
        matches!(err.status(), Some(429) | Some(500..=599))
    }

    pub fn delay(&self) -> Duration {
        BASE_DELAY * 2u32.pow(self.attempts)
    }

    pub fn record(&mut self) {
        self.attempts += 1;
    }
}

pub fn send_with_retry(up: &Upstream, body: &[u8]) -> Result<Vec<u8>, UpstreamError> {
    let mut retry = Retry::new(3);
    loop {
        match up.send(body) {
            Ok(resp) => return Ok(resp),
            Err(e) if retry.should_retry(&e) => {
                std::thread::sleep(retry.delay());
                retry.record();
            }
            Err(e) => return Err(e),
        }
    }
}
"#;

const RETRY_NEW: &str = r#"use std::time::Duration;

use crate::upstream::{Upstream, UpstreamError};

/// How long to wait before the first retry.
const BASE_DELAY: Duration = Duration::from_millis(200);

/// Never wait longer than this between two attempts.
const MAX_DELAY: Duration = Duration::from_secs(8);

/// Retries a request on a 429 or a 5xx.
pub struct Retry {
    attempts: u32,
    max_attempts: u32,
}

impl Retry {
    pub fn new(max_attempts: u32) -> Retry {
        Retry {
            attempts: 0,
            max_attempts,
        }
    }

    pub fn should_retry(&self, err: &UpstreamError) -> bool {
        if self.attempts >= self.max_attempts {
            return false;
        }
        matches!(err.status(), Some(408) | Some(429) | Some(500..=599))
    }

    pub fn delay(&self, retry_after: Option<Duration>) -> Duration {
        let backoff = BASE_DELAY.saturating_mul(2u32.saturating_pow(self.attempts));
        retry_after.unwrap_or(backoff).min(MAX_DELAY)
    }

    pub fn record(&mut self) {
        self.attempts += 1;
    }
}

pub fn send_with_retry(up: &Upstream, body: &[u8]) -> Result<Vec<u8>, UpstreamError> {
    let mut retry = Retry::new(3);
    loop {
        match up.send(body) {
            Ok(resp) => return Ok(resp),
            Err(e) if retry.should_retry(&e) => {
                std::thread::sleep(retry.delay(e.retry_after()));
                retry.record();
            }
            Err(e) => return Err(e),
        }
    }
}
"#;

/// The scrolling demo: a three-hunk change, ↑↓ PgUp PgDn Home End.
struct Scroll {
    hunks: Vec<Hunk>,
    offset: usize,
    max: usize,
    page: usize,
}

impl Scroll {
    fn new() -> Scroll {
        Scroll {
            hunks: hunks(&diff_lines(RETRY_OLD, RETRY_NEW), DEFAULT_CONTEXT),
            offset: 0,
            max: usize::MAX,
            page: 10,
        }
    }
}

impl Demo for Scroll {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        let view = DiffView::new(&self.hunks)
            .path("src/proxy/retry.rs")
            .theme(cx.theme);
        self.max = view.max_scroll(area);
        self.page = (area.height as usize).saturating_sub(2).max(1);
        self.offset = self.offset.min(self.max);
        view.scroll(self.offset).render(area, buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        let before = self.offset;
        self.offset = match key.key {
            Key::Up => self.offset.saturating_sub(1),
            Key::Down => self.offset.saturating_add(1),
            Key::PageUp => self.offset.saturating_sub(self.page),
            Key::PageDown => self.offset.saturating_add(self.page),
            Key::Home => 0,
            Key::End => self.max,
            _ => return false,
        }
        .min(self.max);
        self.offset != before
    }
}

fn small_edit(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let old = "pub fn session_ttl(cfg: &Config) -> Duration {\n    let hours = cfg.session_hours.unwrap_or(12);\n    Duration::from_secs(hours * 60 * 60)\n}\n";
    let new = "pub fn session_ttl(cfg: &Config) -> Duration {\n    let hours = cfg.session_hours.unwrap_or(24).min(72);\n    Duration::from_secs(hours * 60 * 60)\n}\n";
    let mut hs = hunks(&diff_lines(old, new), DEFAULT_CONTEXT);
    // Line numbers as they sit in the file: the function starts at line 41.
    for h in &mut hs {
        h.old_start += 40;
        h.new_start += 40;
        for l in &mut h.lines {
            l.old_no = l.old_no.map(|n| n + 40);
            l.new_no = l.new_no.map(|n| n + 40);
        }
        h.section = Some("impl Config".into());
    }
    DiffView::new(&hs)
        .path("src/auth/session.rs")
        .theme(cx.theme)
        .render(area, buf);
}

fn new_file(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let new = "name: ci\non:\n  push:\n    branches: [main]\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: cargo test --workspace\n";
    let hs = hunks(&diff_lines("", new), DEFAULT_CONTEXT);
    DiffView::new(&hs)
        .path(".github/workflows/ci.yml")
        .change(FileChange::Added)
        .theme(cx.theme)
        .render(area, buf);
}

fn rename_narrow(area: Rect, buf: &mut Buffer, cx: &DemoCx) {
    let old = "use crate::retry::Retry;\n\npub struct Client {\n    base: String,\n}\n";
    let new = "use crate::proxy::retry::Retry;\n\npub struct Client {\n    base: Url,\n}\n";
    let hs = hunks(&diff_lines(old, new), DEFAULT_CONTEXT);
    DiffView::new(&hs)
        .path("src/net/client.rs")
        .renamed_from("src/client.rs")
        .theme(cx.theme)
        .render(area, buf);
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "diff-view",
    name: "Diff view",
    ty: "twolab_tui::diff_view::DiffView",
    summary: "What changed in a file, line by line, and how much — lines added and removed.",
    description: "Unified hunks under a one-row summary: each hunk opens with its @@ header, each line shows its number, a + or - sign and its text, additions in the done colour and deletions in the fail colour (bold and dim under NO_COLOR). Long lines are clipped by cells with an ellipsis and file text is shown inert. The body scrolls under the summary, which says which rows are on screen; too narrow for code, the view becomes the summary and says how many lines it hid.",
    used_in: "Agent file-change review before an edit is applied; a config change in a dashboard before it is saved.",
    source: Source {
        project: "fx",
        url: "https://github.com/vercel-labs/fx/blob/412d841d73dd6e1b8de551022728d9c929caae17/src/core/output/diff.zig#L794-L857",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Data,
    props: &[
        PropDoc {
            name: "DiffView::new(hunks)",
            ty: "&[Hunk]",
            default: None,
            description: "Hunks of DiffLine { kind, old_no, new_no, text }; the widget never diffs.",
        },
        PropDoc {
            name: ".path(p)",
            ty: "impl Into<Cow<str>>",
            default: Some("none"),
            description: "Shown on the summary row, shortened from the left so the file name stays.",
        },
        PropDoc {
            name: ".renamed_from(p) / .change(c)",
            ty: "Cow<str> / FileChange",
            default: Some("Modified"),
            description: "A rename, a new file or a deleted file, said on the summary row.",
        },
        PropDoc {
            name: ".scroll(rows)",
            ty: "usize",
            default: Some("0"),
            description: "Body rows scrolled off the top; clamp with .max_scroll(area).",
        },
        PropDoc {
            name: ".summary(on) / .summary_only(on)",
            ty: "bool",
            default: Some("true / false"),
            description: "The sticky summary row; or nothing but it.",
        },
        PropDoc {
            name: "diff_lines(old, new) / hunks(lines, context)",
            ty: "fn -> Vec<DiffLine> / Vec<Hunk>",
            default: None,
            description: "A small line-level LCS and the grouping into hunks, for hosts without a diff library.",
        },
        PropDoc {
            name: ".lines(width)",
            ty: "Vec<Line<'static>>",
            default: None,
            description: "The body rows as ratatui text, clipped to width.",
        },
    ],
    keyboard: &[
        ("↑ / ↓", "Scroll one row (host-driven; the demo shows it)"),
        ("PgUp / PgDn", "Scroll a page"),
        ("Home / End", "First or last row"),
    ],
    a11y: &[
        "Every line carries a + or - sign, so additions and deletions read without colour; under NO_COLOR additions are bold and deletions dim.",
        "Clipped lines end in …, the summary says which rows are on screen, and a narrow view says how many lines it hid.",
        "File text is inert: tabs become spaces and control or bidirectional characters are spelled out (\\x1b, \\u{202e}).",
        "The line-number column is sized to the widest number in the whole diff, so scrolling never shifts the text.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "scroll",
            title: "Review a change",
            description: "Three hunks in a retry module; scroll with ↑↓ PgUp PgDn.",
            cols: 78,
            rows: 14,
            animated: false,
            interactive: true,
            make: || Box::new(Scroll::new()),
        },
        DemoDoc {
            id: "edit",
            title: "A small edit",
            description: "One line changed, numbered where it sits in the file.",
            cols: 72,
            rows: 7,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(small_edit)),
        },
        DemoDoc {
            id: "new-file",
            title: "A new file",
            description: "Every line added; the summary says it is new.",
            cols: 60,
            rows: 13,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(new_file)),
        },
        DemoDoc {
            id: "rename-narrow",
            title: "Rename, narrow",
            description: "Too narrow for code: the summary alone, and how much it hid. Widen it to see the diff.",
            cols: 26,
            rows: 3,
            animated: false,
            interactive: false,
            make: || Box::new(FnDemo(rename_narrow)),
        },
    ],
};
