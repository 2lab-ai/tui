//! Every widget module. Each has `mod.rs` (the widget) and, behind the
//! `catalog` feature, `docs.rs` (its catalog entry).

/// The catalog page for [`crate::render`]; no widget type of its own.
pub(crate) mod layout;
pub mod spinner;
