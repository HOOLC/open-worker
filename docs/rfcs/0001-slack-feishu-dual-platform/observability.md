# RFC 0001 Deep Dive: Observability and self-iteration contract

This file contains the logging, health, and debugging contract for [RFC 0001](../0001-slack-feishu-dual-platform.md). Logging is part of the feature, not cleanup work.

## One-Screen Summary

| Contract         | Required outcome                                                                                                      |
| ---------------- | --------------------------------------------------------------------------------------------------------------------- |
| Evidence quality | Logs explain accepted, ignored, deduped, degraded, failed, and recovered Feishu outcomes with replayable coordinates. |
| Safety           | Info/warn/admin/smoke evidence must not become a message archive or credential dump.                                  |
| Required matrix  | Feishu RFC evidence is checked against the event/field matrix below and the smoke checker.                            |
| Admin health     | Slack and Feishu states stay independent, with connection and permission posture exposed safely.                      |
| Debug loop       | Production surprises become sanitized fixtures, failing tests, code fixes, and RFC updates when contracts change.     |

## Read Layers

| Layer | Use when                                      | Expand                                         |
| ----- | --------------------------------------------- | ---------------------------------------------- |
| 1     | You need observability intent.                | Read this summary and [Principle](#principle). |
| 2     | You are adding or changing log events.        | Expand "Log event and field contract".         |
| 3     | You are checking leak safety or admin status. | Expand "Safety, retention, and admin health".  |
| 4     | You are debugging production behavior.        | Expand "Debugging and self-iteration".         |

<details>
<summary>Layer 1: Principle</summary>

## Principle

Every Feishu slice should produce enough evidence to answer:

- what happened;
- on which platform;
- for which session/message;
- why the broker chose that route;
- whether the outcome was accepted, ignored, deduped, degraded, failed, or recovered.

Normal logs must be useful without becoming message archives.

</details>

<details>
<summary>Layer 2: Log event and field contract</summary>

## Structured Log Fields

Every platform-runtime log should include known fields from this set:

- `platform`
- `sessionKey`
- `conversationId`
- `conversationKind`
- `rootMessageId`
- `platformThreadId`
- `messageId`
- `attachmentId`
- `fileId`
- `messageCursor`
- `recoveredCount`
- `eventId`
- `source`
- `route`
- `senderKind`
- `handler`
- `turnId`
- `hadActiveTurn`
- `codexThreadId`
- `batchId`
- `jobId`
- `groupMessageMode`
- `degradedReason`
- `ignoredReason`
- `msgType`
- `kind`
- `format`
- `payloadRef`
- `candidateRevision`
- `confirmedCount`
- `errorClass`
- `statusCode`
- `startupRequired`
- `permission`
- `attempt`
- `durationMs`
- `ackDurationMs`

Rules:

- [x] Never log message body text at info/warn level.
- [x] Raw platform payloads may be written only to explicit raw/debug channels or sanitized fixtures.
- [x] Errors include enough coordinates to replay or inspect the affected session.
- [x] Logs distinguish ignored events from accepted events.
- [x] Logs distinguish expected degradation from unexpected failure.
- [x] Retained Feishu message payload refs use `feishu-message:<messageId>`; card callback payload refs use `feishu-card:<eventId>`.
- [x] Accepted behavior coverage counts only accepted Feishu logs that match admin session state and have no same-message ignored log.
- [x] Ignored behavior coverage counts only private-chat ignored logs with `conversationKind=direct` and no persisted Feishu session.
- [x] Deduped behavior coverage counts only when the original accepted Feishu log also matches admin session state and has no same-message ignored log.
- [x] Degraded behavior coverage counts only known Feishu `degradedReason` values; permission-related degraded evidence must include `permission`.

## Required Log Events

| Event                             | Level      | Purpose                                                                                                   |
| --------------------------------- | ---------- | --------------------------------------------------------------------------------------------------------- |
| `chat.platform.starting`          | info       | Platform startup intent and config mode.                                                                  |
| `chat.platform.ready`             | info       | Adapter started and handlers registered.                                                                  |
| `chat.platform.degraded`          | warn       | Missing permission, connection, or smoke capability.                                                      |
| `chat.message.ignored`            | debug/info | Private chat, self/bot message, duplicate, or no matching session.                                        |
| `chat.message.accepted`           | info       | Accepted inbound event coordinates without body text.                                                     |
| `chat.message.deduped`            | debug      | Idempotency evidence tied to a previously accepted `messageId` in the same conversation.                  |
| `chat.session.created`            | info       | Platform-specific root mapping.                                                                           |
| `chat.session.resumed`            | info       | Existing session follow-up route.                                                                         |
| `chat.history.recovered`          | info       | Count, cursor range, and degradation status; recovered coverage also needs a `history_recovery` turn log. |
| `chat.turn.started`               | info       | Session and Codex turn correlation.                                                                       |
| `chat.turn.steered`               | info       | Active turn and follow-up correlation.                                                                    |
| `chat.turn.stopped`               | info       | Stop command interrupted or checked a platform session.                                                   |
| `chat.turn.completed`             | info       | Final state, batch count, and duration.                                                                   |
| `chat.outbound.posted`            | info       | Posted message ID and format.                                                                             |
| `chat.outbound.failed`            | warn/error | Platform send failure without hiding other platform health.                                               |
| `chat.handler.failed`             | warn       | Detached Feishu event handler failed after the long-connection handler returned.                          |
| `chat.card.callback.received`     | info       | Callback to card message/session correlation.                                                             |
| `chat.attachment.download_failed` | warn       | Attachment/resource download failed without hiding the inbound message.                                   |
| `chat.coauthor.confirmed`         | info       | Co-author candidate revision was confirmed through a same-session card callback.                          |

## Required Log Field Matrix

This matrix is enforced for Feishu RFC evidence. Slack platform lifecycle logs use the same event names for admin health, but omit Feishu-only fields such as `groupMessageMode` and permission metadata.

| Event                             | Minimum fields                                                                                                                                                                                                       |
| --------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `chat.platform.starting`          | `platform`, `source`, `groupMessageMode`, `startupRequired`                                                                                                                                                          |
| `chat.platform.ready`             | `platform`, `source`, `groupMessageMode`, `durationMs`                                                                                                                                                               |
| `chat.platform.degraded`          | `platform`, `source`, `groupMessageMode`, `degradedReason`, `permission` when permission-related                                                                                                                     |
| `chat.message.ignored`            | `platform`, `conversationId`, `conversationKind`, `messageId`, `eventId`, `senderKind`, `ignoredReason`, `route`                                                                                                     |
| `chat.message.accepted`           | `platform`, `conversationId`, `conversationKind`, `rootMessageId`, `messageId`, `eventId`, `senderKind`, `msgType`, `route`, `payloadRef` when raw payload is retained, `fileId` when `msgType` is `image` or `file` |
| `chat.message.deduped`            | `platform`, `conversationId`, `messageId`, `eventId`, `route`                                                                                                                                                        |
| `chat.session.created`            | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageId`, `groupMessageMode`                                                                                                                         |
| `chat.session.resumed`            | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageId`, `turnId` when active                                                                                                                       |
| `chat.history.recovered`          | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageCursor`, `recoveredCount`, `degradedReason` when partial, `durationMs`                                                                          |
| `chat.turn.started`               | `platform`, `sessionKey`, `turnId`, `codexThreadId`, `messageId`, `batchId`                                                                                                                                          |
| `chat.turn.steered`               | `platform`, `sessionKey`, `turnId`, `messageId`, `batchId`                                                                                                                                                           |
| `chat.turn.stopped`               | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageId`, `turnId` active if `hadActiveTurn=true`, `hadActiveTurn`                                                                                   |
| `chat.turn.completed`             | `platform`, `sessionKey`, `turnId`, `codexThreadId`, `durationMs`, `batchId`                                                                                                                                         |
| `chat.outbound.posted`            | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageId` or `fileId`, `fileId` when `format` is `file` or `image`, `format`, `durationMs`                                                            |
| `chat.outbound.failed`            | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `format`, `errorClass`, `statusCode`, `attempt`                                                                                                         |
| `chat.handler.failed`             | `platform`, `handler` (`message` or `interactive`), `errorClass`                                                                                                                                                     |
| `chat.card.callback.received`     | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `eventId`, `messageId`, `payloadRef`, `ackDurationMs`, `kind` and `candidateRevision` when co-author action                                             |
| `chat.attachment.download_failed` | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `messageId`, `attachmentId`, `kind`, `errorClass`                                                                                                       |
| `chat.coauthor.confirmed`         | `platform`, `sessionKey`, `conversationId`, `rootMessageId`, `candidateRevision`, `confirmedCount`                                                                                                                   |

</details>

<details>
<summary>Layer 3: Safety, retention, and admin health</summary>

## Log Safety and Retention

- [x] Info/warn logs may include IDs, routing decisions, counts, cursors, payload kinds, format names, durations, and error classes.
- [x] Info/warn logs must not include `appSecret`, verification tokens, access tokens, raw request headers, message body text, raw card JSON, raw rich text JSON, file contents, user email, or unredacted display name.
- [x] Leak checks recurse through nested object/array metadata and reject forbidden raw field names plus token/email-like string values.
- [x] Debug/raw logs are explicit opt-in; fixtures include minimized payloads after redaction.
- [x] Raw HTTP request logs for local Slack/chat/job/integration helper routes redact message text, state reasons, file comments/alt text, rich/card payloads, inline file blobs, job scripts, callback tokens, summaries, details, errors, and MCP call arguments before writing JSONL.
- [x] Normal logs reference retained raw payloads by `payloadRef`, not by copying raw JSON.
- [x] Real-smoke verification checks that `payloadRef` points to the current Feishu `messageId` or card callback `eventId`, not just that the field is present.
- [x] Leak tests use a sentinel message body string and assert it is absent from all info/warn JSONL records.

## Log Test Harness Contract

The current logger writes structured JSONL records to `broker.jsonl` and per-session log files when `meta.sessionKey` is present. Feishu tests should use that public behavior before inventing new abstractions.

Test harness rules:

- [x] Use a temporary `logDir` and read `broker.jsonl` records.
- [x] Treat the structured `message` field as the event name, e.g. `message="chat.message.accepted"`.
- [x] Assert `record.type === "log"`, `record.level`, `record.message`, and required `record.meta` fields.
- [x] Do not assert stdout formatting.
- [x] Prefer parser, adapter, route, admin status, or mock e2e flows that naturally emit logs.
- [x] Add a helper that waits for async JSONL writes to settle or exposes logger flush for tests.
- [x] Add a `feishu-events` raw stream before retaining raw Feishu payloads outside sanitized fixtures.
- [x] Raw-stream tests prove raw Feishu logging is disabled by default and enabled only by explicit config.
- [x] Session log tests prove Feishu uses platform-aware `sessionKey`, not Slack-only fallback coordinates.

## Admin Platform Health Contract

Extend existing `/admin/api/status`; do not add a parallel Feishu-only health endpoint.

Minimum shape:

```ts
type PlatformHealthState = "disabled" | "starting" | "ready" | "degraded" | "failed";

interface PlatformHealthStatus {
  platform: "slack" | "feishu";
  enabled: boolean;
  state: PlatformHealthState;
  startupRequired: boolean;
  groupMessageMode?: "all" | "at_only";
  allMessageDeliveryVerified?: boolean;
  connection?: {
    mode: "socket_mode" | "long_connection" | "http";
    connected: boolean;
    lastConnectedAt?: string;
    lastDisconnectedAt?: string;
  };
  permissions?: Array<{
    name: string;
    requiredFor: string;
    status: "unknown" | "configured" | "verified" | "missing";
  }>;
  lastEvent?: {
    eventId?: string;
    messageId?: string;
    receivedAt: string;
  };
  lastError?: {
    at: string;
    errorClass: string;
    message: string;
  };
}
```

Admin requirements:

- [x] Show Slack and Feishu independently.
- [x] Show `disabled`, `starting`, `ready`, `degraded`, and `failed`.
- [x] Show `groupMessageMode` and all-message delivery verification for Feishu.
- [x] Show `connection.connected=true` only after platform ready evidence, not merely because a configured/default state is ready or degraded.
- [x] Require `connection.connected` as a boolean and require `lastConnectedAt` whenever `connection.connected=true`.
- [x] Show `lastDisconnectedAt` when platform lifecycle logs report a connection close or connection failure.
- [x] Show last platform error and timestamp.
- [x] Do not expose tokens, app secrets, raw message bodies, raw card JSON, or raw rich text JSON.
- [x] Redact pending/inflight inbound message bodies and background job errors in admin status summaries; evidence bundles keep platform health, safe scalar session coordinates/timestamps, and recent structured logs, not account/auth/profile state or message previews.
- [x] Redact unsafe report text fields, evidence text, source URLs, bundle notices, and early CLI errors before writing or printing so failed bundles and copied terminal output do not preserve tokens, user emails, body/payload sentinel text, source URL query/hash values, or full filesystem paths.
- [x] Summarize admin active sessions without raw co-author candidate IDs.
- [x] Summarize live admin API service roots, auth file/profile paths, active session workspaces, background job cwd, and deployment release paths without full host filesystem paths.
- [x] Platform-filtered admin status limits sessions, jobs, and GitHub author mappings to the requested platform while retaining independent Slack/Feishu health and cross-platform `recentBrokerLogs` for same-runtime smoke evidence.
- [x] Allowlist `recentBrokerLogs` top-level event tokens plus metadata in admin status and smoke evidence bundles to RFC-safe scalar fields; malformed broker log lines are reported without echoing their raw text.
- [x] Recursively redact unsafe nested string fields from rollout metadata while preserving safe posture text such as `FEISHU_APP_SECRET=missing`.
- [x] Write rollout pre-rollout broker logs as sanitized evidence snapshots: structured logs keep only allowlisted event/meta fields, known startup markers are kept by name, and other non-structured lines are represented by redacted summaries.
- [x] Summarize ops backup/data-root coordinates without full host filesystem paths; ops summaries also cover active sessions, open inbound messages, and background jobs without raw message bodies, job tokens, or job scripts.
- [x] Summarize `ops:auth:real`, `ops:auth:profiles`, and `ops:ui:real` auth/profile paths before printing or rendering so operator screenshots and copied status output do not preserve full host filesystem paths.

</details>

<details>
<summary>Layer 4: Debugging and self-iteration</summary>

## Log-Driven Debugging Playbooks

| Symptom                                    | Required evidence                                                                | Next TDD/log iteration                                                    |
| ------------------------------------------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Feishu private chat starts a session       | `chat.message.ignored` for `conversationKind=direct` and no session creation     | Add/fix a test named `ignores Feishu private chat events`.                |
| Non-@ follow-up missing in `all` mode      | `chat.platform.ready`, admin health, and accepted/absent inbound log coordinates | Add a minimized fixture for the missed shape or a permission smoke check. |
| Duplicate Feishu event creates two turns   | two deliveries with same `messageId`; missing `chat.message.deduped`             | Add duplicate fixture and dedupe assertion.                               |
| Rich/card content is flattened only        | `chat.message.accepted` lacks `payloadRef` for `post`/`interactive`              | Add raw retention fixture before formatter changes.                       |
| Feishu send fails while Slack works        | `chat.outbound.failed` with Feishu coordinates; Slack health remains ready       | Add send failure route test and admin health assertion.                   |
| Operator cannot explain behavior from logs | Missing required fields for affected event                                       | Add log field matrix test before changing business logic.                 |

## Self-Iteration Loop

For every production surprise:

1. Capture the structured log coordinates.
2. Reduce the platform payload to a sanitized fixture.
3. Write the failing public behavior test.
4. Make the minimal code change to pass.
5. Add or update the required log fields.
6. Update this RFC deep dive if the contract changed.

</details>
