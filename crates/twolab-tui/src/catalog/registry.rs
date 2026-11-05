use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: Agent, then Data, Input and Primitives (generated from the
/// widget folders; within a group the lead widgets first).
pub const ORDER: &[&str] = &[
    "working",
    "shimmer",
    "status-bar",
    "meter",
    "mini-bar",
    "diff-view",
    "tabs",
    "menu",
    "layout",
    "rule",
    "spinner",
    "gradient-text",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::working::docs::DOC,
    &widgets::shimmer::docs::DOC,
    &widgets::status_bar::docs::DOC,
    &widgets::meter::docs::DOC,
    &widgets::mini_bar::docs::DOC,
    &widgets::diff_view::docs::DOC,
    &widgets::tabs::docs::DOC,
    &widgets::menu::docs::DOC,
    &widgets::layout::docs::DOC,
    &widgets::rule::docs::DOC,
    &widgets::spinner::docs::DOC,
    &widgets::gradient_text::docs::DOC,
];

/// Every component's doc, in [`ORDER`].
pub fn docs() -> Vec<&'static ComponentDoc> {
    let mut v: Vec<&'static ComponentDoc> = DOCS.to_vec();
    v.sort_by_key(|d| {
        ORDER
            .iter()
            .position(|s| *s == d.slug)
            .unwrap_or(usize::MAX)
    });
    v
}

pub fn doc_by_slug(slug: &str) -> Option<&'static ComponentDoc> {
    DOCS.iter().copied().find(|d| d.slug == slug)
}
