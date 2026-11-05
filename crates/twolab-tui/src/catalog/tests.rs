//! The catalog contract. Every widget's doc must pass these, so a new widget
//! cannot land without a summary, a source, three demos, accessibility notes —
//! and demos that render at any size, at any time, in every theme.

use std::collections::HashSet;
use std::time::Duration;

use ratatui::style::Color;

use super::*;
use crate::snapshot;
use crate::theme::{Motion, Theme};

const THEMES: [Theme; 4] = [Theme::ansi(), Theme::dark(), Theme::light(), Theme::mono()];
const TIMES_MS: [u64; 5] = [0, 120, 500, 3_000, 61_000];

#[test]
fn order_lists_every_doc_once() {
    let slugs: Vec<&str> = docs().iter().map(|d| d.slug).collect();
    let set: HashSet<&str> = slugs.iter().copied().collect();
    assert_eq!(set.len(), slugs.len(), "duplicate slug in {slugs:?}");
    let order: HashSet<&str> = ORDER.iter().copied().collect();
    assert_eq!(order, set, "ORDER and the docs disagree");
    assert_eq!(ORDER.len(), order.len(), "duplicate in ORDER");
}

#[test]
fn every_doc_is_complete() {
    for d in docs() {
        let s = d.slug;
        assert!(
            s.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "{s}: slug"
        );
        assert!(!d.name.is_empty() && !d.ty.is_empty(), "{s}: name/type");
        assert!(d.summary.ends_with('.'), "{s}: summary is one sentence");
        assert!(d.description.len() >= 80, "{s}: description too short");
        assert!(!d.used_in.is_empty(), "{s}: used_in");
        assert!(d.source.url.starts_with("https://"), "{s}: source url");
        assert!(
            matches!(d.source.license, "MIT" | "Apache-2.0"),
            "{s}: licence {}",
            d.source.license
        );
        assert!(
            matches!(d.source.how, "ported" | "studied" | "new"),
            "{s}: how"
        );
        assert!(
            matches!(d.source.project, "llmux" | "codex" | "fx" | "2lab"),
            "{s}: project"
        );
        assert!(!d.a11y.is_empty(), "{s}: a11y");
        assert!(
            d.usage.contains("twolab_tui"),
            "{s}: usage imports the crate"
        );
        assert!(!d.props.is_empty(), "{s}: props");
        assert!(d.demos.len() >= 3, "{s}: needs ≥ 3 demos");
        let ids: HashSet<&str> = d.demos.iter().map(|m| m.id).collect();
        assert_eq!(ids.len(), d.demos.len(), "{s}: duplicate demo id");
        for m in d.demos {
            assert!(m.cols >= 20 && m.cols <= 140, "{s}/{}: cols", m.id);
            assert!(m.rows >= 1 && m.rows <= 40, "{s}/{}: rows", m.id);
            assert!(!m.title.is_empty(), "{s}/{}: title", m.id);
        }
    }
}

/// Sizes a reader can pick on the page, plus degenerate ones.
fn sizes(cols: u16, rows: u16) -> Vec<(u16, u16)> {
    vec![
        (cols, rows),
        (40, rows),
        (120, rows),
        (cols, rows.saturating_add(6)),
        (12, 2),
        (1, 1),
    ]
}

#[test]
fn every_demo_renders_at_every_size_time_and_theme() {
    for d in docs() {
        for m in d.demos {
            for theme in THEMES {
                for motion in [Motion::Full, Motion::Reduced] {
                    let theme = theme.with_motion(motion);
                    let mut demo = (m.make)();
                    for (c, r) in sizes(m.cols, m.rows) {
                        for ms in TIMES_MS {
                            let cx = DemoCx {
                                theme,
                                t: Duration::from_millis(ms),
                            };
                            let buf = render_demo(demo.as_mut(), c, r, &cx);
                            // The exporter must cope with whatever the widget drew.
                            let j = snapshot::runs_json(&buf);
                            assert!(j.ends_with("]}"), "{}/{}", d.slug, m.id);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn demos_are_deterministic() {
    for d in docs() {
        for m in d.demos {
            let cx = DemoCx {
                theme: Theme::dark(),
                t: Duration::from_millis(1_234),
            };
            let a = render_demo((m.make)().as_mut(), m.cols, m.rows, &cx);
            let b = render_demo((m.make)().as_mut(), m.cols, m.rows, &cx);
            assert_eq!(a, b, "{}/{} differs between two renders", d.slug, m.id);
        }
    }
}

#[test]
fn demos_draw_something_at_their_own_size() {
    for d in docs() {
        for m in d.demos {
            let cx = DemoCx {
                theme: Theme::ansi(),
                t: Duration::from_millis(700),
            };
            let buf = render_demo((m.make)().as_mut(), m.cols, m.rows, &cx);
            assert!(
                !snapshot::text(&buf).trim().is_empty(),
                "{}/{} is blank",
                d.slug,
                m.id
            );
        }
    }
}

/// `NO_COLOR` means no colour: in mono every cell keeps the default
/// foreground and background, so the widget must carry meaning otherwise.
#[test]
fn mono_demos_draw_no_colour() {
    for d in docs() {
        for m in d.demos {
            let cx = DemoCx {
                theme: Theme::mono(),
                t: Duration::from_millis(900),
            };
            let buf = render_demo((m.make)().as_mut(), m.cols, m.rows, &cx);
            for cell in buf.content() {
                assert!(
                    matches!(cell.fg, Color::Reset) && matches!(cell.bg, Color::Reset),
                    "{}/{}: coloured cell {:?} in mono",
                    d.slug,
                    m.id,
                    cell
                );
            }
        }
    }
}

#[test]
fn interactive_demos_take_keys_without_panicking() {
    use crate::input::{Key, KeyInput};
    let keys = [
        KeyInput::new(Key::Down),
        KeyInput::new(Key::Down),
        KeyInput::new(Key::Up),
        KeyInput::new(Key::Right),
        KeyInput::new(Key::Left),
        KeyInput::new(Key::Enter),
        KeyInput::from('x'),
        KeyInput::from('한'),
        KeyInput::new(Key::Backspace),
        KeyInput::new(Key::PageDown),
        KeyInput::new(Key::End),
        KeyInput::new(Key::Home),
        KeyInput::new(Key::Tab),
        KeyInput::new(Key::Esc),
    ];
    for d in docs() {
        for m in d.demos.iter().filter(|m| m.interactive) {
            let mut demo = (m.make)();
            let cx = DemoCx {
                theme: Theme::ansi(),
                t: Duration::ZERO,
            };
            for k in keys {
                demo.key(k);
                let _ = render_demo(demo.as_mut(), m.cols, m.rows, &cx);
                let _ = render_demo(demo.as_mut(), 12, 2, &cx);
            }
        }
    }
}

#[test]
fn catalog_json_names_every_component() {
    let j = catalog_json();
    for d in docs() {
        assert!(
            j.contains(&format!("\"slug\":\"{}\"", d.slug)),
            "{}",
            d.slug
        );
    }
}
