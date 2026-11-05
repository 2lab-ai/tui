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
- The catalog compiles the crate itself to WebAssembly (about 750 KB, 280 KB compressed, for 27 widgets and 85 demos, no imports) and paints each demo's cells in the page. What you see on the site is the Rust code, not a recording.
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
- Animation glyphs were meant to stay one cell wide on a Korean or Japanese terminal. Braille does; most block elements do not (see below).
- Animation is a pure function of one shared tick, so every spinner on screen moves together and tests can pin a frame.

What the extraction found and fixed:

- Tab, menu and settings hit rectangles were measured in `char`s, so a Korean label was clickable in the wrong place. The library measures in cells.
- Truncation re-measured the whole string for every character it removed. The library's `text::truncate` is linear.
- The gradient text split by `char`, so an emoji sequence got one colour per code point. The library splits by grapheme.
- The footer ignored the terminal width and was clipped. The key-hint widget drops low-priority hints first and says how many it dropped.
- Three of the six spinner families (the cooldown clock, the active pulse and the filling-up breath) used block elements such as `▌ ▀ ▄ ▂ █ ▒ ▓`. llmux's comment and test said the whole block-elements range is East-Asian Narrow; it is not — those glyphs are Ambiguous, so a terminal set to draw ambiguous glyphs wide (common with CJK locales) gave them two cells. The test only checked the code-point range. The library draws those three families in braille with the same motion, and its test checks `width_cjk() == 1`.
- Window-state markers (`● ○ ◑`) are also Ambiguous glyphs, against llmux's own narrow-glyph rule.
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
- Animated glyphs stay one cell wide even where a terminal draws East-Asian Ambiguous glyphs wide; the tests check `width_cjk() == 1`.
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

The module is about 750 KB (280 KB compressed) with 27 widgets and their 85 demos — the research estimate of 90 to 150 KB compressed was for the widgets alone; the demo data and the docs text roughly double it. It has no imports and loads in Node, where the site's tests render every demo through it in all four themes.

A panicking demo is contained. With `panic = "abort"` a Rust panic traps the call; the renderer takes a demo out of its slot before running it, so a trap leaves no open borrow and the other demos on the page keep working. The page shows the panic message where the demo was.

Text input goes through an invisible, labelled text field over the terminal, so Korean, Japanese and Chinese typing (which needs IME composition) reaches interactive demos. Tab moves on unless the demo uses it, and an Esc the demo does not use leaves, so the terminal is never a keyboard trap.

## Licensing

- The crate is Apache-2.0. Ported Codex and fx code is a derivative work under the Apache licence (translation from Zig counts), so it can only be offered on Apache terms. A dual "MIT or Apache-2.0" label would mislead.
- `NOTICE` names each upstream, its commit, its copyright line and every ported file. Each ported file starts with a header that names its upstream file and says it was modified.
- llmux code is 2lab's own (MIT); its licence text is kept in `licenses/llmux.LICENSE`.
- No upstream branding is copied: no Codex or fx logos, product colour names or bundled themes.

## Measuring like ratatui, and a ratatui wrap bug

The first draft of `text::width` measured a whole string with `unicode-width`, as llmux did. The review found it disagreeing with the buffer ratatui fills: ratatui measures grapheme by grapheme with its own `CellWidth` (which counts a halfwidth katakana sound mark as a cell) and drops any grapheme with a control character. A random sample of 270,000 CJK and emoji strings gave 4,190 under-counts. The library now measures exactly as ratatui draws, and its row count is ratatui's own (`Paragraph::line_count`), so a scroll limit reaches the last line exactly.

The same review found a bug in ratatui 0.30 itself: its word wrapper can emit a row wider than the area. A two-cell ideographic space (U+3000, the ordinary space in CJK text) at the end of a row makes the render write outside the buffer and panic (`" \u{3000}b"` at width 2), and at odd widths a two-cell glyph can straddle the edge (`가가가` at width 3 draws two rows, the second overflowing). `text::wrap_safe` turns wide breakable spaces into that many ASCII spaces before wrapping, which removes the panic; the straddling glyph remains an upstream issue to report.

## What we took, and from where

27 widgets, each with at least three demos. The [sources and licences](/tui/docs/licences) page links each one to its upstream file.

| Group | Widget | From | What it tells the person |
|---|---|---|---|
| Agent | Working | Codex | What the agent is doing now, for how long, and which key stops it |
| Agent | Shimmer | Codex | That a label is live — work behind it is still in progress |
| Agent | Status bar | fx | Mode, model, how full the context is, and what you just armed |
| Agent | Approval card | fx | What the agent wants to do, how risky it is, and your options |
| Agent | Banner and flash | llmux | Something is on until a deadline; whether the last action worked |
| Data | Meter | llmux | How much of a quota window is used or left and when it resets |
| Data | Mini bar | llmux | A row's share of the biggest one |
| Data | Ledger amount | llmux | Money lined up on the decimal point |
| Data | Status card | Codex | An account at a glance: facts and usage bars |
| Data | Trend chart | llmux | How each series moved day by day, with gaps where there is no data |
| Data | Packed grid | llmux | Many short records on few lines, fields aligned |
| Data | Fact table | fx | Numbers aligned in as many columns as fit |
| Data | JSON view | llmux | JSON coloured, wrapped and scrollable |
| Data | Diff view | fx | What changed, line by line, and how much |
| Input | Composer | fx | What you will send next |
| Input | Choice form | fx | A question with numbered options and a free-text answer |
| Input | Select list | Codex | Which one to use, and which is in effect now |
| Input | Multi-select | Codex | Which of these to include, with fuzzy filtering |
| Input | Completion menu | fx | The commands matching what you typed |
| Input | Key hints | Codex | Which keys do something here, fitted to the width |
| Input | Tabs | llmux | Which view you are on and how to get to the others |
| Input | Menu | llmux | What you can do to this thing, where you clicked |
| Primitives | Layout | Codex | Who gets the rows when there are not enough |
| Primitives | Pager | Codex | The whole text, and where you are in it |
| Primitives | Rule | fx | Where a band or section starts |
| Primitives | Spinner | llmux | That something is in progress, and which kind |
| Primitives | Gradient text | llmux | That a name is special |

Modules under the widgets: `text` (cells), `theme` (tokens, `decolor` for `NO_COLOR` hosts), `color` (blend, contrast, 256 and 16-colour fallback, from Codex), `format` (durations, counts, money, from llmux), `render` (Codex's measure-then-paint composition), `scroll`, `hit`, `osc52` (clipboard escape with tmux and screen passthrough), `snapshot` (buffer to text and styled runs).

## What llmux gained

llmux now depends on the crate and dropped its own copies:

- text cells, wrapping and the spinner families come from the crate; the char-count truncators that shifted rows holding Korean names are gone
- the tab strip, the animated model names, the raw viewer's JSON colours and every plain key bar are crate widgets
- the key bar fits the width: whole hints drop, least important first, with `… +N more`, instead of the terminal cutting a hint mid-word
- `NO_COLOR` keeps its weight: crossterm already dropped colour codes, but its empty colour sequence (`ESC[m`) is a full reset, so bold and reverse went with them (a live 80×30 frame kept 28 of 347 bold glyphs). The frame is now decoloured before output — 372 bold glyphs — and red text turns bold
- copying from the raw viewer works inside tmux and GNU screen
- the cooldown, pulse and filling-up spinners stay one cell wide on CJK terminals

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
