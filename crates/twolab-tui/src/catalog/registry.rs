use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: agent first, then data, input, primitives.
pub const ORDER: &[&str] = &[
    "status-bar",
    "fact-table",
    "completion-menu",
    "rule",
    "spinner",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::status_bar::docs::DOC,
    &widgets::fact_table::docs::DOC,
    &widgets::completion_menu::docs::DOC,
    &widgets::rule::docs::DOC,
    &widgets::spinner::docs::DOC,
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
