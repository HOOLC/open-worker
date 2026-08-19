# Feishu Setup and Smoke Checklist

This runbook covers China Feishu support for the shared Slack + Feishu broker runtime. It intentionally does not cover global Lark or Feishu private-chat product support.

## Scope Checklist

- [x] Target China Feishu Open Platform only.
- [x] Install the self-built app only into intended groups.
- [x] Treat private chats as unsupported; they must be ignored by the broker.
- [x] Keep Slack enabled in the same process unless the rollout explicitly disables Slack.
- [x] Do not claim production parity until the real non-@ follow-up smoke passes in `all` mode.

## App Setup Checklist

- [x] Create or reuse a China Feishu self-built app.
- [x] Enable bot/robot capability for group receive/send workflows.
- [x] Enable long connection event delivery.
- [x] Subscribe to group @bot message events.
- [x] Request all group message delivery capability for `im:message.group_msg`.
- [x] Enable message send/reply capability for bot replies.
- [x] Enable card callback events before relying on interactive card actions.
- [x] Record at least one bot identity (`open_id`, `user_id`, or `union_id`) from the Feishu app/bot console.
- [x] Record the exact Feishu console labels used during setup in the rollout notes or PR evidence.

Use the copy-paste permission request packet for approval: [Feishu permission request](feishu-permission-request.md). The RFC permission rationale is in [RFC permissions](rfcs/0001-slack-feishu-dual-platform/permissions.md).

## Console Label Map

Use this as the starting map when operating the Feishu developer console, then copy the exact labels shown in the real tenant into rollout evidence. Feishu UI text can drift; the evidence must record what was actually clicked, while code and smoke checks continue to use API IDs.

| Setup area           | Console/API label to verify                               | API ID / runtime contract                                                                                                                                                                                                                                                                                                  | Evidence to save                                                                 |
| -------------------- | --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| App type             | China Feishu self-built/custom app                        | `FEISHU_DOMAIN=feishu`, `FEISHU_API_BASE_URL=https://open.feishu.cn/open-apis`                                                                                                                                                                                                                                             | App type/scope note, with tenant/app name redacted if needed.                    |
| Bot capability       | `机器人能力` / bot capability                             | Required before message send, reply, upload, and history APIs work.                                                                                                                                                                                                                                                        | Capability enabled, app version published if the console requires publish.       |
| Event delivery       | `使用长连接接收事件`                                      | Broker uses SDK WebSocket long connection, not webhook-only delivery.                                                                                                                                                                                                                                                      | Event delivery mode and timestamp.                                               |
| Message event        | `接收消息`                                                | `im.message.receive_v1`                                                                                                                                                                                                                                                                                                    | Event is subscribed as application identity.                                     |
| Card callback        | `卡片回传交互`                                            | `card.action.trigger`                                                                                                                                                                                                                                                                                                      | Callback is enabled before relying on card confirmation.                         |
| All group messages   | `获取群组中所有消息`                                      | `im:message.group_msg`                                                                                                                                                                                                                                                                                                     | Approval status plus approver/ticket if available.                               |
| Message send/reply   | `获取与发送单聊、群组消息` or `以应用的身份发消息`        | `im:message` or `im:message:send_as_bot`                                                                                                                                                                                                                                                                                   | Which scope was approved for `messages` create/reply.                            |
| Image/file resources | `获取与上传图片或文件资源`                                | Required for image/file resource transfer; the broker enforces Feishu limits before transfer, downloads message image inputs and sends outbound message images up to 10 MB, falls back to file upload for larger outbound images up to 30 MB, caps file/resource transfers at 30 MB, and rejects larger transfers locally. | Which resource scope was approved.                                               |
| Bot identity         | `open_id`, `user_id`, or `union_id` shown for the bot/app | `FEISHU_BOT_OPEN_ID`, `FEISHU_BOT_USER_ID`, or `FEISHU_BOT_UNION_ID`                                                                                                                                                                                                                                                       | Only record whether each ID is set/missing in PR evidence; do not paste raw IDs. |

Primary references:

