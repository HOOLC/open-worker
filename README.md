# zork

Zork connects IM entries to durable coding agents. The built-in `local_gui`
entry powers the native desktop client, while configured Slack connections are
external entries behind the same message-delivery boundary. The supervisor
runs one Gateway process with an embedded Agent:

- `zork-gateway` owns IM connections, conversation/session mapping, visible
  message history and delivery, the runtime HTTP API, and management APIs.
- The `zork-agent` library owns each session's durable mailbox and transcript,
  provider and tool execution, per-session event streams, and workspace tools.
  Gateway calls this library directly; no Agent child process is started.

Gateway exposes provider ingress on `18790`, runtime/IM APIs on `3000`,
management APIs on `3001`, and the Agent compatibility API on loopback `3010`.
All listeners belong to the same process. `--agent-token` protects the external
Agent API; internal Gateway calls and status observation use library interfaces.
The standalone `zork-agent` binary remains available for independent use with a
separate data root.

When migrating from the two-process version, restart the supervisor once. An old
supervisor cannot hot-reload this process model; the new `zork update` refuses
that transition. After migration, `zork update` and mesh reload restart Gateway
and its embedded Agent together.

## Repository layout

| Path                  | Purpose                                                   |
| --------------------- | --------------------------------------------------------- |
| `crates/zork-gui`     | Native desktop client and management interface            |
| `benchmarks/deep-swe` | Reproducible DeepSWE project, CLI, adapters, and tests    |
| `crates/zork`         | Supervisor and `zork update` control socket               |
| `crates/gateway`      | IM entries, visible delivery, mailbox ingress, and admin  |
| `crates/agent`        | Durable agent runtime and HTTP API                        |
| `crates/profile`      | Profile storage, refresh, probing, and provider execution |
| `crates/slack`        | Slack client, parsing, formatting, and status delivery    |
| `crates/zork-gh`      | Optional GitHub identity helper                           |
| `packages/zork`       | Legacy optional npm launcher metadata                     |

Management lives in the native desktop client; the standalone web admin has been
removed. Gateway retains its management APIs, including `/admin/api/*`. Gateway
timeline APIs expose session creation, inbound messages, and background jobs.
Agent execution history belongs to the Agent API.
Retired Codex CLI transcript parsers and tool aliases, GitHub OAuth binding pages,
deployment stubs, and MCP placeholder routes have been removed. ChatGPT model
access still uses the active `openai-codex-responses` provider adapter.

## Install a native node

After the first native GitHub Release is published:

```sh
curl -fsSL https://github.com/HOOLC/open-worker/releases/latest/download/install.sh | sh
```

The installer selects the matching macOS/Linux ARM64/x64 package, verifies it,
and installs a persistent Gateway through the native CLI. Node.js and npm are
not required. Use the command from Settings → Device connections to join an
existing mesh. See [native releases](docs/native-releases.md) for supported OS
versions, pinned/offline installation and release validation.

## Run locally

Install dependencies and build:

```bash
pnpm install --frozen-lockfile
pnpm build
```

Start Zork with the default data root (`~/.zork`):

```bash
vp run dev
```

The first start creates `config.json` with mode `0600` and prints the management
API URL. Open the native desktop client for management. Once a Profile is
configured, the desktop `local_gui` entry works without IM provider credentials.
Configure one or more Slack connections when an external IM entry is needed. Product settings come from that JSON file rather
than a product `.env` file.

A minimal config looks like this:

```json
{
  "im_connections": [
    {
      "id": "01J00000000000000000000001",
      "name": "work",
      "enabled": true,
      "mode": "normal",
      "provider": "slack",
      "app_token": "xapp-...",
      "bot_token": "xoxb-..."
    }
  ]
}
```

Use a separate data root with:

```bash
vp run dev -- --data /absolute/path/to/data
```

`--listen HOST` changes the three Gateway listener hosts. It deliberately does
not expose Agent. To require authentication on Agent APIs, start Zork with
`--agent-token <value>`. When omitted, no Agent API bearer is required. The
value is not read from config or persisted.

## Session and workspace model

Agent generates each session ULID. Gateway stores the binding from an IM
conversation to the Agent session returned by `POST /v1/sessions`, including
the canonical workspace returned by Agent. Agent's own session directory
contains only durable runtime state:

```text
{data}/sessions/{ULID}/
  segments/
    {first-event-ULID}.jsonl.zst
    {current-first-event-ULID}.jsonl
```

The caller must pass an existing `workspace` directory when creating a
session. Agent canonicalizes and persists that path, and never creates, moves,
copies, or deletes the caller-owned directory. Gateway creates Slack-owned
workspaces below `{data}/workspaces/slack/{channel}/{root-thread-ts}`; the
desktop entry and other callers can instead bind an existing checkout
directly.

Every JSONL line is one event with its own ULID. A multi-event append writes
multiple adjacent lines with one atomic batch boundary; there is no persisted
commit envelope. Snapshots are events in the same JSONL stream, not separate
files. Ordinary events always append to the current uncompressed segment. When
a snapshot is due, Agent checks the current size: if it is already over 32 MiB,
the snapshot becomes the first event of a new segment and the old segment is
compressed asynchronously with zstd level 12. A restart loads the latest
inline snapshot and replays only its later event suffix.

