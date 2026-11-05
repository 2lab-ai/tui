use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: agent first, then data, input, primitives.
#[rustfmt::skip]
pub const ORDER: &[&str] = &[
    // Input
    "tabs",
    "menu",
    // Primitives
    "spinner",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::menu::docs::DOC,
    &widgets::spinner::docs::DOC,
    &widgets::tabs::docs::DOC,
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
