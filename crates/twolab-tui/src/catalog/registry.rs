use super::ComponentDoc;
use crate::widgets;

/// Sidebar order: Agent, then Data, Input and Primitives (generated from the
/// widget folders; within a group the lead widgets first).
pub const ORDER: &[&str] = &[
    "working",
    "shimmer",
    "status-bar",
    "approval-card",
    "banner",
    "meter",
    "mini-bar",
    "ledger",
    "status-card",
    "trend-chart",
    "packed-grid",
    "fact-table",
    "json-view",
    "diff-view",
    "composer",
    "choice-form",
    "select-list",
    "multi-select",
    "completion-menu",
    "key-hints",
    "tabs",
    "menu",
    "layout",
    "pager",
    "rule",
    "spinner",
    "gradient-text",
];

static DOCS: &[&ComponentDoc] = &[
    &widgets::working::docs::DOC,
    &widgets::shimmer::docs::DOC,
    &widgets::status_bar::docs::DOC,
    &widgets::approval_card::docs::DOC,
    &widgets::banner::docs::DOC,
    &widgets::meter::docs::DOC,
    &widgets::mini_bar::docs::DOC,
    &widgets::ledger::docs::DOC,
    &widgets::status_card::docs::DOC,
    &widgets::trend_chart::docs::DOC,
    &widgets::packed_grid::docs::DOC,
    &widgets::fact_table::docs::DOC,
    &widgets::json_view::docs::DOC,
    &widgets::diff_view::docs::DOC,
    &widgets::composer::docs::DOC,
    &widgets::choice_form::docs::DOC,
    &widgets::select_list::docs::DOC,
    &widgets::multi_select::docs::DOC,
    &widgets::completion_menu::docs::DOC,
    &widgets::key_hints::docs::DOC,
    &widgets::tabs::docs::DOC,
    &widgets::menu::docs::DOC,
    &widgets::layout::docs::DOC,
    &widgets::pager::docs::DOC,
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
