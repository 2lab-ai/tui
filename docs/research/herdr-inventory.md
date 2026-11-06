# herdr TUI inventory and health check

herdr `ee5bfb5` (Apache-2.0; ratatui 0.30.0, crossterm 0.29, unicode-width 0.2.2, unicode-segmentation 1.13.3), audited on 6 November 2026 against twolab-tui's 27 widgets. All paths are relative to the herdr clone; `shell/` means `src/client/shell/`. Both repos were read only. Defects marked "probe" were reproduced by copying the herdr function verbatim into a scratch crate outside both repos (ratatui 0.30.0, offline), which was deleted afterwards; herdr itself was not run.

## Summary

1. herdr paints its whole shell (sidebar, tab bar, pane borders, overlays, toasts) straight into a `Buffer` through `put_text`/`set_stringn` helpers and a 19-token `Palette`; there is no `impl Widget` in the UI code, so nothing ports as-is.
2. Four generic patterns are missing from twolab-tui and worth porting: a modal frame with popup sizing, a positioned toast with a queue, an agent list with state glyphs, and a scrollbar with mouse maths. A fifth (shared pane borders) is mostly covered by ratatui 0.30's `Block::merge_borders`.
3. Nine more are features for existing widgets: `composer` (single-line mode, yank), `status_bar` (fair-share shrink), `select_list` (tree rows, width-responsive columns), `pager` (drag selection, release-notes lines), `tabs` (scroll buttons, drop marker), `key_hints` (filterable help), `theme` (selection colour from the host background), `menu` (badge, open above).
4. Health: one reproducible panic (the copy toast leaves the buffer at small heights), one parser panic on non-ASCII input, a wide-character corruption in the agents header, grapheme-splitting truncation, `NO_COLOR` ignored with truecolor always on, and several states told apart by colour alone.
5. The terminal-bound parts (copy-mode motions, link hover, tab surface, pane resizing, OSC 52 environment sniffing) need herdr's runtime and server round-trips; skip them.

## Ranked PORT / MERGE candidates

| # | Verdict | Candidate | Main herdr source | Effort |
|---|---|---|---|---|
| 1 | PORT | `modal`: centred dialog sized in cells or percent, dim backdrop, header/body/footer/actions stack, close button, button row with hit rects, scrolling body with a scrollbar | `src/ui/widgets.rs:15-109`, `src/popup_size.rs:7-90`, `shell/overlays.rs:45-58, 221-307, 308-422, 1256-1317` | M |
| 2 | PORT | `toast`: corner-anchored card, one-row bottom banner on narrow screens, stacking offsets that avoid overlap, FIFO queue with a TTL per kind | `shell/notifications.rs:10-206`, `shell/notification_policy.rs:3-63`, `src/ui/status.rs:15-125`, `src/ui.rs:133-153`, `shell/endpoint_notices.rs:22-80` | M |
| 3 | PORT | `agent_list`: agents with a state glyph, one or more token rows each, priority sort, row gaps, scrollbar, a collapsed index rail, a one-line tally | `shell/agent_sidebar.rs:20-374`, `src/client/shell.rs:178-238`, `shell/sidebar.rs:47-197`, `shell/mobile.rs:236-361` | M |
| 4 | PORT | `scrollbar`: thumb maths, click-to-centre, drag with grab offset, metrics for lists whose rows differ in height | `src/ui/scrollbar.rs:37-184`, `shell/scroll.rs:5-100` | S |
| 5 | MERGE-INTO composer | single-line mode with a horizontal window that keeps the caret visible; kill buffer and Ctrl+Y yank; `replace_on_type` (preset text replaced by the first keystroke) | `shell/text_editor.rs:5-292` | S |
| 6 | MERGE-INTO status_bar | fair-share fit: each flexible segment keeps 1 cell, spare cells go round-robin, each cut with `…`, whole segments drop only when the minimums do not fit | `src/ui/sidebar.rs:106-275` | S |
| 7 | MERGE-INTO select_list | tree rows (`├─ └─ │`, depth indent, `▸/▾` group toggles), meta columns that appear by width (24/12/0 cells), detail lines for the selected row, filter chips | `shell/overlays.rs:704-1051`, `shell/sidebar.rs:458-788` | M |
| 8 | MERGE-INTO pager | mouse drag selection that stays put while scrolling (Anchored → Dragging → Done, at most 3 highlight rects), edge autoscroll, copy through `osc52` | `src/selection.rs:20-297`, `shell/mouse.rs:141-428`, `src/ui/panes.rs:806-830` | M |
| 9 | MERGE-INTO tabs | clickable `<` `>` scroll buttons, trailing `+` action, drag-reorder drop marker and midpoint drop index, centre-the-focused-tab reveal | `shell/tabs.rs:7-372`, `shell/mouse.rs:453-509` | S |
| 10 | MERGE-INTO key_hints | filter the help grid by a query (command or shortcut), scroll it with a scrollbar | `shell/overlays.rs:1053-1254`, `src/input/keybind_help.rs:75-251` | S |
| 11 | MERGE-INTO theme | selection style from the host background: mix 28 % toward white or black by luminance, foreground by contrast, palette fallback | `src/ui/panes.rs:832-938` | S |
| 12 | MERGE-INTO pager | `notes` lines: `###` → upper-case accent heading, `- ` → bullet, `` `code` `` styled (non-breaking spaces when it holds `=`), fenced block with a gutter | `src/ui/release_notes.rs:13-142` | S |
| 13 | MERGE-INTO menu | per-item attention badge, open above a launcher button | `shell/global_menu.rs:4-55`, `shell/overlays.rs:86-163` | S |
| 14 | PORT (low) | `split_frame`: shared borders of adjacent panes as `┼ ├ ┤ ┬ ┴`, the focused pane's edges in accent, a title in each top edge, optional gaps | `src/ui/panes.rs:563-804` | M |

