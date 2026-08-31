# zork-gui successful-finish status acceptance

## User request

> 这啥玩意儿

The attached native screenshot shows a bare green `finished` label with a
vertical rule occupying the conversation body after the last user message.

## Problem

`finished` is a gateway-projected Agent lifecycle event, not conversation
content and not useful live progress. The GUI currently sends every non-clear
status through the inline activity renderer, so a successful terminal event is
left behind looking like a malformed assistant message.

## Fixed behavior

- A successful `finished` event may update the task/header status, but it never
  renders an inline activity row in the conversation body.
- `finished` never creates a transcript item or a synthetic assistant message.
- Transient work activity such as thinking, tools, and waiting keeps its
  existing inline treatment while work is active.
- Failure reporting and explicit gateway-delivered chat messages are unchanged
  by this focused correction.

## Regression strategy

Before implementation, a focused transcript regression must fail because
`should_render_live_activity(AgentStatus::Finished, ...)` returns `true`.

After implementation:

- run the focused message-rendering regression;
- run all `zork-gui` tests and Clippy;
- rebuild and re-sign the native app bundle;
- replay a successful Agent completion and visually confirm there is no bare
  `finished` row below the conversation.

## Constraints

- Do not modify Surge.
- Do not hide an explicit assistant message delivered by the gateway.
- Do not turn `finished` into different transcript copy or a decorative
  replacement row.
- Do not change the accepted composer or shell geometry.

## Completion self-check — 2026-08-27

- Added a regression that first failed because `AgentStatus::Finished` was
  considered renderable live activity, then passed after the correction.
- The activity predicate now rejects both `Clear` and `Finished`. It does not
  manufacture replacement copy and does not touch gateway-delivered messages.
- Rebuilt and re-signed the native app bundle, opened it against the isolated
  real Gateway + fake-model Agent fixture, and triggered a new successful run
  while SSE was connected. The new user bubble appeared and the header reflected
  completion, but no green `finished` row or vertical rule appeared in the
  conversation body.
- All `zork-gui` tests and `cargo clippy --locked -p zork-gui --all-targets --
  -D warnings` passed.
- Surge was not touched.
