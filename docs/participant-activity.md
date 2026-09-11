# Participant activity

The desktop activity slot follows Cue's `ActivityLine.tsx`, `AiActivity.tsx`,
`styles.css`, and `useGradientShimmer.ts` in `clients/packages/app` and
`clients/packages/ui`. It lives after all delivered messages in the scrolling
transcript. It reserves one 32px row, uses 24px avatar containers (16px Cue marks),
an 8px gap, and 13px/20px text. Zork's selected Agent avatars remain in use.
Running activity uses a native adaptation of Cue's blue sweep; explicit waiting
is static. Long summaries truncate in the row and are available on hover. Errors
use Cue's notice frame and wrap; finished, cleared, and interrupted states hide.

Gateway derives operation targets from current native tools: file paths for
`file.read`, `file.write`, and `file.edit`; commands for `shell.run`; upload paths,
assignment targets, and other public operation objects where applicable. Targets
are bounded to 512 characters. File/message bodies and arbitrary argument JSON
are not status text. Execution details remain in Session history.

`GET /v1/im/sessions/{id}/status` returns `{items:[{id,name,avatar,session_id,activity}]}`.
Membership comes from the conversation's Agent/Worker assignment bindings. A
Worker uses its task's Session, while its Leader retains its own persistent
Session. The transport-only Mesh client subscribes to the same event source. Remote Worker
activity travels in an authenticated persistent assignment subscription and is
mapped to the owner's conversation; it does not create a local Worker runtime.

The `participants` SSE event contains the bound participants and their current activity.
The `status` event retains the selected session status for compatibility. Tool results remove only
the matching invocation ID, so concurrent calls with the same tool name remain
independent. SSE subscription atomically captures the current status and receives
subsequent updates. Agent stream reconnects preserve projector state. GUI subscriptions
are scoped to the selected conversation and cancelled when selection changes. Disconnects retain the last reliable state. A generic failed-turn
notification preserves the preceding concrete failure reason.

Verification:

- Rust Gateway/GUI unit tests and Cue UI contract tests.
- `scripts/test-desktop-headless.py`: current desktop message/activity rendering, history, selection and modal regressions.
- `scripts/test-leader-worker.py`: local participant membership and Session IDs.
- `scripts/test-remote-workers.py`: remote Worker identity and live command target.
- `scripts/test-client-mesh.py`: transport-only client status access and grants.

Screenshots from isolated fixtures are in `artifacts/participant-activity/`.

Desktop refreshes use `/v1/im/events` and selected-session SSE subscriptions, with
Mesh subscriptions carrying the same messages. SQLite commit notifications and
kernel filesystem notifications wake producers; quiet connections issue no
periodic snapshot requests. Reconnection uses bounded backoff after failures.
History renders a virtual list, and transcript Markdown documents are cached.
The activity sweep pauses during scrolling and resumes after input settles.

The history timeline draws its bars directly in canvas lanes. Consecutive
completed bars share a drawing layer; open endpoints retain their drawing order.
Hit testing uses the painted bounds, including the four-pixel minimum marker
width, clipping and topmost selection. The desktop delivery gate exercises
hover, selection, zoom, pagination and live updates as well as the frame budget.
