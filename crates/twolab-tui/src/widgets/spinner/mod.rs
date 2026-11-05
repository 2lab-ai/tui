//! Single-glyph animations: that something is in progress, and which kind.
//!
//! Ported from llmux `src/tui/anim.rs` (MIT, 2lab), where each family marks a
//! different state on the same screen — a working Claude request (braille), a
//! working Codex request (block), a cooldown (half-block clock), the active
//! account (pulse), an idle account (drift), an account filling up (shade).
//!
//! Every glyph is braille (U+2800–28FF) or a block element (U+2580–259F).
//! Both blocks are East-Asian *Narrow*, so a spinner stays one column wide on a
//! Korean, Japanese or Chinese terminal; geometric shapes (◐ ● ○) are
//! *Ambiguous* and would push a table out of line there.

#[cfg(feature = "catalog")]
pub(crate) mod docs;

use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// One frame every 120 ms — llmux's render tick, and slow enough that a row of
/// spinners reads as calm rather than busy.
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpinnerKind {
    /// Ten-frame braille orbit: working.
    Braille,
    /// Four-frame quarter-block orbit: working, a second kind.
    Block,
    /// Half-block sweep: a timer — waiting for a reset or a cooldown.
    Clock,
    /// A bar that swells and ebbs: alive and selected, idle.
    Pulse,
    /// One braille dot drifting: idle but ready.
    Drift,
    /// A shade ramp breathing: filling up.
    Breathe,
}

impl SpinnerKind {
    pub const ALL: [SpinnerKind; 6] = [
        SpinnerKind::Braille,
        SpinnerKind::Block,
        SpinnerKind::Clock,
        SpinnerKind::Pulse,
        SpinnerKind::Drift,
        SpinnerKind::Breathe,
    ];

    pub const fn frames(self) -> &'static [char] {
        match self {
            SpinnerKind::Braille => &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'],
            SpinnerKind::Block => &['▖', '▘', '▝', '▗'],
            SpinnerKind::Clock => &['▌', '▀', '▐', '▄'],
            SpinnerKind::Pulse => &['▂', '▃', '▄', '▅', '▆', '▅', '▄', '▃'],
            SpinnerKind::Drift => &['⠁', '⠂', '⠄', '⠂'],
            SpinnerKind::Breathe => &['░', '▒', '▓', '█', '▓', '▒'],
        }
    }

    /// Ticks each glyph is held. Pulses and breaths hold two ticks so they
    /// feel slow next to a working spinner.
    pub const fn hold(self) -> usize {
        match self {
            SpinnerKind::Braille | SpinnerKind::Block | SpinnerKind::Clock => 1,
            SpinnerKind::Pulse | SpinnerKind::Drift | SpinnerKind::Breathe => 2,
        }
    }

    /// The glyph at tick `frame` (one shared counter keeps every spinner on
    /// screen in step).
    pub fn glyph(self, frame: usize) -> char {
        let frames = self.frames();
        frames[(frame / self.hold().max(1)) % frames.len()]
    }

    /// The still glyph shown under reduced motion: the first frame.
    pub fn still(self) -> char {
        self.frames()[0]
    }

    pub const fn name(self) -> &'static str {
        match self {
            SpinnerKind::Braille => "braille",
            SpinnerKind::Block => "block",
            SpinnerKind::Clock => "clock",
            SpinnerKind::Pulse => "pulse",
            SpinnerKind::Drift => "drift",
            SpinnerKind::Breathe => "breathe",
        }
    }
}

/// The tick reached after `t` at `interval`.
pub fn tick(t: Duration, interval: Duration) -> usize {
    let iv = interval.as_millis().max(1);
    (t.as_millis() / iv) as usize
}

/// A spinner glyph with a style. Draws one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spinner {
    pub kind: SpinnerKind,
    pub interval: Duration,
    pub style: Style,
}

impl Spinner {
    pub const fn new(kind: SpinnerKind) -> Spinner {
        Spinner {
            kind,
            interval: DEFAULT_INTERVAL,
            style: Style::new(),
        }
    }

    pub const fn interval(mut self, interval: Duration) -> Spinner {
        self.interval = interval;
        self
    }

    pub const fn style(mut self, style: Style) -> Spinner {
        self.style = style;
        self
    }

    /// The glyph at time `t`, or the still glyph when the theme asks for
    /// reduced motion.
    pub fn glyph_at(&self, t: Duration, theme: &Theme) -> char {
        if theme.reduced_motion() {
            self.kind.still()
        } else {
            self.kind.glyph(tick(t, self.interval))
        }
    }

    pub fn span_at(&self, t: Duration, theme: &Theme) -> Span<'static> {
        Span::styled(self.glyph_at(t, theme).to_string(), self.style)
    }

    /// A widget for one frame.
    pub fn at(self, t: Duration, theme: Theme) -> SpinnerFrame {
        SpinnerFrame {
            spinner: self,
            t,
            theme,
        }
    }
}

/// A [`Spinner`] at one moment, ready to draw.
#[derive(Debug, Clone, Copy)]
pub struct SpinnerFrame {
    spinner: Spinner,
    t: Duration,
    theme: Theme,
}

impl Widget for SpinnerFrame {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        let g = self.spinner.glyph_at(self.t, &self.theme);
        buf.set_string(area.x, area.y, g.to_string(), self.spinner.style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Motion;

    #[test]
    fn working_spinners_are_distinct_families_and_cycle() {
        for f in 0..12 {
            assert_ne!(
                SpinnerKind::Braille.glyph(f),
                SpinnerKind::Block.glyph(f),
                "frame {f}"
            );
        }
        assert_eq!(
            SpinnerKind::Braille.glyph(0),
            SpinnerKind::Braille.glyph(10)
        );
        assert_eq!(SpinnerKind::Block.glyph(0), SpinnerKind::Block.glyph(4));
        assert_eq!(SpinnerKind::Clock.glyph(0), SpinnerKind::Clock.glyph(4));
    }

    #[test]
    fn hold_slows_pulses() {
        assert_eq!(SpinnerKind::Pulse.glyph(0), SpinnerKind::Pulse.glyph(1));
        assert_ne!(SpinnerKind::Pulse.glyph(0), SpinnerKind::Pulse.glyph(2));
    }

    #[test]
    fn every_glyph_is_east_asian_narrow() {
        let narrow = |c: char| {
            let u = c as u32;
            (0x2800..=0x28FF).contains(&u) || (0x2580..=0x259F).contains(&u)
        };
        for k in SpinnerKind::ALL {
            for &g in k.frames() {
                assert!(narrow(g), "{} {g:?} U+{:04X}", k.name(), g as u32);
            }
        }
    }

    #[test]
    fn time_maps_to_ticks_and_reduced_motion_stays_still() {
        let s = Spinner::new(SpinnerKind::Braille);
        let t = Theme::ansi();
        assert_eq!(s.glyph_at(Duration::ZERO, &t), '⠋');
        assert_eq!(s.glyph_at(Duration::from_millis(120), &t), '⠙');
        assert_eq!(s.glyph_at(Duration::from_millis(1199), &t), '⠏');
        let r = t.with_motion(Motion::Reduced);
        assert_eq!(s.glyph_at(Duration::from_millis(500), &r), '⠋');
    }
}
