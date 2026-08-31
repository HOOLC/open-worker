# zork-gui component port acceptance

> The native input/menu component contract remains active. Historical
> mailbox/direct-Agent integration clauses are superseded by
> `GATEWAY_IM_ENTRY_ACCEPTANCE.md`.

## User request

> 那你开始移植我们需要的组件吧

This pass replaces the remaining prototype interactions with the smallest
component layer that zork-gui actually needs. The accepted Codex shell,
composer geometry, and native window chrome remain the visual source of truth;
Longbridge's theme and application shell are not being imported.

## Why this is a source port

zork-gui currently uses `gpui-unofficial` `1.17.0-pre`. The released
`gpui-component` crate uses the official GPUI package and its main branch tracks
a different, moving Zed revision. GPUI entities, windows, contexts, and elements
from those packages are different Rust types, so adding the crate directly
would create two incompatible GPUI runtimes.

The required behavior is therefore adapted into local, zork-owned components:

1. a multiline `ComposerInput` built on this runtime's native
   `EntityInputHandler` and text layout APIs;
2. a compact `SelectorMenu` for profile, reasoning, and model selection.

Porting the complete upstream input editor, Root, theme, popover, and menu stack
is explicitly out of scope. The upstream editor is a general-purpose text
engine; zork only needs a bounded prompt editor and three short option menus.

## ComposerInput behavior contract

- Replace the hand-written printable-key and `String::pop` path in `RootView`.
- Register a real GPUI input handler so macOS IME composition, marked text, and
  character palette input are delivered through the platform input system.
- Store selections as valid UTF-8 byte ranges and convert correctly to and from
  the platform's UTF-16 ranges.
- Move and delete by Unicode grapheme cluster; an emoji or composed character
  must never be split into invalid text.
- Support mouse caret placement and drag selection, visible caret/selection
  painting, Select All, Copy, Cut, Paste, Backspace, Delete, Home, End, and
  shifted selection variants.
- `Enter` emits submit without mutating the value. `Shift+Enter` inserts a
  newline. Empty/whitespace-only submit remains a no-op at the agent boundary.
- Render explicit newlines and soft wrapping inside the existing 44 px editor
  viewport. Long input stays clipped/scrolled inside that viewport and must not
  change the accepted 736 x 141 composer geometry.
- Programmatic set, clear, and failed-send restoration use the same component
  state and preserve the existing create-session/mailbox/cancel flow.

## SelectorMenu behavior contract

- Clicking profile, reasoning, or model opens a menu containing the real
  options instead of silently cycling to the next value.
- The active value is visibly marked. Clicking an item selects that exact value
  and dismisses the menu; clicking the active trigger or pressing Escape also
  dismisses it.
- While a menu is open it owns keyboard focus: Up/Down moves the highlighted
  option, Enter chooses it, and closing the menu restores composer focus. Enter
  must never leak through and submit the prompt behind an open menu.
- Only one menu may be open at once. Menus stay anchored to the existing compact
  controls and do not alter composer height or introduce Longbridge styling.
- Selection still drives the existing new-session payload and pending
  `PUT /selection` update for an existing session.

## Regression order

Before implementation, add regressions that fail because the local component
modules and behavior do not exist. Cover at least:

- grapheme-safe movement/deletion;
- UTF-8/UTF-16 selection conversion and IME replacement;
- Enter versus Shift+Enter;
- exact selector choice and one-open-menu state;
- removal of `RootView`'s manual composer key mutation.

After implementation, run:

- `vp fmt` or the repository-equivalent formatting check;
- `vp test` for zork-gui, including the new component regressions;
- `vp clippy` with warnings denied for all zork-gui targets;
- `uv run crates/zork-gui/tests/test_fake_agent.py`;
- the relevant repository build/CI command;
- native interaction and screenshot QA at the accepted window dimensions.

## Provenance

The interaction design and API contract are adapted from Longbridge
`gpui-component`'s Input/Textarea and Menu/Select components (Apache-2.0). The
minimal GPUI input-handler and painting structure is adapted from the
`gpui-unofficial` input example (Apache-2.0). Any source file containing adapted
code must retain a short provenance comment. No upstream visual theme or assets
are copied.

## Constraints

- Do not modify Surge.
- Do not attach to, restart, or modify local Codex or Codex on mini2.
- Do not change the public zork-agent API or replace the real agent flow with a
  mock path.
- Do not regress `COMPOSER_ACCEPTANCE.md`, `SHELL_ACCEPTANCE.md`, or
  `WINDOW_CHROME_ACCEPTANCE.md`.
- Do not retain the manual composer input path as a fallback branch.

## Completion self-check — 2026-08-27

- `RootView` owns a `ComposerInput` entity and no longer owns a composer string,
  focus handle, raw printable-key handler, or `String::pop` fallback.
- Native macOS QA verified Unicode clipboard input, a visible caret and
  selection, Shift+Enter, explicit newlines, soft wrapping, and internal
  scrolling without changing the fixed composer geometry.
- All three selector triggers open anchored exact-choice menus. Mouse choice,
  one-open-menu state, Up/Down highlight, Enter choice, Escape dismissal, and
  composer-focus restoration were verified against the fake agent's real
  options.
- The source adaptation remains local to the existing `gpui-unofficial`
  runtime; no Longbridge theme, Root, or second GPUI package was introduced.
- All 25 zork-gui Rust tests, formatting, clippy with warnings denied, all five
  fake-agent E2E tests, the zork-gui build, and `vp run build` pass.
- Surge, local/mini2 Codex, the accepted shell/window chrome, and the public
  zork-agent API were not modified.
