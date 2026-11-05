# twolab-tui

Terminal widgets for [ratatui](https://ratatui.rs) 0.30 that show what an agent is doing and what it needs from you — the terminal sibling of [`@2lab.ai/ui`](https://2lab.ai/ui).

Catalog with live demos: **https://2lab.ai/tui** — every demo there is this crate, compiled to WebAssembly.

```toml
[dependencies]
twolab-tui = { git = "https://github.com/2lab-ai/tui" }
```

## What is in it

Widgets come from three working apps, each named on its catalog page with its upstream file and licence:

- from llmux (2lab's quota dashboard): spinner families, quota meter, share bar, ledger amounts, gradient text, banner and flash, tabs, context menu, JSON view, packed grid, trend chart
- from OpenAI Codex: shimmer, working line, key hints, status card, layout containers, select list, multi-select, pager
- from Vercel fx: approval card, question form, diff view, composer, completion menu, status bar, fact table, rule

The catalog groups them by what they tell the person: Agent, Data, Input and Primitives.

## Rules every widget keeps

- Data in, cells out. A widget never reads a clock, the environment or the terminal. Time comes in as elapsed time or a shared tick.
- Colour comes from a `Theme` (`ansi`, `dark`, `light`, `mono`). `Theme::from_env()` honours `NO_COLOR`; under `mono` no cell is coloured and meaning moves to weight and glyphs.
- Width is counted in terminal cells by grapheme. Korean, Japanese and emoji line up.
- Animated widgets hold a still frame under `Motion::Reduced`.
- Nothing is cut silently (`+N more`, `…`), and unknown values show `—`.

The catalog's contract test checks all of these for every demo. See [AGENTS.md](AGENTS.md) for the widget contract and [docs/research.md](docs/research.md) for the research behind it.

## Repository

| Path | What it is |
|---|---|
| `crates/twolab-tui` | The library. One folder per widget under `src/widgets/`, each with `mod.rs` (widget and tests) and `docs.rs` (its catalog entry, behind the `catalog` feature). |
| `crates/catalog-wasm` | The web catalog's renderer: every demo, compiled to `wasm32-unknown-unknown` with no imports. |
| `crates/catalog-term` | `tui-catalog`: the same catalog in a terminal (crossterm). |
| `site/` | The catalog site (Vite, base `/tui/`). |
| `docs/research.md` | The research report shown on the site. |

```bash
cargo run -p tui-catalog   # the catalog in your terminal: ↑↓ widget, ←→ demo, tab to try it, t theme, q quit
cargo test --workspace     # library + catalog contract
npm ci && npm run build:site && npx vite preview   # the web catalog at http://localhost:4173/tui/
```

## Licence

Apache-2.0. Code derived from OpenAI Codex and Vercel fx keeps its Apache-2.0 notices; see [NOTICE](NOTICE). Not affiliated with or endorsed by OpenAI or Vercel.
