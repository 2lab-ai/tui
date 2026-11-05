//! The catalog contract: what every widget ships in its `docs.rs`, and the
//! registry the browser catalog (https://2lab.ai/tui) and the native
//! `tui-catalog` read.
//!
//! It mirrors `@2lab.ai/ui`'s `ComponentDoc`: a one-sentence summary of what the
//! widget tells the person, a source with its licence, the builder methods,
//! keys, accessibility notes, a usage snippet and at least three demos with
//! realistic data. A demo renders into a [`Buffer`] for a given size, theme and
//! time — so the same demo runs in a test, a terminal and a browser.

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::input::KeyInput;
use crate::theme::Theme;

mod registry;

pub use registry::{ORDER, doc_by_slug, docs};

/// Where a widget sits in the catalog's sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Group {
    /// What an agent or a request is doing right now.
    Agent,
    /// Numbers and records: quotas, charts, tables, JSON.
    Data,
    /// What the person types, picks or presses.
    Input,
    /// Building blocks the others share.
    Primitives,
}

impl Group {
    pub const fn label(self) -> &'static str {
        match self {
            Group::Agent => "Agent",
            Group::Data => "Data",
            Group::Input => "Input",
            Group::Primitives => "Primitives",
        }
    }
}

/// Where the widget came from. Every ported widget names its upstream file
/// and licence; the NOTICE file carries the same list.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    /// `llmux`, `codex`, `fx` or `2lab`.
    pub project: &'static str,
    /// A permalink to the code it was taken from (or studied, for a re-write).
    pub url: &'static str,
    pub license: &'static str,
    /// `ported` (code carried over, adapted) or `studied` (behaviour re-written).
    pub how: &'static str,
}

/// One builder method or field.
#[derive(Debug, Clone, Copy)]
pub struct PropDoc {
    pub name: &'static str,
    pub ty: &'static str,
    pub default: Option<&'static str>,
    pub description: &'static str,
}

/// What a demo needs to draw one frame.
#[derive(Debug, Clone, Copy)]
pub struct DemoCx {
    pub theme: Theme,
    /// Time since the demo started. Animated widgets are pure functions of it.
    pub t: Duration,
}

/// A live example. `render` must be deterministic in `(area, cx)` and the keys
/// seen so far — the catalog's contract test renders every demo at several
/// sizes and times.
pub trait Demo {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx);
    /// Returns whether the key changed anything (the page re-renders then).
    fn key(&mut self, _key: KeyInput) -> bool {
        false
    }
}

/// A demo built from a closure — for demos without state.
pub struct FnDemo<F: FnMut(Rect, &mut Buffer, &DemoCx)>(pub F);

impl<F: FnMut(Rect, &mut Buffer, &DemoCx)> Demo for FnDemo<F> {
    fn render(&mut self, area: Rect, buf: &mut Buffer, cx: &DemoCx) {
        (self.0)(area, buf, cx)
    }
}

#[derive(Clone, Copy)]
pub struct DemoDoc {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// Preferred size in cells. The page lets the reader narrow or widen it.
    pub cols: u16,
    pub rows: u16,
    /// Animated: the page drives `t` forward.
    pub animated: bool,
    /// Takes keys: the page forwards them while the frame has focus.
    pub interactive: bool,
    pub make: fn() -> Box<dyn Demo>,
}

#[derive(Clone, Copy)]
pub struct ComponentDoc {
    pub slug: &'static str,
    pub name: &'static str,
    /// The Rust type a host uses.
    pub ty: &'static str,
    /// One sentence: what this widget tells the person.
    pub summary: &'static str,
    pub description: &'static str,
    /// Where it runs in a 2lab app today.
    pub used_in: &'static str,
    pub source: Source,
    pub group: Group,
    pub props: &'static [PropDoc],
    pub keyboard: &'static [(&'static str, &'static str)],
    pub a11y: &'static [&'static str],
    pub usage: &'static str,
    pub demos: &'static [DemoDoc],
}

impl ComponentDoc {
    pub fn demo(&self, id: &str) -> Option<&DemoDoc> {
        self.demos.iter().find(|d| d.id == id)
    }
}

/// Render one frame of a demo into a fresh buffer of `cols × rows`.
pub fn render_demo(demo: &mut dyn Demo, cols: u16, rows: u16, cx: &DemoCx) -> Buffer {
    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    demo.render(area, &mut buf, cx);
    buf
}

/// The whole catalog as JSON (everything but the demo functions), for the page.
pub fn catalog_json() -> String {
    use crate::snapshot::json_str;
    use std::fmt::Write;
    let mut o = String::with_capacity(64 * 1024);
    o.push_str("{\"version\":");
    json_str(env!("CARGO_PKG_VERSION"), &mut o);
    o.push_str(",\"components\":[");
    for (i, d) in docs().iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push('{');
        for (k, v) in [
            ("slug", d.slug),
            ("name", d.name),
            ("type", d.ty),
            ("summary", d.summary),
            ("description", d.description),
            ("usedIn", d.used_in),
            ("group", d.group.label()),
            ("usage", d.usage),
        ] {
            json_str(k, &mut o);
            o.push(':');
            json_str(v, &mut o);
            o.push(',');
        }
        o.push_str("\"source\":{");
        for (j, (k, v)) in [
            ("project", d.source.project),
            ("url", d.source.url),
            ("license", d.source.license),
            ("how", d.source.how),
        ]
        .into_iter()
        .enumerate()
        {
            if j > 0 {
                o.push(',');
            }
            json_str(k, &mut o);
            o.push(':');
            json_str(v, &mut o);
        }
        o.push_str("},\"props\":[");
        for (j, p) in d.props.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str("{\"name\":");
            json_str(p.name, &mut o);
            o.push_str(",\"type\":");
            json_str(p.ty, &mut o);
            o.push_str(",\"default\":");
            match p.default {
                Some(v) => json_str(v, &mut o),
                None => o.push_str("null"),
            }
            o.push_str(",\"description\":");
            json_str(p.description, &mut o);
            o.push('}');
        }
        o.push_str("],\"keyboard\":[");
        for (j, (k, a)) in d.keyboard.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push('[');
            json_str(k, &mut o);
            o.push(',');
            json_str(a, &mut o);
            o.push(']');
        }
        o.push_str("],\"a11y\":[");
        for (j, a) in d.a11y.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            json_str(a, &mut o);
        }
        o.push_str("],\"demos\":[");
        for (j, m) in d.demos.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str("{\"id\":");
            json_str(m.id, &mut o);
            o.push_str(",\"title\":");
            json_str(m.title, &mut o);
            o.push_str(",\"description\":");
            json_str(m.description, &mut o);
            let _ = write!(
                o,
                ",\"cols\":{},\"rows\":{},\"animated\":{},\"interactive\":{}}}",
                m.cols, m.rows, m.animated, m.interactive
            );
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    o
}

#[cfg(test)]
mod tests;
