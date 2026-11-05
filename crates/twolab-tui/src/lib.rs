//! # twolab-tui
//!
//! Terminal UI components for [ratatui](https://ratatui.rs) — the terminal
//! sibling of [`@2lab.ai/ui`](https://2lab.ai/ui). Each widget comes from a 2lab
//! app (llmux) or from a proven open-source agent TUI (OpenAI Codex, Vercel fx),
//! with its source and licence named in its docs and in `NOTICE`.
//!
//! Catalog with live demos: <https://2lab.ai/tui>.
//!
//! Rules every widget follows:
//! - Data in, a buffer out. Widgets never read a clock or a terminal: time
//!   comes in as `elapsed` (or a shared tick), size as the `Rect`.
//! - Colour comes from a [`Theme`]. [`Theme::mono`] (`NO_COLOR`) draws no
//!   colour at all and keeps meaning in weight and glyphs.
//! - Width is measured in terminal cells, by grapheme ([`text`]), the same
//!   unit ratatui renders.
//! - Animated widgets hold a still frame under [`Motion::Reduced`].

pub mod input;
pub mod render;
pub mod snapshot;
pub mod text;
pub mod theme;
mod widgets;

#[cfg(feature = "catalog")]
pub mod catalog;

pub use input::{Key, KeyInput, Mods};
pub use theme::{Level, Motion, Palette, Status, Theme};
pub use widgets::*;
