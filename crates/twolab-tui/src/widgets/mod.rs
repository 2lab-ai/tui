//! Every widget module. Each has `mod.rs` (the widget) and, behind the
//! `catalog` feature, `docs.rs` (its catalog entry).

pub mod approval_card;
pub mod banner;
pub mod choice_form;
pub mod completion_menu;
pub mod diff_view;
pub mod fact_table;
#[path = "gradient-text/mod.rs"]
pub mod gradient_text;
pub mod json_view;
pub mod key_hints;
pub mod layout;
pub mod ledger;
pub mod menu;
pub mod meter;
#[path = "mini-bar/mod.rs"]
pub mod mini_bar;
pub mod multi_select;
pub mod packed_grid;
pub mod rule;
pub mod select_list;
pub mod shimmer;
pub mod spinner;
pub mod status_bar;
pub mod status_card;
pub mod tabs;
pub mod trend_chart;
pub mod working;