Why this order: 1–4 are what an agent dashboard draws on every screen and twolab has no equivalent; 5–11 are cheap additions to widgets that already exist; 12–13 are small; 14 is mostly ratatui's own `Block::merge_borders` (`ratatui-widgets-0.3.0/src/block.rs:701`) plus focus colouring.

---

## Per-pattern detail

Common facts, so they are not repeated below:
- **Drawing**: almost everything goes through `put_text`/`put_segment`/`put_right_text` (`shell/render.rs:343-377`), which call `Buffer::set_stringn` and clip without `…`. Width is `UnicodeWidthStr::width` of the whole string (`render.rs:375-377`, `src/ui/text.rs:3-5`), not ratatui's per-grapheme measure.
- **Colour**: `Palette` (`src/app/state.rs:35-74`, 19 tokens, 18 named themes at 78-527, overrides at 552+). Every style is built inline from a palette field. There is no `NO_COLOR` handling and no mono palette (H3).
- **Mouse**: each render returns rects into `ShellHitMap` or `OverlayRender` (`shell/overlays.rs:6-34`, 25 fields); input code tests them. This matches twolab's `hit` module.
- **Time**: `std::time::Instant::now()` in shell state (`shell/notification_policy.rs:36,81`, `shell/state.rs:1822-1829`), never inside a render function.

### P1 Modal dialogs and popup sizing — PORT `modal` (#1)
- **What the user sees**: a bordered panel centred over a dimmed screen, with a title, a close button, a body that scrolls, a footer hint line and buttons (`↵ save`, `^c clear`, `esc cancel`).
- **Sources**: `centered_popup_rect` `src/ui/widgets.rs:15-28`; `modal_stack_areas` (header/content/footer/actions with gaps) `widgets.rs:30-88`; button rects `widgets.rs:90-109`; backdrop dim `shell/overlays.rs:45-58`; `panel` (border and fill) `overlays.rs:221-263`; `popup`/`popup_with_width_cap` `264-280`; `button` `281-285`; button row `row` `286-301`; release notes `308-422`; announcement `424-538`; onboarding `540-642`; rename `644-702`; confirm-close (red border) `1256-1317`; `PopupSize::{Cells, Percent}` and `resolve_popup_geometry` `src/popup_size.rs:7-90` (used for popup panes, `shell/composition.rs:561-575`).
- **How it is built**: hand-drawn box characters into `Buffer`; body is `Paragraph::wrap(trim: false).scroll(..)`, scroll range from `Paragraph::line_count` (`src/ui/release_notes.rs:196-252`); scrollbar column reserved only when content overflows (`release_notes.rs:205-209`). State is `scroll: u16` per overlay. Width: `display_width` for labels, but button width by `chars().count()` (`widgets.rs:92-93`). Narrow: `popup` returns `None` below 4×4 (`overlays.rs:271-273`), body hidden below inner 20×8 (`overlays.rs:318-323`), onboarding needs 11 inner rows (`overlays.rs:553`), `resolve_popup_geometry` keeps at least 6×4.
- **Coupling to cut**: `Palette`, `ClientShellOverlay`, the 25-field `OverlayRender`, product strings and sizes (`RELEASE_NOTES_MODAL_SIZE` 80×24, `release_notes.rs:10-11`).
- **Overlap**: new. `approval_card` and `choice_form` are bands, not frames; `layout` can supply the vertical stack.
- **API sketch**:
  ```rust
  let modal = Modal::new("v0.9.5")
      .subtitle("what's new in this release")
      .size(Size::Cells(80), Size::Percent(80))      // clamped to the area, min 6×4
      .body(lines)                                   // Vec<Line<'static>>, wraps, scrolls
      .footer(KeyHints::new([KeyHint::new("esc", "close")]))
      .actions([Button::primary("enter", "save"), Button::new("esc", "cancel")])
      .tone(None /* or Some(Status::Failed) for a destructive confirm */)
      .scrim(true)                                   // dims what is under it; mono keeps DIM
      .theme(theme);
  let layout: ModalLayout = modal.render(area, buf, &mut ModalState);   // outer, body, scrollbar, buttons: Vec<(Rect, usize)>
  state.key(key) -> ModalOutcome; state.scroll_to(rows)
  ```

