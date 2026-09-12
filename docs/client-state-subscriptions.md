# Client state subscriptions

All client features follow the [Rust core/UI boundary](client-core-ui-boundary.md):
UI is presentation and input capture only. Business operations, validation,
network requests, persistence and operation lifecycles belong to the core.
The migration inventory in that document distinguishes this contract from
remaining violations in the current application adapters.

Upstream notification transport follows [Mesh notifications](mesh-notifications.md):
commit/file changes drive shared sources, and reconnects restore committed cursors.
Healthy Device and settings subscriptions do not install periodic business refreshes.

`zork-client-core::state` owns business state, persistence, merging, change
detection and notification coalescing. A platform receives immutable `Arc`
snapshots and core-generated changes. It does not compare server revisions,
merge messages, restore an optimistic outbox copy or determine unread state.

| Core source | Ownership | Platform work |
| --- | --- | --- |
| `Device` | Shared connection, sessions, agents, tasks, artifacts, mesh and unread projection | Selected route, expanded groups and locale |
| `Conversation` | Cached/live transcript, overlap deduplication, activity, pagination and stop confirmation | Apply the supplied list splice and preserve the viewport |
| Draft and outbox | Transactional draft clearing, comments, durable sends and explicit retry/withdrawal | Editable text buffer, composition and selection |
| `Conversation.overview` | Session SSE snapshot, cumulative usage, runtime metadata and bounded recent activity | Usage row and hover presentation |
| `History` | Explicit detail pages, merge cursors and server clock offset | Expansion, zoom, pan and scroll anchors |
| `Profiles` and `Agents` | Catalog snapshots, edits and refresh publication | Forms, dialogs and field selection |
| `Resources` | On-demand Skill/MCP/service details, bounded cache and connection fencing | Selected detail, parsed document and scroll/focus |
| `Directory.applications` and `Device.content_indices` | Explicit publication, conversation membership and source availability | Visible application and file/page rows |

Conversation content indices contain separate page/file groups built by core, with stable ordering and conversation membership. Search reads only the selected group and shares the original index for an empty query. Native content tabs reuse the existing RootView subscription and artifacts-domain invalidation; they retain only input, scroll and read-only filtered indices. No per-tab network connection or subscription is added.

`Device.content_indices` is the shared conversation index for both files and pages.
The former file-only `artifact_indices` projection and its unused desktop mirror
have been removed; artifact-domain versions and consumer acknowledgements are unchanged.

Page references and applications use the existing catalog Resource records and the Device artifacts domain. They are durable membership data, separate from on-demand resource inspections and execution history. The shared directory aggregates application publications across its currently connected device identities. Resource inspections use the value subscription with explicit prepare/apply/acknowledge and urgent invalidation on revoked access; no periodic inspection task is installed. See [page and resource contracts](mesh-resources.md).

Session execution history remains authoritative on the execution node. The
client requests history pages on demand and matches their events in Rust core
memory. Loaded records, derived entries, pagination cursors and lookup indexes
are not written to the client database or history files, and are not restored
from client storage on restart. UI consumes read-only snapshots or deltas and
keeps presentation state. Memory can remain while controllers or snapshots are
retained; closing a panel does not imply immediate eviction of every shared
object. This execution-history contract is distinct from the delivered-message
cache. A "persistent list" below means immutable versions sharing RAM, not disk
persistence.

Every session SSE connection starts with a current `snapshot`, then delivers
future events and compact `session_updated` projections. Reconnect and lag reset
the snapshot baseline; they do not query or replay the execution archive. The
producer folds cumulative usage, run count and two recent completed activities
into its existing durable session snapshots. Idle overview reads use the latest
snapshot plus its append-only suffix and never use all-archive recovery as a
fallback. Old snapshots without these fields report incomplete coverage.

`ConversationTopics::OVERVIEW` publishes this small read model in core RAM.
Desktop statistics and hover previews share the conversation stream and never
load execution history to initialize aggregates. The initial overview is applied
before independently catching up delivered chat messages. Explicit detail pages
contain records and pagination metadata only; loading them cannot change session
totals. An open detail reader may refresh its requested range after a reconnect
or a history-change hint, without an aggregate-driven archive read.

Within one `ClientStore`, opening the same device with the same connection
returns the same controller. Conversation controllers are shared by session ID.
Registries hold weak references, while the application runtime and recent
conversation cache retain the controllers they need. A raw state subscription
does not retain its producer. Platform projections explicitly retain their
shared controllers; closing a view releases its observation, without cancelling
the underlying business operation. Owned IO tasks end when their controllers drop.

