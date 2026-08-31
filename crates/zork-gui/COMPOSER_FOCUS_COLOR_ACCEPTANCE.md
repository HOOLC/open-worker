# zork-gui composer focus color acceptance

## User request

> 怎么输入框颜色不太对，这张怎么蓝了

This correction removes the unintended blue wash from the focused follow-up
composer without changing its accepted geometry, native input behavior, or
selection feedback.

## Baseline evidence — 2026-08-27

- `design-audit/82-composer-blue-user-report.png`: the exact screenshot
  supplied by the user. The 98 px input surface reads pale blue while focused.
- `design-audit/83-composer-blue-current-focus.jpeg`: the same state reproduced
  in the current rebuilt native app.

The blue is produced locally by
`ComposerInput::render` applying `rgba(0x339CFF0A)` to the entire editor on
`focus_visible`. Over the near-white composer surface this becomes an obvious
cool tint. It is not caused by the screenshot, failure status, Markdown, the
native window, or the source composer surface.

The measured Codex composer contract already stored in
`design-audit/codex-composer-cdp-computed.json` specifies a near-white
96%-style surface and a transparent editor, with no editor background or
outline. The blue focus wash therefore conflicts with the accepted source.

## Required behavior

1. Focused and unfocused composer surfaces keep the same neutral near-white
   fill. Focus must not recolor the whole editor blue.
2. Text focus remains visible through the insertion caret; selected text keeps
   the existing blue selection highlight. Removing the surface wash must not
   remove caret painting, selection painting, input handling, or keyboard
   bindings.
3. The 736 × 141 composer, 98 px input surface, 61 px workspace tray, shadow,
   radius, selectors, send/stop control, and fixed placement remain unchanged.
4. New-task workspace focus styling is outside this screenshot and remains
   unchanged in this correction.

## Regression strategy

Before implementation, a component regression must fail because the focused
editor still contains the full-surface `rgba(0x339CFF0A)` wash. It must also
prove the focus handle, caret path, and selection highlight remain present.

After implementation:

- rebuild and re-sign the native app bundle;
- capture and inspect the corrected focused composer in the same native state;
- run `cargo fmt -p zork-gui -- --check`;
- run `cargo test --locked -p zork-gui`;
- run `cargo clippy --locked -p zork-gui --all-targets -- -D warnings`;
- run `cargo build --locked -p zork-gui` and `vp run build`.

## Constraints

- Do not modify Surge.
- Do not connect to, restart, or modify Codex on this machine or mini2.
- Do not change the public agent API.
- Do not replace the neutral focus treatment with another tinted fill.

## Completion self-check — 2026-08-27

- Removed only the erroneous full-editor
  `focus_visible(...rgba(0x339CFF0A))` fill. Focus tracking, insertion-caret
  painting, input handling, and the existing `rgba(0x339CFF30)` text-selection
  highlight remain.
- `design-audit/84-composer-neutral-fixed-focus.jpeg` is the rebuilt and
  re-signed native bundle with the follow-up composer explicitly focused. The
  surface stays neutral and the caret is visible.
- `design-audit/86-composer-focus-crop-before-after.png` places the exact user
  report and corrected focused composer crop side by side at the same scale.
- Empty editor samples at `(600, 680)`, `(700, 680)`, and `(800, 680)` changed
  from `(248, 251, 255)` in the reproduced baseline to `(255, 255, 255)` after
  the fix. This directly confirms the blue wash is gone rather than merely
  hidden by a different screenshot state.
- Composer geometry, shadow, tray, selectors, send action, transcript, and
  native window chrome are unchanged.

Final gates:

- `cargo fmt -p zork-gui -- --check`: passed.
- `cargo test --locked -p zork-gui`: 35 passed.
- `cargo clippy --locked -p zork-gui --all-targets -- -D warnings`: passed.
- `uv run crates/zork-gui/tests/test_fake_agent.py`: 7 passed.
- `cargo build --locked -p zork-gui`: passed.
- `vp run build`: passed.

Surge, local/mini2 Codex, and the public agent API were not modified.
