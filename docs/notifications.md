# Desktop notifications

The first release covers the running desktop client, including an inactive or
minimized window. Android background push and cross-device read receipts are
separate follow-up work. Quitting the desktop client stops its subscriptions;
a node continuing to work does not itself post notifications on another Mac.

## Events and policy

`zork-client-core::notifications` owns classification and a device-local SQLite
ledger. The desktop adapter owns OS authorization, posting, sound and click
handling. Existing committed catalog synchronization supplies the inputs; no
additional notification polling requests are sent to a Station.

- A new delivered assistant reply can notify. Worker assignment/rework inputs,
  user messages, tool activity and streaming fragments do not. Remote tasks notify
  from their owner node; executor mirrors do not emit duplicate alerts.
- A task entering Review notifies that its result needs review. A final failed
  run or Mesh `needs_attention` notifies that attention is required. Running,
  completed and cancelled tasks withdraw obsolete task-state notifications.
- The first complete online catalog establishes a baseline without notifying
  about existing history. Replies recovered after disconnection must have a
  valid timestamp no more than ten minutes old. Several replies within a
  conversation collapse to the newest notification; catalog markers are not a
  complete message-delivery log.
- A visible current conversation suppresses system notifications, including
  when reading its older messages. Only following the visible tail marks it
  read. OS acceptance, clicking a notification and task completion are distinct
  from that read rule.
- Desktop subscriptions coalesce bursts for 500 ms. State notifications take
  priority over replies to the same task while waiting for OS delivery. The
  pending queue is bounded to 32 conversations, each expiring after ten minutes;
  at most 128 OS-accepted routes are retained.
- Opening a conversation, muting it, disabling notifications, deleting its
  source or revoking device access retracts corresponding notifications where
  supported. Hiding previews also retracts previously accepted previews.

The baseline and pending queue commit as a single ledger value. Notifications
are acknowledged only after macOS accepts the request. The OS identifier is
stable per node/conversation/task; event metadata distinguishes successive
messages even when their visible text is identical. Before posting, macOS
pending and delivered requests are checked to recover an acceptance whose local
receipt was interrupted. OS posting and SQLite cannot form one transaction;
this is best-effort recovery, not a claim that every notification is viewed
exactly once. Transient platform errors retry three times with bounded backoff;
further retries wait for another ledger update or restart. Permission denial
suppresses the pending item and does not backfill it when permission is enabled.

`notify` delivers asynchronous PTC/monitoring messages to the calling Agent
Session mailbox; the hidden `chat.notify` alias retains that same behavior. It
does not publish a Chat message, bypass these policies or invoke the operating
system.

## Settings and routing

The client settings sidebar includes Notifications, using shared Switch controls
for the master setting,
conversation-name preview, system sound and current-conversation mute. It also includes permission
status, permission refresh, test notification and a link to macOS notification
settings. Names are hidden by default. Notification bodies contain the event
type, never message text, tool arguments or credentials.

Permission is requested on the first notification or an explicit test. A bare
`cargo run` binary has no application bundle and reports that notifications are
unavailable instead of invoking an API that can abort the process. The existing
GPUI notification delegate handles clicks; the macOS adapter shares its center
and adds authorization results, sound and acceptance callbacks.

Click routing uses the persisted node and session identity and waits for client
startup when necessary. Removed/revoked devices are rejected. Existing cached
conversation behavior handles offline nodes and archived sessions. Tasks with
no session open their leader/device and explain that no conversation is
available; task IDs are never passed as session IDs.

## Verification

After changes, run:

```sh
python3 scripts/lib/build_env.py -- cargo test --locked -p zork-client-core
python3 scripts/lib/build_env.py -- cargo test --locked -p zork-gui --lib
python3 scripts/lib/build_env.py -- cargo build --locked -p zork-gui
python3 scripts/test-notifications.py
```

Core tests cover baseline, restart, duplicate snapshots, coalescing, expiry,
view suppression, persistent preferences, task transitions, revocation and the
separation between OS receipts and reading. The native settings fixture uses an
isolated client without real nodes, checks persistence, disabled controls and
standalone-binary permission feedback, and captures 1280×800 and 900×600 layouts
under `artifacts/notifications/`. It does not establish that the real OS banner,
sound and cold-start activation have been verified in an installed app bundle.

The packaged application permission query was also checked without requesting
authorization; see `artifacts/notifications/bundle-report.json`. The development
app uses ad-hoc signing because the configured trusted signing certificates are
revoked. OS banner, sound playback and cold-start click verification remain
separate from these checks.