### P2 Toasts, banners and notices — PORT `toast` (#2)
- **What the user sees**: a small card in a corner (`● claude needs attention` / body), a `● copied to clipboard` box, yellow config-error rows at the top right, a `◐ host · reconnecting` pill, and on narrow screens a one-row bottom banner (`claude waiting · repo`).
- **Sources**: card `shell/notifications.rs:107-178`; kind → colour `180-206`; bottom banner and title rewrite (`needs attention` → `waiting`) `10-105`; queue (cap 8, oldest dropped) and TTLs (attention 8 s, finished 5 s, update 3 s) `shell/notification_policy.rs:3-63`; copy toast with 6 positions `src/ui/status.rs:15-88`; config diagnostic rows `status.rs:90-125`; lifecycle pill and notice card `shell/endpoint_notices.rs:22-80`; overlap offset `src/ui.rs:133-153`; composition order and occlusion `shell/composition.rs:455-557`.
- **How it is built**: `Clear` + `Block::bordered` + `Paragraph` lines. Card width = widest of title and body + 6 (`notifications.rs:120-125`), clamped inside the area (`133-142`, test `457-478`). The copy toast is not clamped (H1). The kind is shown only by the dot's colour (H2).
- **Coupling to cut**: `ClientVisibleNotification`, `SemanticNotificationKind`, endpoint ids, config positions, `Instant`.
- **Overlap**: `banner::Flash` covers a one-line outcome; a positioned card with a queue is new.
- **API sketch**:
  ```rust
  let t = Toast::new(Status::Waiting, "claude needs attention").body("repo · tab 2").theme(theme);
  let rect = t.render_at(bounds, Corner::TopRight, offset_rows, buf);   // always inside bounds
  let mut q = ToastQueue::new().cap(8);
  q.push(t, ttl, now);              // now: Duration since start, passed in
  q.tick(now); q.visible()          // Option<&Toast>
  toast::stack(Corner::TopRight, &[taken_rects]) -> u16   // offset that avoids overlap
  ```
  Kind must show as a glyph or word too, never colour alone; pick glyphs with `width_cjk() == 1` (`!`, `✓`, `◉`), not `●` or `•`.

### P3 Scrollbar — PORT `scrollbar` (#4)
- **What the user sees**: a one-column track with a thumb on panes, lists, the navigator, help and release notes; it can be clicked and dragged.
- **Sources**: `scrollbar_thumb` `src/ui/scrollbar.rs:37-71`; grab offset, click-to-centre, drag `73-134`; render `136-184`; list metrics for rows of different heights with gaps `shell/scroll.rs:5-54`; reveal-a-row `56-77`; list render `79-100`; pane drag mapping `shell/mouse.rs:38-52`. Duplicated inline for help `shell/overlays.rs:1219-1229`.
- **How it is built**: `ScrollMetrics { offset_from_bottom, max_offset_from_bottom, viewport_rows }` (scrollback convention); thumb length `round(viewport*track/total)` ≥ 1. Glyphs `▕` track, `▐` thumb (focused). Narrow: no thumb when the track is 0 rows or nothing overflows.
- **Coupling**: only `ScrollMetrics` and palette colours; very portable.
- **Overlap**: `json_view` uses ratatui's `Scrollbar` (no mouse); `scroll::ScrollState` handles keys. Mouse maths are new.
- **API sketch**: `Scrollbar::new(ScrollPos { top, max, viewport }).theme(theme).render(track, buf)`; `scrollbar::thumb(pos, track)`, `scrollbar::offset_at(pos, track, row)`, `scrollbar::grab(pos, track, row) -> Option<u16>`, `scrollbar::offset_dragged(pos, track, row, grab)`; `scrollbar::list_pos(&heights, &gaps, body_h, start)`. Thumb must differ from the track by glyph or `REVERSED` under mono (H2); `▕` is Ambiguous, `▐` is not (H9).

