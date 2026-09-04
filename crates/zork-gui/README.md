# zork-gui

A GPUI (Zed UI framework) desktop client for zork-gateway's built-in
`local_gui` IM entry, with a native light task shell grounded in the current
Codex Desktop layout. The gateway owns conversations and deliberate message
delivery; `zork-agent` remains an internal execution service.

## Layout

```
┌──────────────┬─────────────────────────────────────────────┐
│ zork         │  measured Codex-style task canvas           │
│ New task     │       What should we get done in …?         │
│ Projects     │       [ four real prompt suggestions ]      │
│ workspace    │                                             │
│   Task …     │       ┌ workspace context ──────────┐       │
│ • Task …     │       │ multiline task prompt        │       │
│              │       │ model · thinking       Send │       │
│ ● Local agent│       └──────────────────────────────┘       │
├──────────────┼─────────────────────────────────────────────┤
│ grouped task │ selected task: centered messages, separate  │
│ navigation   │ agent activity, fixed composer              │
└──────────────┴─────────────────────────────────────────────┘
```

## Running

```sh
cargo run -p zork start                        # gateway + agent
cargo run -p zork-gui                          # gateway runtime on http://127.0.0.1:3000
cargo run -p zork-gui -- --gateway-url http://127.0.0.1:3000
```

### Dev automation API

`--dev` starts a token-protected HTTP API on loopback only. The default port is
`8765`; pass `--dev-port 0` to choose an available port. A token is generated
and printed at startup unless `--dev-token` or `ZORK_GUI_DEV_TOKEN` supplies
one.

```sh
ZORK_GUI_DEV_TOKEN=local-dev cargo run -p zork-gui -- --dev

curl -H "Authorization: Bearer $ZORK_GUI_DEV_TOKEN" \
  http://127.0.0.1:8765/v1/elements

curl -H "Authorization: Bearer $ZORK_GUI_DEV_TOKEN" \
  http://127.0.0.1:8765/v1/screenshot -o /tmp/zork-gui.png

curl -H "Authorization: Bearer $ZORK_GUI_DEV_TOKEN" \
  -H 'Content-Type: application/json' \
  -X POST http://127.0.0.1:8765/v1/actions \
  -d '{"type":"type_text","target":{"element_id":"composer-input"},"text":"hello"}'
```

| Route | Purpose |
|---|---|
| `GET /health` | Unauthenticated process readiness; contains no UI data |
| `GET /v1` | Protocol metadata and supported action names |
| `GET /v1/elements` | Visible actionable elements with IDs, roles, labels, bounds, centers, and supported actions |
| `GET /v1/elements?include_hidden=true` | Also include rendered elements clipped by a viewport or scroll mask |
| `GET /v1/elements?after_revision=N&timeout_ms=3000` | Wait for a newer rendered frame, up to 30 seconds |
| `GET /v1/screenshot` | Current window as PNG, with dimensions and UI revision in response headers |
| `POST /v1/actions` | Dispatch `click`, `move`, `type_text`, `key`, `scroll`, or `drag` input |

Coordinates use logical pixels relative to the window content. The element
response includes `scale_factor`; PNG dimensions are logical dimensions times
that factor. An action target is either `{"element_id":"..."}` or an explicit
`{"x":10,"y":20}` point. Scroll deltas are logical pixels; a negative
`delta_y` reveals content below in GPUI scroll areas.

The automation boundary is intentionally black-box. It can read rendered
geometry and pixels, but its only mutations are GPUI mouse and keyboard events.
There are no commands for creating sessions, changing models, sending messages,
or mutating `RootView` directly. Component-ID actions resolve to the visible
center and then dispatch the same events as coordinate actions. The API is not
started without `--dev`, does not bind a non-loopback interface, and does not
enable CORS.

The real-process fixture starts a fake-model Agent plus the production gateway
on isolated ports and verifies the explicit-message boundary:

```sh
cargo build --locked -p zork-agent -p zork-gateway -p zork-call
uv run crates/zork-gui/tests/test_gateway_entry.py
```

The fixture proves that ordinary assistant transcript text stays hidden, an
explicit `zork-call chat post-message` becomes one persistent assistant row,
and two tasks sharing one workspace still resolve their exact gateway binding.

## Gateway-owned conversation API

| UI piece | Source |
|---|---|
| Compact workspace/task rail and status | `GET /v1/im/sessions`, polled every 2 s |
| Delivered message history | `GET /v1/im/sessions/{id}/messages` (cursor pages of 100) |
| Live delivery/activity | SSE `GET /v1/im/sessions/{id}/events` — `message` and `status` |
| Composer send | `POST /v1/im/sessions/{id}/messages` (optionally preceded by `PUT .../selection`) |
| Cancel | `POST /v1/im/sessions/{id}/cancel` |
| Context menu | `GET` / `PUT /v1/im/sessions/{id}/context` |
| New task | `POST /v1/im/sessions`; the initial prompt is then sent as an IM message |

An existing task's composer has a context menu for summary retention targets
(0, 8k, 20k, 40k tokens, plus the current custom value) or handoff documents.
Changes are saved immediately and apply at the next context transition. The
gateway reads and updates the Agent's policy directly; it has no policy copy.

### Status projection

The gateway projects `clear`, `thinking`, `tools_started`, `tool_finished`,
`waiting`, `failed`, `finished`, and `interrupted` as activity. These events may
change the task status label/footer but never add chat rows. Agent commentary,
final transcript text, streaming deltas, tool results, and waits are internal.
A visible assistant reply exists only after the Agent explicitly invokes
`zork-call chat post-message`.

