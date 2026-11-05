// Derived from OpenAI Codex (Apache-2.0), commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72:
//   codex-rs/tui/src/summary_shimmer.rs   the time-pure, whole-grapheme sweep
//   codex-rs/tui/src/shimmer.rs           the bold/dim fallback without RGB
// Modified by 2lab: colours and motion come from a `Theme` (no probed globals,
// no process clock), the band degrades to the 256-colour palette or to
// bold/dim steps, mono never colours, reduced motion is plain bold, and the
// sweep cadence is a parameter.

//! A band of light sweeping across a label: this is live, work is in progress.
//!
//! Ported from OpenAI Codex `codex-rs/tui/src/summary_shimmer.rs` (Apache-2.0),
//! with the stepped fallback of `shimmer.rs`. Only brightness changes: the
//! words never move, so the label stays readable mid-sweep.
//!
//! The sweep is a pure function of `t`, the time since the label appeared
//! (pass the time since it last *changed* and the sweep restarts with it):
//! wait 600 ms, then one 1 s pass every 4 s ([`Cadence::CALM`]). The band is
//! a cosine bump at least six columns wide, measured in cells, so a two-cell
//! CJK glyph brightens as one and emoji or combining marks are never split.
//!
//! How it draws depends on the theme:
//! - **True colour** (`Theme::dark`, `Theme::light`, or any theme given the
//!   terminal's own colours with [`Shimmer::colors`]): text rests halfway
//!   between its colour and the background and the band lifts it to full
//!   strength. [`ColorLevel::Ansi256`] maps each step to the fixed xterm
//!   palette.
//! - **ANSI / 16 colours**: the user's palette is unknown, so the band is
//!   weight only — dim at rest, normal at the edge, bold at the crest.
//! - **Mono** (`NO_COLOR`): the same weight steps, never a colour.
//! - **Reduced motion**: the whole label in plain bold, still.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::borrow::Cow;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_segmentation::UnicodeSegmentation;

use crate::color::{self, ColorLevel, Rgb};
use crate::text;
use crate::theme::Theme;

/// When the band passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// Still time before the first pass, so a label that flickers past never
    /// sweeps at all.
    pub delay: Duration,
    /// How long one pass across the label takes.
    pub sweep: Duration,
    /// Time from the start of one pass to the start of the next (≥ `sweep`).
    pub every: Duration,
}

impl Cadence {
    /// Codex's status-header timing: wait 600 ms, then a 1 s pass every 4 s.
    pub const CALM: Cadence = Cadence {
        delay: Duration::from_millis(600),
        sweep: Duration::from_secs(1),
        every: Duration::from_secs(4),
    };

    /// Back-to-back 2 s passes with no wait — for a label that lives only a
    /// few seconds (connecting, signing in).
    pub const CONTINUOUS: Cadence = Cadence {
        delay: Duration::ZERO,
        sweep: Duration::from_secs(2),
        every: Duration::from_secs(2),
    };
}

impl Default for Cadence {
    fn default() -> Self {
        Cadence::CALM
    }
}

/// Each grapheme of `text` with the band's strength over it at `t`, from
/// 0.0 (at rest) to 1.0 (the crest). Whole graphemes, positioned by cell.
pub fn sweep(text: &str, t: Duration, cadence: Cadence) -> Vec<(&str, f64)> {
    let width = text::width(text) as f64;
    let half = (width * 0.1).max(3.0);
    let pass = cadence.sweep.as_secs_f64();
    let every = cadence.every.as_secs_f64().max(pass);
    // Before the first pass, between passes and for a degenerate cadence the
    // band sits off the right edge: everything at rest.
    let position = if pass > 0.0 {
        let into = (t.saturating_sub(cadence.delay).as_secs_f64() % every).min(pass);
        into / pass * (width + 2.0 * half) - half
    } else {
        width + half
    };
    let mut column = 0.0;
    text.graphemes(true)
        .map(|g| {
            let w = text::width(g) as f64;
            let center = column + w / 2.0;
            column += w;
            let distance = ((center - position).abs() / half).min(1.0);
            let intensity = 0.5 * (1.0 + (std::f64::consts::PI * distance).cos());
            (g, intensity)
        })
        .collect()
}