## Versioned observation

`zork-observe`, re-exported as `zork_client_core::observe`, supplies the common
mechanism. It depends on neither a UI framework nor a Tokio runtime. `Source<S,D>`
commits an immutable state root, source-scoped cursor, topic mask and bounded
change journal together. Its publishing API has no `PartialEq` bound: a business
reducer decides whether the touched fields changed. The small-value compatibility
wrapper `ValueSource::publish` still performs equality checking outside the state
and journal lock; high-frequency reducers use explicit changes.

The same protocol is exposed by Device, Conversation, History, Profiles and Agents:

1. Register before reading. The first `prepare()` returns an explicit Reset,
   including an empty snapshot.
2. Wait for readiness, then prepare when the platform is ready to consume. A
   readiness hint performs no diff or encoding and is not an event count.
3. A prepared batch is relative to this reader's **applied** cursor. Repeated
   preparation returns the same batch until it is acknowledged or discarded.
4. Apply all ordered changes, then acknowledge the batch. Discarding or cancelling
   preserves the previous baseline. One reader holds at most one prepared batch.
5. Acknowledgement rechecks concurrent publications before parking. Topic routing
   filters unrelated domains before waking their listeners.
6. A source replacement invalidates earlier prepared batches. `valid()` supports
   validation immediately before a foreign result is applied. Permission removal
   publishes an urgent clearing snapshot; it does not wait for a visible frame.
7. A publisher may close with its last committed state still readable. Dropping a
   reader releases its registration and immediately closes its detached readiness.

Each journal defaults to 512 commits and 2 MiB of estimated payload plus record
storage. Falling behind either bound produces a Reset. These limits cover journal
retention, not all business history, platform caches or externally held snapshots.
`snapshot()`/`changed()` remain convenience wrappers for immediate synchronous
application; cancellable or cross-thread handoffs use explicit preparation and
acknowledgement.

Conversation stores records in a persistent `List<TranscriptLine>` with shared
payloads, a persistent ID lookup and incremental byte accounting. Appending or
editing does not clone the full history. Its producer records `ListEdit`s; readers
compose ordered edits against their own applied versions. Each edit's indices
refer to the list after the preceding edit. Arrival counts use a cumulative
sequence and at most 32 recent IDs, and new readers do not replay old arrivals.
Delivery metadata belongs to the same conversation snapshot: disappearance from
the outbox cannot promote a pending row. Only an authoritative transcript update
promotes a retained pending row to delivered; withdrawal/deletion removes the row.

History owns an incremental execution ledger and an order index. Immutable page
records are shared with the ledger's per-step/per-invocation evidence; appending
or prepending a page projects only affected entries. Model selection is indexed
in event order, and wait spans join their next matching deadline, input wake or
interruption. A prepended selection can update existing starts up to the first
already-known selection. That dependency interval is resolved once per page.
Stable event positions preserve equal-time ordering without renumbering old rows.
The persistent list is updated directly with ordered entry edits; it no longer
replays, sorts and compares the entire loaded history for each new page.

## Platform consumption

GPUI's `FrameDelivery` coalesces Device, Conversation and open History updates
before preparing or converting them. It uses `on_next_frame`, which requests the
platform frame, and catches up a view when it attaches to a window. Native
message documents share unchanged records; sparse edits only touch their affected
cache cells. Urgent revocation bypasses ordinary frame scheduling.

Android uses an independent registry with handles and generations. Rust futures
wait for readiness and issue small JNI callbacks; no waiting observer occupies a
Java IO thread or the command executor lock. The callback only marks dirty.
Choreographer schedules preparation, JSON/DTO work runs on IO, and Main applies
ordered splices inside one Compose snapshot before acknowledgement. Only hints
conflate. `SnapshotStateList.toList()` shares the immutable presentation root, and
a presentation revision avoids a deep list comparison in surrounding UI state.

The wire envelope includes `handle`, `generation`, `batch`, `from` and `reset`.
`from` is the last acknowledged batch in this handle; batch IDs from different
handles or generations are not comparable. Close aborts the Rust waiter and drops
its JNI reference. The Kotlin callback also checks its lease, covering callbacks
already in flight during A → B → A navigation.

A conversation wire projection initially requests the latest 100 records. Reading
history pins it to a stable message anchor, so new arrivals cannot evict the rows
being read. Explicit older/newer requests enlarge that observer's range in steps
of 100; returning to the tail resumes tail selection. Reset encodes this requested
range, never the entire history merely because another consumer loaded it.
Changed records and ordered splices replace full `message_order`/JSON diffing.
The old singleton Session and `subscribe/poll/settings_poll` command paths are
removed. New peers create their shared Device before observation; removing a peer
revokes existing readers as well as clearing the stored replica.