### P4 Agent list (sidebar "agents" panel) — PORT `agent_list` (#3)
- **What the user sees**: under a rule, ` agents` with a `grouped`/`priority` toggle; one block per agent: state glyph, agent name, workspace · tab · pane, optional second row; the focused agent highlighted; a scrollbar when it overflows. Collapsed, the sidebar is a 4-column rail of `1 ●`; on narrow screens a tally `◉ 2 blocked · ● 1 done · 3 working …`.
- **Sources**: order (priority blocked > done > working > idle > unknown, then newest change) `shell/agent_sidebar.rs:20-50`, `src/client/shell.rs:202-211`; header and toggle `agent_sidebar.rs:90-152`; list with variable heights and gaps `154-235`; row data `237-319`; row render `321-374`; glyph/colour/word tables `src/client/shell.rs:178-238`; rail `shell/sidebar.rs:47-197`; tally with `…` when it does not fit `shell/mobile.rs:236-361`.
- **How it is built**: `Paragraph` per row line from `resolved_token_spans` (P6); hits `(Rect, pane_id)`; `agent_scroll: usize`. Width: `display_width` plus a local char-per-cell `put_text` (`agent_sidebar.rs:376-382`, H4). Narrow: header needs 2 rows; rows that do not fit are skipped with no count (H8).
- **Coupling**: `ClientShellSnapshot`, `AgentStatus`, sidebar token config, `config.agents.row_gap`.
- **Overlap**: `select_list` is single-line rows; `spinner` can supply the working glyph. herdr's `AgentStatus` maps onto twolab `Status` (Working→Running, Blocked→Waiting, Done→Done) plus Idle and Unknown.
- **API sketch**:
  ```rust
  AgentList::new(items)                      // AgentItem::new(state, "claude").line(spans).focused(true)
      .sort(Sort::Priority).gap(1)
      .frame(tick)                           // working glyph animates; theme.motion == Reduced holds one frame
      .theme(theme)
      .render_hits(area, buf, &mut ScrollState) -> Vec<(Rect, usize)>;
  AgentList::rail(&items)                    // collapsed: index + glyph
  agent_list::tally(&states, width) -> Line<'static>   // drops whole counts, ends with "…"
  ```
  Glyphs must differ per state and pass `width_cjk() == 1`; herdr's `● ○ ◐ · ×` all fail that (H9) and three states share `●` (H2).