Agent provides versioned logical tools including `shell.run`, `file.read`,
`file.write`, `file.edit`, `history.list`, and `wait`. File paths resolve from
the canonical workspace, and shell commands retain full output in a readable
live file while returning only a bounded tail.

Every ordinary input is appended to the session's one durable mailbox. Before
each model request, Agent drains every message available at that boundary into
the transcript in mailbox order. This rule is identical whether the session
was waiting, requesting a model, or executing a tool. Sending `-stop` in Slack
or pressing Stop in the desktop client requests Agent cancellation; neither is
a mailbox message.

Assistant transcript messages are internal Agent context and are never
projected into an IM conversation. Agent owns the application-level system
instructions, tool definitions, and bounded tool environment containing the
workspace roots, gateway URL, exact Agent session id, and the optional `gh`
credential wrapper. Gateway capabilities are registered directly as dynamic
tools. The model creates a visible assistant message by invoking
`chat.post_message` with `{"text":"Work is complete.","kind":"final"}`.
The runtime supplies the session binding; the model cannot override it.
`zork-call` is no longer installed or required by the runtime.

The native desktop starts with no active node. Configure model connections and Leader/Worker Agents in Settings; choose Leaders from the conversation sidebar. See [desktop setup and service configuration](docs/desktop-node.md).

`progress`, `final`, `block`, and `wait` describe the purpose of the visible
message; `block` and `wait` require a reason. This metadata does not affect
mailbox delivery or Agent execution.

Gateway returns from input delivery as soon as Agent confirms the mailbox
append. It does not inspect Agent execution state or wait for model/tool work.
Gateway remains the only component holding provider credentials and executing
an explicit entry-specific delivery action. Background-job events also enter
Agent through the same mailbox.

## Context

Each created session has an Agent-owned context policy. Use the desktop composer
to choose `compaction` (summary plus recent original messages) or `handoff`
(handoff document), with common retention targets for `keep_recent_tokens`.
Settings apply at the next context transition; they do not restart the task.

The gateway exposes `GET`/`PUT /admin/api/sessions/{session_key}/context` and
`GET`/`PUT /v1/im/sessions/{session_id}/context` for its desktop entry. Both
read and update Agent state directly. New sessions inherit `context` from the
Agent configuration, whose default is `{"strategy":"compaction","keep_recent_tokens":20000}`.

## Profiles

Profiles are managed through the desktop client and management APIs. They live under
`{data}/profiles/`; a session records only `profile_id`, `model`, and `thinking`,
never credentials or custom header values.
Agent loads and refreshes the profile immediately before a provider attempt,
and request callers cannot inject a provider URL.

The session UI has three independent selectors: model, thinking depth, and
Profile. Only the Profile selector has `自动`; Gateway resolves that value to a
concrete compatible Profile without changing the selected model or thinking.

## Slack app setup

Enable Socket Mode and create an app-level token with `connections:write`.
Typical bot scopes are:

- `app_mentions:read`
- `chat:write`
- `channels:history`
- `files:read` and `files:write` when file input/output is needed
- `users:read` when display names are needed

Subscribe to `app_mention` and `message.channels`. Add the corresponding
history scope and message event for private channels or direct messages you
choose to support.

## Update and deployment

`zork update --data DIR` asks the running supervisor to drain and stop each
owned child, start the updated binary, and wait for its ready PID before moving
on. This is a controlled restart, so the listener being replaced has a short
unavailable interval.

For Docker:

```bash
docker compose up -d
```

The supplied compose file publishes only the three Gateway ports. Agent remains
private inside the container. To build updated Linux binaries into `.data/bin`
and restart them, run:

```bash
bash scripts/dev/update-binaries.sh
```

## Checks

```bash
pnpm format:check
pnpm lint
pnpm build                       # build once before process tests
pnpm test                        # JS behavior and process tests
pnpm test:rust                   # all backend Rust tests
pnpm test:desktop                # native GUI tests on macOS
```

Core CI runs the JS and backend suites plus the real Agent/Gateway
entry check. Process tests use the binaries from the build step. Desktop tests
run separately on macOS when GUI, shared configuration, or Cargo dependencies
change. DeepSWE adapter unit tests run only for benchmark-project changes (or
manually with `pnpm benchmark:deep-swe:test`); they do not run model evaluations.

Performance gates are separate from functional tests. For storage, query, or
runtime performance changes and before release, run the standalone release
benchmarks listed in [the validation guide](docs/zork-agent-status.md#复现命令).
Do not run them alongside builds or other load tests.

The runtime needs `git`, `gh`, and `rg` on `PATH` for coding work.

## License

[MIT](LICENSE)

## Design workspace

The design handbook, interactive references and assets live in [`apps/zork-design`](apps/zork-design/README.md). It is a pnpm workspace package using the root lockfile. Run `pnpm design:dev`, `pnpm design:check`, `pnpm design:test` or `pnpm design:build` from this repository. Shared GPUI examples are built with `pnpm design:components:web`; all design inputs resolve inside the monorepo.
