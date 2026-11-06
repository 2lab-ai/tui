# llmux dashboard vs the twolab-tui catalog — adoption audit

llmux main `eec32c9` (`src/tui/`, 38,299 lines) against twolab-tui `450a1b9` (27 widgets). Read-only audit, 2026-11-06.
Paths are relative to the llmux clone unless they start with `tui/` (the catalog clone) or `herdr/`.
Live evidence: 80×24 frames captured by attaching `llmux dashboard` (previous session's debug build, `LLMUX_DEMO_MODE=1`) to the running daemon; quoted in section 5.

## Summary

1. PR #231 took 7 catalog pieces (`grep twolab_tui:: src` = 25 hits): `text` cells, `SpinnerKind`, `GradientText`, `KeyHints` for plain key bars, `Tabs` for the main tab row, the `json_view` line highlighter, `theme::decolor`/`Theme::from_env`, `osc52`. Everything else the catalog ported *from* llmux (meter, mini_bar, ledger, packed_grid, trend_chart, menu, banner, format) still exists twice — the original in llmux and the fixed port in the crate.
2. The biggest user-visible gaps are prompts and pagers, not data cells: the reset gate keeps its subject only in the 5-second status line (`src/tui/mod.rs:184`, `:1332-1337`, `:3500-3524`), so after 5 s the footer reads `redeem ONE reset?` with no account; `remove selected?` never names the account (`src/tui/ui.rs:10626-10637`); the Logs overlay cannot scroll and shows a blank pane with no "no lines" message (`src/tui/ui.rs:9454-9468`, live frame 5.3).
3. At 80 columns the new-login picker loses OpenRouter, Gemini and its own Enter/Esc keys off the right edge (`src/tui/ui.rs:10648-10672`, frame 5.5); the config editor's labels run into their values (`per-account limitsper account`, `src/tui/ui.rs:3233`, frame 5.4); `?` help prints `a g U K p l saccounts` and drops its last rows silently (`src/tui/ui.rs:2606`, frame 5.2); the models table shows `243578.5` for 243.6 **billion** tokens because the `M` is clipped (`src/tui/format.rs:96-104`, column `src/tui/ui.rs:9990`, frame 5.1).
4. Plan, by user value: confirm gates (choice_form) > select_list (new login, config) > pager (logs, input/account modals) > key_hints for the last hand-built prompts and `?` help > multi_select (keys model filter) > format EXTEND (G/T units) > flash > menu > status_bar (header, settings bar) > status_card > trend_chart > tabs + json_view in the raw viewer > the dedup ports (meter, packed_grid, mini_bar, ledger) that change little on screen but move `FOLD_OFF_179X43_DIGEST` (`src/tui/ui.rs:19457`).
5. NOT-APPLICABLE: completion_menu, diff_view (no surface), composer as-is (EXTEND for a masked single-line field). Uncovered by the catalog and worth porting from herdr: a draggable scrollbar for tables/modals, content-fit popup sizing, a toast stack, mouse text selection/copy mode (mouse capture blocks the terminal's own selection), and a sidebar-style account list with state dots (section 4).

## 1. Ranked plan (ADOPT / EXTEND, highest user value first)

| # | Widget | llmux surface (file:line) | What the user gains | Effort | Risk (tests / digests that move) |
|---|---|---|---|---|---|
| 1 | choice_form (`no_other()`, 2 options); approval_card only after an EXTEND to 2 choices | reset gate `src/tui/mod.rs:3470-3576`, subject `:3500-3524`, `:4812-4829`, footer `src/tui/ui.rs:10638-10647`; remove gate `src/tui/mod.rs:4234-4262`, footer `src/tui/ui.rs:10626-10637`; config confirm `src/tui/ui.rs:3282-3288`, `src/tui/mod.rs:2961-2994` | The gate names its subject for as long as it is open (`Redeem ONE reset for a@x.com? 2 owned · 1 applicable`), lists `1 Redeem / 2 Cancel` with the digit and Enter/Esc it needs, and the "cannot be undone" note stays instead of expiring after 5 s; remove says which account | M | mod.rs `reset_gate_targets_a_codex_account_and_cancel_queues_nothing` (8760), `reset_gate_refuses_without_a_codex_account` (8808), `reset_confirm_follows_the_row_under_the_cursor_after_a_reorder` (8840), `accounts_overlay_houses_add_remove_switch_modes` (7571), `config_sched_mode_confirms_then_applies` (6826), `config_ceiling_zero_requires_confirm` (6851). Keep "any other key cancels" for remove. No digest |
| 2 | select_list | new-login picker in the footer `src/tui/ui.rs:10648-10672`, keys `src/tui/mod.rs:4149-4176`; config editor `src/tui/ui.rs:3154-3305`, keys `src/tui/mod.rs:2774-2804` | Login: one row per provider (5, `src/tui/mod.rs:234-241`) with digits 1–5, nothing off-screen at 80 cols. Config: label column measured on the widest label, read-only rows marked and skipped by the cursor, `N of M` + `▴/▾` when it scrolls, notes clipped with `…` | S (login) / M (config: keep value-cell click rects `src/tui/ui.rs:3240-3256`) | mod.rs `new_login_picker_*` (6054-6128), `misc_and_config_keys_round_trip` (7008); ui.rs config tests (2). No digest |
| 3 | pager | Logs `src/tui/ui.rs:1319-1322`, `:9454-9468`, keys `src/tui/mod.rs:2480-2486`; input modal `src/tui/ui.rs:3445-3489`; account modal `src/tui/ui.rs:4015-4073`; three copies of the scroll keys `src/tui/mod.rs:1921-1958`, `:2052-2093` | Logs: scroll back through the 500-line ring (`src/tui/logs.rs:12`), long lines wrap instead of being cut, `~` rows and a `NN%` position, an honest empty. Modals: a position instead of a bare `↑↓ scroll` | M | ui.rs logs (1), input modal (2), `account_modal_*` (4); mod.rs `account_click_opens_detail_modal_pinned_by_id` (6616), `account_modal_swallows_keys_and_clicks` (6657). No digest |
| 4 | key_hints (`KeyHints` + `KeyHelp`) | hand-built prompts `src/tui/ui.rs:10593-10672` (limits, add key, remove, reset, login); raw viewer legend `src/tui/ui.rs:4495-4498`; `?` help `src/tui/ui.rs:2602-2637` | Prompts fit like the other bars (whole hints drop, `… +N more`); the raw-viewer legend (~100 cells) stops being cut on both sides at 80 cols; `?` lays out every binding in aligned groups in 3/2/1 columns and counts what does not fit | S | ui.rs keybar (4) and footer (1) tests. FOLD digest unaffected (Normal-mode bar already KeyHints) |
| 5 | multi_select | keys `f` model picker `src/tui/ui.rs:2562-2600`, keys `src/tui/mod.rs:2741-2773` | Type to filter (fuzzy, matched letters highlighted), `k of n selected` correct under a filter, scrolled window says what it hides; popup sized to content (today 80%×70% for an empty list, frame 5.6) | S | mod.rs `f_picker_toggles_models_and_enter_applies_the_filter` (7353), `picker_all_clear_and_escape_leave_the_active_filter_alone` (7390), `the_model_picker_still_offers_options_while_a_query_is_in_flight` (7464); keep `a`/`c` as host keys |
| 6 | format (crate module) — EXTEND then ADOPT | llmux `src/tui/format.rs` (129 call sites); `human_count` `src/tui/format.rs:96-104`; models table tok column `src/tui/ui.rs:9954`, width 8 at `:9990` | Needs first: a `G`/`T` step in `tui/crates/twolab-tui/src/format.rs:164-175` (it promises "at most five cells" but returns `243578.5M` too). Then the models table reads `243.6G` instead of a clipped `243578.5`; `999_999` reads `1.0M` not `1000.0k`; NaN percent reads `—` | S | ui.rs strip (10) and cost (13) tests; **FOLD digest moves** only if the fixture has a value ≥ 999,950 |
| 7 | banner (`Flash`, `Banner`) | status row `src/tui/ui.rs:10421-10427`, `src/tui/mod.rs:1332-1341` (86 `set_status` sites); raw flash `src/tui/ui.rs:4489-4499`, `src/tui/mod.rs:635`, `:2109-2155`, `:2179-2185`; event banner `src/tui/ui.rs:350-384` | Every message says ok / warn / fail (today all yellow, `refresh failed` included); one TTL (5 s vs 3 s today); draw stops reading `Instant::now()` | S | ui.rs banner tests (5); mod.rs `control_status_lines_never_flatten_failures_into_success` (9124). No digest (fixture has no status) |
| 8 | menu | context menu `src/tui/ui.rs:3312-3404`, keys `src/tui/mod.rs:2490-2566`, `MenuChrome` `src/tui/ui.rs:7964-7978` | Opens above the row when clicked near the bottom (today clamped over the clicked row, `:3369-3373`); each item shows its key (`⏎`, `p`, `L`, `r`); `delete` styled destructive; items that cannot run shown disabled and skipped; width in cells (`chars().count()` at `:3357-3360`) | S | mod.rs `right_click_menu_runs_the_account_flows` (6303), `menu_action_follows_pinned_account_across_reorder` (6511); ui.rs menu (1) |
| 9 | status_bar — EXTEND for the settings bar | header `src/tui/ui.rs:4812-4836`, `:4854-4918`; group-settings bar `src/tui/ui.rs:529-662` | Header drops `pid` / port / version / uptime segments whole and keeps the verdict (today cut after `llmux 0.2.26`, frame 5.1). Settings bar (clickable `f m e` toggles) needs per-segment hit rects first; then it stops being cut mid-segment at 80 cols | S (header) / M (bar) | ui.rs header tests (7); **FOLD digest** moves if a style changes (text fits at 179) |
| 10 | status_card | account modal body `src/tui/ui.rs:3515-3958`, windows as text `:3684-3700`, rows `:3492-3512` | 5h / 7d / Fable windows get a bar of what is left with the reset beside it; long values wrap under the value column; `—` for absent windows | M | ui.rs `account_modal_*` (4). No digest |
| 11 | trend_chart | tokens per day `src/tui/ui.rs:815-934`; perf t/s `src/tui/ui.rs:1835-1980` | Under `NO_COLOR` series are told apart by `* + x o` (today identical lines); legend `●` (2 cells on CJK-wide terminals) goes; tokens chart stops zero-filling days before history (`:829-831`); one renderer for both | M | ui.rs chart (2), perf (1). No digest |
| 12 | tabs | raw viewer Request/Response tabs `src/tui/ui.rs:4608-4640` | Same look and click behaviour as the main tab row; widths in cells (today `t.label.len()`, `:4613`) | S | none pinned |
| 13 | json_view — EXTEND | raw viewer body `src/tui/ui.rs:4642-4697`, keys `src/tui/mod.rs:2052-2093` | `lines 12–40 of 213` footer; long strings wrap instead of `H/L` pan. Needs first: row heights cached per width (it re-wraps every line every frame, `tui/crates/twolab-tui/src/widgets/json_view/mod.rs:482-484`, `:499-505`; bodies reach 8 MiB) and a way to keep the General/headers sections | M | raw-modal tests; no digest |
| 14 | meter | `src/tui/ui.rs:5997-6042`, `:6054-6095`, `:6097-6134`, `:6136-6335` | In-bar reset text placed by grapheme in cells (today `chars()`, `:6006`); otherwise the same | M | ui.rs quota_bar (7) + gauge (12); **FOLD digest moves** on any style difference |
| 15 | packed_grid | fold `src/tui/ui.rs:5436-5895`; models strip `src/tui/ui.rs:9629-9904` | One implementation; per-entry click rects and `+N more` for the strip | L | ui.rs fold (22) + strip (10); **FOLD digest moves** |

Lower value: working (connecting screen `src/tui/ui.rs:4700-4733`: spinner + how long it has been retrying), shimmer (loading labels `src/tui/ui.rs:1345`, `:2460`, `:4580-4592`), mini_bar (`src/tui/format.rs:75-88`; `src/tui/ui.rs:9846`, `:9949`, `:10344`), ledger (`src/tui/ui.rs:1224-1317`), fact_table (model detail `src/tui/ui.rs:10159-10283`, misc facts `:2639-2658`), rule (28 `Borders::TOP` section rules), `scroll`, `hit`, `input`, `theme`. EXTEND: composer (single-line + masked mode for the API-key field), approval_card (2-choice mode).

## 2. Per-widget entries (catalog order)

### approval_card
- Catalog: a permission request as one card — kind + risk header, the action verbatim, three numbered choices (once / session / deny), recommended marked (`tui/crates/twolab-tui/src/widgets/approval_card/mod.rs:156-166`, `:345`).
- llmux today: not used. Destructive gates are hand-built footer lines: remove `src/tui/ui.rs:10626-10637`, reset `src/tui/ui.rs:10638-10647` with its subject in the expiring status line (`src/tui/mod.rs:3500-3524`, `:4812-4829`, TTL `:184`), config confirm `src/tui/ui.rs:3282-3288`.
- Gap: a `High` risk header for "spends a reset upstream and cannot be undone"; the subject stays visible.
- Verdict: EXTEND — decisions are fixed to three (`Decision::ALL`); llmux needs two (confirm / cancel). Until then use choice_form (rank 1).
- Effort S after the extension; risk as choice_form.

### banner
- Catalog: `Banner` = timed event + countdown; `Flash` = ok/warn/fail message that expires (`tui/.../banner/mod.rs:72-135`, `:201-333`).
- llmux today: not used; the catalog is a port of llmux's own code. Event banner `src/tui/ui.rs:350-384` (reads `format::local_offset_secs` itself); status row `src/tui/ui.rs:10421-10427` + `src/tui/mod.rs:1332-1341` (all messages yellow); raw-viewer flash `src/tui/ui.rs:4489-4499` (3 s, `Instant` read inside draw).
- Gap: failure reads differently from success (`fail` tag/colour, bold in mono); one lifetime; `flash_or` hands the row back to the key legend (`banner/mod.rs:333`).
- Verdict: ADOPT. 86 `set_status` call sites need an outcome; start by mapping the `*failed*` / `cancelled` sites.
- Effort S–M; risk: banner tests (5), `control_status_lines_never_flatten_failures_into_success` (`src/tui/mod.rs:9124`); event-banner text unchanged if the same `month_day_hm` format is kept.

### choice_form
- Catalog: a question, numbered options with the recommended one marked, optional free-text row; digit answers, Enter marked row, Esc cancels (`choice_form/mod.rs:294-392`).
- llmux today: not used. Same gates as approval_card above, plus the scheduler-mode confirm routed through `ConfigConfirm` (`src/tui/mod.rs:2961-2975`).
- Gap: see rank 1. Under a 24-row terminal the form windows options with `⋯ N more` rather than dropping the question.
- Verdict: ADOPT with `.no_other()`; draw it in the accounts overlay above the footer, keep the y/n keys as aliases.
- Effort M (three gates, one `Mode` handler each); risk: mod.rs gate tests listed in rank 1.

### completion_menu
- Catalog: inline menu of commands matching what you typed (exact > prefix > substring).
- llmux today: no typed command line anywhere; bindings are single letters (`src/tui/mod.rs:3239-3290` and per-overlay handlers).
- Verdict: NOT-APPLICABLE — needs a new `:`/`/` palette surface first. If one is ever added, this is the widget.

### composer
- Catalog: multi-line grapheme-aware input with caret, wrap, growth (`composer/mod.rs:75-573`).
- llmux today: three single-line inputs that only append/pop at the end: add-key `src/tui/mod.rs:4185-4204` (rendered masked `src/tui/ui.rs:10605-10625`), limits `src/tui/mod.rs:3927-3962` (`src/tui/ui.rs:10593-10604`), config edit `src/tui/mod.rs:2897-2959` (`src/tui/ui.rs:3271-3280`). No ←/→, Home/End, Ctrl-U; caret drawn as a literal `▏` at the end.
- Gap: move the caret, fix a typo mid-value, Korean values edited by grapheme.
- Verdict: EXTEND — needs a single-line mode (horizontal scroll, no wrap) and a mask mode that never renders the secret (AGENTS credential rule cited at `src/tui/ui.rs:10591-10592`). choice_form's one-line field is the alternative.
- Effort M; risk: `parse_limits_input_*` (`src/tui/mod.rs:6008`), add-account tests.

### diff_view
- Catalog: unified hunks with +/- and line numbers.
- llmux today: config edits are single values; `confirm_prompt` already states old → new (`src/tui/mod.rs:5893-5924`).
- Verdict: NOT-APPLICABLE — no file or multi-line change to review.

### fact_table
- Catalog: label/value facts packed into as many columns as fit, values right-aligned, units in their own column (`fact_table/mod.rs:128-208`).
- llmux today: not used. Model drill-down `src/tui/ui.rs:10159-10283` (requests / ok / err / tokens in, out, cache read, cache write / cost as free text); misc build/daemon facts `src/tui/ui.rs:2639-2658`; session detail `src/tui/ui.rs:1487-1559`.
- Gap: token counts line up on their last digit; on a wide pane two columns instead of a long ragged list; `—` for unknown.
- Verdict: ADOPT (low value).
- Effort S; risk: model detail tests only, no digest.

### gradient_text
- Catalog: per-grapheme drifting colour for special names.
- llmux today: ALREADY (PR #231) — `src/tui/ui.rs:9217-9240`, `:9318-9380`, `:13602`.
- Verdict: ALREADY.

### json_view
- Catalog: pretty-printed, coloured JSON, wrapped in cells, scrollable, `lines N–M of T` footer, scrollbar on overflow.
- llmux today: highlighter ALREADY (`src/tui/ui.rs:4211-4226`); the viewer around it is hand-rolled: 2048-byte chunking `src/tui/ui.rs:4171`, `:4228-4251`; slice + horizontal pan + ratatui scrollbars `src/tui/ui.rs:4642-4697`; keys `src/tui/mod.rs:2052-2093` (`H`/`L` pan by `RAW_PAN` `:5674`).
- Gap: a position footer; long strings wrapped at the width instead of chunked at 2048 bytes (chunking breaks string highlighting mid-token) and panned.
- Verdict: EXTEND — `JsonView` recomputes `wrapped_rows` for every line on every render (`json_view/mod.rs:482-484`, called twice per render at `:499-505`), which is too slow for 8 MiB bodies at the 120 ms tick (`src/tui/mod.rs:180`); needs a per-width row cache (the pager already has one) and must accept the General/headers lines (`src/tui/ui.rs:4174-4209`, `:4292-4365`) as plain lines.
- Effort M; risk: raw-modal tests, `raw_body_lines_*` ports.
- Row cache: done in `1cef220`. Correction to that commit's message, which says "a new or resized Vec is noticed on its own": it keyed the cache on the slice address, and a new body of the same length can reuse the old allocation, so it drew the old body's rows. The cache is now keyed on a host generation, the line count and the width; one `JsonViewState` per body, and `invalidate()` or `set_generation(n)` whenever the lines change.

### key_hints
- Catalog: `KeyHints` footer fitted to width by priority; `KeyHelp` grouped help in 3/2/1 columns.
- llmux today: ALREADY for plain bars (`src/tui/ui.rs:10379-10418`). Hand-rolled: five prompt lines `src/tui/ui.rs:10593-10672`; raw-viewer legend `src/tui/ui.rs:4495-4498`; `?` help as a fixed list with `{k:<10}` padding `src/tui/ui.rs:2602-2637` (13-cell key text overflows into its description, frame 5.2).
- Gap: prompts never cut mid-word; help built from the same hint data as the footers so it cannot drift (it already lists `R` as reload while the attached footer says `R disabled`).
- Verdict: ADOPT (finish the job).
- Effort S; risk: keybar tests (4); none of these lines are in the FOLD frame.

### layout
- Catalog: Codex measure-then-paint `Renderable`s (column / flex / row / inset).
- llmux today: ratatui `Layout` + its own planners (`MiddlePlan` `src/tui/ui.rs:6374-6408`, `band_geometry` `:6509-6567`, `centered_rect` `:3426-3437`, `overlay_rect` `:3414-3421`).
- Gap: none on its own; it matters only as the host of pager/select_list content.
- Verdict: NOT-APPLICABLE now; comes in with pager/select_list (their docs name llmux's account modal and raw viewer as planned hosts).

### ledger
- Catalog: money right-aligned on the decimal point, cents a tier dimmer, `—` for unknown.
- llmux today: not used; the catalog ported it from `usage_cost_cell` / `usage_cost_parts` / `group_thousands` `src/tui/ui.rs:1224-1317` (usage overlay). Other cost cells use plain `format_cost` (`src/tui/ui.rs:133-141`) — the models table shows `$112293.0` (cut from `$112293.04`, column 9 at `:9991`, frame 5.1).
- Gap: usage overlay unchanged; models table and keys panel cost columns could line up on the point and stop losing a digit.
- Verdict: ADOPT (dedup + widen the models/keys cost column to `Amount::width`).
- Effort S; risk: `usage_cost_*` tests (4), cost tests (13); FOLD digest moves if the strip's `$` column changes.

### menu
- Catalog: popup anchored at the click, flips above/left, keys per item, disabled/destructive items, rects in cells.
- llmux today: hand-rolled original `src/tui/ui.rs:3312-3404`, `MENU_ITEMS = 4` `src/tui/mod.rs:2490`, keys `:2494-2519`, run `:2533-2566`.
- Gap: rank 8.
- Verdict: ADOPT.
- Effort S; risk: the two mod.rs menu tests; the pinned-account rule (`src/tui/mod.rs:2545`) stays host logic.

### meter
- Catalog: reverse-video quota bar, reset text centred in it, percent label, thresholds.
- llmux today: hand-rolled original `src/tui/ui.rs:5997-6335` (`quota_bar_line`, `quota_bar_text`, `quota_cell_line`, `window_gauge_parts`, `GaugeParts`, `fable_gauge_parts`), level colours `src/tui/ui.rs:122-131`, `src/tui/format.rs:15-29`.
- Gap: grapheme/cell placement (no visible change for ASCII countdowns); one implementation.
- Verdict: ADOPT, after diffing `Meter::line` against `quota_cell_line` at the fixture's values.
- Effort M; risk: high — every accounts row is in `FOLD_OFF_179X43_DIGEST` (`src/tui/ui.rs:19457`), plus quota_bar (7) and gauge (12) tests.

### mini_bar
- Catalog: `▰▱` share bar, `—` when the share cannot be computed.
- llmux today: `format::gauge_bar` `src/tui/format.rs:75-88` at `src/tui/ui.rs:9846`, `:9949`, `:10344` (heatmap colours by `level_color` `:10345`).
- Gap: `max == 0` reads `—` instead of an empty bar that looks like 0 %.
- Verdict: ADOPT.
- Effort S; risk: FOLD digest (strip bars), gauge tests.

### multi_select
- Catalog: Codex multi-picker with fuzzy filter, `k of n selected`, require-one, reorder.
- llmux today: hand-rolled `src/tui/ui.rs:2562-2600`, state `src/tui/mod.rs:390-399`, keys `:2741-2773`.
- Gap: rank 5. Host bug the widget will not fix by itself: options are copied once at open (`src/tui/mod.rs:2689-2700`) and not refreshed when the first answer lands (`:2666-2676`), so `f` pressed during the first load shows "no models observed" over a panel full of models (frame 5.6).
- Verdict: ADOPT (+ rebuild items in `apply_keys_load` while the picker is open).
- Effort S; risk: three mod.rs picker tests.

### packed_grid
- Catalog: row-major packing with fields aligned per column, widths on the widest value, cursor, click rects, `+N more`.
- llmux today: hand-rolled originals twice — fold `src/tui/ui.rs:5436-5895` and models strip `src/tui/ui.rs:9629-9904` (`strip_columns`/`strip_per_line` duplicate `fold_columns`).
- Gap: one packer; strip gets click rects per entry.
- Verdict: ADOPT, late (largest digest blast radius).
- Effort L; risk: fold (22) + strip (10) tests and the FOLD digest.

### pager
- Catalog: title rule, wrapped content, `~` past the end, bottom rule with `NN%`, cached line heights, `Pager::key`.
- llmux today: not used. Logs tail-only `src/tui/ui.rs:9454-9468` (no scroll keys at all, `src/tui/mod.rs:2480-2486`); input modal `src/tui/ui.rs:3445-3489` and account modal `:4015-4073` (Paragraph scroll, max-scroll returned to the runtime); keys `src/tui/mod.rs:1921-1958`.
- Gap: rank 3.
- Verdict: ADOPT. Logs first (no scroll exists today), then the two modals.
- Effort M; risk: modal tests in ui.rs/mod.rs; the "close when the entry aged out" contract (`src/tui/ui.rs:3446-3453`) stays host logic.

### rule
- Catalog: one-row section rule with a label, ASCII fallback when box glyphs draw two cells.
- llmux today: 28 section headings drawn as `Block::new().borders(Borders::TOP).title(..)` (e.g. `src/tui/ui.rs:734`, `:1039`, `:2657`, `:9455`, `:10313`).
- Gap: on a CJK-wide terminal (`─` is East-Asian Ambiguous) the rules overflow; with `Rule::ambiguous_wide(true)` they fall back to `-`. Ratatui block borders elsewhere stay affected.
- Verdict: ADOPT only together with a host "ambiguous wide" setting; low value otherwise.
- Effort S–M (28 sites); risk: FOLD digest (band and strip rules are in the frame).

### select_list
- Catalog: pick-one list, `(current)`, disabled rows with reasons, search, digits 1–9, `N of M`.
- llmux today: not used. New-login picker `src/tui/ui.rs:10648-10672` + `src/tui/mod.rs:4134-4176`; config editor `src/tui/ui.rs:3154-3305` (fixed `{:<18}` label `:3233`/`:3237`, fixed value hit at `x + 22` `:3244`, scroll without indicator `:3185`).
- Gap: rank 2. Config rows map well: `description` = value + state, `disabled(reason)` = `ro` rows with their note.
- Verdict: ADOPT. Config needs value-cell click rects kept by the host (`ConfigHit` `src/tui/ui.rs:2731-2737`) — check the list returns row rects; if not, EXTEND with per-row rects.
- Effort S / M; risk: `new_login_picker_*` (6 tests), config tests.

### shimmer
- Catalog: a band of light across a live label; still under reduced motion.
- llmux today: not used. Loading labels: sessions spinner `src/tui/ui.rs:1345`, keys title `· loading…` `:2460`, raw loading `:4580-4592`.
- Gap: cosmetic.
- Verdict: ADOPT (low value), or skip.
- Effort S; risk: tests that look for `loading…` text (`src/tui/ui.rs:11306`, `:13173`, `:13194`) keep passing if the text is the same.

### spinner
- Catalog: six one-cell families.
- llmux today: ALREADY (`src/tui/ui.rs:19`, `:5897-5968`, `:8331-8342`, `:9568-9588`).
- Verdict: ALREADY.

### status_bar
- Catalog: one row of ` · `-joined segments that drop by priority, right-end cue.
- llmux today: not used. Header `src/tui/ui.rs:4812-4836` (falls back once to moving `cfg` into the tab row, then lets the terminal clip), lead spans `:4854-4918`; group-settings bar `src/tui/ui.rs:529-662` (clickable, widths by `chars().count()` `:537`, `:549`); footer status row `src/tui/ui.rs:10421-10427`.
- Gap: rank 9.
- Verdict: ADOPT for the header; EXTEND for the settings bar (segments need hit rects from `layout`).
- Effort S / M; risk: header tests (7), FOLD digest if styles change.

### status_card
- Catalog: aligned label/value rows plus a compact bar per limit with reset, value wrap under its column.
- llmux today: not used. Account modal `src/tui/ui.rs:3515-3958` (sections `identity` … `resets`, `:3563-3858`), windows printed as `util 0.123 · eff 0.456 · resets …` `:3690`; the `detail` band under the accounts table `src/tui/ui.rs:7521-7889`.
- Gap: rank 10.
- Verdict: ADOPT for the modal; the band has a fixed 6-row budget (`BAND_ROWS` `src/tui/ui.rs:6361`) and is in the FOLD frame — leave it.
- Effort M; risk: `account_modal_*` tests (4).

### tabs
- Catalog: tab strip with cell-measured click rects, scroll to active, hidden count.
- llmux today: ALREADY for the main row (`src/tui/ui.rs:664-693`). Hand-rolled inside the raw viewer `src/tui/ui.rs:4608-4640`.
- Verdict: ALREADY (main) / ADOPT (raw viewer).
- Effort S; risk: none pinned.

### trend_chart
- Catalog: braille lines for ≤4 series, gaps for missing days, dim low-sample points, glyph per series without colour.
- llmux today: not used; ported from llmux's perf chart `src/tui/ui.rs:1835-1980`. Tokens chart `src/tui/ui.rs:815-934` zero-fills (`:829-831`, `:848-853`), colour-only series `DAILY_CHART_COLORS` `:807`.
- Gap: rank 11.
- Verdict: ADOPT.
- Effort M; risk: chart (2) + perf (1) tests.

### working
- Catalog: status row with spinner, shimmer header, elapsed time, interrupt key, wrapped detail.
- llmux today: no agent turn. Nearest surface: attach connecting screen `src/tui/ui.rs:4700-4733` — static yellow text, no elapsed time.
- Gap: "connecting to daemon — retrying… 42s · q quit" with a spinner.
- Verdict: ADOPT for the connecting screen only; NOT-APPLICABLE for activity rows (table columns; a proxied request cannot be interrupted from the dashboard).
- Effort S; risk: connecting test (1).

### Crate modules

| Module | llmux today | Gain | Verdict | Effort / risk |
|---|---|---|---|---|
| `scroll` (`ScrollState`, `ListAction`) | cursor lists hand-clamped: models `src/tui/mod.rs:3159-3166`, sessions `:3171-3213`, perf `:2568-2613`, config `:2774-2804`, keys panel `KEYS_PAGE` `:416` | PgUp/PgDn/Home/End behave the same in every list; arrives with select_list/multi_select anyway | ADOPT (with #2/#5) | S; mod.rs `keys_scrolling_covers_every_row_and_clamps_at_both_ends` (7326), `usage_scroll_clamps_stored_offset_no_overscroll_debt` (7096) |
| `format` | own copy `src/tui/format.rs` (129 call sites), `format_cost` `src/tui/ui.rs:133-141`, `group_thousands` `:1306-1317` | see rank 6 | EXTEND (G/T units) then ADOPT | S; FOLD digest if a fixture value crosses 999,950 |
| `hit` (`HitMap`) | ten hit structs `src/tui/ui.rs:2731`, `:7907-8131`; per-surface lookups in `on_mouse` `src/tui/mod.rs:1528-1900`; char-count widths `src/tui/ui.rs:537`, `:549` | settings-bar clicks land right on non-ASCII labels; one lookup path | ADOPT incrementally (new widgets first) | M; mouse tests in mod.rs |
| `input` (`KeyInput`) | crossterm `KeyCode` matched directly in every handler (`src/tui/mod.rs:1434-1525`) | prerequisite for `Pager::key`, `SelectList::key`, `MultiSelect::key`, `ChoiceForm::key` | ADOPT (enable the crate's `crossterm` feature) | S; none |
| `theme` | partly ALREADY (`Theme::from_env` `src/tui/mod.rs:60-63`, `decolor` `:5521`, `KEYBAR_THEME` `src/tui/ui.rs:10414-10417`); 149 literal `Color::` and 161 `Style::new()` left in drawing code (`src/tui/ui.rs:1-10677`); four "selected" looks: config `REVERSED` `:3234`, picker black-on-cyan `:2585`, menu cyan `REVERSED|BOLD` `:3387`, raw tab black-on-cyan `:4628` | one selected style; `NO_COLOR` keeps weight everywhere, not only after `decolor` | ADOPT gradually (each widget brings it) | L overall; FOLD digest on every MAIN change |

## 3. Risk notes for the plan

- `FOLD_OFF_179X43_DIGEST` (`src/tui/ui.rs:19457`, test `:19462-19472`, fixture `fold_view` `:19349` on `recompose_view`) pins MAIN at 179×43 — header, tabs, accounts table, band, models strip, activity, footer. Ranks 1–5, 7, 8, 10–13 do not touch it. Ranks 6, 9, 14, 15 and mini_bar/ledger/rule do; re-pin with a frame diff in the PR as #231 did (`:19434-19456`).
- Ranks 1–5 replace `Mode` handlers in `src/tui/mod.rs`; their tests drive keys, not frames, and should survive with key aliases (`y`, `n`, `Esc`) kept.
- Every stateful widget takes `KeyInput`; do `input` first.

## 4. Pain points no catalog widget covers yet

| Pain point | Where (file:line) | herdr source that would cover it |
|---|---|---|
| Mouse capture is always on (`EnableMouseCapture` `src/tui/mod.rs:5166`), so the terminal's own selection is gone: no way to copy an account email, an error from the status line or an activity row — only the raw viewer has copy buttons (`src/tui/ui.rs:4504-4570`) | `src/tui/mod.rs:5156-5176` | copy mode + selection: `herdr/src/copy_mode.rs`, `herdr/src/selection.rs` (drag-select, word on double-click, copy via the existing `osc52`) |
| Status messages are one row, cut at the right edge (`refresh failed: error sending request for url (http://localhost:3456/llmux/refr`, frame 5.5), overwritten by the next message and gone after 5 s; a 5-account refresh result with several failures cannot be read | `src/tui/ui.rs:10421-10427`, `src/tui/mod.rs:184`, `:1332-1341`, `refresh_message` `:4684-4724` | toasts: `herdr/src/ui/status.rs` (positioned toast, `ToastClipboardPosition`) — a stack with per-toast TTL |
| Popups sized by percentage, not content: 80%×80% input modal for a two-line excerpt (`src/tui/ui.rs:3456`), keys picker 80%×70% for an empty list (`:2568`, frame 5.6), account modal 80%×85% (`:4028`), raw viewer 94%×92% (`:4483`) | `centered_rect` `src/tui/ui.rs:3426-3437` | popup sizing: `herdr/src/popup_size.rs` (`resolve_popup_geometry`) |
| Tables scroll with no scrollbar and the mouse cannot drag one; the raw viewer's ratatui scrollbars are display-only | keys panel `src/tui/ui.rs:2498-2558` (title-only `rows a-b/n`), sessions `:1381-1485`, usage `:1004-1200`, raw viewer `:4660-4697` | scrollbar: `herdr/src/ui/scrollbar.rs` (`scrollbar_thumb`, `scrollbar_offset_from_drag_row`) |
| Context menu: the catalog `menu` covers it; herdr's adds nothing llmux needs beyond flip/clamp | `src/tui/ui.rs:3312-3404` | covered by catalog `menu` |
| Accounts at 80 cols: 29 accounts, table shows 1–4 rows on MAIN with `+25 more` (frame 5.1) and the right columns cut (`rst if`); a narrow sidebar list with one state dot per account would show all of them | `draw_accounts_with` `src/tui/ui.rs:5113-5260`, `table_layout` `:4967-5076` | sidebar list with state dots: `herdr/src/ui/sidebar.rs` (note: `● ○` are Ambiguous — use braille/quadrant dots per the catalog rule) |
| Summary band lines are cut at the right edge with no `…` (`→ re-evaluati`, `backoff claude:ai15@ins`, frame 5.1) | `band_left_lines` `src/tui/ui.rs:6970-6981`, `draw_band` `:7046-7137` | none needed — `text::truncate_line` (already a dependency) |
| Logs overlay in attach mode renders an empty pane with no message (frame 5.3) | `src/tui/ui.rs:9454-9468` | catalog `pager` + an honest empty line |
| Keys picker options frozen at open (shows "no models observed" over a full panel) | `src/tui/mod.rs:2666-2700` | host fix (rebuild items on load) |
| The `?` help is hand-maintained and already disagrees with the attached footer (`R` reload vs `R disabled (attached)`) | `src/tui/ui.rs:2628` vs `:10458-10463` | catalog `KeyHelp` fed from the footer hint tables |

## 5. Live frames (80×24, attached, `LLMUX_DEMO_MODE=1`)

Captured with a pty + pyte (a throwaway script outside the repos) driving a debug build of `llmux dashboard`. Keys sent: `?`, `l`, `K`, `f`, `c`, `a`, `n`, `Esc`, `q`. Note: entering the accounts tab (`a`) fires the automatic usage refresh (`accounts_entry_refresh_due`, `src/tui/mod.rs:4660-4682`); that request failed to send (5.5), so nothing changed on the daemon. No further captures opened the accounts tab.

5.1 MAIN
```
00| [FAIL] POLLER STALE last ok 17h30m claude:a1@example.com +2  llmux 0.2.26|
03|   group   #  account       status   7d RESET    LEFT  7d Fbl RESET LEFT rst if |
07|          codex  codex:c1@example.com  codex:c2@example.com → re-evaluati|
11| poller   21 oauth · last ok 19s–17h30m ago · next ~1s · backoff claude:ai15@ins|
13|   group   model                         req     tok      $         last        |
14| ⠼ CLAUDE  claude-opus-5-5               698.3k  243578.5 $112293.0 4 in-flight |
21| sched default  │  claude 21 acc  │  codex gpt-6-astra effort:bypass fast:off  │|
23| q quit  R disabled (attached)  u used/left  t eta/utc  o sort  … +2 more       |
```
5.2 `?` (misc)
```
04|   a g U K p l saccounts / stats / usage / keys / perf / logs / sessions        |
06|   click     tabs switch · activity row expands · config value edits · session s|
21|   config ~/.config/llmux.json                                   |
23| misc — ?/Esc back  q quit                                                      |
```
(the `accounts N · raw-io …` line after `config` is dropped with no marker)

5.3 `l` (logs): rows 03–22 blank, title ` logs ─────`, footer `logs — l/Esc back  q quit`.

5.4 `c` (config)
```
09|   per-account limitsper account                  ro       accounts tab [L]     |
10|   paused accounts   per account                  ro       accounts tab / contex|
16|   token url / client model / traceconfig file                  ro       codex.*|
```
5.5 `a` then `n` (new login)
```
22| refresh failed: error sending request for url (http://localhost:3456/llmux/refr|
23| new login — [Claude (Anthropic OAuth)]  Codex (ChatGPT OAuth)   Grok (xAI OAuth|
```
5.6 `K` then `f` (keys model picker opened while the first query was loading)
```
05|│  └ cla┌ models — 0 selected ─────────────────────────────────────────┐.31    │|
06|│key-a (│  no models observed in this window                           │9271.69│|
08|│  └ cod│ Space toggle · a all · c clear · Enter apply · Esc cancel    │097.76 │|
18|│  └ gro└──────────────────────────────────────────────────────────────┘.79    │|
```
