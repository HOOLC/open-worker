# zork-gui Codex composer acceptance

> The measured composer geometry remains active. Historical direct-Agent send
> clauses are superseded by `GATEWAY_IM_ENTRY_ACCEPTANCE.md`.

## User request

> composer 差得有点多

> 还是不对啊，你可以直接 cdp 连 codex，可以连 mini2 上的免得把自己重启了，然后直接看 css

The previous Codex shell pass got the overall application hierarchy close, but
the primary composer remained visibly different from the source. Screenshot
inference is no longer an acceptable source for component measurements. This
pass is limited to correcting that component from the live Codex DOM and
computed CSS without redesigning the accepted task shell or weakening any real
zork behavior.

## CDP source-of-truth requirement — 2026-08-27

- Connect only to the Codex instance on `3720-Mac-mini-2.local` through an SSH
  tunnel to its loopback-only DevTools endpoint. Do not restart or attach to
  the local Codex process that owns this task.
- Capture the live composer DOM, element rectangles, computed styles, matching
  CSS rules, state, viewport, device scale, and a CDP screenshot before any
  implementation edit.
- Record the captured data in `design-audit/codex-composer-cdp.json`; new
  geometry/style regressions must fail against the old zork values first.
- Final QA must compare the CDP source capture and native zork capture at the
  same visible width without one-axis stretching.

## Visual source of truth

- Live CDP source crop: `design-audit/33-codex-cdp-source-composer.png`
  (736 × 141 px at DPR 1).
- Live send-state crop:
  `design-audit/34-codex-cdp-source-composer-send-state.png`.
- Live computed styles and matched rules:
  `design-audit/codex-composer-cdp.json`.
- Earlier screenshot-only source crop:
  `design-audit/15-source-composer-focus.jpg` (640 × 120 px); retained as
  history, not used for measurements.
- Baseline implementation crop: `design-audit/16-implementation-composer-focus.jpg`
  (600 × 150 px).
- Previous screenshot-derived implementation crop:
  `design-audit/30-implementation-composer-final-focus.jpg` (retained only as
  history).
- Final live-CDP-corrected implementation crop:
  `design-audit/37-zork-cdp-corrected-composer.jpg` (682 × 130 px).
- Final focused comparison:
  `design-audit/42-cdp-native-side-by-side-final-focus.jpg`.

At a 1479 × 826 CSS viewport and DPR 1, the live Codex composer root is
736 × 141. Its project utility surface is inset 13 px horizontally, starts 4 px
below the root, is 61 px high, and overlaps a 736 × 98 input surface through an
18 px negative flow margin. The prompt and bottom controls share the 98 px
surface. The primary action is a 28 px circular icon button.

## Visual acceptance

- The default logical composer is 736 px wide and 141 px tall.
- The project utility surface is 710 × 61 px: 13 px inline inset, 4 px top
  inset, 20 px top radius, `#f6f6f6` fill, and `6px 6px 27px` padding.
- The primary surface is 736 × 98 px, begins 43 px below the root, has a 25 px
  radius, no border, a 96%-opaque white fill, and the captured four-layer
  prominent shadow.
- The editor is 44 px high with 12 px horizontal inset, 14 px system text, and
  20 px line height. The controls occupy a 28 px row with 8 px side inset and
  8 px bottom inset.
- The workspace tray shows concise workspace context rather than letting the
  absolute path dominate the component.
- The input surface has no hard horizontal divider or clipped outer outline.
  It overlaps the tray as its own softly elevated rounded container; its
  prompt and controls still share one surface.
- The prompt starts nearer the top and the control row sits compactly at the
  bottom, matching the source's vertical rhythm.
- The surface rests 16 px above the logical window bottom so its shadow and
  rounded lower edge do not feel pinned to the frame.
- Profile and reasoning stay on the quiet left side; model and the primary
  action sit on the right. Each real selector uses a small icon-library glyph,
  and the model keeps a disclosure caret; labels must not visually dominate
  the prompt.
- Send is a 28 × 28 circular icon button using a real icon-library asset.
  Working state swaps that asset for a stop icon while preserving cancel.
- Focus, error, pending model-change, sending, and stopping states do not
  increase the default component height or create a separate diagnostics row.

## Interaction acceptance

- Workspace editing, profile/model/reasoning cycling, Enter to send,
  Shift+Enter for newline, initial task creation, and send-to-stop behavior all
  remain wired to the current agent flow.
- The implementation must use the same composer component structure for new
  tasks and selected-task follow-ups; no visual fallback branch is retained.

## Regression strategy

Before implementation, the UI contract must fail for the new measurements,
tray/surface structure, compact selector placement, and circular icon action.
After implementation, native focused source/implementation evidence must be
recaptured and `design-qa.md` must again end with `final result: passed`.

## Constraints

- Do not modify Surge.
- Do not add fake Codex-only controls.
- Do not use emoji, text glyphs, CSS drawings, handcrafted SVGs, or approximate
  inline art for the action icon; use a licensed icon-library asset.
- Do not change the public zork-agent API.

## Completion self-check — 2026-08-27

- Final logical geometry is 736 × 141 with a 61 px tray, 98 px input surface,
  and 18 px overlap; source and native crops align proportionally in
  `design-audit/42-cdp-native-side-by-side-final-focus.jpg`.
- The surface uses the measured 25 px radius, four-layer shadow, 44 px editor,
  28 px controls, 14 px placeholder, and 13 px control labels.
- New-task and follow-up modes share one renderer while retaining workspace,
  profile, reasoning, model, send, and stop behavior.
- Folder, selector, disclosure, send, and stop visuals use embedded Phosphor
  assets covered by `assets/icons/PHOSPHOR_LICENSE.txt`.
- All 12 zork-gui Rust tests, clippy, formatting, and all 5 fake-agent E2E tests
  pass. Repository-wide failures recorded in `design-qa.md` remain unrelated.
- Surge and the public agent API were not modified.
