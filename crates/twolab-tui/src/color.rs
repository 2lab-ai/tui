// Derived from OpenAI Codex (Apache-2.0), commit e7f32cf1928a35a9232a633a51c2a03b5a7e2c72:
//   codex-rs/tui/src/color.rs              blend, is_light, perceptual_distance
//   codex-rs/tui/src/terminal_palette.rs   best colour per colour level, the xterm palette
//   codex-rs/tui/src/terminal_probe.rs     the OSC 10/11 reply parser (not the probe)
//   codex-rs/tui/src/style/contrast.rs     WCAG luminance, contrast ratio, readable foreground
// Modified by 2lab: no process globals (the colour level and the terminal's
// colours are arguments), the 256-colour table is computed instead of listed,
// `best_color` also maps to the 16 named colours, and the terminal probe itself
// is left to the host.
//
// Derived from herdrdev/herdr `src/ui/panes.rs` (`automatic_selection_bg`,
// `selection_fg_for_bg`, `mix_rgb`) at ee5bfb5041bb3e4a49d591c72308aae3c81ecdde
// (Apache-2.0): a selection colour from the host background. Modified by 2lab:
// the WCAG luminance and contrast here are reused, no palette fallback (the
// theme falls back to reversed instead).

//! Colour arithmetic on RGB, and graceful fallback from true colour.
//!
//! Widgets get their colours from a [`Theme`](crate::Theme). This module is
//! for the few that compute a colour from theme tokens — a shimmer blends the
//! text toward the background — and for hosts that know more about the
//! terminal than the library does.
//!
//! - [`blend`], [`luma`], [`is_light`], [`relative_luminance`],
//!   [`contrast_ratio`], [`readable_on`]: arithmetic on `(r, g, b)`.
//! - [`selection_colors`]: a selection's background and foreground computed
//!   from the terminal's own background (herdr), behind
//!   [`Theme::selection_on`](crate::Theme::selection_on).
//! - [`best_color`] turns an RGB value into what a terminal at a
//!   [`ColorLevel`] can show: itself, the nearest xterm 256-colour index, or
//!   the nearest of the 16 named colours.
//! - [`parse_osc_color`] reads a terminal's reply to an OSC 10 (foreground) or
//!   OSC 11 (background) query. The library never writes to or reads from a
//!   terminal: a host sends [`QUERY_FG`] / [`QUERY_BG`] itself, before its event
//!   reader starts, and hands the reply bytes here.
//!
//! Nothing here reads the environment. Deciding the colour level (`COLORTERM`,
//! `TERM`, a config switch) is the host's call.

use ratatui::style::Color;

/// An sRGB colour as `(red, green, blue)`.
pub type Rgb = (u8, u8, u8);

/// What the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ColorLevel {
    /// 24-bit colour (`COLORTERM=truecolor`).
    #[default]
    TrueColor,
    /// The xterm 256-colour palette. Indices 16–255 are the same on nearly
    /// every terminal, so they are the ones used.
    Ansi256,
    /// The 16 named colours, whose actual hues the user's theme decides.
    Ansi16,
}

/// `fg` laid over `bg` at opacity `alpha` (0.0 = all `bg`, 1.0 = all `fg`).
/// Channels are truncated, as upstream does, so results stay bit-identical to
/// it.
pub fn blend(fg: Rgb, bg: Rgb, alpha: f32) -> Rgb {
    let a = alpha.clamp(0.0, 1.0);
    let mix = |f: u8, b: u8| (f as f32 * a + b as f32 * (1.0 - a)) as u8;
    (mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2))
}

/// Perceived brightness on a 0–255 scale (YIQ luma: 0.299 R + 0.587 G +
/// 0.114 B).
pub fn luma(rgb: Rgb) -> f32 {
    0.299 * rgb.0 as f32 + 0.587 * rgb.1 as f32 + 0.114 * rgb.2 as f32
}

/// Whether `bg` is a light background (luma above the midpoint).
pub fn is_light(bg: Rgb) -> bool {
    luma(bg) > 128.0
}