Settings observe committed catalog/cache changes, profile authorization and
operation state. Staged replica pages, identical cache writes and unrelated draft
edits do not wake them. Storage cursor/cache-token-only changes also stay outside
the UI revision. The existing 60-second online freshness deadline is a core
deadline, not a UI polling interval; it still fires if the task starts after the
deadline, then parks without repeating. Draft submission captures draft and transcript
roots under the same submission gate before encoding, preserving their transaction.

Invitation approval uses the same independent observation handles and applied-batch
contract through `Key::Invitation`, before a Device exists. Its core-owned task
observes only the authenticated claim, confirms after approval, and releases its
stream on pause or cancellation. Generation changes fence replaced invitations;
the final SQLite transaction compares the expected pending invitation before
accepting membership. Snapshots contain no invitation secret or challenge.
Android consumes this source without a delay/command loop. Legacy next/poll
invitation commands only read the shared state and ensure its monitor is running.

Android and desktop upgrades share `Device::upgrade`: one command followed by
the existing device feed, with a release and operation identity plus one absolute
deadline. Pausing observation leaves the remote installation running and its
outcome uncertain until reconnection. Browser metadata and latest frames publish
per-host invalidations; GPUI uses `FrameDelivery` before preparing presentation.
These small latest-state sources do not claim incremental large-list complexity.

The WASM gallery imports the public core Profiles/Agents contracts through its
in-memory Station adapter. It uses the same observation engine and compiles
without the native core dependencies. It remains a gallery, not a complete Web
conversation client or a separate WASM wire bridge.

## Current limits and evidence

History's initial materialization still visits all loaded records. Subsequent
updates depend on the new page, affected entries and index paths. Completing an
entry may replay that entry's own evidence; loading an earlier model selection
may legitimately change a large prefix of model annotations. Presentation
grouping, usage summaries and timeline layout still rebuild from their entries;
the core reducer benchmark does not claim these UI costs are incremental. See
the [History measurements](history-incremental-validation.md) for the measured
scope and the full-replay equivalence tests.
Profiles/Agents expose the applied-batch protocol, but their small catalogs still
use snapshot comparisons and synchronous native/gallery consumption. Per-record
catalog routing and chunked streaming text can be added without changing this
protocol; the current transcript API receives complete message records.

The [design and implementation record](client-subscription-design.md) and
[measured report](client-subscription-validation.md) distinguish generic protocol,
business reducer, wire reference mirror, native rendering and real-device evidence.

## Retained presentation

GPUI `Regions` provides retained boundaries for conversation content, composer,
navigation groups and settings sections. Core topic changes invalidate their
presentation regions; animation callbacks notify the animated view. Animated
views must not be placed inside a cached common parent containing unrelated
regions: refreshing that parent makes GPUI traverse its descendants again.
Intrinsic-height regions own a full-width, non-shrinking flex container in both
fresh and retained paths, and remeasure on content or width changes before
reusing their measured height. GPUI lays cached contents out as a separate root;
without this shared constraint, implicit child stretching and centering differ
after caching. Conversation geometry remains owned by the common RootView;
message interactions must not be needed to correct composer placement. Text
shaping for the composer runs inside its region. Plain message selection only
invalidates the transcript, and does not install an unused link click listener:
GPUI click listeners request a full-window refresh for pressed-state tracking.
Plain text also suppresses GPUI's default pressed state during capture, while
its explicit selection/focus handler remains active.
Focus changes may still request a framework-wide refresh; geometry must remain
identical on that path.

`ComposerEdited` denotes a committed edit. `ComposerLayoutChanged` also covers
IME preedit and programmatic text replacement, allowing layout to update without
persisting uncommitted input. Caret/selection changes remain local input state.

The automation registry replays retained region targets, including deferred
popovers, and discards targets when a region changes or disappears. GPUI 1.17
does not replay AccessKit nodes for cached paint ranges, so regions use ordinary
rendering while accessibility is active. Cached scene reuse avoids component
render/layout/prepaint/paint work; it does not mean the GPU redraws only changed
pixels.

Regression coverage lives in core state tests, the shared region/input tests and
`headless_isolation`. The latter drives the real brand in a production
conversation fixture and also publishes a core message to verify that retained
content updates. Rendering performance must additionally pass the current
100,000-message and artifact-list gates.