- [Use long connection to receive events](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case?lang=zh-CN)
- [Message overview and event list](https://open.feishu.cn/document/server-docs/im-v1/introduction)
- [Get message content and `im:message.group_msg`](https://open.feishu.cn/document/server-docs/im-v1/message/get?lang=zh-CN)
- [Send message scopes](https://open.feishu.cn/document/server-docs/im-v1/message/create?lang=zh-CN)
- [Reply message scopes](https://open.feishu.cn/document/server-docs/im-v1/message/reply?lang=zh-CN)
- [Card callback `card.action.trigger`](https://open.feishu.cn/document/feishu-cards/card-callback-communication)
- [Upload image resource scope](https://open.feishu.cn/document/server-docs/im-v1/image/create)
- [Upload file resource scope](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/reference/im-v1/file/create)

## Broker Environment

Set these in the runtime environment:

```env
FEISHU_ENABLED=true
FEISHU_APP_ID=cli_...
FEISHU_APP_SECRET=...
FEISHU_BOT_OPEN_ID=ou_...
# FEISHU_BOT_USER_ID=ou_...
# FEISHU_BOT_UNION_ID=on_...
FEISHU_DOMAIN=feishu
FEISHU_API_BASE_URL=https://open.feishu.cn/open-apis
FEISHU_GROUP_MESSAGE_MODE=all
FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED=false
FEISHU_STARTUP_REQUIRED=true
LOG_RAW_FEISHU_EVENTS=false
```

Set at least one `FEISHU_BOT_*` identity. The broker uses it to recognize real `@bot` mentions before starting or resuming a session; evidence should record only whether each value is set or missing, not the raw ID value.

Use `FEISHU_GROUP_MESSAGE_MODE=at_only` only for development or limited pilot when all-group-message permission is unavailable. In that mode, the broker must surface degraded context guarantees and must not claim full production parity.

Set `FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED=true` only after the real `all` mode non-@ follow-up smoke passes. Until then, admin health keeps Feishu degraded with `all_message_delivery_unverified`.

Use `FEISHU_STARTUP_REQUIRED=false` only when Slack should continue running while Feishu setup is incomplete or degraded. Production Feishu rollout should use strict startup.

## Evidence Checker

Before starting the rollout runtime, run the local environment preflight:

```sh
pnpm manual:feishu-smoke -- --preflight --env-file .env --output-dir evidence/feishu-smoke
```

`pnpm rfc:feishu-audit` can be run before or after preflight to show which local RFC assets, implementation surfaces, test slices, behavior evidence probes, and package-script gates are present and which real tenant evidence files are still missing. `pnpm rfc:feishu-audit:local` exits on the local gate only for CI/local readiness, but its JSON still keeps `ok=false` until real tenant gates pass. It does not send Feishu messages and does not replace the real smoke.

The preflight checks Slack credentials, Feishu credentials, bot identity posture, China Feishu API base, `FEISHU_GROUP_MESSAGE_MODE=all`, strict startup, and raw Feishu logging posture. Use `--env-file` when the rollout settings are in a local env file instead of the current shell; with `pnpm`, put `--` before the smoke-checker arguments so Node's own `--env-file` flag does not intercept it. Value flags accept both `--flag value` and `--flag=value` forms, and missing values fail before another flag is swallowed. Exported shell variables still take precedence. Its evidence files are `feishu-preflight-report.json` and `feishu-preflight-summary.md`. Preflight evidence records secret-bearing settings as set/missing, records only known enum/boolean values for environment posture, and `FEISHU_API_BASE_URL` evidence omits query/hash values.

For a package-based rollout on a macOS VM, run the preflight manually whenever the deployed worker has `FEISHU_ENABLED=true` before switching traffic; store the preflight evidence under the rollout's backup directory. Rollout JSON and metadata report repo-relative backup coordinates instead of full host filesystem paths.

A sanitized `/admin/api/status` platform-health summary records only posture-safe Slack and Feishu enabled/state/degraded/permission status values without copying recent broker logs or permission explanation text.

After performing the real smoke actions below, run:

```sh
mkdir -p evidence/feishu-smoke
cp docs/feishu-setup-evidence.example.json evidence/feishu-smoke/feishu-setup-evidence.json
# Fill evidence/feishu-smoke/feishu-setup-evidence.json with the exact real-tenant labels first.
pnpm manual:feishu-smoke -- --base-url http://127.0.0.1:3000 --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json --output-dir evidence/feishu-smoke
```

The example evidence file intentionally starts with pending/placeholder values. The checker requires `apiName=im:message.group_msg`, `status=approved`, redacted approval evidence, and explicit send/reply, card callback, and resource transfer permission posture evidence. It rejects example text such as "replace", "approval ticket", or other placeholders. Replace those values with exact real-tenant console labels plus redacted approval/configuration evidence before treating setup evidence as complete.

If `BROKER_ADMIN_TOKEN` is configured, either export it in the shell or pass `--admin-token <token>`. Add `--wait-ms 60000` to poll while you are performing the Feishu actions, or `--json` to save machine-readable evidence.

If the checker cannot fetch `/admin/api/status`, it still returns a machine-readable `admin.status_available` failure report. The failure evidence records the sanitized base URL and HTTP status or error class, but omits query/hash values and does not echo the response body.

The checker reads `/admin/api/status?platform=feishu`, recent `broker.jsonl` events, and the setup evidence file. Final smoke and saved `--status-file` verification require `--setup-evidence-file`; a saved admin status JSON alone is not enough for RFC signoff. It cannot send Feishu messages by itself; it verifies that the real tenant actions produced the required health/log evidence. It also checks that admin health exposes independent Slack/Feishu states, current Feishu admin health is `state=ready`, Slack readiness is backed by `chat.platform.ready source=socket_mode` or admin `connection.mode=socket_mode` with `connected=true` and `lastConnectedAt`, Feishu permission posture (`bot_identity=configured`, `im:message.group_msg=verified`, `im:message:send_as_bot=configured`), and `FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED=true` is backed by a same-session non-@ `msgType=text` follow-up transition in the saved logs rather than by the admin flag alone. Feishu readiness must be backed by `chat.platform.ready source=long_connection` or admin `connection.mode=long_connection` with `connected=true` and `lastConnectedAt`, observed Feishu log events include the RFC required fields, bot/app/self sender logs are ignored before any same-message accepted/session/turn dispatch log, duplicate replay evidence has no later same-message accepted/session/turn dispatch after `chat.message.deduped`, image/file accepted logs include `fileId` resource identifiers, Phase 4 evidence includes same group @ session inbound rich/card/resource accepted logs plus same-session outbound rich text, card, and file/image `chat.outbound.posted` logs with uploaded `fileId` for `format=file|image`, card callbacks occur after and are tied to a same group @ session broker-posted card, matching the card `messageId` when Feishu supplies one and otherwise proving ordered same-session/root coordinates plus callback `eventId`/`payloadRef`, Feishu info/warn log metadata does not expose raw body or secret-like fields, setup evidence does not include raw App Secret/access tokens/message bodies/user emails/raw bot IDs, and the real tenant console labels plus `im:message.group_msg` approval and send/card/resource posture were recorded.

To verify a saved evidence bundle later, save the admin status JSON and run:

```sh
pnpm manual:feishu-smoke -- --status-file admin-status.json --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json --output-dir evidence/feishu-smoke --json
```

The evidence directory contains:

- `admin-status.json`: the admin status snapshot used for verification.
- `feishu-setup-evidence.json`: sanitized real-tenant setup labels and permission approval evidence; forbidden fields are omitted and secret-like strings are redacted.
- `feishu-preflight-report.json`: machine-readable preflight checks when `--preflight` is used.
- `feishu-preflight-summary.md`: PR/approval-friendly preflight summary when `--preflight` is used.
- `feishu-smoke-report.json`: machine-readable pass/fail checks.
- `feishu-smoke-summary.md`: PR/approval-friendly summary.

The output `admin-status.json` is sanitized by the smoke checker: it keeps allowlisted platform health, filters `state.sessions` to Feishu session coordinates, keeps only safe scalar session tokens/timestamps, and keeps recent structured logs needed for verification, but omits account/auth/profile state and pending/inflight message previews. When the live admin fetch fails, the same file records `adminStatus.available=false` with sanitized `admin.status_available` evidence instead of a misleading empty status snapshot; later `--status-file` checks replay that explicit failure report. The generated report JSON, markdown summary, and human-readable CLI output also redact unsafe report text fields and evidence text before writing or printing, summary/source URLs omit query/hash values, bundle write notices print sanitized filenames only, and early CLI errors redact unsafe text plus full filesystem paths. The live admin API also redacts inbound message bodies and background job errors in status summaries, summarizes service/auth/session/job/deployment paths without full host filesystem paths, summarizes active sessions without raw co-author candidate IDs, and live admin status and smoke evidence bundles allowlist `recentBrokerLogs` top-level event tokens plus metadata to RFC-safe scalar fields while dropping secret-like string values; malformed broker log lines are reported without echoing their raw text. Rollout metadata recursively redacts unsafe nested metadata string fields while preserving safe posture text such as `FEISHU_APP_SECRET=missing`. It also writes its pre-rollout broker log snapshot as sanitized evidence, keeping allowlisted structured event/meta fields, preserving known startup markers by name, and redacting other non-structured lines. Ops tooling summarizes backup/data-root coordinates without full host filesystem paths, and summarizes active sessions, pending/inflight inbound messages, and background jobs without raw message bodies, job tokens, or job scripts.

Keep `feishu-setup-evidence.json` as posture evidence, not a credential dump: record exact console labels, approval status, and redacted ticket/screenshot references; record bot identity only as set/missing posture. The checker also scans freeform notes and arrays, so do not paste raw `ou_...` / `oc_...` IDs or emails there.

When the smoke checker copies setup evidence into an output bundle, forbidden setup fields are omitted and secret-like string values are redacted so failed evidence bundles do not preserve credentials, message bodies, raw bot IDs, or user emails.

## Real Smoke Checklist

Capture the broker log lines, admin status snapshot, and platform message IDs for each step.

- [x] Broker starts with Slack and Feishu enabled in one process.
- [x] Admin status shows Slack and Feishu independently, with Feishu permission states `bot_identity=configured`, `im:message.group_msg=verified`, and `im:message:send_as_bot=configured`.
- [x] Feishu long connection reaches ready state.
- [x] A Feishu group @bot text message emits an ordered `chat.message.accepted route=bot_mention msgType=text -> chat.session.created|resumed` transition whose `sessionKey`, `conversationId`, and `rootMessageId` match admin session state, with no same-message ignored log.
- [x] A Feishu private-chat event is ignored with `conversationKind=direct` and creates no session.
- [x] A Feishu bot/app/self sender fixture or captured event is ignored with `ignoredReason=ignored_self` and has no same-message accepted/session/turn dispatch log.
- [x] Codex posts a text reply to the same group @ session's originating Feishu group/root message and emits `chat.outbound.posted` with `format=text` and session coordinates matching admin session state.
- [x] The same Feishu session emits an ordered `chat.turn.started|steered -> chat.outbound.posted format=text -> chat.turn.completed` chain, where `chat.turn.completed` appears after its text reply and has `turnId` / `batchId` matching the same-session, non-history-recovery `chat.turn.started` or `chat.turn.steered` log.
- [x] `-stop` in the same group @ session interrupts the active Codex turn and emits an ordered `chat.message.accepted -> chat.session.resumed -> chat.turn.stopped` chain with matching stop `messageId`, `hadActiveTurn=true`, an active `turnId`, no same-message ignored log, and session coordinates matching admin session state.
- [x] In `FEISHU_GROUP_MESSAGE_MODE=all`, a non-@ text follow-up reaches the same active group @ session through an ordered `chat.message.accepted route=group_message msgType=text -> chat.turn.steered|chat.session.resumed` transition, with matching `messageId`, session coordinates matching admin session state, and no same-message ignored log. Test both a thread/root reply and a rootless group message when the Feishu client allows both shapes.
- [x] In `FEISHU_GROUP_MESSAGE_MODE=at_only`, admin/runtime output clearly reports reduced context guarantees.
- [x] Bounded history recovery emits same-session `chat.turn.steered` or `chat.turn.started` with `source=history_recovery`, and `chat.history.recovered` with `recoveredCount > 0` plus session coordinates matching admin session state; `chat.history.recovered` alone is not counted as recovered behavior coverage.
- [x] Rich `post`, interactive card, image, and file messages are summarized without silently discarding raw structure or resource metadata, with accepted logs matching the same group @ session in admin state and no same-message ignored log.
- [x] Feishu outbound rich text, interactive card, and file/image paths are exercised in the same group @ session and emit `chat.outbound.posted` with `format=markdown` or `format=rich_text`, `format=card`, and `format=file` or `format=image`, all with session coordinates matching admin session state; `format=file|image` logs must include the uploaded `fileId`.
- [x] Feishu co-author candidate confirmation card is clicked after a same group @ session `chat.outbound.posted format=card`, and emits an ordered same-session `chat.outbound.posted format=card -> chat.card.callback.received -> chat.coauthor.confirmed` chain whose callback `messageId` matches that outbound card when Feishu supplies one, or otherwise proves ordered same-session/root coordinates plus callback `eventId`/`payloadRef`, plus matching `candidateRevision`, `confirmedCount > 0`, and session coordinates matching admin session state.
- [x] Duplicate delivery or replay emits `chat.message.deduped` with the same `messageId` and `conversationId` as the original accepted event, with no later same-message accepted/session/turn dispatch.
- [x] Controlled degraded/failure evidence is captured, e.g. `chat.platform.degraded` and one of `chat.outbound.failed`, `chat.handler.failed`, or `chat.attachment.download_failed`; coordinate-bearing send/download failures must match admin session state and carry the required failure fields, and detached handler failures must name a known Feishu handler (`message` or `interactive`) plus `errorClass`.
- [x] Slack still receives an event and posts a reply in the same runtime, with ordered Slack `chat.message.accepted -> chat.outbound.posted format=text` evidence that includes accepted/reply `messageId` values and shares the same `sessionKey`, `conversationId`, and `rootMessageId`.

## Evidence To Save

- [x] Environment mode, with secrets redacted.
- [x] `/admin/api/status` output with platform health.
- [x] Relevant `broker.jsonl` events:
  - `chat.platform.ready`
  - Slack `chat.message.accepted`
  - Slack `chat.outbound.posted` with `format=text`, a posted `messageId`, and the same `sessionKey`, `conversationId`, and `rootMessageId` as the accepted Slack event, whose accepted log also has a `messageId`
  - `chat.message.accepted` with `route=bot_mention`
  - `chat.message.accepted` with `route=group_message`
  - `chat.message.ignored`
  - `chat.message.ignored` with `ignoredReason=ignored_self` and no same-message accepted/session/turn dispatch log
  - `chat.message.deduped` with `messageId` and `conversationId` that match the original `chat.message.accepted`, with no later same-message accepted/session/turn dispatch
  - `chat.session.created`
  - `chat.turn.started`
  - `chat.turn.steered`
  - Ordered same-session `chat.message.accepted -> chat.session.resumed -> chat.turn.stopped` stop evidence with matching stop `messageId`, active `turnId`, and no same-message ignored log
  - Ordered same-session `chat.turn.started|steered -> chat.outbound.posted format=text -> chat.turn.completed` evidence, with the completion after the text reply and `turnId` / `batchId` matching the non-history-recovery turn start/steer log
  - `chat.outbound.posted` with `format=markdown` or `format=rich_text`, `format=card`, and `format=file` or `format=image`, all with session coordinates matching the same group @ session, and uploaded `fileId` when `format=file|image`
  - `chat.outbound.failed`, from a controlled fault or saved incident/fault-injection bundle, with session coordinates matching admin session state plus `format`, `errorClass`, `statusCode`, and `attempt`
  - Ordered same-session `chat.outbound.posted format=card -> chat.card.callback.received` evidence where `sessionKey`, `conversationId`, and `rootMessageId` match the group @ session, the callback occurs after the outbound card, the callback `messageId` matches that outbound card `messageId` when Feishu supplies one, missing card message IDs are covered by callback `eventId`/`payloadRef`, and co-author action `kind` / `candidateRevision` are present
  - `chat.coauthor.confirmed` after that callback with the same `sessionKey` and `candidateRevision`, `confirmedCount > 0`, and `conversationId` / `rootMessageId` matching admin session state
  - `chat.history.recovered` with `recoveredCount > 0`, paired by `sessionKey` with `chat.turn.steered` or `chat.turn.started` using `source=history_recovery`, and `conversationId` / `rootMessageId` matching admin session state
  - `chat.platform.degraded`, if any degraded mode is expected
  - `chat.handler.failed` or `chat.attachment.download_failed`, if those are the chosen failed-behavior evidence; handler failures must include `handler=message|interactive` and `errorClass`, and download failures must include session coordinates matching admin session state plus `messageId`, `attachmentId`, `kind`, and `errorClass`
- [x] Feishu group/root/message IDs for replay.
- [x] Slack channel/thread/message IDs proving Slack still works.

## Rollback

- Set `FEISHU_ENABLED=false` if Feishu must be disabled while Slack continues.
- Set `FEISHU_STARTUP_REQUIRED=false` only for a temporary degraded pilot.
- Keep raw Feishu event logging disabled unless collecting a focused, redacted fixture.
- Add any real payload shape that caused a bug as a minimized fixture before changing parser behavior.