/// How a [`Shimmer`] draws under a given theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShimmerMode {
    /// Blend between `fg` (the crest) and halfway to `bg` (at rest), shown at
    /// `level`.
    Blend { fg: Rgb, bg: Rgb, level: ColorLevel },
    /// Weight only: dim, normal, bold.
    Steps,
    /// Reduced motion: plain bold.
    Still,
}

/// A label with a sweeping band of light.
#[derive(Debug, Clone, PartialEq)]
pub struct Shimmer<'a> {
    text: Cow<'a, str>,
    style: Style,
    colors: Option<(Rgb, Rgb)>,
    level: ColorLevel,
    cadence: Cadence,
}

impl<'a> Shimmer<'a> {
    pub fn new(text: impl Into<Cow<'a, str>>) -> Shimmer<'a> {
        Shimmer {
            text: text.into(),
            style: Style::new(),
            colors: None,
            level: ColorLevel::TrueColor,
            cadence: Cadence::CALM,
        }
    }

    /// Base style, patched over `theme.text()`. A true-colour foreground here
    /// becomes the band's crest (an accent-coloured shimmer).
    pub fn style(mut self, style: Style) -> Shimmer<'a> {
        self.style = style;
        self
    }

    /// The terminal's own foreground and background — what a host read with
    /// [`color::parse_osc_color`]. Lets the band blend in true colour on an
    /// ANSI theme too. Ignored under mono and reduced motion.
    pub fn colors(mut self, fg: Rgb, bg: Rgb) -> Shimmer<'a> {
        self.colors = Some((fg, bg));
        self
    }

    /// What the terminal can show. `Ansi256` maps each blended step to the
    /// fixed xterm palette; `Ansi16` falls back to weight steps.
    pub fn level(mut self, level: ColorLevel) -> Shimmer<'a> {
        self.level = level;
        self
    }

    pub fn cadence(mut self, cadence: Cadence) -> Shimmer<'a> {
        self.cadence = cadence;
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Cells the label takes (the same at every `t`: no layout jitter).
    pub fn width(&self) -> usize {
        text::width(&self.text)
    }

    pub fn mode(&self, theme: &Theme) -> ShimmerMode {
        if theme.reduced_motion() {
            return ShimmerMode::Still;
        }
        if theme.is_mono() || self.level == ColorLevel::Ansi16 {
            return ShimmerMode::Steps;
        }
        let fg = self
            .colors
            .map(|c| c.0)
            .or_else(|| self.style.fg.and_then(color::rgb_of))
            .or_else(|| color::rgb_of(theme.text));
        // The theme has no page-background token; a raised surface is the
        // nearest one and sits a step off the page, which is close enough
        // for a halfway blend.
        let bg = self
            .colors
            .map(|c| c.1)
            .or_else(|| color::rgb_of(theme.surface_raised));
        match (fg, bg) {
            (Some(fg), Some(bg)) => ShimmerMode::Blend {
                fg,
                bg,
                level: self.level,
            },
            _ => ShimmerMode::Steps,
        }
    }

    /// The label at time `t`, adjacent graphemes of the same style merged.
    pub fn spans_at(&self, t: Duration, theme: &Theme) -> Vec<Span<'static>> {
        let base = theme.text().patch(self.style);
        let mode = self.mode(theme);
        if mode == ShimmerMode::Still {
            return vec![Span::styled(
                self.text.to_string(),
                base.add_modifier(Modifier::BOLD),
            )];
        }
        let mut runs: Vec<(String, Style)> = Vec::new();
        // Blended levels repeat along the label; remember each conversion.
        let mut seen: Vec<(Rgb, ratatui::style::Color)> = Vec::new();
        for (g, intensity) in sweep(&self.text, t, self.cadence) {
            let style = match mode {
                ShimmerMode::Blend { fg, bg, level } => {
                    let rgb = color::blend(fg, bg, (0.5 + 0.5 * intensity) as f32);
                    let c = match seen.iter().find(|(k, _)| *k == rgb) {
                        Some((_, c)) => *c,
                        None => {
                            let c = color::best_color(rgb, level);
                            seen.push((rgb, c));
                            c
                        }
                    };
                    base.fg(c)
                }
                _ => step(base, intensity),
            };
            match runs.last_mut() {
                Some((s, st)) if *st == style => s.push_str(g),
                _ => runs.push((g.to_string(), style)),
            }
        }
        runs.into_iter()
            .map(|(s, st)| Span::styled(s, st))
            .collect()
    }

    pub fn line_at(&self, t: Duration, theme: &Theme) -> Line<'static> {
        Line::from(self.spans_at(t, theme))
    }

    /// A widget for one frame: one row, clipped to the area with `…`.
    pub fn at(self, t: Duration, theme: Theme) -> ShimmerFrame<'a> {
        ShimmerFrame {
            shimmer: self,
            t,
            theme,
        }
    }
}

