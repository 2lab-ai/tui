use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use super::{MultiItem, MultiSelect, Outcome};
use crate::catalog::{ComponentDoc, Demo, DemoCx, DemoDoc, Group, PropDoc, Source};
use crate::input::KeyInput;
use crate::render::{FlexRenderable, Renderable, RenderableItem};
use crate::theme::Status;

const USAGE: &str = r#"use twolab_tui::multi_select::{MultiItem, MultiSelect, Outcome};

let mut picker = MultiSelect::new(vec![
    MultiItem::new("zhuge", "zhuge@2lab.ai").description("Max 20x, 5h 42%").checked(true),
    MultiItem::new("ai3", "ai3@iq.io").description("Pro, 5h 88%"),
])
.title("Accounts in rotation")
.require_selection()
.reorderable()
.theme(theme);

match picker.key(key) {
    Outcome::Applied(ids) => save_rotation(&ids),
    Outcome::Cancelled => close(),
    _ => {}
}
frame.render_widget(&picker, area);"#;

/// A picker and what the last apply or cancel said.
struct PickDemo {
    picker: MultiSelect,
    status: Option<(String, bool)>,
}

impl Demo for PickDemo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        self.picker.set_theme(cx.theme);
        let mut col = FlexRenderable::new();
        col.push(1, RenderableItem::borrowed(&self.picker));
        if let Some((msg, applied)) = &self.status {
            let style = if *applied {
                cx.theme.status(Status::Done)
            } else {
                cx.theme.dim()
            };
            col.push(
                0,
                RenderableItem::owned(Line::styled(format!("  {msg}"), style)),
            );
        }
        col.render(area, buf);
    }

    fn key(&mut self, key: KeyInput) -> bool {
        match self.picker.key(key) {
            Outcome::Applied(ids) => {
                let msg = if ids.is_empty() {
                    "applied: none".to_string()
                } else {
                    format!("applied: {}", ids.join(", "))
                };
                self.status = Some((msg, true));
                true
            }
            Outcome::Cancelled => {
                self.status = Some(("cancelled, nothing changed".into(), false));
                true
            }
            Outcome::Changed => {
                self.status = None;
                true
            }
            Outcome::Ignored => false,
        }
    }
}

fn accounts() -> Box<dyn Demo> {
    let items = vec![
        MultiItem::new("zhuge", "zhuge@2lab.ai")
            .description("Max 20x, 5h 42%, 7d 18%")
            .checked(true),
        MultiItem::new("simayi", "simayi@2lab.ai")
            .description("Max 5x, 5h 7%, 7d 31%")
            .checked(true),
        MultiItem::new("ai3", "ai3@iq.io").description("Pro, 5h 88%, 7d 64%"),
        MultiItem::new("dev1", "dev1@2lab.ai").description("Pro, cooling down, resets in 1h 12m"),
        MultiItem::new("ai2", "ai2@iq.io")
            .description("Max 5x, 7d 96%, resets Monday 09:00")
            .checked(true),
        MultiItem::new("ops", "ops@2lab.ai (API key)")
            .description("pay as you go, $41.20 this month")
            .pinned(),
    ];
    Box::new(PickDemo {
        picker: MultiSelect::new(items)
            .title("Accounts in rotation")
            .subtitle("Requests go to the first account with room, top to bottom.")
            .require_selection()
            .reorderable(),
        status: None,
    })
}

fn tool_items() -> Vec<MultiItem> {
    [
        ("bash", "Run a shell command in the workspace", true),
        ("read_file", "Read a file, or a range of lines", true),
        ("write_file", "Create or overwrite a file", true),
        ("edit_file", "Replace an exact string in a file", true),
        ("list_dir", "List a directory", true),
        ("glob", "Find files by pattern", true),
        ("grep", "Search file contents with a regex", true),
        ("web_search", "Search the web, with citations", false),
        (
            "web_fetch",
            "Fetch a URL that is already in the conversation",
            false,
        ),
        ("code_execution", "Run Python in a sandbox", false),
        ("memory", "Read and write the memory directory", false),
        ("todo_write", "Keep a task list for this session", true),
        ("ask_user", "Ask the person a question and wait", true),
        ("notebook_edit", "Edit a Jupyter notebook cell", false),
    ]
    .into_iter()
    .map(|(name, desc, on)| MultiItem::new(name, name).description(desc).checked(on))
    .collect()
}

fn tools() -> Box<dyn Demo> {
    Box::new(PickDemo {
        picker: MultiSelect::new(tool_items())
            .title("Tools for the review agent")
            .placeholder("Type to filter tools"),
        status: None,
    })
}

fn filter() -> Box<dyn Demo> {
    Box::new(PickDemo {
        picker: MultiSelect::new(tool_items())
            .title("Tools for the review agent")
            .with_query("fe"),
        status: None,
    })
}