/// WCAG 2 relative luminance, 0.0 (black) to 1.0 (white).
pub fn relative_luminance(rgb: Rgb) -> f64 {
    let lin = |c: u8| {
        let v = f64::from(c) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(rgb.0) + 0.7152 * lin(rgb.1) + 0.0722 * lin(rgb.2)
}

/// WCAG 2 contrast ratio between two colours, 1.0 to 21.0.
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (relative_luminance(a), relative_luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// The contrast WCAG asks of body text (AA).
pub const MIN_TEXT_CONTRAST: f64 = 4.5;

/// `preferred` if it reads on `background` (≥ [`MIN_TEXT_CONTRAST`]),
/// otherwise the least change toward black or white that does — so as much of
/// the hue as the surface allows survives.
pub fn readable_on(preferred: Rgb, background: Rgb) -> Rgb {
    if contrast_ratio(preferred, background) >= MIN_TEXT_CONTRAST {
        return preferred;
    }
    let (black, white) = ((0, 0, 0), (255, 255, 255));
    let end = if contrast_ratio(black, background) >= contrast_ratio(white, background) {
        black
    } else {
        white
    };
    (1..=255)
        .map(|step| blend(end, preferred, step as f32 / 255.0))
        .find(|c| contrast_ratio(*c, background) >= MIN_TEXT_CONTRAST)
        .unwrap_or(end)
}

/// `base` moved `amount` (0.0–1.0) of the way to `target`, each channel
/// rounded (herdr `mix_rgb`).
pub fn mix(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    let a = amount.clamp(0.0, 1.0);
    let ch = |b: u8, t: u8| (f32::from(b) + (f32::from(t) - f32::from(b)) * a).round() as u8;
    (
        ch(base.0, target.0),
        ch(base.1, target.1),
        ch(base.2, target.2),
    )
}

/// How far a selection moves the background toward white or black.
pub const SELECTION_MIX: f32 = 0.28;

/// A selection on a terminal whose background is `bg`, as `(background,
/// foreground)` (herdr `automatic_selection_bg`, `selection_fg_for_bg`): the
/// background moved [`SELECTION_MIX`] toward white when it is dark (relative
/// luminance under 0.5), toward black when it is light; the foreground black
/// or white, whichever contrasts more with that — at least 4.5:1 for any
/// background.
pub fn selection_colors(bg: Rgb) -> (Rgb, Rgb) {
    let (black, white) = ((0, 0, 0), (255, 255, 255));
    let target = if relative_luminance(bg) < 0.5 {
        white
    } else {
        black
    };
    let sel = mix(bg, target, SELECTION_MIX);
    let fg = if contrast_ratio(black, sel) > contrast_ratio(white, sel) {
        black
    } else {
        white
    };
    (sel, fg)
}

/// Distance between two colours as the eye sees it (CIE76: Euclidean distance
/// in CIELAB, D65 white).
pub fn perceptual_distance(a: Rgb, b: Rgb) -> f32 {
    let (l1, a1, b1) = lab(a);
    let (l2, a2, b2) = lab(b);
    ((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()
}

fn lab(rgb: Rgb) -> (f32, f32, f32) {
    let lin = |c: u8| {
        let c = c as f32 / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(rgb.0), lin(rgb.1), lin(rgb.2));
    let x = (r * 0.4124 + g * 0.3576 + b * 0.1805) / 0.95047;
    let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
    let z = (r * 0.0193 + g * 0.1192 + b * 0.9505) / 1.08883;
    let f = |t: f32| {
        if t > 0.008856 {
            t.powf(1.0 / 3.0)
        } else {
            7.787 * t + 16.0 / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// The xterm defaults for the 16 named colours, in index order. Real
/// terminals let the user's theme change these, so they are only a guess at
/// what index 0–15 looks like.
pub const XTERM_16: [Rgb; 16] = [
    (0, 0, 0),
    (128, 0, 0),
    (0, 128, 0),
    (128, 128, 0),
    (0, 0, 128),
    (128, 0, 128),
    (0, 128, 128),
    (192, 192, 192),
    (128, 128, 128),
    (255, 0, 0),
    (0, 255, 0),
    (255, 255, 0),
    (0, 0, 255),
    (255, 0, 255),
    (0, 255, 255),
    (255, 255, 255),
];

/// The 16 named colours, in the same order as [`XTERM_16`].
const NAMED: [Color; 16] = [
    Color::Black,
    Color::Red,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Magenta,
    Color::Cyan,
    Color::Gray,
    Color::DarkGray,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightMagenta,
    Color::LightCyan,
    Color::White,
];

/// The RGB value of xterm palette index `i`: the 16 defaults, then the
/// 6×6×6 cube (levels 0, 95, 135, 175, 215, 255), then 24 greys from 8 to 238.
pub const fn xterm_rgb(i: u8) -> Rgb {
    const fn level(v: u8) -> u8 {
        if v == 0 { 0 } else { 55 + 40 * v }
    }
    match i {
        0..=15 => XTERM_16[i as usize],
        16..=231 => {
            let n = i - 16;
            (level(n / 36), level((n % 36) / 6), level(n % 6))
        }
        _ => {
            let g = 8 + 10 * (i - 232);
            (g, g, g)
        }
    }
}

/// The xterm index in 16–255 nearest to `rgb` (indices 0–15 are skipped:
/// their hues depend on the user's theme).
pub fn nearest_ansi256(rgb: Rgb) -> u8 {
    let target = lab(rgb);
    let mut best = (16u8, f32::INFINITY);
    for i in 16..=255u8 {
        let (l, a, b) = lab(xterm_rgb(i));
        let d = (l - target.0).powi(2) + (a - target.1).powi(2) + (b - target.2).powi(2);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// The named colour nearest to `rgb`, judged against the xterm defaults.
pub fn nearest_ansi16(rgb: Rgb) -> Color {
    let mut best = (0usize, f32::INFINITY);
    for (i, c) in XTERM_16.iter().enumerate() {
        let d = perceptual_distance(*c, rgb);
        if d < best.1 {
            best = (i, d);
        }
    }
    NAMED[best.0]
}

/// The colour closest to `rgb` that a terminal at `level` can show.
pub fn best_color(rgb: Rgb, level: ColorLevel) -> Color {
    match level {
        ColorLevel::TrueColor => Color::Rgb(rgb.0, rgb.1, rgb.2),
        ColorLevel::Ansi256 => Color::Indexed(nearest_ansi256(rgb)),
        ColorLevel::Ansi16 => nearest_ansi16(rgb),
    }
}

/// The RGB behind a colour when it is fixed: true colour, or an xterm index
/// of 16 or more. `None` for [`Color::Reset`] and the named colours — the
/// user's terminal theme decides those.
pub fn rgb_of(color: Color) -> Option<Rgb> {
    match color {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(i) if i >= 16 => Some(xterm_rgb(i)),
        _ => None,
    }
}

/// Asks the terminal for its default foreground (OSC 10). Write it to the
/// TTY, then read the reply under a short deadline (upstream waits 250 ms).
pub const QUERY_FG: &[u8] = b"\x1b]10;?\x1b\\";

/// Asks the terminal for its default background (OSC 11).
pub const QUERY_BG: &[u8] = b"\x1b]11;?\x1b\\";

/// The colour in the first well-formed OSC 10 or OSC 11 reply found in
/// `reply` — e.g. `ESC ] 11 ; rgb:1e1e/1e1e/2e2e ESC \`. Accepts `rgb:` and
/// `rgba:`, 1–4 hex digits per channel (scaled to 8 bits), terminated by BEL
/// or ST, with unrelated bytes (typeahead) around it.
///
/// When one buffer holds both replies, use [`parse_osc_slot`] to tell them
/// apart.
pub fn parse_osc_color(reply: &[u8]) -> Option<Rgb> {
    let mut from = 0;
    while let Some(i) = find(&reply[from..], b"\x1b]") {
        let at = from + i;
        let rest = &reply[at + 2..];
        if (rest.starts_with(b"10;") || rest.starts_with(b"11;"))
            && let Some(rgb) = osc_payload(&rest[3..])
        {
            return Some(rgb);
        }
        from = at + 2;
    }
    None
}

/// The colour in the OSC reply for `slot` (10 = foreground, 11 =
/// background) inside `reply`, or `None` if that reply is missing, partial or
/// malformed.
pub fn parse_osc_slot(reply: &[u8], slot: u8) -> Option<Rgb> {
    let prefix = format!("\x1b]{slot};");
    let at = find(reply, prefix.as_bytes())?;
    osc_payload(&reply[at + prefix.len()..])
}

fn osc_payload(rest: &[u8]) -> Option<Rgb> {
    let end = rest.iter().enumerate().find_map(|(i, b)| match b {
        0x07 => Some(i),
        0x1b if rest.get(i + 1) == Some(&b'\\') => Some(i),
        _ => None,
    })?;
    parse_rgb_spec(std::str::from_utf8(&rest[..end]).ok()?)
}

/// `rgb:RRRR/GGGG/BBBB` or `rgba:…/AAAA` (alpha ignored).
fn parse_rgb_spec(spec: &str) -> Option<Rgb> {
    let (kind, values) = spec.trim().split_once(':')?;
    let rgba = kind.eq_ignore_ascii_case("rgba");
    if !rgba && !kind.eq_ignore_ascii_case("rgb") {
        return None;
    }
    let mut parts = values.split('/');
    let r = channel(parts.next()?)?;
    let g = channel(parts.next()?)?;
    let b = channel(parts.next()?)?;
    if rgba {
        channel(parts.next()?)?;
    }
    parts.next().is_none().then_some((r, g, b))
}

fn channel(hex: &str) -> Option<u8> {
    if !(1..=4).contains(&hex.len()) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from(u16::from_str_radix(hex, 16).ok()?);
    let max = (1u32 << (hex.len() * 4)) - 1;
    Some((value * 255 / max) as u8)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_matches_upstream_truncation() {
        assert_eq!(blend((240, 240, 240), (16, 16, 16), 0.5), (128, 128, 128));
        assert_eq!(blend((255, 0, 0), (0, 0, 255), 1.0), (255, 0, 0));
        assert_eq!(blend((255, 0, 0), (0, 0, 255), 0.0), (0, 0, 255));
        // Out-of-range alpha is clamped rather than wrapping a channel.
        assert_eq!(blend((200, 200, 200), (0, 0, 0), 2.0), (200, 200, 200));
    }

    #[test]
    fn light_and_dark_backgrounds() {
        assert!(is_light((255, 255, 255)));
        assert!(is_light((244, 244, 245)));
        assert!(!is_light((24, 24, 27)));
        assert!(!is_light((0, 0, 128)));
        assert!(luma((255, 255, 255)) > luma((128, 128, 128)));
    }

    #[test]
    fn contrast_ratio_spans_one_to_twenty_one() {
        assert!((contrast_ratio((0, 0, 0), (255, 255, 255)) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio((90, 90, 90), (90, 90, 90)) - 1.0).abs() < 1e-9);
    }

    /// Upstream `foreground_meets_minimum_after_palette_conversion`, true
    /// colour branch.
    #[test]
    fn readable_on_meets_the_minimum() {
        for bg in [
            (255, 255, 255),
            (245, 240, 220),
            (130, 130, 130),
            (18, 20, 30),
        ] {
            for preferred in [
                (139, 98, 20),
                (196, 167, 103),
                (125, 211, 252),
                (2, 132, 199),
            ] {
                let c = readable_on(preferred, bg);
                assert!(
                    contrast_ratio(c, bg) >= MIN_TEXT_CONTRAST,
                    "{c:?} on {bg:?}"
                );
            }
        }
        // Already readable: untouched.
        assert_eq!(readable_on((0, 0, 0), (255, 255, 255)), (0, 0, 0));
    }

    /// herdr `automatic_selection_rgb_style_is_readable_with_or_without_host_background`.
    #[test]
    fn selection_colours_from_the_host_background() {
        for (bg, sel, fg) in [
            ((239, 241, 245), (172, 174, 176), (0, 0, 0)),
            ((26, 27, 38), (90, 91, 99), (255, 255, 255)),
            ((45, 53, 59), (104, 110, 114), (255, 255, 255)),
        ] {
            assert_eq!(selection_colors(bg), (sel, fg), "{bg:?}");
        }
        // herdr `automatic_selection_background_uses_host_background`.
        let (sel, _) = selection_colors((12, 14, 16));
        assert!(relative_luminance(sel) > relative_luminance((12, 14, 16)));
        // Readable on every grey and on saturated backgrounds.
        let mut bgs: Vec<Rgb> = (0..=255u8).map(|v| (v, v, v)).collect();
        bgs.extend([
            (255, 0, 0),
            (0, 0, 255),
            (0, 128, 0),
            (255, 255, 0),
            (128, 0, 128),
        ]);
        for bg in bgs {
            let (sel, fg) = selection_colors(bg);
            assert!(contrast_ratio(fg, sel) >= MIN_TEXT_CONTRAST, "{bg:?}");
            assert_ne!(sel, bg, "{bg:?}: the selection must differ");
        }
        assert_eq!(mix((0, 0, 0), (255, 255, 255), 0.5), (128, 128, 128));
        assert_eq!(mix((10, 20, 30), (0, 0, 0), 2.0), (0, 0, 0), "clamped");
    }

    #[test]
    fn xterm_table_is_the_standard_one() {
        assert_eq!(xterm_rgb(1), (128, 0, 0));
        assert_eq!(xterm_rgb(16), (0, 0, 0));
        assert_eq!(xterm_rgb(17), (0, 0, 95));
        assert_eq!(xterm_rgb(21), (0, 0, 255));
        assert_eq!(xterm_rgb(59), (95, 95, 95));
        assert_eq!(xterm_rgb(196), (255, 0, 0));
        assert_eq!(xterm_rgb(231), (255, 255, 255));
        assert_eq!(xterm_rgb(232), (8, 8, 8));
        assert_eq!(xterm_rgb(244), (128, 128, 128));
        assert_eq!(xterm_rgb(255), (238, 238, 238));
    }

    /// Upstream `best_color_uses_truecolor_without_quantization`.
    #[test]
    fn best_color_keeps_true_colour() {
        assert_eq!(
            best_color((12, 34, 56), ColorLevel::TrueColor),
            Color::Rgb(12, 34, 56)
        );
    }

    #[test]
    fn best_color_quantises_to_the_fixed_256_palette() {
        assert_eq!(
            best_color((255, 0, 0), ColorLevel::Ansi256),
            Color::Indexed(196)
        );
        assert_eq!(
            best_color((128, 128, 128), ColorLevel::Ansi256),
            Color::Indexed(244)
        );
        assert_eq!(
            best_color((0, 0, 0), ColorLevel::Ansi256),
            Color::Indexed(16)
        );
        for rgb in [(12, 34, 56), (250, 250, 250), (125, 211, 252)] {
            let Color::Indexed(i) = best_color(rgb, ColorLevel::Ansi256) else {
                panic!("indexed");
            };
            assert!(i >= 16, "{rgb:?} -> {i}");
        }
    }

    /// Upstream resets to the default colour on 16-colour terminals; here a
    /// true-colour effect lands on the nearest named colour instead.
    #[test]
    fn best_color_falls_back_to_the_16_named_colours() {
        assert_eq!(
            best_color((250, 10, 10), ColorLevel::Ansi16),
            Color::LightRed
        );
        assert_eq!(best_color((120, 0, 0), ColorLevel::Ansi16), Color::Red);
        assert_eq!(
            best_color((134, 134, 137), ColorLevel::Ansi16),
            Color::DarkGray
        );
        assert_eq!(
            best_color((230, 230, 233), ColorLevel::Ansi16),
            Color::White
        );
        assert_eq!(best_color((0, 0, 0), ColorLevel::Ansi16), Color::Black);
    }

    #[test]
    fn rgb_of_only_fixed_colours() {
        assert_eq!(rgb_of(Color::Rgb(1, 2, 3)), Some((1, 2, 3)));
        assert_eq!(rgb_of(Color::Indexed(244)), Some((128, 128, 128)));
        assert_eq!(rgb_of(Color::Indexed(3)), None);
        assert_eq!(rgb_of(Color::Reset), None);
        assert_eq!(rgb_of(Color::Cyan), None);
    }

    /// Upstream `parses_osc_colors_with_bel_and_st`.
    #[test]
    fn parses_osc_colors_with_bel_and_st() {
        assert_eq!(
            parse_osc_color(b"\x1b]10;rgb:ffff/8000/0000\x07"),
            Some((255, 127, 0))
        );
        assert_eq!(
            parse_osc_color(b"\x1b]11;rgba:00/80/ff/ff\x1b\\"),
            Some((0, 128, 255))
        );
    }

    /// Upstream `parses_one_to_four_digit_color_components`.
    #[test]
    fn parses_one_to_four_digit_components() {
        assert_eq!(parse_rgb_spec("rgb:f/e/d"), Some((255, 238, 221)));
        assert_eq!(parse_rgb_spec("rgb:00/80/ff"), Some((0, 128, 255)));
        assert_eq!(parse_rgb_spec("rgb:fff/800/000"), Some((255, 127, 0)));
        assert_eq!(
            parse_rgb_spec("rgba:ffff/8000/0000/ffff"),
            Some((255, 127, 0))
        );
        assert_eq!(parse_rgb_spec("rgb:fffff/0/0"), None);
        assert_eq!(parse_rgb_spec("rgb:+f/0/0"), None);
        assert_eq!(parse_rgb_spec("hsl:1/2/3"), None);
    }

    /// Upstream `parses_default_colors_from_one_buffer` and
    /// `parses_default_colors_with_unrelated_bytes`.
    #[test]
    fn parses_both_slots_from_one_buffer_with_typeahead() {
        let buf = b"typed\x1b]10;rgb:eeee/eeee/eeee\x1b\\noise\x1b]11;rgb:1111/1111/1111\x07";
        assert_eq!(parse_osc_slot(buf, 10), Some((238, 238, 238)));
        assert_eq!(parse_osc_slot(buf, 11), Some((17, 17, 17)));
        assert_eq!(parse_osc_color(buf), Some((238, 238, 238)));
        let swapped = b"\x1b]11;rgb:1111/1111/1111\x07\x1b]10;rgb:eeee/eeee/eeee\x1b\\";
        assert_eq!(parse_osc_slot(swapped, 10), Some((238, 238, 238)));
        assert_eq!(parse_osc_slot(swapped, 11), Some((17, 17, 17)));
    }

    /// Upstream `ignores_malformed_or_partial_default_color_responses`.
    #[test]
    fn ignores_malformed_or_partial_replies() {
        assert_eq!(parse_osc_slot(b"\x1b]11;rgb:nope\x07", 11), None);
        assert_eq!(parse_osc_slot(b"\x1b]11;rgb:11/11/11/11\x07", 11), None);
        assert_eq!(parse_osc_slot(b"\x1b]11;rgb:1111/1111/1111", 11), None);
        assert_eq!(parse_osc_color(b"\x1b]12;rgb:1111/1111/1111\x07"), None);
        assert_eq!(parse_osc_color(b""), None);
        // A malformed first reply does not hide a good second one.
        assert_eq!(
            parse_osc_color(b"\x1b]10;rgb:zz/zz/zz\x07\x1b]11;rgb:1111/1111/1111\x07"),
            Some((17, 17, 17))
        );
    }
}
