//! Text with a slow colour shimmer: this name is special.
//!
//! Ported from llmux `src/tui/ui.rs` (MIT, 2lab): `gradient_phase`,
//! `gradient_rainbow`, `gradient_solid`, `GradientCfg` and
//! `parse_hex_color`, which make headline model names (`fable-5`,
//! `gpt-5.6-sol`) breathe in their group's colour and the `max` effort
//! badge cycle through a rainbow.
//!
//! Each grapheme gets its own colour from a sine phase
//! `FREQ × index + DRIFT × speed × ticks`, where a tick is llmux's 120 ms
//! frame. [`Gradient::Rainbow`] offsets R, G and B by 120° so the hue turns;
//! [`Gradient::Solid`] scales one base colour, so only its brightness
//! moves. Brightness stays inside a luma band (0.45–1.00 by default) so no
//! letter fades into the background.
//!
//! Changes from llmux:
//! - split by grapheme cluster, not `char`, so a ZWJ emoji or a decomposed
//!   Hangul syllable is one coloured unit (llmux gave each scalar its own
//!   style);
//! - time comes in as `t: Duration` and the phase is kept in `f64`, reduced
//!   modulo 2π, so it never freezes on a long run (llmux wrapped its `f32`
//!   frame counter at a million ticks);
//! - reduced motion holds the text still — bold, in the base colour — and
//!   mono draws bold with no colour at all;
//! - on [`Palette::Light`](crate::theme::Palette::Light) the band moves
//!   down (0.25–0.70) so the letters stay dark enough to read.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;
use std::f64::consts::TAU;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::text;
use crate::theme::{Palette, Theme};

/// An RGB triple.
pub type Rgb = (u8, u8, u8);

/// Phase step between neighbouring graphemes, in radians.
pub const FREQ: f64 = 0.30;
/// Phase drift per tick at speed 1, in radians: a full luma cycle in about
/// 2.2 s.
pub const DRIFT: f64 = 0.35;
/// One tick: llmux's render interval, the unit [`DRIFT`] is measured in.
pub const TICK: Duration = Duration::from_millis(120);
/// The default luma band.
pub const LUMA: (f32, f32) = (0.45, 1.00);
/// The luma band on a light palette.
pub const LIGHT_LUMA: (f32, f32) = (0.25, 0.70);

/// Which colours the shimmer moves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Gradient {
    /// The hue turns: R, G, B are one sine 120° apart.
    Rainbow,
    /// One colour whose brightness breathes along the text.
    Solid(Rgb),
}

/// Parse `#rrggbb` (case-insensitive, `#` required). Short forms, a missing
/// `#` or bad hex give `None` — fall back to a known colour, never guess.
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let hex = s.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// The phase of grapheme `index` at time `t`, reduced to `0..2π`.
pub fn phase(index: usize, t: Duration, speed: f32) -> f64 {
    let ticks = t.as_secs_f64() / TICK.as_secs_f64();
    (FREQ * index as f64 + DRIFT * f64::from(speed) * ticks).rem_euclid(TAU)
}

fn wave(phase: f64) -> f32 {
    (phase.sin() * 0.5 + 0.5) as f32
}

fn lit(raw: f32, (lo, hi): (f32, f32)) -> f32 {
    lo + raw * (hi - lo)
}

fn byte(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// The rainbow colour at `phase` within the luma band.
pub fn rainbow(phase: f64, luma: (f32, f32)) -> Color {
    let chan = |offset: f64| byte(lit(wave(phase + offset), luma) * 255.0);
    Color::Rgb(chan(0.0), chan(TAU / 3.0), chan(2.0 * TAU / 3.0))
}

/// `base` scaled by the luma at `phase`: the hue never changes.
pub fn solid(base: Rgb, phase: f64, luma: (f32, f32)) -> Color {
    let factor = lit(wave(phase), luma);
    let scale = |c: u8| byte(f32::from(c) * factor);
    Color::Rgb(scale(base.0), scale(base.1), scale(base.2))
}

/// Text that shimmers. Build it, set the time, draw it with
/// [`GradientText::line`] or as a [`Widget`].
#[derive(Debug, Clone, PartialEq)]
pub struct GradientText<'a> {
    text: Cow<'a, str>,
    gradient: Gradient,
    speed: f32,
    t: Duration,
    luma: Option<(f32, f32)>,
    still: Option<Style>,
    theme: Theme,
}

