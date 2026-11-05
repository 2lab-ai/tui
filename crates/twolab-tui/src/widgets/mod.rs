//! Every widget module. Each has `mod.rs` (the widget) and, behind the
//! `catalog` feature, `docs.rs` (its catalog entry).

pub mod meter;
#[path = "mini-bar/mod.rs"]
pub mod mini_bar;
pub mod spinner;
