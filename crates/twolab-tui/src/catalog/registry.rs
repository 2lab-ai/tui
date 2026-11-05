use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: agent first, then data, input, primitives.
#[rustfmt::skip]
pub const ORDER: &[&str] = &[
    // Data
    "json-view",
    "packed-grid",
    "trend-chart",
    // Input
    "tabs",
    "menu",
    // Primitives
    "spinner",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::json_view::docs::DOC,
    &widgets::menu::docs::DOC,
    &widgets::packed_grid::docs::DOC,
    &widgets::spinner::docs::DOC,
    &widgets::tabs::docs::DOC,
    &widgets::trend_chart::docs::DOC,
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