pub(crate) const DOC: ComponentDoc = ComponentDoc {
    slug: "multi-select",
    name: "Multi-select",
    ty: "twolab_tui::multi_select::MultiSelect",
    summary: "Which of these to include — and how many are in, even while a filter hides some.",
    description: "Codex's multi-select picker: `› [x] name   description` rows with an aligned description column, a fuzzy filter (case-insensitive subsequence, ported with its tests) that sorts by match quality and highlights the matched characters, Space to toggle, Enter to apply, Esc to cancel, a `k of n selected` line, an optional require-one rule that says why Enter did nothing, and optional ←/→ reordering with pinned items.",
    used_in: "Codex's status-line and feature pickers. llmux's keys picker (accounts in rotation) is the planned host.",
    source: Source {
        project: "codex",
        url: "https://github.com/openai/codex/blob/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui/src/bottom_pane/multi_select_picker.rs#L119-L859",
        license: "Apache-2.0",
        how: "ported",
    },
    group: Group::Input,
    props: &[
        PropDoc {
            name: "MultiSelect::new(items)",
            ty: "Vec<MultiItem>",
            default: None,
            description: "MultiItem::new(id, name).description(d).checked(bool).pinned().",
        },
        PropDoc {
            name: ".title(s) / .subtitle(s) / .placeholder(s)",
            ty: "impl Into<String>",
            default: Some("\"Type to search\""),
            description: "Header lines and the empty filter's placeholder.",
        },
        PropDoc {
            name: ".require_selection()",
            ty: "",
            default: Some("off"),
            description: "Refuse Enter while nothing is checked, and say so; Esc still cancels.",
        },
        PropDoc {
            name: ".reorderable()",
            ty: "",
            default: Some("off"),
            description: "←/→ (Ctrl-H/L) move the highlighted item while the filter is empty; pinned items stay put.",
        },
        PropDoc {
            name: ".with_query(s) / .set_search_query(s)",
            ty: "impl Into<String>",
            default: Some("\"\""),
            description: "Filter text: fuzzy subsequence on the name, best match first.",
        },
        PropDoc {
            name: ".max_rows(n) / .row_display(d) / .glyphs(g) / .theme(t)",
            ty: "usize / RowDisplay / SelectGlyphs / Theme",
            default: Some("8 / SingleLine / › ▴ ▾"),
            description: "Rows at once, wrapped or single-line descriptions, cursor and overflow glyphs, colours.",
        },
        PropDoc {
            name: ".key(key) -> Outcome",
            ty: "KeyInput",
            default: None,
            description: "Ignored, Changed, Applied(checked ids in list order) or Cancelled.",
        },
        PropDoc {
            name: "fuzzy_match(haystack, needle)",
            ty: "-> Option<(Vec<usize>, i32)>",
            default: None,
            description: "The matcher on its own: matched char positions and a score, smaller is better.",
        },
    ],
    keyboard: &[
        ("↑ ↓ / Ctrl-P Ctrl-N", "Move, wrapping at the ends"),
        (
            "PgUp PgDn / Ctrl-B Ctrl-F",
            "Move a page, stopping at the ends",
        ),
        ("Home End", "First or last row"),
        ("Space", "Check or uncheck the highlighted row"),
        ("Enter", "Apply: return the checked ids"),
        ("Esc", "Cancel without applying"),
        (
            "← → / Ctrl-H Ctrl-L",
            "Move the highlighted item up or down (reorderable, no filter)",
        ),
        (
            "typing, Backspace",
            "Edit the filter (letters never move the cursor)",
        ),
    ],
    a11y: &[
        "Checked is a glyph ([x] / [ ]), not only a colour; the highlighted row also carries the › cursor and the theme's selection.",
        "Matched characters are bold (and underlined on the highlighted row), so the highlight survives NO_COLOR.",
        "The k of n selected line counts hidden rows too, and says how many the filter hides.",
        "A refused Enter says why (choose at least one to apply) instead of doing nothing.",
    ],
    usage: USAGE,
    demos: &[
        DemoDoc {
            id: "accounts",
            title: "Accounts in rotation",
            description: "Choose which accounts take requests and in what order (left/right). The API-key account is pinned last; at least one must stay checked.",
            cols: 80,
            rows: 14,
            animated: false,
            interactive: true,
            make: accounts,
        },
        DemoDoc {
            id: "tools",
            title: "Choose tools",
            description: "Fourteen tools, eight at a time: Space toggles, ▴/▾ and N of M show the rest.",
            cols: 76,
            rows: 15,
            animated: false,
            interactive: true,
            make: tools,
        },
        DemoDoc {
            id: "filter",
            title: "Filter with matches",
            description: "\"fe\" matches web_fetch contiguously, so it sorts first; read_file, write_file and edit_file follow with their matched letters highlighted.",
            cols: 76,
            rows: 12,
            animated: false,
            interactive: true,
            make: filter,
        },
    ],
};