/// The fallback without RGB, as upstream's `shimmer.rs`: dim at rest, normal
/// on the band's shoulders, bold at its crest.
fn step(base: Style, intensity: f64) -> Style {
    if intensity < 0.2 {
        base.remove_modifier(Modifier::BOLD)
            .add_modifier(Modifier::DIM)
    } else if intensity < 0.6 {
        base.remove_modifier(Modifier::DIM)
    } else {
        base.remove_modifier(Modifier::DIM)
            .add_modifier(Modifier::BOLD)
    }
}

/// A [`Shimmer`] at one moment, ready to draw.
#[derive(Debug, Clone)]
pub struct ShimmerFrame<'a> {
    shimmer: Shimmer<'a>,
    t: Duration,
    theme: Theme,
}

impl Widget for ShimmerFrame<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let line = text::truncate_line(
            self.shimmer.line_at(self.t, &self.theme),
            area.width as usize,
        );
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot;
    use crate::theme::Motion;
    use ratatui::style::Color;

    const DARK: (Rgb, Rgb) = ((240, 240, 240), (16, 16, 16));
    const LIGHT: (Rgb, Rgb) = ((16, 16, 16), (240, 240, 240));

    /// Red channel of every grapheme's foreground.
    fn levels(spans: &[Span]) -> Vec<u8> {
        spans
            .iter()
            .flat_map(|s| {
                let fg = s.style.fg;
                s.content.graphemes(true).map(move |_| match fg {
                    Some(Color::Rgb(r, _, _)) => r,
                    c => panic!("expected interpolated RGB, got {c:?}"),
                })
            })
            .collect()
    }

    fn at(text: &str, ms: u64, colors: (Rgb, Rgb)) -> Vec<Span<'static>> {
        Shimmer::new(text)
            .colors(colors.0, colors.1)
            .spans_at(Duration::from_millis(ms), &Theme::dark())
    }

    /// Upstream snapshot
    /// `short_and_long_labels_sweep_smoothly_in_both_themes`, value for value.
    #[test]
    fn short_and_long_labels_sweep_smoothly_in_both_themes() {
        const EXPECTED: &str = "\
dark Working 0ms: [128, 128, 128, 128, 128, 128, 128]
dark Working 850ms: [238, 198, 144, 128, 128, 128, 128]
dark Working 1100ms: [128, 156, 212, 240, 212, 156, 128]
dark Working 1350ms: [128, 128, 128, 128, 144, 198, 238]
dark Working 3600ms: [128, 128, 128, 128, 128, 128, 128]
dark Working 5100ms: [128, 156, 212, 240, 212, 156, 128]
dark Preparing bootstrap diagnostics 0ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
dark Preparing bootstrap diagnostics 850ms: [128, 128, 128, 132, 175, 226, 237, 198, 145, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
dark Preparing bootstrap diagnostics 1100ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 159, 213, 240, 213, 159, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
dark Preparing bootstrap diagnostics 1350ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 145, 198, 237, 226, 175, 132, 128, 128, 128]
dark Preparing bootstrap diagnostics 3600ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
dark Preparing bootstrap diagnostics 5100ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 159, 213, 240, 213, 159, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
dark 界é界 0ms: [128, 128, 128]
dark 界é界 850ms: [198, 129, 128]
dark 界é界 1100ms: [184, 240, 184]
dark 界é界 1350ms: [128, 129, 198]
dark 界é界 3600ms: [128, 128, 128]
dark 界é界 5100ms: [184, 240, 184]
light Working 0ms: [128, 128, 128, 128, 128, 128, 128]
light Working 850ms: [17, 57, 111, 128, 128, 128, 128]
light Working 1100ms: [128, 100, 44, 16, 44, 100, 128]
light Working 1350ms: [128, 128, 128, 128, 111, 57, 17]
light Working 3600ms: [128, 128, 128, 128, 128, 128, 128]
light Working 5100ms: [128, 100, 44, 16, 44, 100, 128]
light Preparing bootstrap diagnostics 0ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
light Preparing bootstrap diagnostics 850ms: [128, 128, 128, 123, 80, 29, 18, 57, 110, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
light Preparing bootstrap diagnostics 1100ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 127, 96, 42, 16, 42, 96, 127, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
light Preparing bootstrap diagnostics 1350ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 110, 57, 18, 29, 80, 123, 128, 128, 128]
light Preparing bootstrap diagnostics 3600ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
light Preparing bootstrap diagnostics 5100ms: [128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 127, 96, 42, 16, 42, 96, 127, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128, 128]
light 界é界 0ms: [128, 128, 128]
light 界é界 850ms: [57, 126, 128]
light 界é界 1100ms: [72, 16, 72]
light 界é界 1350ms: [128, 126, 57]
light 界é界 3600ms: [128, 128, 128]
light 界é界 5100ms: [72, 16, 72]";
        let mut frames = Vec::new();
        for (theme, colors) in [("dark", DARK), ("light", LIGHT)] {
            for text in ["Working", "Preparing bootstrap diagnostics", "界e\u{301}界"] {
                for ms in [0, 850, 1100, 1350, 3600, 5100] {
                    let lv = levels(&at(text, ms, colors));
                    frames.push(format!("{theme} {text} {ms}ms: {lv:?}"));
                }
            }
        }
        // The upstream snapshot prints the label with a precomposed é; the
        // input is e + U+0301.
        assert_eq!(
            frames.join("\n"),
            EXPECTED.replace("界\u{e9}界", "界e\u{301}界")
        );
    }

    /// Upstream `working_has_overlapping_highlights_without_frame_to_frame_flashes`.
    #[test]
    fn working_sweeps_without_frame_to_frame_flashes() {
        let mut previous: Vec<u8> = Vec::new();
        for ms in (0..=5600).step_by(32) {
            let b = levels(&at("Working", ms, DARK));
            if b.iter().any(|v| *v > 220) {
                assert!(b.iter().filter(|v| **v > 160).count() >= 2, "{ms}ms {b:?}");
            }
            for (cur, prev) in b.iter().zip(&previous) {
                assert!(u8::abs_diff(*cur, *prev) <= 25, "{ms}ms {b:?}");
            }
            previous = b;
        }
        assert_eq!(
            levels(&at("Working", 1100, DARK)),
            [128, 156, 212, 240, 212, 156, 128]
        );
    }

    /// Upstream `sweep_preserves_combining_characters_and_emoji_clusters`.
    #[test]
    fn sweep_preserves_combining_marks_and_emoji_clusters() {
        let text = "e\u{301}👨‍👩‍👧‍👦界";
        let graphemes: Vec<&str> = sweep(text, Duration::ZERO, Cadence::CALM)
            .into_iter()
            .map(|(g, _)| g)
            .collect();
        assert_eq!(graphemes, ["e\u{301}", "👨‍👩‍👧‍👦", "界"]);
        // Mid-sweep the spans still only split between graphemes.
        let spans = at(text, 1100, DARK);
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, text);
        assert_eq!(levels(&spans).len(), 3);
        let reduced = Theme::dark().with_motion(Motion::Reduced);
        assert_eq!(
            Shimmer::new(text).spans_at(Duration::from_secs(1), &reduced),
            vec![Span::styled(
                text,
                reduced.text().add_modifier(Modifier::BOLD)
            )]
        );
    }

    #[test]
    fn theme_tokens_feed_the_blend_without_probed_colours() {
        let t = Theme::dark();
        let mode = Shimmer::new("Working").mode(&t);
        assert_eq!(
            mode,
            ShimmerMode::Blend {
                fg: (0xe6, 0xe6, 0xe9),
                bg: (0x27, 0x27, 0x2a),
                level: ColorLevel::TrueColor
            }
        );
        // An RGB foreground in the base style is the crest.
        let accent = Shimmer::new("Working").style(t.accent()).mode(&t);
        assert!(matches!(
            accent,
            ShimmerMode::Blend {
                fg: (0x7d, 0xd3, 0xfc),
                ..
            }
        ));
    }

    #[test]
    fn ansi_without_rgb_and_ansi16_use_weight_steps() {
        let spans = Shimmer::new("Working").spans_at(Duration::from_millis(1100), &Theme::ansi());
        assert!(
            spans
                .iter()
                .all(|s| matches!(s.style.fg, None | Some(Color::Reset)))
        );
        let crest = spans.iter().find(|s| s.content.contains('k')).unwrap();
        assert!(crest.style.add_modifier.contains(Modifier::BOLD));
        let edge = spans.first().unwrap();
        assert!(edge.style.add_modifier.contains(Modifier::DIM));
        // At rest the whole label is one dim run.
        let rest = Shimmer::new("Working").spans_at(Duration::ZERO, &Theme::ansi());
        assert_eq!(rest.len(), 1);
        assert!(rest[0].style.add_modifier.contains(Modifier::DIM));

        let s16 = Shimmer::new("Working")
            .level(ColorLevel::Ansi16)
            .spans_at(Duration::from_millis(1100), &Theme::dark());
        assert!(s16.iter().all(|s| s.style.fg == Theme::dark().text().fg));
    }

    #[test]
    fn ansi256_quantises_to_fixed_palette() {
        let spans = Shimmer::new("Working")
            .level(ColorLevel::Ansi256)
            .spans_at(Duration::from_millis(1100), &Theme::dark());
        for s in &spans {
            assert!(
                matches!(s.style.fg, Some(Color::Indexed(i)) if i >= 16),
                "{s:?}"
            );
        }
        assert!(spans.len() > 1, "the band is still visible: {spans:?}");
    }

    #[test]
    fn mono_is_weight_only_even_with_probed_colours() {
        let t = Theme::mono();
        for ms in [0, 850, 1100, 1350] {
            for s in Shimmer::new("Working")
                .colors(DARK.0, DARK.1)
                .spans_at(Duration::from_millis(ms), &t)
            {
                assert!(matches!(s.style.fg, None | Some(Color::Reset)), "{s:?}");
                assert!(matches!(s.style.bg, None | Some(Color::Reset)), "{s:?}");
            }
        }
    }

    #[test]
    fn widget_clips_by_cells_with_an_ellipsis() {
        let area = Rect::new(0, 0, 10, 1);
        let mut buf = Buffer::empty(area);
        Shimmer::new("Preparing bootstrap diagnostics")
            .at(Duration::from_millis(1100), Theme::dark())
            .render(area, &mut buf);
        assert_eq!(snapshot::text(&buf), "Preparing…");
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        Shimmer::new("한국어 요약")
            .at(Duration::ZERO, Theme::ansi())
            .render(Rect::new(0, 0, 5, 1), &mut buf);
        assert_eq!(snapshot::text(&buf), "한국…");
    }

    #[test]
    fn degenerate_cadence_rests() {
        let c = Cadence {
            delay: Duration::ZERO,
            sweep: Duration::ZERO,
            every: Duration::ZERO,
        };
        for (_, i) in sweep("Working", Duration::from_millis(700), c) {
            assert!(i.is_finite() && i < 0.01);
        }
        assert!(sweep("", Duration::from_secs(9), Cadence::CALM).is_empty());
    }
}
