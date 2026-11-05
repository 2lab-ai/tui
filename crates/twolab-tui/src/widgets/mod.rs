//! Every widget module. Each has `mod.rs` (the widget) and, behind the
//! `catalog` feature, `docs.rs` (its catalog entry).

pub mod approval_card;
pub mod choice_form;
pub mod diff_view;
#[path = "gradient-text/mod.rs"]
pub mod gradient_text;
pub mod layout;
pub mod menu;
pub mod meter;
#[path = "mini-bar/mod.rs"]
pub mod mini_bar;
pub mod rule;
pub mod shimmer;
pub mod spinner;
pub mod status_bar;
pub mod tabs;
pub mod working;
