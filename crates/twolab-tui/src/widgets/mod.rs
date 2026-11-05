//! Every widget module. Each has `mod.rs` (the widget) and, behind the
//! `catalog` feature, `docs.rs` (its catalog entry).

#[path = "gradient-text/mod.rs"]
pub mod gradient_text;
pub mod ledger;
pub mod meter;
#[path = "mini-bar/mod.rs"]
pub mod mini_bar;
pub mod spinner;
