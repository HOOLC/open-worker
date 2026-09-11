# Approved native GUI implementation

The local HTML review in `artifacts/brand-review-site/src/gui.html` on mini1
established the product layout through revisions nav34 / brand36 on 2026-09-06.
The GPUI client is the product implementation; that HTML remains a design preview.

## Design authority and shared UI (2026-09-09)

This document is the entry point for approved desktop layout and behavior.
[Interaction states](ui-interaction-states.md) defines the shared component state
contract, and [UI unification](ui-system-unification.md) tracks implementation and
remaining migration. `docs/local-first-cue.md` and earlier HTML revisions retain
their historical/reference role; they do not override subsequently approved rules.
Android's platform-specific geometry and behavior remain in
[`apps/android/design.md`](../apps/android/design.md).

- Keep the existing warm-white surfaces, ink text, capsule buttons, 12 px fields,
  20 px cards and 32 px modal corners. Navigation geometry below is unchanged;
  button density does not redefine navigation row height.
- Actions use `design::INTERACTION`; form borders and semantic feedback surfaces
  use `design::FORM`. Common typography uses `TextRole`. Page code selects a role
  and composes controls instead of defining another ordinary state palette.
- Standard icon actions use 32 px / 12 px radius; compact actions use 28 px / 10 px.
  Composer actions use the 24 px small preset with 14 px glyphs. The ready-to-send
  action uses `BRAND_ACCENT`; disabled and stop states retain their neutral semantics.
  Pressed feedback changes the surface while text remains legible. A keyboard
  focus outline is independent of hover and selected state.
- Field and select backgrounds remain stable on hover/focus. Borders communicate
  hover and focus. An invalid focused field keeps its error surface and uses a
  stronger error border; its explanation stays adjacent to the field.
- Text actions use foreground/underline feedback, including keyboard focus.
  Choice and switch controls retain selection while showing a distinct focus
  outline. Disabled controls do not advertise hover or pressed interaction.
- `status_notice` is the shared feedback entry: info, success, warning, error and
  loading have explicit semantics. Saving preserves form data on error; busy
  actions keep their geometry and reject duplicate submission.
- Migrate through real component state examples and a representative composed
  page before moving remaining call sites. Record validation by platform and state;
  compilation and a static reference are not interaction or performance evidence.

## Navigation and layout

- Device → Leader → Task is the conversation hierarchy. Workers are managed in
  device settings and receive assignments through a Leader.
- Device/task navigation and settings use `navigation::TabGroup`: 32 px rows,
  a shared sliding hover surface, and an independently sliding 2 × 14 px active
  marker in brand orange (`#E9643B`). Active tabs have no selected fill or font-weight
  change. Indentation changes inner padding, not hit areas; section gaps do not
  clip either moving surface. Client/device settings share the same section title
  metrics, leading padding and 2 px tab gaps.
- Sidebar width is measured in pixels, draggable between 200 and 420 px, and
  persisted. Chat and settings use the same sidebar width.
- The compact 44 px conversation header aligns its first participant avatar with
  message avatars. The composer grows with its content and has no manual resize grip.
- The name is Zork. Brand, functional icons, animal avatars, and provider logos are
  local SVG resources. A short hover animation links the mascot and wordmark.

## Conversation behavior

Messages carry durable Gateway IDs and stored timestamps. Known Agent authors use
bound Agent identities; unknown metadata is omitted. Leader assignment/rework
messages are displayed as Agent messages rather than human messages.

Text selection refers to actual rendered message text. A comment retains the
source conversation, durable message ID when available, author, quote, and comment.
Comments remain in a per-conversation draft queue until the main composer sends
one payload. Offline sends use the existing durable client outbox.

`GET /v1/im/sessions` includes `can_send`. A Task with a local owning Leader accepts
human comments through the existing message endpoint. The comment and a Leader
notification commit in one SQLite transaction, keyed by the client request ID.
Retries cannot duplicate either record. The Leader decides whether to request
Worker rework; comments do not start a Worker turn, change review state, or reopen
closed Tasks. Inbound mirrors without a local owner remain read-only.

`GET /v1/node/conversations/read-markers` returns the last durable visible message
ID and timestamp per local conversation. Clients persist seen IDs and refresh on
committed session invalidations. Running status does not imply unread status.
Imported messages emit `messages_changed`; internal execution history retains the
separate `history_changed` event.

## Device and model settings

Device settings have Overview, Agents, and Model Connections entries. Overview
uses actual Gateway version information from `/v1/node/info`. The update control
reports unsupported full installation upgrades until a real updater exists.

Create connections by choosing subscription/API mode, then provider. New OAuth
connections start without fabricated preset models. The connection detail manages
models; Agent selection chooses a connection and one of its configured models.

`PUT /v1/node/profiles/{id}/models` updates only models, preserving provider,
endpoint, headers, credentials, and probe state. Atomic replacement plus a stable
per-profile file lock prevents model edits and credential refresh from overwriting
one another. Public responses never include credentials or private headers.

`POST /v1/node/profiles/{id}/models/refresh` updates the saved model catalog
using the connection's provider endpoint and authentication (including supported
subscription connections). New models with complete limits are enabled; incomplete
models remain disabled until configured. Existing manual settings and disabled
states survive refresh. Missing upstream models are retained. The legacy
`GET /v1/node/profiles/{id}/discovered-models` remains a read-only ID list.

Each model has a shared switch. `PUT /v1/node/profiles/{id}/models/enabled`
persists `{model_id, enabled}`. Older documents default to enabled. Disabled
models are excluded from Agent selection and rejected again before inference;
an already running request is not cancelled. Model rows use a virtual list with
at most four visible rows. Catalog mutations wait for the existing projection
coordinator; unchanged toggles do not write or emit sync notifications.

`PATCH /v1/node/agents/{id}/model` preserves Agent identity, workspace allocation,
and grants. An existing Leader session changes selection for the next turn;
Worker changes apply to future assignments and leave existing Task selections
intact. All Node administration routes require the Node administrator token and
are explicitly allowlisted for paired clients.

## Verification

Load the current repository build environment before Cargo or story tools.
For component/form changes, the current targeted checks are:

- `python3 scripts/storybook/test_package.py`
- `cargo test --locked -p zork-gui --features headless-bench --test headless_interaction_states --test headless_form_states`
- Native `interaction-overview`, `interaction-form`, model detail and connection creation story exports, inspected at compact and wide sizes as applicable.
- Rebuild Web examples with `scripts/storybook/build_web.py`; record actual browser input verification separately from compilation.

Broader desktop, process-boundary and rendering performance validation follows
`.agents/skills/zork-validation/SKILL.md` according to the affected behavior.
Personal-device installation follows its separate build/install/startup workflow.

Integration fixtures use isolated data directories and fake model inference.
They do not use personal connections or provider credentials.