### P5 Workspace ("spaces") list — part of MERGE #7
- **What the user sees**: ` spaces`, one block per workspace with state glyph, name, branch, `↑2 ↓1`; linked git worktrees indented under their parent with `├─ └─ │` and a `▸/▾` toggle; an accent line where a dragged workspace will drop; ` new` and `menu` in the footer.
- **Sources**: render `shell/sidebar.rs:199-456`; grouping by worktree key `458-554`; toggle `603-631`; aggregated status of a collapsed group `633-661`; rows and connectors `692-788`; reveal focused `274-291`; drop line `364-376`.
- **How it is built / coupling**: same as P4, with git worktree data. Width as P4. O(W²) in places (H11).
- **Verdict**: tree rows and the group toggle go into `select_list` (#7); the git specifics are skipped.

### P6 Configurable token rows and their fit — MERGE-INTO status_bar (#6)
- **What the user sees**: each sidebar row is a user-configured list of tokens (state icon, workspace, tab, branch, `$custom`) joined by ` · `, shortened to the sidebar width.
- **Sources**: token resolution and per-value style rules `src/ui/sidebar/tokens.rs:6-176`; separators `179-187`; fit `src/ui/sidebar.rs:106-275`; style patch `277-296`.
- **How it is built**: fixed tokens (icon, git counts) always kept; if minimums do not fit, flexible tokens are re-added from the last to the first while they fit; spare width is dealt one cell at a time round-robin; each token cut with `truncate_end` (H5). Dropped tokens are not counted (H8).
- **Coupling**: herdr's TOML token config and conditional styles (skip those).
- **Overlap**: `status_bar` drops segments by priority; it does not share width.
- **API sketch**: `Segment::new(text).flex()` / `.fixed()`, `StatusBar::fit(Fit::Share)`; drop order still by `priority`, and a `+N` cue when segments go.

### P7 Single-line text editor — MERGE-INTO composer (#5)
- **What the user sees**: rename field, navigator search, help filter, copy-mode search prompt.
- **Sources**: `shell/text_editor.rs:5-238` (state, emacs keys, kill/yank, word classes), `240-265` viewport, `267-292` render, tests `488-526`.
- **How it is built**: `String` + byte cursor snapped to grapheme boundaries (`58-67`); CR/LF/Tab become spaces, controls dropped (`69-96`); `replace_on_type` clears preset text on the first key (`88-91`); horizontal window measured per grapheme with unicode-width (`240-265`); the caret is returned as a position for the host cursor. Tests pin purity and grapheme safety at widths 0–80.
- **Coupling**: `crate::input::TerminalKey` and `normalize_key_combo` (`172`).
- **Overlap**: `composer` already edits by grapheme with Ctrl+U/K/W but wraps and has no yank.
- **API sketch**: `Composer::new().single_line()`; `ComposerState::with_text("main").replace_on_type(true)`; Ctrl+Y yanks the last kill.

### P8 Tab bar — MERGE-INTO tabs (#9)
- **What the user sees**: tabs of the focused workspace, the focused one filled with accent; `<` `>` buttons and `…` when they overflow; `+` new tab; a `│` drop marker while dragging; right-aligned status segments.
- **Sources**: `shell/tabs.rs:7-224`; status `226-292`; drop marker `294-328`; centred reveal `330-353`; scroll limit `355-372` (test `383-405`); drop index by midpoint `shell/mouse.rs:453-509`.
- **How it is built**: widths by `display_width`, minimum tab 8 cells (`shell/state.rs:3`); overflow mode only when there is room for buttons (`tabs.rs:39-40`); status dropped first when it would squeeze the strip (`237-245`). The focused tab is marked only by background colour (`103-116`, H2).
- **Overlap**: `tabs` already scrolls to the active tab and counts hidden ones (`‹2 … 3›`).
- **API sketch**: `Tabs::scroll_buttons(true).action("+").drop_marker(Some(i))`; `TabsLayout { scroll_left, scroll_right, action, .. }`; `tabs::drop_index_at(&layout, col)`.

### P9 Pane borders and titles — PORT (low) `split_frame` (#14)
- **What the user sees**: one shared line between split panes with proper junctions, the focused pane's edges in accent, the pane's agent label in its top edge.
- **Sources**: `src/ui/panes.rs:563-608` (collect and paint), `610-665` (split dividers), `667-706` (pane edges), `708-730` (does a cell touch the focused pane), `732-783` (titles), `785-804` (junction table), title cut `25-32`.
- **How it is built**: a `HashMap<(u16,u16), LineCell{up,down,left,right}>` per frame, mapped to box glyphs. Focus check scans all panes per cell (H11).
- **Coupling**: `PaneInfo`, `SplitBorder`, `AppState` flags.
- **Overlap**: ratatui 0.30 `Block::merge_borders(MergeStrategy)` composes junctions when bordered blocks overlap; herdr adds focus colouring and gap mode.
- **API sketch**: `SplitFrame::new(&[(rect, Some(title))]).focused(i).gaps(false).theme(theme).render(area, buf)`.

### P10 Selection and its highlight — MERGE-INTO pager (#8) and theme (#11)
- **What the user sees**: drag in a pane to select text; it stays put while the pane scrolls; it autoscrolls at the edge; double-click selects a word.
- **Sources**: `src/selection.rs:20-260` (phases, `Selection<P>`, `visible_rects` ≤ 3 rects, `contains`), `262-297` (row maps); highlight `src/ui/panes.rs:806-830`; colours `832-938`; drag and edge autoscroll (3–15 lines per tick) `shell/mouse.rs:141-428`; word selection via server `shell/word_selection.rs`; OSC 52 with SSH/WSL/VS Code detection `selection.rs:299-389`.
- **How it is built**: absolute rows, so scrolling does not move the selection; colour = host background mixed 28 % toward white or black, foreground by contrast, palette fallback for symbolic colours.
- **Coupling**: `ScrollMetrics`, pane ids (already generic `P`), the environment sniffing for OSC 52 (skip; twolab `osc52` handles tmux/screen).
- **API sketch**: `PagerState::mouse(MouseInput) -> Option<Selection>`; `Pager::selection_rects()`; `pager.selected_text()`; `theme.selection_on(host_bg: Option<Rgb>) -> Style` (mono: `REVERSED`).

### P11 Copy mode — SKIP
- **What the user sees**: `COPY` badge in the mode bar, a vi-like cursor, `/ ?` search with `n/N` and a `3/12` count, `v` to select, `y` to copy.
- **Sources**: `shell/copy_mode.rs:12-856` (key routing, search, a copy pipeline deferred until the server answers), `src/copy_mode.rs:5-86` (column helpers using ghostty width tables), bar `shell/render.rs:124-195`.
- **Why skip**: motions and search run against the terminal emulator on the server. The reusable parts are the selection model (P10) and the single-line editor (P7).

### P12 Mode bar — SKIP (covered by key_hints + status_bar)
- **What the user sees**: bottom row of the pane: ` PREFIX `/` NAVIGATE `/` RESIZE `/` COPY ` badge, then key/label pairs, ` update ready` at the right.
- **Sources**: `shell/render.rs:31-230`.
- **Note**: hints are clipped at the width mid-hint (`render.rs:200-213`, H8). `key_hints` already drops whole hints and says `… ? more`.

### P13 Context menu — SKIP (covered by menu)
- **What the user sees**: right-click on a workspace, tab or pane shows a bordered list at the pointer.
- **Sources**: items per target `shell/context_menu.rs:4-85`, open `87-168`, keys `170-221`; render `shell/overlays.rs:165-219`.
- **Note**: clamps into the screen (`184-192`) but does not flip and drops items that do not fit (`197-200`, H8). twolab `menu` flips, scrolls, says `+N more` and has disabled/destructive items.

### P14 Global menu — MERGE-INTO menu (#13)
- **What the user sees**: `menu` in the sidebar footer (`● menu` when something needs attention) opens a list above it; items with updates carry a `●`.
- **Sources**: `shell/global_menu.rs:4-110`; render `shell/overlays.rs:86-163` (opens above: `y = launcher.y - height`, line 114); launcher `shell/sidebar.rs:378-430`.
- **API sketch**: `MenuItem::badge(true)`; `Menu::anchor_above(launcher_rect)`.

### P15 Navigator ("Go to") — MERGE-INTO select_list (#7)
- **What the user sees**: a large popup listing machines → workspaces → panes as a tree, each pane row with state dot, agent and state columns, a search line with a count, two detail lines for the selected row, filter keys `a/b/w/i/d`.
- **Sources**: `shell/overlays.rs:704-1051`; rows and filters `shell/aggregate_navigation.rs`; connectors `864-876`; columns that appear at ≥ 64 and ≥ 36 cells `887-897`; meta columns fixed at 11 cells `940-962`; detail lines `1007-1022`.
- **How it is built**: scroll keeps the selection visible (`805-810`); stale rows dim. Agent names over 11 cells are cut silently (H8).
- **API sketch**: `ListItem::tree(depth, last_sibling)`, `SelectList::columns([Column::new(11).min_width(36), Column::new(11).min_width(64)])`, `SelectList::detail(|item| lines)`.

### P16 Help overlay — MERGE-INTO key_hints (#10)
- **What the user sees**: `keybinds` popup, groups in accent, key column then label, `/` filters by command or shortcut, scrolls.
- **Sources**: `shell/overlays.rs:1053-1115` (lines), `1117-1254` (render); groups and filter `src/input/keybind_help.rs:75-251`.
- **Note**: the scroll range is estimated from char counts instead of ratatui's wrap (H7); widths by `chars().count()` and bytes (H6).
- **API sketch**: `HelpGrid::new(groups).filter(query).scroll(&mut ScrollState)`.

### P17 Release notes and product announcements — MERGE-INTO pager (#12); the shell goes to `modal`
- **What the user sees**: `v0.9.5` / `what's new in this release`, upper-case section headings, `•` bullets, inline code on a tinted background, fenced blocks with a `▏` gutter; an "update ready" preview with the install command.
- **Sources**: `src/ui/release_notes.rs:13-142` (lines), `144-194` (preview), `196-252` (metrics); `shell/overlays.rs:308-538`; data `src/release_notes.rs:129-176` (changelog section), `src/product_announcements.rs:81-131` (update manifest).
- **How it is built**: a tiny markdown subset to `Line`s; row count by `Paragraph::line_count` (correct). The per-line widths computed with `chars().count()` are thrown away (`overlays.rs:389-391, 504-507`).
- **API sketch**: `pager::notes(md: &str, theme) -> Vec<Line<'static>>`, then `Pager::new(title, lines)`.

### P18 Settings — SKIP (tabs + select_list in a modal)
- `shell/settings_overlay.rs:1-415`: section tabs, choice rows `▸ label ✓`, integration list with `✓ installed`. Product-specific; covered by `tabs`, `select_list` and P1.

### P19 Worktree create / open / remove — SKIP
- `shell/worktree_overlays.rs:1-418`: forms with a text field and a filtered list. Covered by `composer` (P7), `select_list` and P1.

### P20 Onboarding — SKIP
- `src/ui/onboarding.rs:1-18`, `shell/overlays.rs:540-642`: static welcome text with the live prefix key and `↵ continue`. Copy, not a widget; P1 covers the frame.

### P21 Link hover — SKIP
- `shell/link_hover.rs:7-366`: on hover over a pane, asks the server for OSC 8 / detected link regions, then underlines those cells in the frame (`321-365`). Needs the terminal's link data; the generic part is one modifier loop.

### P22 Narrow ("mobile") layout — SKIP as a widget
- `shell/config.rs:385-401`: at ≤ 64 columns (`src/config.rs:78`) the sidebar and tab bar give way to a 2-row header (`shell/mobile.rs:50-235`) and a full-screen switcher (`363-574`, left scrollbar `1092`); toasts become bottom banners. A host layout decision; the tally (P4) and banner (P2) are the reusable bits.

### P23 Machine / endpoint status and diagnostics — SKIP
- `shell/endpoints.rs:719-729` (glyph + word + colour: `◐ reconnecting`, `! attention`, `· disabled`), `shell/endpoint_sidebar.rs:15-649` (machine sections), `shell/machine_diagnostics.rs:6-205` (hover badge, retry, diagnostic toast). SSH-specific. The glyph-plus-word presentation is the right model for P4's states.

### P24 Theme tokens and terminal theme — SKIP (covered)
- `src/app/state.rs:35-550` (19 tokens, 18 bundled palettes, overrides) and `src/terminal_theme.rs:1-172` (OSC 10/11/4 query, parse, set/reset). twolab's `Theme` and `color::parse_osc_color`/`parse_osc_slot` cover this; bundled third-party palettes are not needed. Token mapping if a host migrates: `accent→accent()`, `text→text()`, `overlay0→dim()`, `overlay1/subtext0→secondary()`, `red/yellow/green/teal→status(..)`, `active_row_bg/selection_bg→selected()`. See H3 and H10.

### P25 Tab surface, pane rendering, popup pane — SKIP
- `src/ui/tab_surface.rs:28-209`, `src/ui/panes.rs:81-562`, `src/ui.rs:52-131`: which tab's panes to draw, terminal sizes, scrollbar gutters, CJK IME cursor reveal (`tab_surface.rs:172-205`). All terminal-runtime work.

### P26 Drag handles — optional MERGE-INTO layout (not ranked)
- `shell/mouse.rs:8-37` (sidebar width from column, section split from row), `441-451` (split ratio from pointer with grab offset, clamped 0.1–0.9); `src/ui/sidebar.rs:30-76` (section heights with 3-row minimums). Small helper: `layout::ratio_at(pointer, origin, len, grab)`.

---

## Health check (점검)

Severity: **high** = panic or corrupt output from reachable input; **medium** = wrong output in common setups; **low** = edge or cost.

| # | Sev | Defect | Where | Failing input and evidence |
|---|---|---|---|---|
| H1 | high | Copy toast rect can leave the buffer; `Clear` indexes it and panics | `src/ui/status.rs:37-47` (top positions: `y = area.y + offset.min(height)`, no clamp), `status.rs:63`; caller `shell/composition.rs:535-559`; ratatui `Clear` indexes every cell (`ratatui-widgets-0.3.0/src/clear.rs:39-45`) | **Probe.** 100×4 terminal, tab bar on top (pane surface `26,1,74,3`, `shell/config.rs:417-428`), a config diagnostic visible (offset 1), `clipboard_toast_position = top-*`, copy → rect `y=2 h=3` → `index outside of buffer … index is (77, 4)`. Also 100×8 with a top-right 4-row notification card overlapping (offset 5) → `(77, 8)`. 100×30 is fine. Positions are non-default (`src/config/model.rs:85-93` default bottom-center). The notification card clamps (`notifications.rs:133-142`); this one does not. |
| H2 | medium | Meaning carried by colour alone | `src/client/shell.rs:184-189` (`Dots`, the default: working, blocked and done are all `●`, told apart only by yellow/red/teal, `225-238`); toast kind by dot colour (`shell/notifications.rs:89-95, 152`, `endpoint_notices.rs:70-77`); focused tab by background only (`shell/tabs.rs:103-116`); scrollbar thumb same glyph as track (`shell/scroll.rs:85-96`, unfocused pane `src/ui/scrollbar.rs:174`, help `shell/overlays.rs:1219-1229`) | By reading. A colour-blind user, a monochrome terminal or any future `NO_COLOR` support sees blocked and working as the same `●`, and no scrollbar thumb. The `Symbols` style (`shell.rs:191-195`) and endpoint presentation (`shell/endpoints.rs:719-729`, glyph + word) show the fix. |
| H3 | medium | `NO_COLOR` ignored; truecolor always emitted | no `NO_COLOR` anywhere in `src/` (grep); the client writes its own SGR (`src/protocol/render_ansi.rs:347-410`: `38;2;r;g;b`), bypassing crossterm's `NO_COLOR` check; default palette is RGB (`src/app/state.rs:78-90`); no 256/16-colour fallback found (grep) | By reading. `NO_COLOR=1 herdr` still paints Catppuccin RGB; on a 256-colour terminal RGB is sent as is. |
| H4 | medium | Agents-header text written one `char` per cell: wide characters overwrite each other | `shell/agent_sidebar.rs:376-382` (`set_char` per char, `take(width)` chars), used for the agent-view label `137-150` | **Probe** (ratatui `Buffer::diff` of the result). View label `작업중` (set through `agent.view.set`, label ≤ 32 chars, `src/app/agent_view.rs:17,193-205`) → the screen shows only `작`; `레뷰 대기` → `레 대`. ASCII is unaffected. |
| H5 | medium | Truncation splits grapheme clusters and over-cuts | `src/ui/text.rs:7-34` (walks `chars()`); used for pane titles (`src/ui/panes.rs:25-32`) and every sidebar token (`src/ui/sidebar.rs:231-270`) | **Probe.** `truncate_end("🇰🇷 team", 2)` = `"🇰…"` (half a flag); `("👍🏽ok", 3)` = `"👍…"` (skin tone dropped though `👍🏽…` fits in 3); `("👩‍💻👩‍💻", 3)` = `"👩\u{200d}…"` (dangling ZWJ). Its own test only covers CJK (`text.rs:40-46`). |
| H6 | low | Widths in bytes or `char`s | bytes: `src/ui/status.rs:25` (copy toast; latent, message is fixed ASCII, `shell/state.rs:1826-1828`), `status.rs:110` (config diagnostic: a non-ASCII path or TOML excerpt makes the box 1 cell wider per Hangul syllable, text sits left of a blank strip), `shell/overlays.rs:1083` (group heading); chars: `src/ui/widgets.rs:92-93` (button width, latent: ASCII labels), `shell/overlays.rs:1066, 1094` (help key column, padded with `{:<N}` which counts chars), `src/ui/release_notes.rs:25-109` (dead data) | By reading. |
| H7 | low | Help scroll range estimated, not measured | `shell/overlays.rs:1184-1190` (`ceil(chars / width)` per line) vs ratatui word wrap; release notes do it right (`src/ui/release_notes.rs:243-252`) | **Probe** with the default labels from `src/input/keybind_help.rs` (keys approximated): text width 21/22/23/24 → estimate 73/67/60/58 rows, rendered 71/64/59/57, so scrolling runs 1–3 blank rows past the end; per line it also under-counts (`" h / j / k / l / left / right move focus"` at width 20: estimate 2, rendered 3), so other bindings can leave the last rows unreachable. Text width 21–24 means a 28–31-column terminal. |
| H8 | low | Text and rows cut without saying so | `put_text` clips with no `…` (`shell/render.rs:368-373`): mode bar hints (`render.rs:200-213`; COPY hints are 95 cells, so a 54-column pane loses the last 41 mid-hint — probe arithmetic), navigator meta columns at 11 cells (`shell/overlays.rs:940-962`), toast title/body (`shell/notifications.rs:151-176`), product announcement title (`overlays.rs:458-465`); rows dropped with no count: context menu (`overlays.rs:197-200`), collapsed rail (`shell/sidebar.rs:60-64, 130-134`), agent and space lists (`agent_sidebar.rs:214-218`, `sidebar.rs:309-311`), sidebar tokens (`src/ui/sidebar.rs:160-175`) | By reading. The tab strip (`tabs.rs:181-212`) and mobile tally (`mobile.rs:350-360`) do show `…`. |
| H9 | low | East-Asian Ambiguous glyphs placed as one cell | `●○◐◆·×▕▏│─├└┌┼…↑↓•` all have `width_cjk() == 2` (probe; narrow: `◉ ✓ ▐ » « ▸ ▾ ⋮`). Sharpest case: the collapsed sidebar is 4 columns (`shell/config.rs:405`) with the state icon at x+2 (`shell/sidebar.rs:95-103`) and the separator `│` at x+3 (`shell/render.rs:22-28`) | Terminal-dependent, not run: with ambiguous-wide rendering (common under CJK locales) the icon covers the separator and rows shift. herdr does handle CJK IME cursors (`src/ui/tab_surface.rs:172-205`), so CJK users are in scope. |
| H10 | low | OSC colour reply parser panics on non-ASCII | `src/terminal_theme.rs:146-155` slices `hex[..digits]` by bytes; called on host stdin (`src/raw_input.rs:1032-1035`, `src/client/input.rs:309-324`) | **Probe.** `parse_rgb_color("#aé")` → panic `end byte index 2 is not a char boundary`. Reachability (a terminal or stdin source sending `ESC]11;#aé ESC\`) not verified. |
| H11 | low | O(n²) work per frame | `shell/agent_sidebar.rs:248-275` (four linear finds and a count per agent row, called for every agent `237-246`); view-order filter `24-35`; collapsed-group status for every entry `shell/sidebar.rs:239-257` + `633-661`, toggle lookup `556-571`; reveal re-runs O(n) metrics per step `shell/scroll.rs:56-77`; border focus scan per cell `src/ui/panes.rs:590-592` | By reading; n = agents, workspaces or panes, tens in practice. Index by id once per frame. |
| H12 | info | Tiny sizes otherwise guarded | popups `None` below 4×4 (`src/ui/widgets.rs:18-20`, `shell/overlays.rs:271-273`), `panel` needs 2×2 (`overlays.rs:227-229`), help 20×6 (`1127`), navigator 4×9 (`714`), editor intersects its area (`shell/text_editor.rs:273`), cards clamp (`notifications.rs:133-142`, test `457-478`) | By reading; H1 is the exception found. |

Checked and not found:
- ratatui 0.30's wrap overflow (U+3000 at a row end, a wide glyph straddling an odd width) does not trigger at herdr's narrowest wrapped text (19 cells): 0 cases over widths 19–40 for `"a"×k + U+3000 + "b"`, and none for Han or Hangul runs at widths 19/21/23/25 (probe).
- Direct `buffer[(x, y)]` indexing in `panel`, menus and the mode bar stays inside the buffer, because every rect is clamped to `buffer.area` first (`overlays.rs:107-116, 181-192`, `render.rs:45-50`).

## Method

- Read every file under `src/ui/`, the rendering parts of `src/client/shell/` (render, overlays, sidebar, agent_sidebar, tabs, notifications, notification_policy, endpoint_notices, endpoint_sidebar, mobile, scroll, text_editor, settings_overlay, context_menu, global_menu, link_hover, copy_mode, machine_diagnostics, mouse, composition), plus `src/selection.rs`, `src/copy_mode.rs`, `src/popup_size.rs`, `src/terminal_theme.rs`, `src/release_notes.rs`, `src/product_announcements.rs`, `src/protocol/render_ansi.rs` and the palette in `src/app/state.rs`.
- Probes: H1, H4, H5, H7, H9, H10 and the "not found" checks were run on verbatim copies of the herdr functions in a throwaway crate (ratatui 0.30.0, unicode-width 0.2.2, offline), then deleted. No herdr test or binary was run.
