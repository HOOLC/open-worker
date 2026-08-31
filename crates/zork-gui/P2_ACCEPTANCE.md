# zork-gui P2 acceptance

> Historical contract (superseded 2026-08-27): this records the retired
> direct-`zork-agent` integration. The current behavioral contract is
> `GATEWAY_IM_ENTRY_ACCEPTANCE.md`; mailbox/tool/wait/delta transcript events
> must not be restored as GUI messages.

## User request

> 你来继续搞吧

This continues the review of `zork-gui`: turn the current compiling prototype
into a client that works against the real `zork-agent` HTTP/SSE contract.

## Baseline before repair

- The crate compiles, but its SSE opener waits for the long-lived reader task
  to finish before returning the stream.
- The real agent projects mailbox input with role `mailbox`; the GUI expects
  `user`.
- The real agent emits separate `wait` and `status` SSE events; the GUI ignores
  both event names.
- The documented fake agent does not route session subresources correctly and
  does not match the real public contract.
- Profile discovery is attempted only once, so starting the GUI before the
  agent leaves session creation unavailable until restart.
- There are no Rust regression tests for the crate.

## Required behavior

1. A successful SSE request returns a consumable stream as soon as response
   headers are available, while the HTTP connection remains open. Dropping the
   stream cancels its reader task.
2. Public role `mailbox` is rendered as the local user role. The legacy `user`
   spelling may remain accepted for compatibility, but the fake agent must emit
   the real spelling.
3. SSE parsing preserves UTF-8 when a multi-byte character is split across
   network chunks.
4. The UI consumes `message`, `wait`, `assistant_delta`, and `status` events.
   Rich status events drive visible thinking/tool/wait/failure/finished state;
   polling remains the coarse connectivity/session fallback.
5. Profile discovery retries after transport/API failures during startup.
6. A failed selection update remains pending for the next send. An SSE mailbox
   event racing the POST response must not duplicate the local user line.
7. `tests/fake_agent.py` routes every documented endpoint and returns payloads,
   roles, event names, and status codes compatible with the real agent.
8. README claims match the current agent contract and verification commands.
9. GPUI uses the platform text/window backends required to render glyphs on
   macOS and open desktop windows on Linux; a desktop smoke test must show
   readable labels and transcript text.

## Verification

- Rust regression tests cover the mailbox role, live SSE return, split UTF-8,
  and event decoding.
- A Python end-to-end test starts the fake agent and exercises profiles,
  sessions, message pagination, selection, mailbox append, cancel, and SSE.
- `cargo fmt -p zork-gui -- --check` passes.
- `cargo test --locked -p zork-gui` passes with non-zero test coverage.
- `cargo clippy --locked -p zork-gui --all-targets -- -D warnings` passes.
- Repository CI commands are run after the crate-level checks.
- A local fake-agent desktop smoke test opens a session, sends a message, and
  observes readable history, streaming output, and rich status changes.

## Completion self-check — 2026-08-26

- Required behaviors 1–9 are implemented.
- `zork-gui`: 11 Rust tests and 5 fake-agent end-to-end tests pass, including
  the later Codex UI contract and new-task validation regressions.
- Workspace Rust suite: 151 tests pass.
- Repository suite: formatting, lint, build, and all 78 end-to-end tests pass.
- Desktop smoke test verified readable glyphs, 100-message pagination, one-copy
  mailbox rendering, thinking/wait status transitions, tool cards, and a
  persisted `fake-1/low` → `fake-2/off` selection change.
- No agent or Surge configuration was changed by this repair.

The later composer correction on 2026-08-27 adds one UI contract regression;
the current zork-gui total is 12 Rust tests plus 5 fake-agent E2E tests.

## Non-goals

- Markdown rendering, IME/caret/paste support, and diff/handoff panes are not
  part of this repair. Packaging and Codex visual parity are tracked by the
  later `CODEX_UI_ACCEPTANCE.md` and `design-qa.md` gate.
- No agent or Surge configuration is changed for the GUI repair.