## Native component layer

- `ComposerInput` uses GPUI's platform input handler rather than raw key-string
  mutation. It supports IME marked text, UTF-16 selection exchange, Unicode
  grapheme editing, mouse selection, clipboard actions, explicit newlines, soft
  wrap, and a fixed-height scrolled viewport. Enter submits and Shift+Enter
  inserts a newline.
- `SelectorMenu` gives profile, reasoning, and model controls exact choices
  instead of click-to-cycle behavior. One menu opens at a time; mouse,
  Up/Down/Enter, and Escape are supported without leaking Enter to the
  composer.
- `MessageDocument` parses the coding-message subset of GFM and renders
  headings, emphasis, strike-through, inline/fenced code, lists/task lists,
  block quotes, rules, tables, and links with the existing Codex-style palette.
  Incomplete Markdown and unsupported inline content remain readable.
- Transcript projection keeps optimistic user sends to one visible copy,
  reconciles HTTP/SSE/reconnect ordering, preserves full delivered text, and
  keeps partial-history titles on the stable task id until the true first page
  is known.
- These components are local adapters for the existing `gpui-unofficial`
  runtime. They do not import Longbridge's theme or a second incompatible GPUI
  runtime.

## Known limitations

- Markdown intentionally does not fetch remote images, execute embedded HTML,
  render Mermaid/math, or provide syntax highlighting in this pass.
- One window; no diff/handoff pane (P3).

## Codex UI regression evidence

- `CODEX_UI_ACCEPTANCE.md` fixes the user goal and visible acceptance criteria.
- `COMPOSER_ACCEPTANCE.md` fixes the live-CDP-measured 736 × 141 composer
  geometry and interaction contract from Codex on mini2.
- `COMPOSER_FOCUS_COLOR_ACCEPTANCE.md` fixes the neutral focused-editor color
  while preserving caret and selection feedback.
- `SHELL_ACCEPTANCE.md` fixes the live-CDP-measured sidebar, home, task-header,
  transcript, and selected-task alignment contract from Codex on mini2.
- `WINDOW_CHROME_ACCEPTANCE.md` fixes the complete native-window contract:
  transparent full-size content, no native title, and AppKit traffic lights at
  the Codex source-defined 16 × 16 inset.
- `COMPONENT_PORT_ACCEPTANCE.md` fixes the native input and exact selector-menu
  behavior while preserving the accepted Codex geometry.
- `GATEWAY_IM_ENTRY_ACCEPTANCE.md` fixes the current message/gateway boundary.
- `MESSAGE_RENDERING_ACCEPTANCE.md` is historical evidence for the retired
  direct-Agent projection; its visual Markdown/history findings still apply.
- `src/design.rs` contains the testable palette, hierarchy, and 1280 × 800
  geometry contract used by the renderer.
- `tests/codex_ui_contract.rs` prevents the shell, task rows, transcript, and
  composer from drifting back to the old dashboard treatment.
- `design-audit/` contains the live Codex CDP DOM/style evidence, before/after
  native captures, baseline audit, proportional focused comparisons, final
  Unicode/wrap/menu interaction captures `63` through `66`, and the message
  baseline/final native matrix in captures `67` through `81`; captures `82`
  through `86` document the reported blue focus wash, its neutral fix, and the
  focused before/after comparison.
- `design-qa.md` is the final visual QA gate and comparison history.
- `assets/icons/` contains the embedded Phosphor icon subset and its MIT
  license; no text glyphs or handcrafted SVG approximations are used.

## Verification

```sh
cargo test --locked -p zork-gui
cargo clippy --locked -p zork-gui --all-targets -- -D warnings
cargo build --locked -p zork-agent -p zork-gateway -p zork-call
uv run crates/zork-gui/tests/test_gateway_entry.py
```

## Files

- `src/api.rs` — HTTP + SSE client and wire types.
- `src/automation/` — dev-only loopback API, rendered-element geometry registry,
  screenshot encoder, and GPUI user-input event bridge.
- `src/design.rs` — Codex visual contract, design tokens, and default geometry.
- `src/components/text_input.rs` — native multiline GPUI input handler, edit
  model, selection/caret renderer, and component-scoped key bindings.
- `src/components/selector_menu.rs` — one-open-menu and keyboard highlight
  state.
- `src/components/message.rs` — local GFM message model and GPUI renderer.
- `src/transcript.rs` — role projection, optimistic echo reconciliation,
  history prepending/title, and live-activity rules.
- `src/views.rs` — `RootView`: layout, session/composer/status rendering,
  SSE + polling tasks.
- `src/main.rs` — CLI parsing, GPUI bootstrap.
- `src/window_chrome.rs` — shared transparent, untitled native window options.
- `tests/api_regression.rs` — real-contract and SSE transport regressions.
- `tests/codex_ui_contract.rs` — shell, transcript, composer, and geometry
  regressions.
- `tests/component_port_regression.rs` — Unicode/IME input and exact menu
  regressions.
- `tests/message_rendering_regression.rs` — Markdown, long-content,
  reconciliation, status, and history/title regressions.
- `tests/test_gateway_entry.py` — real Agent + gateway explicit-delivery E2E.
- `tests/fake_agent.py` and `tests/test_fake_agent.py` — retained Agent API
  component fixture; the GUI itself does not connect to this API.
