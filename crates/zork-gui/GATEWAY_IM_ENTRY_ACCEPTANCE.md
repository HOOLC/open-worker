# zork-gui gateway / IM entry acceptance

## User request

> 你接着做 ui 吧，现在消息显示不太对，应该跟接入 im 一样，agent 需要调 tool 来发送消息，而不是所有 event 都作为消息发到客户端，感觉可能也需要一个 gateway，gateway 应该支持多种 im entry

## Problem statement

The current GUI reads `zork-agent`'s public transcript directly. That makes
mailbox input, assistant transcript text, tool results, waits, and streaming
deltas look like one end-user chat. This is the wrong trust and product
boundary: an Agent transcript records execution, while an IM conversation
records messages deliberately delivered to people.

The existing Slack path already has the desired contract. Incoming Slack
messages enter the Agent mailbox, but commentary/final transcript text is not
automatically posted back. A visible reply exists only when the Agent invokes
`zork-call chat post-message` through a workspace tool.

## Fixed architecture

1. `zork-gateway` owns IM sessions, visible message history, delivery, and the
   client-facing SSE stream. `zork-gui` no longer reads Agent messages or
   `assistant_delta` events directly.
2. The desktop client is a built-in `local_gui` IM entry. It requires no
   credentials or persisted connection configuration. Configured Slack
   connections are other entries behind the same gateway delivery boundary.
3. Every normal IM entry follows one contract:
   - entry input is persisted as one visible user message and appended to the
     bound Agent mailbox;
   - Agent commentary, final text, tool calls/results, waits, and internal
     events never become visible messages by themselves;
   - the Agent creates a visible assistant message only by explicitly calling
     `zork-call chat post-message`;
   - Agent status remains available as non-transcript activity.
4. Gateway delivery is selected by the session's entry/platform instead of
   assuming every connection is Slack. The built-in GUI entry persists and
   broadcasts the message; a Slack entry calls Slack. Unknown entries fail
   explicitly.
5. Agent workspace tools receive their Agent session id, and `zork-call`
   resolves the exact gateway binding by that id. Multiple tasks may therefore
   use the same project directory without sending a reply to the wrong IM
   session.

## Client API contract

The GUI uses the runtime listener (default `http://127.0.0.1:3000`) and only
the gateway-owned `/v1/im` surface:

- `GET /v1/im/profiles`
- `GET|POST /v1/im/sessions`
- `PUT /v1/im/sessions/{agent_session_id}/selection`
- `GET|POST /v1/im/sessions/{agent_session_id}/messages`
- `GET /v1/im/sessions/{agent_session_id}/events`
- `POST /v1/im/sessions/{agent_session_id}/cancel`

Message pages and `message` SSE events contain only `user` or `assistant`
messages previously accepted/delivered by the gateway. `status` SSE events are
activity metadata and never add transcript rows. There is no
`assistant_delta`, tool-result message, or persisted-wait message in this API.

## Acceptance criteria

1. Sending a GUI prompt immediately shows exactly one user row, persists it in
   gateway history, and appends it to the bound Agent mailbox.
2. Agent assistant text, streamed deltas, tool results, and wait events do not
   appear in GUI history or its live message stream.
3. An explicit `zork-call chat post-message --kind ...` from the bound Agent
   creates exactly one assistant row in the GUI and survives restart/history
   reload.
4. The same gateway delivery function successfully targets both a configured
   Slack entry and the built-in GUI entry; destination mismatch and unknown
   entry errors remain explicit.
5. Transient status (`thinking`, tool activity, and waiting) can stay visible
   as activity without being confused with a chat message. Successful
   `finished` is header/task state only and never occupies a conversation row;
   see `FINISHED_STATUS_ACCEPTANCE.md`.
6. Creating, selecting, changing model/thinking, canceling, paginating, and
   reconnecting a GUI task continue to work through gateway APIs.
7. Two Agent sessions using the same workspace resolve their own gateway
   session by Agent session id, never by ambiguous cwd alone.

## Regression strategy

Before implementation, tests must demonstrate that the old direct-Agent
contract fails these requirements: `assistant_delta`, tool, and wait transcript
events are accepted as GUI messages; the GUI paths target `/v1/sessions`; and
gateway delivery is hard-coded to Slack.

After implementation, run:

- focused gateway DB/entry/API tests;
- `cargo test --locked -p zork-gateway -p zork-call -p zork-agent -p zork-gui`;
- `cargo clippy --locked -p zork-gateway -p zork-call -p zork-agent -p zork-gui --all-targets -- -D warnings`;
- the local GUI gateway fixture/E2E contract;
- repository `vp test`, `vp check`, and `vp run build` gates as applicable;
- native GUI verification against a real gateway + Agent pair.

## Constraints and non-goals

- Do not modify Surge.
- Do not expose the Agent transcript through the new IM API.
- Do not silently fall back from gateway APIs to direct Agent APIs.
- This pass establishes the entry boundary and implements Slack plus the
  built-in GUI entry. Adding another external provider is a new provider
  adapter, not a reason to change Agent or GUI message semantics.
- Preserve the accepted shell, composer, Markdown, selector, and native-window
  visual contracts except where old tool/wait/streaming transcript rows are
  intentionally removed.

## Completion self-check — 2026-08-27

- Gateway now owns a durable visible-message journal whose role constraint is
  exactly `user | assistant`. Agent mailbox, assistant transcript, tool, wait,
  and delta events cannot be inserted into that journal or decoded as GUI
  messages.
- The provider dispatch boundary implements the built-in `local_gui` entry and
  configured Slack entries. Local delivery persists and broadcasts through the
  same gateway operation that dispatches Slack delivery; unsupported entry
  types fail explicitly.
- The desktop client uses only `/v1/im` gateway endpoints. Its transcript model
  has only user and assistant messages; gateway-projected status has a separate
  activity path and never creates a transcript row.
- Each Agent tool invocation receives `ZORK_AGENT_SESSION_ID`. `zork-call`
  resolves gateway context by that exact id before considering the legacy cwd
  lookup, so tasks sharing one workspace cannot cross-deliver replies.
- The real-process fixture starts production Agent and Gateway binaries. It
  proves that ordinary internal assistant output stays hidden, an explicit
  `zork-call chat post-message` survives in visible history, and exact routing
  still holds for two sessions with the same workspace.
- Native GPUI verification used the rebuilt and re-signed app bundle against an
  isolated real Gateway + fake-model Agent pair. The selected task showed two
  user bubbles and only the one explicitly delivered assistant reply. A new
  composer send added exactly one user bubble while the completed Agent's
  internal output remained absent. That pass exposed the bare inline
  `finished` regression now governed by `FINISHED_STATUS_ACCEPTANCE.md`.

Verification results:

- `cargo test --locked -p zork-gateway -p zork-call -p zork-agent -p zork-gui`
  passed.
- `cargo clippy --locked -p zork-gateway -p zork-call -p zork-gui --all-targets -- -D warnings`
  passed. The workspace-wide variant including `zork-agent` is still blocked by
  unrelated existing large-error/large-enum provider lints; Agent tests pass.
- `uv run crates/zork-gui/tests/test_gateway_entry.py` passed.
- `vp check` passed (86 formatted files, 70 linted files), `vp test` passed all
  80 tests, and `vp run build` passed.
- `cargo build --locked -p zork-gui`, app-bundle signing verification, and
  `git diff --check` passed.
- Surge was not read, changed, restarted, or otherwise touched.
