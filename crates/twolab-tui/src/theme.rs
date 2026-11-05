//! Semantic colours and motion, shared by every widget.
//!
//! The token names follow `@2lab.ai/ui` (`text`, `text-secondary`, `border`,
//! `accent`, `st-run|wait|done|fail`), so a dashboard and its terminal sibling
//! say the same thing in the same colour.
//!
//! [`Theme::ansi`] is the default. It uses the 16 named terminal colours, so the
//! user's own terminal palette decides the actual hues and a light or dark
//! background both work without detection. [`Theme::dark`] and
//! [`Theme::light`] pin true-colour values for hosts that know their background.
//! [`Theme::mono`] draws no colour at all — what `NO_COLOR` asks for — and keeps
//! meaning in weight (bold, dim, reversed) and glyphs instead.

use ratatui::style::{Color, Modifier, Style};

/// Whether widgets animate. `Reduced` keeps every animated widget on one
/// still frame that still says the same thing (a spinner becomes its first
/// glyph, a shimmer becomes plain bold text).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    #[default]
    Full,
    Reduced,
}

/// How far into the red a value is. Gauges, quota bars and heat cells pick
/// their colour from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Ok,
    Warn,
    Crit,
}

impl Level {
    /// The level of a used fraction (0.0–1.0) against two thresholds.
    /// `warn_at` ≤ `crit_at`; a non-finite value reads as [`Level::Ok`].
    pub fn of(used: f64, warn_at: f64, crit_at: f64) -> Level {
        if !used.is_finite() {
            Level::Ok
        } else if used >= crit_at {
            Level::Crit
        } else if used >= warn_at {
            Level::Warn
        } else {
            Level::Ok
        }
    }
}

/// State of a task, run or request — the four `@2lab.ai/ui` states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Status {
    Running,
    Waiting,
    Done,
    Failed,
}

/// Which palette a theme was built from. Hosts rarely need this; the catalog
/// shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Palette {
    Ansi,
    Dark,
    Light,
    Mono,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub palette: Palette,
    /// Body text.
    pub text: Color,
    /// Labels, secondary facts.
    pub text_secondary: Color,
    /// Hints, separators, empty parts of a bar.
    pub muted: Color,
    /// Frame lines.
    pub border: Color,
    /// The one colour that says "this is the thing to look at".
    pub accent: Color,
    /// Background of a raised surface (popups, the selected row).
    pub surface_raised: Color,
    pub run: Color,
    pub wait: Color,
    pub done: Color,
    pub fail: Color,
    pub ok: Color,
    pub warn: Color,
    pub crit: Color,
    pub motion: Motion,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::ansi()
    }
}

impl Theme {
    /// The 16 named terminal colours: the user's palette decides the hues.
    pub const fn ansi() -> Theme {
        Theme {
            palette: Palette::Ansi,
            text: Color::Reset,
            text_secondary: Color::Gray,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            accent: Color::Cyan,
            surface_raised: Color::Reset,
            run: Color::Cyan,
            wait: Color::Yellow,
            done: Color::Green,
            fail: Color::Red,
            ok: Color::Green,
            warn: Color::Yellow,
            crit: Color::Red,
            motion: Motion::Full,
        }
    }

    /// True colour for a dark background.
    pub const fn dark() -> Theme {
        Theme {
            palette: Palette::Dark,
            text: Color::Rgb(0xe6, 0xe6, 0xe9),
            text_secondary: Color::Rgb(0xa1, 0xa1, 0xaa),
            muted: Color::Rgb(0x63, 0x63, 0x6e),
            border: Color::Rgb(0x3f, 0x3f, 0x46),
            accent: Color::Rgb(0x7d, 0xd3, 0xfc),
            surface_raised: Color::Rgb(0x27, 0x27, 0x2a),
            run: Color::Rgb(0x60, 0xa5, 0xfa),
            wait: Color::Rgb(0xfb, 0xbf, 0x24),
            done: Color::Rgb(0x4a, 0xde, 0x80),
            fail: Color::Rgb(0xf8, 0x71, 0x71),
            ok: Color::Rgb(0x4a, 0xde, 0x80),
            warn: Color::Rgb(0xfb, 0xbf, 0x24),
            crit: Color::Rgb(0xf8, 0x71, 0x71),
            motion: Motion::Full,
        }
    }

    /// True colour for a light background.
    pub const fn light() -> Theme {
        Theme {
            palette: Palette::Light,
            text: Color::Rgb(0x18, 0x18, 0x1b),
            text_secondary: Color::Rgb(0x52, 0x52, 0x5b),
            muted: Color::Rgb(0xa1, 0xa1, 0xaa),
            border: Color::Rgb(0xd4, 0xd4, 0xd8),
            accent: Color::Rgb(0x02, 0x84, 0xc7),
            surface_raised: Color::Rgb(0xf4, 0xf4, 0xf5),
            run: Color::Rgb(0x25, 0x63, 0xeb),
            wait: Color::Rgb(0xb4, 0x53, 0x09),
            done: Color::Rgb(0x15, 0x80, 0x3d),
            fail: Color::Rgb(0xdc, 0x26, 0x26),
            ok: Color::Rgb(0x15, 0x80, 0x3d),
            warn: Color::Rgb(0xb4, 0x53, 0x09),
            crit: Color::Rgb(0xdc, 0x26, 0x26),
            motion: Motion::Full,
        }
    }

