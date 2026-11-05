//! The OSC 52 escape: put text on the clipboard of the terminal the person
//! is looking at — the one that works over SSH, where `pbcopy` or `wl-copy`
//! would copy on the wrong machine.
//!
//! Pure: this builds the bytes; the host writes them to its terminal.
//! Inside tmux the escape must be wrapped in tmux's passthrough (DCS) or tmux
//! swallows it; inside GNU screen, in screen's. llmux wrote the bare escape,
//! so a copy inside tmux silently did nothing.

/// The multiplexer between the app and the terminal, which decides how the
/// escape is wrapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mux {
    None,
    Tmux,
    Screen,
}

impl Mux {
    /// `Tmux` when `TMUX` is set, `Screen` when `STY` is set, else `None`.
    /// Takes the variables as arguments so tests (and the browser) need no
    /// environment: `Mux::detect(std::env::var_os("TMUX").is_some(), std::env::var_os("STY").is_some())`.
    pub fn detect(tmux: bool, screen: bool) -> Mux {
        if tmux {
            Mux::Tmux
        } else if screen {
            Mux::Screen
        } else {
            Mux::None
        }
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 (RFC 4648, with padding).
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The escape that copies `text` to the clipboard (`c` selection),
/// wrapped for `mux`.
pub fn sequence(text: &str, mux: Mux) -> String {
    let osc = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    match mux {
        Mux::None => osc,
        // tmux passthrough: ESC P tmux; <payload with every ESC doubled> ESC \.
        // Needs `set -g allow-passthrough on` on tmux 3.3+; older tmux passes it.
        Mux::Tmux => format!("\x1bPtmux;{}\x1b\\", osc.replace('\x1b', "\x1b\x1b")),
        // GNU screen caps one DCS string (768 bytes by default): send the
        // escape in 256-byte pieces, each in its own DCS, which screen joins.
        Mux::Screen => osc
            .as_bytes()
            .chunks(256)
            .map(|c| format!("\x1bP{}\x1b\\", String::from_utf8_lossy(c)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (i, o) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(i.as_bytes()), o, "{i:?}");
        }
        assert_eq!(base64("한".as_bytes()), "7ZWc");
    }

    #[test]
    fn bare_escape_outside_a_multiplexer() {
        assert_eq!(sequence("hi", Mux::None), "\x1b]52;c;aGk=\x07");
    }

    #[test]
    fn tmux_wraps_and_doubles_every_escape() {
        let s = sequence("hi", Mux::Tmux);
        assert_eq!(s, "\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\");
    }

    #[test]
    fn screen_wraps_in_dcs() {
        assert_eq!(sequence("hi", Mux::Screen), "\x1bP\x1b]52;c;aGk=\x07\x1b\\");
    }

    #[test]
    fn screen_splits_long_escapes() {
        let s = sequence(&"x".repeat(600), Mux::Screen);
        let pieces = s.matches("\x1bP").count();
        assert!(pieces >= 4, "{pieces}");
        let joined: String = s
            .split("\x1b\\")
            .map(|p| p.trim_start_matches("\x1bP"))
            .collect();
        assert_eq!(joined, sequence(&"x".repeat(600), Mux::None));
    }

    #[test]
    fn detect_prefers_tmux() {
        assert_eq!(Mux::detect(true, true), Mux::Tmux);
        assert_eq!(Mux::detect(false, true), Mux::Screen);
        assert_eq!(Mux::detect(false, false), Mux::None);
    }
}
