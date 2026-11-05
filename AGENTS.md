# AGENTS.md — how to work in 2lab-ai/tui

`twolab-tui` is a ratatui 0.30 component library: the terminal sibling of `@2lab.ai/ui`. One folder per widget, a
catalog site (`site/`) that runs every widget's demos live in the browser (the crate compiled to
`wasm32-unknown-unknown`), docs at https://2lab.ai/tui.

## Gates (run before every commit)
```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # includes the catalog contract (src/catalog/tests.rs)
npm run check && npm run typecheck && npm test
npm run build:site                # builds the wasm module (rustup toolchain) + the site into site-dist/tui
```

## Widget contract
- Folder `crates/twolab-tui/src/widgets/<slug>/`:
  - `mod.rs` — the widget. Public types with builder methods; `impl Widget` (or `StatefulWidget`) and, where a host
    places the result inside its own table cell or paragraph, a `line()` / `lines()` / `spans()` that returns ratatui
    text. Module docs say where it came from (project, file, licence) and what it tells the person.
  - `docs.rs` — `pub(crate) const DOC: ComponentDoc` behind `#[cfg(feature = "catalog")]`: summary (one sentence ending
    in a full stop, what it tells the person), description (≥ 80 chars), `used_in`, `source` (project `llmux|codex|fx|2lab`,
    a permalink, licence `MIT|Apache-2.0`, how `ported|studied|new`), props (builder methods), keyboard, a11y (≥ 1),
    a usage snippet that imports `twolab_tui`, and **≥ 3 demos with realistic data** (no network, no clock).
  - Tests in `mod.rs` (`#[cfg(test)]`): render into a `Buffer` and assert on `snapshot::text` / `snapshot::lines` and on
    cell styles. Port the upstream tests that pin behaviour.
- Register the widget in `src/widgets/mod.rs` and in `src/catalog/registry.rs` (`ORDER` + `DOCS`).
- The catalog contract test renders every demo at several sizes (down to 1×1), several times, in all four themes, with
  and without reduced motion, and checks: no panic, deterministic, not blank, and **no colour at all under
  `Theme::mono()`** (`NO_COLOR`). Interactive demos get a key sequence.

## Rules every widget keeps
- **Data in, cells out.** A widget never reads a clock, the environment or the terminal. Time comes in as `elapsed:
  Duration` (or a shared `frame` tick), size as the `Rect`. `std::time::Instant::now()` panics on wasm32 — never call it
  in the library.
- **Colour from the `Theme`** (`theme.rs`): `text()`, `secondary()`, `dim()`, `accent()`, `selected()`, `level(..)`,
  `status(..)`. Never a literal `Color::X` in a widget, except inside the theme. Mono must keep meaning in weight
  (bold, dim, reversed) and glyphs.
- **Cells, not chars.** Measure with `text::width`, clip with `text::truncate` / `text::truncate_line`, pad with
  `text::pad`. Hit rects and column widths are cell widths.
- **Animated glyphs stay one cell on a CJK terminal**: check `width_cjk() == 1`, not a code-point range. Braille
  (U+2800–28FF) and the quadrant blocks `▖ ▗ ▘ ▝ ▐ ░` qualify; most block elements (`▌ ▀ ▄ █ ▒ ▓`, the eighth
  blocks `▂ … ▆`) and `● ○ ◑ • · — … │ ─ ← → ↑ ↓` are East-Asian Ambiguous and take two cells there. Use those only
  where a host opts in, or where the widget is not a glyph-per-state marker (a bar, a rule).
- **Reduced motion** (`theme.reduced_motion()`): hold one still frame that says the same thing. Clocks still count.
- **Never cut silently**: when rows or text are dropped, say so (`+N more`, `…`). **Honest empties**: `—` for unknown,
  never a made-up `0`.
- No layout jitter: widths that depend on live values are measured on the widest form the value can take.

## Ported code
- Code carried over from OpenAI Codex or Vercel fx (both Apache-2.0) keeps a header comment naming the upstream file,
  the upstream commit and "Modified by 2lab". Add the file to `NOTICE`. Do not port OpenAI or Vercel branding (logos,
  product colour names, bundled themes).
- Code from llmux (MIT, 2lab) is relicensed by its owner; name the upstream file in the module docs.

## Commits
Conventional commits (`feat(meter): …`), lowercase.
