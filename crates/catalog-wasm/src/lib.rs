//! The catalog page's renderer, compiled to `wasm32-unknown-unknown` with no
//! imports and no generated glue.
//!
//! The page writes a UTF-8 string into memory it got from [`tui_alloc`], calls
//! an export, and reads the answer at [`tui_out_ptr`] / [`tui_out_len`]. Every
//! answer is JSON. Demos live in a handle table so the page can keep one per
//! preview (the main stage and each home-page tile) and send keys to it.
//!
//! `std::time::Instant` panics on this target, so nothing here reads a clock:
//! the page passes the demo's elapsed milliseconds in.

use std::cell::RefCell;
use std::time::Duration;

use twolab_tui::catalog::{self, Demo, DemoCx};
use twolab_tui::input::{Key, KeyInput, Mods};
use twolab_tui::snapshot;
use twolab_tui::theme::{Motion, Theme};

thread_local! {
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static DEMOS: RefCell<Vec<Option<Box<dyn Demo>>>> = const { RefCell::new(Vec::new()) };
}

fn answer(s: String) {
    OUT.with(|o| *o.borrow_mut() = s.into_bytes());
}

/// # Safety
/// `ptr..ptr+len` must be memory the page wrote through [`tui_alloc`].
unsafe fn read_str(ptr: *const u8, len: usize) -> String {
    if ptr.is_null() || len == 0 {
        return String::new();
    }
    // SAFETY: the caller guarantees `ptr..ptr+len` is initialised memory it wrote.
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8_lossy(bytes).into_owned()
}

/// Memory for the page to write an argument into. Freed with [`tui_free`].
#[unsafe(no_mangle)]
pub extern "C" fn tui_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `ptr` and `len` must come from one [`tui_alloc`] call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tui_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        // SAFETY: `ptr` came from `tui_alloc(len)`, which leaked a Vec of that capacity.
        drop(unsafe { Vec::from_raw_parts(ptr, 0, len.max(1)) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tui_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn tui_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

/// The catalog (docs without demo functions) as JSON.
#[unsafe(no_mangle)]
pub extern "C" fn tui_catalog() {
    answer(catalog::catalog_json());
}

/// Open a demo: `"<slug>/<demo-id>"`. Returns a handle (≥ 1), or 0 when the
/// slug or id is unknown.
///
/// # Safety
/// See [`read_str`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tui_open(ptr: *const u8, len: usize) -> u32 {
    // SAFETY: forwarded from the caller's contract.
    let path = unsafe { read_str(ptr, len) };
    let Some((slug, id)) = path.split_once('/') else {
        return 0;
    };
    let Some(doc) = catalog::doc_by_slug(slug) else {
        return 0;
    };
    let Some(m) = doc.demo(id) else {
        return 0;
    };
    let demo = (m.make)();
    DEMOS.with(|d| {
        let mut d = d.borrow_mut();
        if let Some(i) = d.iter().position(Option::is_none) {
            d[i] = Some(demo);
            (i + 1) as u32
        } else {
            d.push(Some(demo));
            d.len() as u32
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn tui_close(handle: u32) {
    DEMOS.with(|d| {
        if let Some(slot) = d.borrow_mut().get_mut(handle.wrapping_sub(1) as usize) {
            *slot = None;
        }
    });
}

/// `palette`: 0 ansi, 1 dark, 2 light, 3 mono. `reduced`: 1 for reduced motion.
fn theme_of(palette: u32, reduced: u32) -> Theme {
    let t = match palette {
        1 => Theme::dark(),
        2 => Theme::light(),
        3 => Theme::mono(),
        _ => Theme::ansi(),
    };
    t.with_motion(if reduced == 1 {
        Motion::Reduced
    } else {
        Motion::Full
    })
}

/// Render one frame as styled runs (see `snapshot::runs_json`). Returns 0 on
/// an unknown handle, 1 otherwise.
#[unsafe(no_mangle)]
pub extern "C" fn tui_render(
    handle: u32,
    cols: u32,
    rows: u32,
    t_ms: f64,
    palette: u32,
    reduced: u32,
) -> u32 {
    let cols = cols.clamp(1, 400) as u16;
    let rows = rows.clamp(1, 200) as u16;
    let t = if t_ms.is_finite() && t_ms > 0.0 {
        Duration::from_millis(t_ms as u64)
    } else {
        Duration::ZERO
    };
    let cx = DemoCx {
        theme: theme_of(palette, reduced),
        t,
    };
    DEMOS.with(|d| {
        let mut d = d.borrow_mut();
        match d
            .get_mut(handle.wrapping_sub(1) as usize)
            .and_then(Option::as_mut)
        {
            Some(demo) => {
                let buf = catalog::render_demo(demo.as_mut(), cols, rows, &cx);
                answer(snapshot::runs_json(&buf));
                1
            }
            None => 0,
        }
    })
}

/// Key codes the page sends. A printable key is `0` with `ch` set.
fn key_of(code: u32, ch: u32) -> Option<Key> {
    Some(match code {
        0 => Key::Char(char::from_u32(ch)?),
        1 => Key::Enter,
        2 => Key::Esc,
        3 => Key::Backspace,
        4 => Key::Delete,
        5 => Key::Tab,
        6 => Key::BackTab,
        7 => Key::Left,
        8 => Key::Right,
        9 => Key::Up,
        10 => Key::Down,
        11 => Key::Home,
        12 => Key::End,
        13 => Key::PageUp,
        14 => Key::PageDown,
        _ => return None,
    })
}

/// Send a key to a demo. `mods`: bit 0 ctrl, bit 1 alt, bit 2 shift.
/// Returns 1 when the demo changed (render again), else 0.
#[unsafe(no_mangle)]
pub extern "C" fn tui_key(handle: u32, code: u32, ch: u32, mods: u32) -> u32 {
    let Some(key) = key_of(code, ch) else {
        return 0;
    };
    let input = KeyInput {
        key,
        mods: Mods {
            ctrl: mods & 1 != 0,
            alt: mods & 2 != 0,
            shift: mods & 4 != 0,
        },
    };
    DEMOS.with(|d| {
        let mut d = d.borrow_mut();
        match d
            .get_mut(handle.wrapping_sub(1) as usize)
            .and_then(Option::as_mut)
        {
            Some(demo) => u32::from(demo.key(input)),
            None => 0,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out() -> String {
        OUT.with(|o| String::from_utf8(o.borrow().clone()).unwrap())
    }

    fn open(path: &str) -> u32 {
        unsafe { tui_open(path.as_ptr(), path.len()) }
    }

    #[test]
    fn open_render_close() {
        tui_catalog();
        assert!(out().contains("\"slug\":\"spinner\""));
        let h = open("spinner/families");
        assert!(h >= 1);
        assert_eq!(tui_render(h, 44, 6, 240.0, 0, 0), 1);
        assert!(out().starts_with("{\"cols\":44,\"rows\":6,"));
        tui_close(h);
        assert_eq!(tui_render(h, 44, 6, 0.0, 0, 0), 0);
        assert_eq!(open("spinner/nope"), 0);
        assert_eq!(open("nope/families"), 0);
        assert_eq!(open("garbage"), 0);
    }

    #[test]
    fn handles_are_reused() {
        let a = open("spinner/families");
        tui_close(a);
        let b = open("spinner/rows");
        assert_eq!(a, b);
        tui_close(b);
    }
}
