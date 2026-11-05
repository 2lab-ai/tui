# Research: what a terminal widget library for agent apps needs

ratatui gives you the cells, the layout and 17 basic widgets, but none of the widgets an agent or a quota dashboard needs. twolab-tui fills that gap with widgets taken from three working apps — llmux, OpenAI Codex and Vercel fx — under one theme, one motion setting and one width rule.

This report covers:

1. [The short answer](#the-short-answer)
2. [What terminal UI libraries already offer](#what-terminal-ui-libraries-already-offer)
3. [What agent and dashboard apps need that ratatui lacks](#what-agent-and-dashboard-apps-need-that-ratatui-lacks)
4. [What we learned from each source](#what-we-learned-from-each-source)
5. [The rules every widget keeps](#the-rules-every-widget-keeps)
6. [How the catalog runs terminals in a browser](#how-the-catalog-runs-terminals-in-a-browser)
7. [Licensing](#licensing)
8. [What we took, and from where](#what-we-took-and-from-where)
9. [What we left out, and why](#what-we-left-out-and-why)

Read sections 1 and 5 if you only want the decisions. Read section 4 before you port another widget. Every claim names its source; the research was done on 5 November 2026 against llmux `524b6e4`, Codex `e7f32cf1` and fx `412d841d`.

## The short answer

- Five ecosystems (ratatui and its community crates, Charm, Textual, Ink, OpenTUI) agree on a standard set: panel, text, selectable list, select, table, progress bar, spinner, text input, text area, scroll view, tabs, tree and a key-hint footer.
- ratatui 0.30 ships 17 primitives but no spinner, shimmer, key hints, toast, command palette, text input, scroll view, diff, markdown, status bar or modal. Community crates exist for most of these, but they are small, young, and share no theme, motion or focus model.
- The three apps we studied had already solved most of these, each in its own way. llmux had quota meters, spinners and tabs; Codex had shimmer, key hints, pickers and a pager; fx had the approval band, the question form, the diff view and the composer.
- The library's value is the shared rules, not any one widget: time is an input, colour comes from a theme, width is measured in cells, motion can be turned off, and nothing is cut without saying so.
- The catalog compiles the crate itself to WebAssembly (about 155 KB before compression, no imports) and paints each demo's cells in the page. What you see on the site is the Rust code, not a recording.
- The crate is Apache-2.0, because the Codex and fx code it carries can only be offered under Apache terms.

## What terminal UI libraries already offer

Each row is a kind of widget. A dash means it is not built in and we found no maintained crate.

| Widget kind | ratatui | ratatui community | Charm (Go) | Textual (Python) | Ink (JS) | OpenTUI |
|---|---|---|---|---|---|---|
| Panel | Block | tui-box-text | Lip Gloss | Static, Collapsible | Box | Box |
| Selectable list | List | tui-widget-list | list | ListView, OptionList | Select | Select |
| Table | Table | — | table | DataTable | — | TextTable |
| Tabs | Tabs | — | — | Tabs | — | TabSelect |
| Progress | Gauge, LineGauge | tui-chart | progress | ProgressBar | ProgressBar | — |
| Scroll view or pager | — | tui-scrollview | viewport | ScrollableContainer | — | ScrollBox |
| Text input | — | tui-input, tui-prompts | textinput | Input | TextInput | Input |
| Text area | — | ratatui-textarea | textarea | TextArea | — | Textarea |
| Multi-select | — | tui-checkbox | Huh MultiSelect | SelectionList | MultiSelect | — |
| Spinner | — | throbber-widgets-tui | spinner | LoadingIndicator | Spinner | — |
| Shimmer | — | tui-shimmer, tachyonfx | — | — | — | Timeline |
| Toast | — | ratatui-toaster | — | notify() | Alert | — |
| Command palette | — | ratatui-command-palette (experimental) | — | Command palette | — | — |
| Key hints | — | ratatui-which-key | help, key | Footer, KeyPanel | — | — |
| Diff | — | clankerdiff-ratatui | — | — | — | Diff |
| Markdown | — | tui-markdown (proof of concept) | Glamour | Markdown | — | Markdown |

Two findings matter for a library like this one:

- In June 2026 the ratatui organisation reserved about 250 crate names such as `ratatui-toast`, `ratatui-spinner` and `ratatui-help`, all still empty. So the gaps are known, and the `ratatui-*` names are taken. This crate is called `twolab-tui`.
- Widget libraries should not depend on a terminal backend. ratatui-core and ratatui-widgets are `no_std` with `std` opt-in, which is why this crate builds for the browser with no changes.

## What agent and dashboard apps need that ratatui lacks

These are the gaps we found in all five ecosystems when we looked at them as a coding agent or a quota dashboard would:

1. A spinner with a reduced-motion form.
2. A working line: what the agent is doing now and for how long (Codex's status row; `<ui-working>` on the web).
3. Key hints that fit the width, and a full help list.
4. A toast or flash that says whether the last action worked.
5. A pager or scroll view with a position marker.
6. A diff view with add and delete marks that survive no-colour terminals.
7. Approval and question prompts — what the agent wants to do and what it needs you to decide (`<ui-approval-card>` and `<ui-choice-form>` on the web).
8. Quota and usage readings: a meter with a reset time, a share bar, money lined up on the decimal point.
9. A heat list or trend chart that shows a gap where there is no data, not a zero.

## What we learned from each source

### llmux

llmux is 2lab's quota dashboard: a 38,000-line ratatui TUI (`src/tui/`). Its widgets are drawn from one read-only view plus a small chrome struct, and every draw returns the rectangles it drew so a mouse click can be matched against them.

What it does well, and what became library rules:

- No layout jitter. Widths are measured on the widest form a value can take (for example the longest countdown), so the layout only moves on events, not on clock ticks.
- Never cut silently. Titles say `+N more` or `N–M of T` when rows do not fit.
- Honest empties. Unknown values show `—`, never a made-up `0`.
- Animation glyphs are braille and block elements only. Both are East-Asian Narrow, so a spinner never pushes a Korean or Japanese table out of line.
- Animation is a pure function of one shared tick, so every spinner on screen moves together and tests can pin a frame.

What the extraction found and fixed:

- Tab, menu and settings hit rectangles were measured in `char`s, so a Korean label was clickable in the wrong place. The library measures in cells.
- Truncation re-measured the whole string for every character it removed. The library's `text::truncate` is linear.
- The gradient text split by `char`, so an emoji sequence got one colour per code point. The library splits by grapheme.
- The footer ignored the terminal width and was clipped. The key-hint widget drops low-priority hints first and says how many it dropped.
- Window-state markers (`● ○ ◑`) were East-Asian Ambiguous glyphs, against llmux's own narrow-glyph rule.
- About 300 styles were built inline from named colours, with three different "selected" styles. The library has one theme.

### OpenAI Codex

Codex's TUI (`codex-rs/tui`) is the largest ratatui agent UI we found, on ratatui 0.30.2 with no fork. Its useful parts for a library:

- A `Renderable` trait with `desired_height(width)` and three containers (column, flex, inset). Measuring before painting lets a modal size itself to its content.
- Shimmer: a band of light that sweeps across "Working". It blends towards the terminal's real foreground in true colour and falls back to dim, normal and bold bands elsewhere.
- Key hints rendered from the same bindings that match input, so the hint cannot drift from the key.
- One motion module with an explicit reduced form for every animation, seeded from the operating system's reduce-motion setting.
- A pager with a title rule, `~` past the end and a percentage, and a selectable list with a description column.
- Discipline: a lint bans raw RGB colours outside approved helpers.

What had to change in the port:

- Codex reads `Instant::now()` inside render. That panics in WebAssembly and ties tests to the clock. Every ported widget takes elapsed time instead.
- Palette, theme and motion live in process globals. The ports take them as arguments.
- Codex uses East-Asian Ambiguous glyphs (`•`, `└`, `─`, `…`, `→`). The library keeps narrow defaults and makes the others opt-in.

### Vercel fx

fx is a coding agent written in Zig with its own renderer: each row is an ANSI string, run through fx's own terminal emulator and diffed against a model of the screen. It runs inline — the conversation goes into the terminal's own scrollback, and a small band (activity row, composer, rule, hint row) lives at the bottom of the normal screen.

The terminal emulator and the diff are not worth porting; ratatui already does both. The value is in the presentation rules:

- The working row blinks one marker, locked to the elapsed clock, and the clock freezes while the agent waits for you.
- A status bar drops segments in a fixed order as the width shrinks.
- The approval band shows the command verbatim with numbered options; the question form adds a free-text row.
- The diff view keeps line numbers and `+`/`-` marks, so it still reads with no colour.
- The composer grows with its content up to a limit and follows the cursor.

2lab's own Rust port, xfx, had already turned many of these rules into Rust with tests. We used it as the specification and the test oracle, and rewrote the output as ratatui lines.

fx's browser demo compiles the whole CLI to WASI and points xterm.js at its output. That needs a WASI shim and JavaScript Promise Integration, so it only runs in recent Chrome and Edge. The catalog uses a lighter model (section 6).

## The rules every widget keeps

These are enforced by the catalog's contract test, which renders every demo at several sizes (down to 1×1), several times, in four themes, with and without reduced motion.

- Data in, cells out. A widget never reads a clock, the environment or the terminal. Time comes in as elapsed time or a shared tick, size as the area.
- Colour comes from a theme with the same token names as `@2lab.ai/ui`. The default theme uses the 16 named colours, so the user's terminal palette decides the hues and light or dark both work.
- No colour means no colour. Under `NO_COLOR` every cell keeps the terminal default, and meaning moves to bold, dim, reversed and glyphs. The test fails if any demo draws a coloured cell in this theme.
- Width is counted in terminal cells by grapheme, the unit ratatui renders. Clipped text ends in `…` and never splits a grapheme.
- Animated glyphs are East-Asian Narrow.
- Reduced motion holds one still frame that still says the same thing. Clocks keep counting: reduced motion stops movement, not information.
- Never cut silently, and show `—` for unknown.
- Every demo is deterministic for a given size, theme and time.

## How the catalog runs terminals in a browser

We compared four ways to show a ratatui widget on a web page:

| Approach | Live | Cost | Why not, or why |
|---|---|---|---|
| Recorded GIFs (VHS), as llmux and Charm do | No | One recording per change | Cannot resize or take keys |
| Ratzilla (ratatui's own web backend) | Yes | wasm-bindgen toolchain | Works, but a full backend for what is a cell export |
| The whole app on WASI with xterm.js, as fx does | Yes | WASI shim, JSPI, recent Chromium only | Built for one app, not a widget gallery |
| The crate compiled to `wasm32-unknown-unknown`, cells exported as JSON | Yes | None beyond rustup's wasm target | Chosen |

How it works:

1. The catalog crate exports a handful of C functions: open a demo, render it at a size, theme and time, send it a key, close it.
2. Rendering draws into a ratatui buffer, then exports styled runs. A run is either ASCII text in one style or exactly one non-ASCII glyph with its cell width.
3. The page gives every non-ASCII glyph a fixed box of its cell width, so box drawing, braille and Korean stay on the grid whatever the browser's fallback font measures.
4. Text enters the page as text nodes only. Nothing a widget draws is parsed as HTML.
5. The page passes the time in. `Instant::now()` panics in this target; a Rust panic is caught and shown as a message instead of a broken page.

The module is about 155 KB (58 KB compressed) with one widget and grows with each widget's demos; it has no imports and loads in Node for tests.

## Licensing

- The crate is Apache-2.0. Ported Codex and fx code is a derivative work under the Apache licence (translation from Zig counts), so it can only be offered on Apache terms. A dual "MIT or Apache-2.0" label would mislead.
- `NOTICE` names each upstream, its commit, its copyright line and every ported file. Each ported file starts with a header that names its upstream file and says it was modified.
- llmux code is 2lab's own (MIT); its licence text is kept in `licenses/llmux.LICENSE`.
- No upstream branding is copied: no Codex or fx logos, product colour names or bundled themes.

## What we took, and from where

The [sources and licences](/tui/docs/licences) page lists every widget with its upstream file, licence and whether it was ported (code carried over and adapted) or studied (behaviour re-written).

## What we left out, and why

- Terminal emulators, screen diffs and terminal-mode handling (fx, xfx, Codex's custom terminal). ratatui and the host app own these; widgets render into a buffer and work the same inline or full screen.
- Full markdown and syntax highlighting. Codex's and fx's renderers run to thousands of lines, and syntax highlighting needs a large grammar set that would multiply the catalog's size. A host that needs them should use a dedicated crate.
- A full multi-line editor with undo, selection and search. ratatui-textarea already does this well; the composer here covers what an agent prompt needs.
- Upstream width tables (fx ships its own generated Unicode tables). `unicode-width` matches what ratatui renders.
- Interface sounds. `@2lab.ai/ui` has them on the web; a terminal bell is the host's choice.

## Sources

- ratatui 0.30 and its widget crates: <https://ratatui.rs/highlights/v030/>, <https://github.com/ratatui/ratatui>
- Charm Bubbles, Lip Gloss and Huh: <https://github.com/charmbracelet/bubbles>, <https://github.com/charmbracelet/lipgloss>, <https://github.com/charmbracelet/huh>
- Textual widget gallery: <https://textual.textualize.io/widget_gallery/>
- Ink and ink-ui: <https://github.com/vadimdemedes/ink>, <https://github.com/vadimdemedes/ink-ui>
- OpenTUI: <https://github.com/anomalyco/opentui>
- OpenAI Codex TUI at `e7f32cf1`: <https://github.com/openai/codex/tree/e7f32cf1928a35a9232a633a51c2a03b5a7e2c72/codex-rs/tui>
- Vercel fx at `412d841d`: <https://github.com/vercel-labs/fx/tree/412d841d73dd6e1b8de551022728d9c929caae17>
- xfx, 2lab's Rust port of fx: <https://github.com/2lab-ai/xfx>
- llmux at `524b6e4`: <https://github.com/2lab-ai/llmux/tree/524b6e437fd9e67bf2d3fdd507267107291cb6ac/src/tui>
- NO_COLOR: <https://no-color.org/>
- Apache License 2.0: <https://www.apache.org/licenses/LICENSE-2.0>