    /// No colour (`NO_COLOR`). Widgets carry meaning in weight and glyphs.
    pub const fn mono() -> Theme {
        Theme {
            palette: Palette::Mono,
            text: Color::Reset,
            text_secondary: Color::Reset,
            muted: Color::Reset,
            border: Color::Reset,
            accent: Color::Reset,
            surface_raised: Color::Reset,
            run: Color::Reset,
            wait: Color::Reset,
            done: Color::Reset,
            fail: Color::Reset,
            ok: Color::Reset,
            warn: Color::Reset,
            crit: Color::Reset,
            motion: Motion::Full,
        }
    }

    /// [`Theme::mono`] when `NO_COLOR` is set to anything non-empty
    /// (<https://no-color.org>), else [`Theme::ansi`].
    pub fn from_env() -> Theme {
        match std::env::var_os("NO_COLOR") {
            Some(v) if !v.is_empty() => Theme::mono(),
            _ => Theme::ansi(),
        }
    }

    pub const fn with_motion(mut self, motion: Motion) -> Theme {
        self.motion = motion;
        self
    }

    pub const fn is_mono(&self) -> bool {
        matches!(self.palette, Palette::Mono)
    }

    pub const fn reduced_motion(&self) -> bool {
        matches!(self.motion, Motion::Reduced)
    }

    /// Body text.
    pub fn text(&self) -> Style {
        Style::new().fg(self.text)
    }

    /// Labels and secondary facts.
    pub fn secondary(&self) -> Style {
        Style::new().fg(self.text_secondary)
    }

    /// Hints and separators. In mono this is the terminal's dim weight.
    pub fn dim(&self) -> Style {
        if self.is_mono() {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(self.muted)
        }
    }

    pub fn border(&self) -> Style {
        if self.is_mono() {
            Style::new().add_modifier(Modifier::DIM)
        } else {
            Style::new().fg(self.border)
        }
    }

    /// The thing to look at. In mono, bold.
    pub fn accent(&self) -> Style {
        if self.is_mono() {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(self.accent)
        }
    }

    /// A selected row or item: reversed in mono and on ANSI (the terminal
    /// picks a readable pair), a raised surface on true-colour palettes.
    pub fn selected(&self) -> Style {
        match self.palette {
            Palette::Mono | Palette::Ansi => Style::new().add_modifier(Modifier::REVERSED),
            Palette::Dark | Palette::Light => Style::new()
                .bg(self.surface_raised)
                .fg(self.text)
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn level_color(&self, level: Level) -> Color {
        match level {
            Level::Ok => self.ok,
            Level::Warn => self.warn,
            Level::Crit => self.crit,
        }
    }

    /// A level as a style. Mono keeps the three apart by weight: normal,
    /// bold, bold + reversed.
    pub fn level(&self, level: Level) -> Style {
        if self.is_mono() {
            match level {
                Level::Ok => Style::new(),
                Level::Warn => Style::new().add_modifier(Modifier::BOLD),
                Level::Crit => Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED),
            }
        } else {
            Style::new().fg(self.level_color(level))
        }
    }

    pub fn status_color(&self, status: Status) -> Color {
        match status {
            Status::Running => self.run,
            Status::Waiting => self.wait,
            Status::Done => self.done,
            Status::Failed => self.fail,
        }
    }

    pub fn status(&self, status: Status) -> Style {
        if self.is_mono() {
            match status {
                Status::Failed => Style::new().add_modifier(Modifier::BOLD),
                _ => Style::new(),
            }
        } else {
            Style::new().fg(self.status_color(status))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_thresholds_are_inclusive_and_nan_is_ok() {
        assert_eq!(Level::of(0.79, 0.8, 0.95), Level::Ok);
        assert_eq!(Level::of(0.8, 0.8, 0.95), Level::Warn);
        assert_eq!(Level::of(0.95, 0.8, 0.95), Level::Crit);
        assert_eq!(Level::of(f64::NAN, 0.8, 0.95), Level::Ok);
    }

    #[test]
    fn mono_has_no_colour_anywhere() {
        let t = Theme::mono();
        for s in [
            t.text(),
            t.secondary(),
            t.dim(),
            t.border(),
            t.accent(),
            t.selected(),
            t.level(Level::Crit),
            t.status(Status::Failed),
        ] {
            assert!(matches!(s.fg, None | Some(Color::Reset)), "{s:?}");
            assert!(matches!(s.bg, None | Some(Color::Reset)), "{s:?}");
        }
    }

    #[test]
    fn mono_keeps_levels_apart() {
        let t = Theme::mono();
        assert_ne!(t.level(Level::Ok), t.level(Level::Warn));
        assert_ne!(t.level(Level::Warn), t.level(Level::Crit));
    }
}
