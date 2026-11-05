use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: agent first, then data, input, primitives.
pub const ORDER: &[&str] = &[
    // Agent
    "approval-card",
    // Data
    "diff-view",
    // Input
    "choice-form",
    "composer",
    // Primitives
    "spinner",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::approval_card::docs::DOC,
    &widgets::choice_form::docs::DOC,
    &widgets::composer::docs::DOC,
    &widgets::diff_view::docs::DOC,
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
