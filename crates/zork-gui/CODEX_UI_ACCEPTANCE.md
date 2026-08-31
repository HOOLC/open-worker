# zork-gui Codex UI acceptance

> The visual contract in this document remains active. Its references to the
> direct `zork-agent` behavioral source are historical and are superseded by
> `GATEWAY_IM_ENTRY_ACCEPTANCE.md`.

## User request

> 看下 zork-gui
>
> 你来继续搞吧
>
> 打开给我看看
>
> 你这跟 codex ui 也不想啊

The current client is functionally connected to `zork-agent`, but the visible
result does not resemble Codex Desktop closely enough. This pass replaces the
prototype styling and information architecture with a faithful native desktop
interpretation of the real Codex task experience.

## Product goal

At the default desktop window size, a user should immediately recognize the
same product structure and visual language as Codex Desktop while completing
the zork core flow:

1. scan recent tasks;
2. open a task and read the conversation;
3. understand running, waiting, tool, failure, and finished states;
4. write and send a follow-up from the fixed bottom composer;
5. create a task for the current workspace.

The real Codex Desktop window captured during this pass is the visual source of
truth. The existing `zork-agent` HTTP/SSE contract remains the behavioral source
of truth.

## Visual acceptance

- The window uses Codex-like application chrome, proportions, typography,
  neutral palette, borders, radii, density, and selected/hovered hierarchy.
- The left rail reads as a task navigator, not a generic dark admin sidebar.
  It includes a compact app header, a clear new-task action, recent-task rows,
  workspace context, and restrained status affordances.
- The main pane reads as a Codex task: compact task header, centered readable
  transcript column, quiet assistant prose, distinct user prompts, and inline
  activity/tool/wait treatment rather than chat bubbles everywhere.
- The composer is a substantial rounded surface fixed above the bottom edge,
  with multiline prompt space, model/reasoning controls, send/stop affordance,
  and visible keyboard guidance.
- Empty, loading, offline, selected, working, waiting, streaming, error, and
  populated states share one coherent design system.
- At 1280 x 800, persistent navigation, task context, transcript, and composer
  remain visible without overlap or clipped primary controls.

## Interaction acceptance

- Selecting a task refreshes the transcript and live SSE stream.
- New task creation remains available and uses the selected profile/model/
  thinking configuration.
- Enter sends; Shift+Enter inserts a newline; stop is available while working.
- Profile, model, and reasoning controls remain usable without visually
  dominating the composer.
- Existing pagination, optimistic mailbox rendering, duplicate suppression,
  SSE status projection, and reconnect behavior do not regress.

## Regression strategy

Before implementation, tests must fail against the old UI model for the new
Codex shell, task-row content, transcript presentation, composer structure, and
1280 x 800 geometry. API and fake-agent regressions remain green throughout.

After implementation:

- `cargo fmt -p zork-gui -- --check`
- `cargo test --locked -p zork-gui`
- `cargo clippy --locked -p zork-gui --all-targets -- -D warnings`
- `uv run crates/zork-gui/tests/test_fake_agent.py`
- repository CI commands relevant to the touched Rust workspace
- native screenshot comparison at the same viewport and task state as the
  captured Codex reference, with `design-qa.md` ending in
  `final result: passed`

## Constraints and non-goals

- Do not modify Surge.
- Do not change agent configuration or the public agent API for visual parity.
- Do not pretend unavailable Codex-only capabilities (cloud sync, worktrees,
  review/diff tooling) exist; matching visible structure must not create dead
  primary controls.
- Do not retain the current generic dark-dashboard layout as a fallback branch.

## Completion self-check — 2026-08-26

- The old dashboard shell has been removed rather than kept as a fallback.
- The native window is forced light and uses the Codex palette, task rail,
  centered task canvas, inline activity treatment, and fixed rounded composer.
- Creating a task posts the session and then delivers the initial prompt to its
  mailbox; selection, pagination, SSE, sending, and cancel behavior remain
  covered by regression tests.
- Full-view and focused source/implementation comparisons are recorded in
  `design-audit/`; `design-qa.md` ends in `final result: passed`.
- The later composer-specific pass corrects the tray/surface measurements,
  uses proportional native-source comparison, and embeds real icon assets.
- The full verification set listed above passes. Surge was not modified.