impl<'a> GradientText<'a> {
    pub fn new(text: impl Into<Cow<'a, str>>, gradient: Gradient) -> GradientText<'a> {
        GradientText {
            text: text.into(),
            gradient,
            speed: 1.0,
            t: Duration::ZERO,
            luma: None,
            still: None,
            theme: Theme::default(),
        }
    }

    pub fn rainbow(text: impl Into<Cow<'a, str>>) -> GradientText<'a> {
        GradientText::new(text, Gradient::Rainbow)
    }

    pub fn solid(text: impl Into<Cow<'a, str>>, base: Rgb) -> GradientText<'a> {
        GradientText::new(text, Gradient::Solid(base))
    }

    /// Drift speed; 1.0 is llmux's default. A non-finite or non-positive
    /// speed is a bad config value and reads as 1.0 — it never freezes.
    pub fn speed(mut self, speed: f32) -> GradientText<'a> {
        self.speed = if speed.is_finite() && speed > 0.0 {
            speed
        } else {
            1.0
        };
        self
    }

    /// The moment to draw: time since the host's animation started.
    pub fn at(mut self, t: Duration) -> GradientText<'a> {
        self.t = t;
        self
    }

    /// Override the luma band (each 0–1, `min ≤ max`).
    pub fn luma(mut self, min: f32, max: f32) -> GradientText<'a> {
        let c = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                1.0
            }
        };
        let (a, b) = (c(min), c(max));
        self.luma = Some((a.min(b), a.max(b)));
        self
    }

    /// The still style for reduced motion. Default: bold in the base colour
    /// (the accent, for a rainbow). In mono only its weight is kept.
    pub fn still(mut self, style: Style) -> GradientText<'a> {
        self.still = Some(style);
        self
    }

    pub fn theme(mut self, theme: Theme) -> GradientText<'a> {
        self.theme = theme;
        self
    }

    pub fn current_speed(&self) -> f32 {
        self.speed
    }

    /// The luma band in use.
    pub fn band(&self) -> (f32, f32) {
        self.luma.unwrap_or(match self.theme.palette {
            Palette::Light => LIGHT_LUMA,
            _ => LUMA,
        })
    }

    /// The colour of grapheme `index` at the set time, ignoring motion and
    /// mono — the raw gradient.
    pub fn color_at(&self, index: usize) -> Color {
        let p = phase(index, self.t, self.speed);
        match self.gradient {
            Gradient::Rainbow => rainbow(p, self.band()),
            Gradient::Solid(base) => solid(base, p, self.band()),
        }
    }

    /// Whether this frame moves: not under reduced motion, not in mono.
    pub fn animates(&self) -> bool {
        !self.theme.reduced_motion() && !self.theme.is_mono()
    }

    fn still_style(&self) -> Style {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        if self.theme.is_mono() {
            let mods = self.still.map_or(Modifier::empty(), |s| s.add_modifier);
            return bold.add_modifier(mods);
        }
        self.still.unwrap_or(match self.gradient {
            Gradient::Rainbow => self.theme.accent().add_modifier(Modifier::BOLD),
            Gradient::Solid((r, g, b)) => bold.fg(Color::Rgb(r, g, b)),
        })
    }

    /// One bold span per grapheme, each its own colour — or one still span
    /// under reduced motion or in mono. The spans' text is always exactly
    /// the input.
    pub fn spans(&self) -> Vec<Span<'static>> {
        if self.text.is_empty() {
            return Vec::new();
        }
        if !self.animates() {
            return vec![Span::styled(self.text.to_string(), self.still_style())];
        }
        self.text
            .graphemes(true)
            .enumerate()
            .map(|(i, g)| {
                Span::styled(
                    g.to_string(),
                    Style::new()
                        .fg(self.color_at(i))
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect()
    }

    pub fn line(&self) -> Line<'static> {
        Line::from(self.spans())
    }
}

