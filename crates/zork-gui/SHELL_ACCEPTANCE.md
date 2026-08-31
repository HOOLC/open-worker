# zork-gui live Codex shell acceptance

> The measured shell geometry remains active. Historical direct-Agent/tool-row
> integration clauses are superseded by `GATEWAY_IM_ENTRY_ACCEPTANCE.md`.

## User request

> 其它部分也按这个思路优化一下

The already-corrected composer remains the accepted component baseline. This
pass applies the same live-CDP measurement method to the surrounding native
shell: sidebar, home empty state, selected-task header, task list, and
transcript presentation. Screenshot inference is not an acceptable source for
new measurements.

## Visual source of truth — 2026-08-27

- Live Codex home capture: `design-audit/44-codex-cdp-shell-empty-full.png`.
- Live Codex selected-task capture:
  `design-audit/45-codex-cdp-shell-task-full.png`.
- Live selected-task first-message capture:
  `design-audit/47-codex-cdp-shell-task-first-message.png`.
- Home DOM/computed CSS: `design-audit/codex-shell-home-cdp.json`.
- Selected-task DOM/computed CSS:
  `design-audit/codex-shell-task-cdp.json`.
- Message DOM/computed CSS: `design-audit/codex-thread-message-cdp.json`.
- Source viewport: 1479 × 826 CSS px at DPR 1, read from the Codex instance on
  `3720-Mac-mini-2.local` through a loopback-only CDP tunnel.

## Sidebar acceptance

- The logical sidebar is 275 px wide. Its navigation begins below a 46 px
  toolbar region and its bottom utility row is 46 px high.
- Content uses an 8 px inline inset. Primary navigation rows are 29–30 px high,
  use a 12.5 px selected/hover radius, and render at 14 px with a 21 px line
  height.
- Section labels render at 14/21, weight 500, with the live tertiary text color
  `rgba(26, 28, 31, 0.494)`.
- The selected workspace and selected task use the live soft fill
  `rgba(26, 28, 31, 0.055)`, not the old opaque gray block.
- Workspace rows use a real 16 px folder icon. Task rows are indented and do
  not show the old leading status bullet plus model suffix. A working state may
  retain one quiet right-aligned status mark because it is backed by zork data.
- The sidebar surface is near-white and the main surface supplies the subtle
  separation/elevation; the old visibly gray panel and hard right divider are
  removed.
- The footer presents the real local-agent state in the same 29 px row rhythm;
  it does not add a separate full-width diagnostics bar.

## Home-state acceptance

- The heading is dynamic: `What should we get done in {workspace}?`.
- The heading uses 28 px type, 33.6 px line height, weight 400, and the live
  primary text color `#1a1c1f`. The old explanatory subtitle is removed.
- A real icon-library product glyph sits above the heading without attempting
  to counterfeit Codex's proprietary mark.
- Four usable zork prompt suggestions occupy one 710 × 104 px row with a 12 px
  gap. Each card is 168.5 px wide at the source viewport, has a 20 px radius,
  `12px 16px` padding, a 13/20 weight-500 label, and the captured outline plus
  `0 2px 4px -1px rgba(0,0,0,.1)` elevation.
- Suggestion cards are not decorative or dead controls: activating one fills
  the shared composer with a useful prompt and moves keyboard focus there.
- The accepted 736 × 141 composer remains 16 px above the bottom edge.

## Selected-task acceptance

- The task header is 46 px high. It contains a 16 px folder glyph, an editable-
  style single-line title at 14/24 weight 500, and one restrained real zork
  status affordance. The old two-line 52 px title/subtitle/status header is
  removed.
- Transcript content is 736 px wide and begins 76 px inside the main task
  surface (`x = 351` at the source viewport). The follow-up composer aligns to
  the same left edge, preserving the live task-page reading rail without
  fabricating the source's right-side controls. Assistant prose uses 14 px type
  and 22 px line height.
- User prompts are right aligned, limited to 77% of the transcript width, use
  `8px 12px` padding, a 20 px radius, and the live 5%-text neutral fill.
- Tool, wait, streaming, pagination, SSE, selection, cancel, and send behavior
  remain connected to the current agent API.
- The selected-task composer remains the previously accepted shared zork
  component; this pass does not silently change that earlier goal.

## Product constraints

- Do not add fake pull-request, sites, scheduled-task, plugin, source, output,
  voice, account, or proprietary Codex controls.
- Do not fabricate Codex logos or icons. Reuse the embedded MIT-licensed
  Phosphor subset for zork-owned actions.
- Do not change the public zork-agent API.
- Do not modify the local Codex process or Surge.

## Regression and QA strategy

1. Add contract assertions for the live shell measurements and prompt-card
   behavior; capture their failure against the old implementation.
2. Delete the old large-row/gray-panel/two-line-header styling and implement
   the measured structure without fallback branches.
3. Capture native home and selected-task states at the default 1280 × 800
   logical window.
4. Put source and implementation states together at equal visible widths with
   proportional scaling. Fix every actionable P0/P1/P2 issue before changing
   `design-qa.md` back to `final result: passed`.
5. Run formatting, all zork-gui Rust tests, clippy, and fake-agent E2E checks.
