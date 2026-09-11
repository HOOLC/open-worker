# Participant Session history

The native participant entry opens the session ledger in a right sidebar while the
conversation stays visible. The reference is Cue commit `e9a817c0c`, particularly
`SessionHistory.tsx`, `SessionHistoryTimeline.tsx`, `SessionJson.tsx`,
`sessionHistory.css`, and `RightSidebar.tsx` in its shared client packages.

The sidebar retains the execution timeline, zoom/pan/range selection and raw
start/end metadata. Its record list is a reading-oriented projection of the same
durable entries, not a second chat transcript. Model calls stay on the timeline;
internal assistant text never becomes a sent message. Model failures surface as
execution errors without a model-call row.

Activity rows use the shared two-line component: a semantic icon, action and
optional source/destination on the first line, with a bounded plain-text preview
on the second. There are no row dividers or trailing arrows. Times are relative
to the server-adjusted clock. Full content and the original JSON stages open in
a dismissible modal. Known local conversation/task targets navigate to their
conversation; known Agents open their identity details. Missing identities or
external targets without a known route are text, not fabricated links.

Consecutive successful file reads/writes/edits, tool/history queries, and a narrow
allowlist of simple shell inspection commands share one expandable summary.
Counts distinguish unique read/written paths from command/query calls. History
URIs count as history queries. Writes and repeated edits to the same exact path
count as one written file. Tests, installs, compound or unknown shell commands,
errors, unfinished tools, waits, and outward messages/assignments stay visible.
Unknown dynamically registered tools remain independent rows.

The complete entries still drive timeline hit testing. Clicking a grouped tool
span expands its group and reveals that exact invocation; selecting a model span
does not jump to an unrelated row. Expanded members remain virtual list items.
Projection and preview formatting happen when entries change, not on each frame;
updates preserve the reader's anchor when a prepend extends a group. Empty,
loading, retry and earlier-page states stay inside the history surface.

`wait` returns its deadline immediately, so the tool result alone must not close
the displayed waiting interval. History joins the observed deadline or a later
step that consumes mailbox input; requested duration and measured pause duration
remain distinct. Waiting is always a separate item. The visible page refreshes
relative labels every 30 seconds without issuing polling requests.

## Zork data adaptation

- Agent: `GET /sessions/{session_id}/history` accepts `limit` (default 100, max 200),
  `before`, or `after`. It returns durable events, server time, and page cursors.
  Combining before and after is invalid. Snapshot events are excluded.
- Station: `GET /v1/im/sessions/{session_id}/history` checks the desktop IM session
  binding, then uses its authenticated Agent client. Mesh permits this GET only
  for a peer with a client grant; revocation also blocks history reads.
- Rust core joins model steps and tool results only by their exact IDs. Tool labels,
  operation arguments, outcomes, raw JSON, usage, and durations come from Zork.
  An orphan result does not acquire a fabricated start or duration. Model identity
  comes from the selection in effect at request start when that fact is loaded.
  Missing model identity, cache counts, and first-token timing remain unknown.
- Execution records remain distinct from delivered conversation messages.
- Session execution history is stored on the execution node. The client requests
  pages on demand and holds loaded records, event matching and derived entries
  only in Rust core memory; it does not persist execution-history pages or
  indexes in its database/files or restore them from client storage on restart.
  UI consumes core snapshots/deltas and retains presentation state.

Cue's Router name and result envelope are product data; Zork shows its Agent name
and actual tool output. The surrounding left-side conversation remains Zork's
existing conversation. This port does not implement Cue's browser-tab subsystem;
closing the history tab closes its sidebar. The original plus glyph is retained
as sidebar chrome, without introducing an unrelated browser workflow.

## Verification

Use the bounded build environment documented by AGENTS.md. The checks are:

```sh
python3 scripts/test-desktop-headless.py
cargo build --locked -p zork-gui
cargo test --locked -p zork-gui --lib session_history
python3 scripts/test-client-mesh.py
```

The current native `scripts/test-leader-mesh-ui.py` fixture connects a client through
Mesh to a Leader node and opens its execution records from member information.
It checks real records, zoom/fit and closing the panel. The headless renderer
covers fixed history data, bounded visible rows and deterministic replay. These
checks use isolated fixtures and no account credentials or production data.

The history endpoint currently reads the connected node's own Agent store. It
does not forward a task's history to a separate Worker execution node, so that
case currently reports a load failure. This backend limitation is independent
of the retired direct-Station UI.

Side-by-side screenshot assessments and captures are generated locally under
`artifacts/session-history/`. Reproducible fixture data belongs to the GUI test
fixtures; captured images are evidence, not substituted UI.

## Reading-oriented UI checks

- `cargo test --locked -p zork-client-types`: message boundaries, all 22 currently
  registered tool kinds, routine eligibility, receipt-bound sources, orphan
  results and observed wait intervals.
- `cargo test --locked -p zork-gui --features headless-bench --test headless_history`:
  actual GPUI windows at 1280×800 and 900×600; two-line geometry, group expansion,
  timeline-to-group navigation, independent waits, failures, modal Escape and
  target click propagation. Offline fixtures only.
- `cargo run --locked -p zork-gui --features headless-bench --bin zork-gui-render-bench -- <output>`:
  deterministic list scrolling and bounded visible rows. The replay settles panel
  entrance animation before measuring, identically for baseline and changed code.

Shared SVG assets are under `crates/zork-ui/assets/history/`, using the existing
24×24 viewBox, 1.7px currentColor stroke and rounded ends/joins.

### Usage overview

The history page places the virtualized record list at the top, compact model/Profile
information and usage statistics in the middle, and the timeline at the bottom.
The list takes the remaining height; scrolling records does not move the statistics
or timeline. Token totals, input, output, cached reads and cache-hit rate share one
compact row instead of large metric cards. The session SSE always starts with a
snapshot, including model and thinking selection, configured context window,
the current generation's latest provider-reported input token count, and the
same public Profile quota shape used by settings. Future `session_updated`
frames maintain this compact state. No context estimate is added to the reported
count; before a report or after context replacement it is unknown. Quota retains
its provider reporting and last-checked semantics. History pages contain only
explicitly requested execution details and paging metadata; they do not carry
runtime or a snapshot.

Cumulative input/output/cache usage comes from the authoritative session snapshot
and its incremental producer fold, independently of loaded detail pages. Total tokens includes input and output; cached input is already part of
input and is not added again. Cache hit rate is token-weighted across calls with
cache reports. Missing cache reports are excluded from its denominator, with a
partial-coverage label. Missing reports and zero-denominator rates display an em
dash. Legacy snapshots with unknown aggregate coverage display an em dash instead
of substituting a partial-history total. Metadata and bounded hover highlights
travel through the shared core conversation overview topic. Neither opens a
history loader; older detail pages never recalculate session totals.