impl Widget for &GradientText<'_> {
    /// One row, clipped to the area with `…`.
    fn render(self, area: Rect, buf: &mut Buffer) {
        let area = area.intersection(buf.area);
        if area.is_empty() {
            return;
        }
        let line = text::truncate_line(self.line(), usize::from(area.width));
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

impl Widget for GradientText<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::theme::Motion;

    const CLAUDE: Rgb = (255, 121, 198);
    const CODEX: Rgb = (86, 220, 220);

    fn fg(spans: &[Span]) -> Vec<Option<Color>> {
        spans.iter().map(|s| s.style.fg).collect()
    }

    fn text_of(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn tick(n: u32) -> Duration {
        TICK * n
    }

    #[test]
    fn rainbow_cycles_across_frames() {
        let a = GradientText::rainbow("max").at(tick(0)).spans();
        let b = GradientText::rainbow("max").at(tick(1)).spans();
        assert_eq!(a.len(), 3, "one span per grapheme");
        assert_ne!(fg(&a), fg(&b), "the rainbow moves with time");
        assert_eq!(text_of(&a), "max");
        assert!(
            a.iter()
                .all(|s| s.style.add_modifier.contains(Modifier::BOLD))
        );
    }

    #[test]
    fn solid_keeps_its_hue_and_drifts() {
        let on0 = GradientText::solid("fable-5", CLAUDE).at(tick(0)).spans();
        let on8 = GradientText::solid("fable-5", CLAUDE).at(tick(8)).spans();
        assert_eq!(on0.len(), 7);
        assert_ne!(fg(&on0), fg(&on8), "the gradient drifts");
        for s in on0.iter().chain(&on8) {
            let Some(Color::Rgb(r, g, b)) = s.style.fg else {
                panic!("solid renders true colour, got {:?}", s.style.fg);
            };
            assert!(r >= b && b >= g, "magenta hue kept: ({r},{g},{b})");
        }
        let codex = GradientText::solid("gpt-5.6-sol", CODEX)
            .at(tick(3))
            .spans();
        for (i, s) in codex.iter().enumerate() {
            assert_eq!(s.style.fg, Some(solid(CODEX, phase(i, tick(3), 1.0), LUMA)));
        }
    }

    #[test]
    fn luma_stays_inside_the_band() {
        for n in 0..40 {
            for i in 0..12 {
                let Color::Rgb(r, g, b) = rainbow(phase(i, tick(n), 1.0), LUMA) else {
                    unreachable!()
                };
                for c in [r, g, b] {
                    assert!((114..=255).contains(&c), "{c} below 0.45 luma");
                }
                let Color::Rgb(r, g, b) = rainbow(phase(i, tick(n), 1.0), LIGHT_LUMA) else {
                    unreachable!()
                };
                for c in [r, g, b] {
                    assert!((63..=179).contains(&c), "{c} outside the light band");
                }
            }
        }
    }

    #[test]
    fn phase_matches_llmux_frames_and_speed_scales_the_drift() {
        // At t = k ticks the phase is llmux's FREQ·i + DRIFT·k (mod 2π).
        let want = (FREQ * 4.0 + DRIFT * 9.0).rem_euclid(TAU);
        assert!((phase(4, tick(9), 1.0) - want).abs() < 1e-9);
        // Four ticks: both advances stay below 2π, so no wrap in between.
        let base = phase(0, tick(4), 1.0) - phase(0, tick(0), 1.0);
        let fast = phase(0, tick(4), 2.0) - phase(0, tick(0), 2.0);
        assert!((fast - 2.0 * base).abs() < 1e-9, "{fast} vs {base}");
    }

    #[test]
    fn a_long_run_never_freezes() {
        let year = Duration::from_secs(365 * 86_400);
        let a = GradientText::rainbow("max").at(year).spans();
        let b = GradientText::rainbow("max").at(year + TICK).spans();
        assert_ne!(fg(&a), fg(&b));
    }

    #[test]
    fn speed_is_sanitised() {
        assert_eq!(GradientText::rainbow("x").speed(2.5).current_speed(), 2.5);
        for bad in [0.0, -3.0, f32::NAN, f32::INFINITY] {
            assert_eq!(GradientText::rainbow("x").speed(bad).current_speed(), 1.0);
        }
    }

    #[test]
    fn parse_hex_takes_only_full_hash_forms() {
        assert_eq!(parse_hex("#102030"), Some((0x10, 0x20, 0x30)));
        assert_eq!(parse_hex("#a0B0c0"), Some((0xa0, 0xb0, 0xc0)));
        assert_eq!(parse_hex(" #ffffff "), Some((255, 255, 255)));
        for bad in [
            "not-a-color",
            "#12345",
            "",
            "102030",
            "#12345g",
            "#ｆｆｆｆｆｆ",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad:?}");
        }
        // A configured base drives the solid gradient directly.
        let base = parse_hex("#804020").unwrap();
        for s in GradientText::solid("fable-5", base).at(tick(3)).spans() {
            let Some(Color::Rgb(r, g, b)) = s.style.fg else {
                panic!()
            };
            assert!(r >= g && g >= b, "configured hue kept ({r},{g},{b})");
        }
    }

    /// The llmux defect: `chars()` gave each scalar of a cluster its own
    /// colour. Here one cluster is one span.
    #[test]
    fn splits_by_grapheme_cluster() {
        let family = "👨\u{200d}👩\u{200d}👧\u{200d}👦";
        let s = GradientText::rainbow(format!("{family} e\u{301}한")).spans();
        let parts: Vec<&str> = s.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(parts, [family, " ", "e\u{301}", "한"]);
    }

    #[test]
    fn reduced_motion_holds_still_in_the_base_colour() {
        let t = Theme::ansi().with_motion(Motion::Reduced);
        let a = GradientText::solid("fable-5", CLAUDE)
            .theme(t)
            .at(tick(0))
            .spans();
        let b = GradientText::solid("fable-5", CLAUDE)
            .theme(t)
            .at(tick(9))
            .spans();
        assert_eq!(a, b);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].style.fg, Some(Color::Rgb(255, 121, 198)));
        assert!(a[0].style.add_modifier.contains(Modifier::BOLD));
        let r = GradientText::rainbow("max").theme(t).spans();
        assert_eq!(r[0].style, t.accent().add_modifier(Modifier::BOLD));
        let own = Style::new()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD);
        assert_eq!(
            GradientText::rainbow("max").theme(t).still(own).spans()[0].style,
            own
        );
    }

    #[test]
    fn mono_is_bold_without_colour_even_in_full_motion() {
        let own = Style::new()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::ITALIC);
        for g in [
            GradientText::rainbow("max"),
            GradientText::solid("fable-5", CLAUDE).still(own),
        ] {
            let g = g.theme(Theme::mono()).at(tick(5));
            let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
            (&g).render(buf.area, &mut buf);
            for c in buf.content() {
                assert_eq!((c.fg, c.bg), (Color::Reset, Color::Reset));
            }
            assert!(buf[(0, 0)].modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn light_palette_uses_the_darker_band() {
        let g = GradientText::rainbow("x").theme(Theme::light());
        assert_eq!(g.band(), LIGHT_LUMA);
        assert_eq!(GradientText::rainbow("x").theme(Theme::dark()).band(), LUMA);
        assert_eq!(GradientText::rainbow("x").luma(0.9, 0.2).band(), (0.2, 0.9));
    }

    #[test]
    fn widget_clips_with_an_ellipsis() {
        let g = GradientText::solid("gpt-5.6-sol", CODEX).at(tick(2));
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        g.render(buf.area, &mut buf);
        assert_eq!(snapshot::text(&buf), "gpt-5…");
        assert!(GradientText::rainbow("").spans().is_empty());
    }
}
