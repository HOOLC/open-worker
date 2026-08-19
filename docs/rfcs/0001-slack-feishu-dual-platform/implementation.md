# RFC 0001 Deep Dive: Implementation, TDD, verification, and rollout

This file contains the execution contract for [RFC 0001](../0001-slack-feishu-dual-platform.md). Keep implementation PRs small and cite the relevant phase/ticket.

## One-Screen Summary

| Question            | Answer                                                                                                               |
| ------------------- | -------------------------------------------------------------------------------------------------------------------- |
| How to implement    | Ship vertical TDD slices, each with RED, GREEN, OBSERVE, and REGRESSION evidence.                                    |
| Current state       | Phases 1-5 are implemented, documented, and covered by mock/unit/doc gates plus saved real-tenant evidence.          |
| Completion evidence | Real tenant setup, real Feishu smoke, rollout-runtime evidence, and RFC audit all pass in the saved evidence bundle. |
| Safe PR size        | Cite one phase/ticket, one behavior, and the minimum evidence bundle needed for that behavior.                       |
| Evidence boundary   | Mock e2e proves broker behavior; real smoke proves Feishu tenant permissions and client delivery.                    |

## Read Layers

| Layer | Use when                                    | Expand                                                                                |
| ----- | ------------------------------------------- | ------------------------------------------------------------------------------------- |
| 1     | You need the implementation status quickly. | Read this summary and the [Review gates](review-gates.md#completion-evidence-ledger). |
| 2     | You are picking the next PR slice.          | Expand "Plan, TDD, and phase gates".                                                  |
| 3     | You are writing or reviewing a PR.          | Expand "PR mechanics and tracer backlog".                                             |
| 4     | You are proving readiness.                  | Expand "Verification and real smoke".                                                 |
| 5     | You are changing the RFC itself.            | Expand "Risks, open questions, and drift control".                                    |

<details>
<summary>Layer 2: Plan, TDD, and phase gates</summary>

## Implementation Plan

### Phase 0: RFC readiness

- [x] State China Feishu first.
- [x] State group-only behavior and private-chat exclusion.
- [x] State all-group-message target and `at_only` degraded mode.
- [x] State rich/card raw preservation and phased support.
- [x] State simultaneous Slack + Feishu support, not migration.
- [x] Split RFC into progressive-disclosure entry point plus deep dives.
- [x] Add RFC markdown/link regression tests for the progressive-disclosure docs.
- [x] Accept open-question gates before production rollout.

### Phase 1: Platform-neutral foundation, Slack unchanged

- [x] Introduce `platform`-aware session coordinates and `sessionKey`.
- [x] Preserve legacy Slack session reads.
- [x] Introduce generic `/chat/*` contracts beside `/slack/*` wrappers.
- [x] Delegate Slack `/slack/*` compatibility wrappers through platform-aware chat route contracts.
- [x] Keep Slack e2e and Slack runtime behavior green.
- [x] Add logs with `platform` and `sessionKey` for shared session actions.

### Phase 2: Feishu adapter foundation

- [x] Add Feishu config behind `FEISHU_ENABLED`.
- [x] Add Feishu long-connection adapter shell.
- [x] Parse group @bot text, private text, rich `post`, `interactive`, image/file metadata, and duplicate `message_id`.
- [x] Ignore private chats and bot/self messages.
- [x] Add Feishu API wrapper for send/reply/history/resource calls with fake client tests.
- [x] Expose independent Slack/Feishu admin health.

### Phase 3: Feishu group text MVP

- [x] Group @bot text creates or resumes session.
- [x] Non-@ follow-up reaches active session in `all` mode.
- [x] `at_only` mode marks context degraded.
- [x] `-stop` interrupts matching active Feishu turn.
- [x] Text reply posts to originating Feishu group/root message.
- [x] Restart recovery keeps enough session state to resume.
- [x] Slack regression remains green.

### Phase 4: Rich text, cards, and files

- [x] Preserve inbound rich `post` and card structure.
- [x] Convert Markdown-ish output to Feishu `post`.
- [x] Send static interactive cards for operational prompts.
- [x] Route `card.action.trigger` callbacks.
- [x] Upload outbound images/files and download inbound images into Codex image input.
- [x] Keep visible transfer status for unsupported or unavailable resources.
- [x] Add per-chat throttling/chunking before rich/file rollout.

### Phase 5: Admin, co-authors, and rollout

- [x] Add Feishu setup docs and real smoke checklist.
- [x] Add platform-filtered session/admin views.
- [x] Add platform-aware co-author mapping.
- [x] Add Feishu card-based co-author confirmation for candidate revisions.
- [x] Prove Slack and Feishu can both be ready in one production-like process.

## TDD Contract

Implementation uses vertical-slice TDD, not a horizontal "write all tests first, then implement everything" plan.

Rules:

- [x] Start each behavior with one failing test that expresses user-visible or platform-visible behavior.
- [x] Prefer parser, route, adapter, session-store, and mock e2e public interfaces over private-method tests.
- [x] Implement only enough code to pass the current failing test.
- [x] Refactor only after the slice is green.
- [x] Do not add speculative Feishu behavior without a test that names the behavior.
- [x] Keep Slack regression tests in the loop for every shared-runtime change.
- [x] Every PR description names the red test or explains why the PR is docs-only.

Tracer bullets:

| Phase               | First failing test                                              | First green behavior                                        |
| ------------------- | --------------------------------------------------------------- | ----------------------------------------------------------- |
| Phase 1 foundation  | Legacy Slack session and Feishu-shaped session collide          | Platform-aware keys work while Slack legacy keys still load |
| Phase 1 routes      | `/chat/post-state` cannot address a Slack session               | `/slack/post-state` delegates without behavior change       |
| Phase 2 adapter     | Feishu private-chat event is not ignored                        | Parser emits only accepted group messages                   |
| Phase 2 startup     | `FEISHU_ENABLED=false` still instantiates Feishu                | Slack-only startup unchanged                                |
| Phase 3 text MVP    | Mock Feishu group @bot cannot start a session                   | Session persists and text reply posts                       |
| Phase 3 all-message | Non-@ follow-up does not reach active session                   | Follow-up queues or steers into same session                |
| Phase 4 rich/cards  | `post` and `interactive` payloads are flattened                 | Codex summary plus raw storage                              |
| Phase 4 callbacks   | Card callback cannot reach intended session                     | Callback acks and records session action                    |
| Phase 5 rollout     | Admin cannot show Slack ready and Feishu degraded independently | Platform health is independent                              |
| Phase 5 co-authors  | Feishu session candidates cannot be confirmed before commit     | Feishu card callback confirms the candidate revision        |

## Fixture and Replay Contract

Required initial fixtures:

| Fixture                                                              | Behavior proven                                                                                  |
| -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `feishu/group-at-text.json`                                          | Group @bot text can create or resume a session.                                                  |
| `feishu/private-text.json`                                           | Private chat is ignored and creates no session.                                                  |
| `feishu/group-app-self-message.json`                                 | Bot/app/self group sender is ignored before dispatch.                                            |
| `feishu/group-followup-text.json`                                    | Non-@ follow-up routes to an active session in `all` mode.                                       |
| `feishu/group-followup-parent-only.json`                             | Parent-only Feishu thread replies use `parent_id` as the session root when `root_id` is omitted. |
| `feishu/group-rich-post.json`                                        | Inbound rich text keeps raw `post` data plus readable summary.                                   |
| `feishu/group-interactive-card.json`                                 | Inbound card keeps raw `interactive` data plus readable summary.                                 |
| `feishu/card-action-trigger.json` and `feishu/card-action-skip.json` | Card callbacks map confirmation/skip actions to the intended session and ack quickly.            |
| `feishu/duplicate-message.json`                                      | Duplicate `message_id` is deduped.                                                               |
| `feishu/group-image.json` and `feishu/group-file.json`               | Resource messages preserve metadata and report download/transfer status.                         |
| `feishu/history-page.json`                                           | Bounded history recovery can rebuild context and cursor state.                                   |

Fixture rules:

- [x] Include raw platform payload, expected normalized message, expected route, and expected log event names.
- [x] Remove tenant secrets, user PII, credentials, and message body content not needed for behavior.
- [x] If a production bug depends on payload shape, add minimized fixture before changing runtime code.
- [x] Replay tests exercise public parser, adapter, route, or mock e2e interfaces.
- [x] Feishu duplicate processing keys are based on `conversationId + messageId`; `rootMessageId` drift on replay must not bypass dedupe.

## Phase Quality Gates

| Phase | TDD gate                                                                                                                 | Logging/health gate                                                             | Minimum verification                                         |
| ----- | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- | ------------------------------------------------------------ |
| 0     | RFC explains scope, non-goals, permissions, degradation, and test strategy.                                              | Observability contract is explicit enough to implement.                         | `test/rfc-0001-docs.test.ts` plus Markdown/link/diff checks. |
| 1     | Red tests cover platform-aware keys, legacy Slack compatibility, generic `/chat/*` routes, and Slack wrapper delegation. | Session logs distinguish `platform`, `sessionKey`, and route source.            | Session, route, Slack wrapper, and Slack e2e tests.          |
| 2     | Red tests cover private-chat ignore, group @ accept, duplicate dedupe, API payloads, and feature-flag startup.           | Emit platform startup/ready/degraded and message ignored/accepted/deduped logs. | Config, parser, API, adapter tests.                          |
| 3     | Mock e2e covers group @ start, non-@ follow-up, private ignore, `-stop`, recovery, and text reply.                       | Emit session, history, turn, outbound, and `at_only` degradation logs.          | Feishu mock e2e plus Slack regression.                       |
| 4     | Tests cover inbound raw retention, outbound rich/card rendering, callbacks, and resource degradation.                    | Emit card callback and resource logs without raw body content.                  | Parser/API/formatter/card e2e tests.                         |
| 5     | Tests cover admin health, co-author mapping, card confirmation, and independent health.                                  | Admin shows ready/degraded/disabled per platform.                               | Admin tests plus real Slack + Feishu smoke.                  |

</details>

<details>
<summary>Layer 3: PR mechanics and tracer backlog</summary>

## Issue-Ready Tracer Bullet Backlog

Phase 1:

- [x] `P1-01`: Add platform-aware session keys without breaking legacy Slack.
- [x] `P1-02`: Add generic `/chat/post-state` while preserving `/slack/post-state`.
- [x] `P1-03`: Wrap Slack inbound flow as `ChatInputMessage` without changing Slack behavior.

Phase 2:

- [x] `P2-01`: Add Feishu config and feature-flagged startup.
- [x] `P2-02`: Parse Feishu group/private/rich/card/resource fixtures.
- [x] `P2-03`: Deduplicate Feishu message delivery by `message_id`.
- [x] `P2-04`: Add bounded history API wrapper and degraded recovery status.
- [x] `P2-05`: Expose independent Slack and Feishu platform health in admin status.
- [x] `P2-06`: Add JSONL log harness assertions for required `chat.*` fields.

Phase 3:

- [x] `P3-01`: Create Feishu session from group @bot text.
- [x] `P3-02`: Deliver non-@ follow-up in `all` mode to active session.
- [x] `P3-03`: Make `at_only` degradation visible in admin and prompt context.
- [x] `P3-04`: Post Feishu text replies through generic `/chat/post-message`.

Phase 4:

- [x] `P4-01`: Preserve inbound rich `post` and card structure.
- [x] `P4-02`: Render outbound rich text and operational cards.
- [x] `P4-03`: Route `card.action.trigger` callbacks.

Phase 5:

- [x] `P5-01`: Add platform-aware GitHub author mapping.
- [x] `P5-02`: Confirm Feishu co-author candidates through card callbacks.
- [x] `P5-03`: Let broker-managed background jobs bind to generic Slack/Feishu chat coordinates while preserving Slack legacy aliases.

## Copy-Paste PR Checklist

```md
## RFC trace

- Requirement:
- Phase / ticket:
- RFC sections reviewed:

## TDD slice

- RED:
- GREEN:
- REFACTOR:
- REGRESSION:

## Observability

- OBSERVE:
- Required log events:
- Required log fields:
- Info-level leak check:
- Admin/health evidence:

## Regression / evidence

- Slack regression:
- Feishu unit/mock evidence:
- Real smoke evidence:
- Deferred gates:

## RFC maintenance

- State whether no RFC update was needed, the RFC was updated, or a follow-up issue was linked for deferred compatible work.
```

</details>

<details>
<summary>Layer 4: Verification and real smoke</summary>

## Verification Plan

Evidence ladder:

| Evidence             | Can prove                                                               | Cannot prove                          | Required before                 |
| -------------------- | ----------------------------------------------------------------------- | ------------------------------------- | ------------------------------- |
| RFC review           | Scope, constraints, phase order, trade-offs                             | Runtime behavior                      | Phase 1 continues               |
| Unit tests           | Parser, formatter, API wrapper, storage, routes                         | Real Feishu permissions               | Each focused PR merges          |
| Mock e2e             | Broker create/resume/stop/reply behavior                                | Tenant setup or real client rendering | Phase 3 ready for real smoke    |
| Preflight evidence   | Local env posture for rollout smoke                                     | Real Feishu delivery or permissions   | Before starting rollout runtime |
| Real Feishu smoke    | Long connection, group @, non-@ `all`, bounded history, visible replies | Broad rollout safety                  | Production Feishu MVP claim     |
| Production telemetry | Real traffic health and debug loops                                     | Unimplemented rich/card/file phases   | Broad rollout                   |

### Local mock gate

Run `pnpm test:e2e:feishu-mock` for the local Feishu mock e2e gate. It covers the Feishu bridge, Feishu long-connection adapter, fixture replay, and dual-platform runtime mock tests for group @ start/resume, non-@ follow-up, private/self ignore, `-stop`, history recovery, text/rich/card/file behavior, co-author card callbacks, and Slack+Feishu same-process readiness. This is necessary evidence before real smoke, but it does not prove tenant permissions or real client delivery.

Run `pnpm rfc:feishu-audit` for a progressive RFC readiness summary. It checks that the short RFC entry, deep-dive files, setup evidence template, real-smoke checker, local implementation surfaces, local test slices, behavior evidence probes, rollout/check/status scripts, preflight posture, setup evidence, and saved smoke evidence are wired. `pnpm rfc:feishu-audit:local` exits on `localOk` for CI/local readiness while preserving `ok=false` until real setup and smoke evidence exists. The audit is intentionally incomplete until real Feishu setup and smoke evidence exists; it never sends Feishu messages and cannot replace `pnpm manual:feishu-smoke`.

### Preflight and rollout posture

Run `pnpm manual:feishu-smoke -- --preflight --env-file .env --output-dir evidence/feishu-smoke` before starting the rollout runtime to verify local environment posture: Slack and Feishu credentials, Feishu bot identity, China Feishu API base, `all` mode, strict startup, and raw logging safety.

Use `--env-file` when the rollout settings live in a local env file instead of the current shell; with `pnpm`, put `--` before smoke-checker arguments so Node's own `--env-file` flag does not intercept it. Value flags accept both `--flag value` and `--flag=value` forms, and missing values fail before another flag is swallowed. Exported shell variables still take precedence. Preflight evidence records secret-bearing settings as set/missing, records only known enum/boolean values for environment posture, and `FEISHU_API_BASE_URL` evidence omits query/hash values.

Rollout preflight runs when the deployed worker has `FEISHU_ENABLED=true`, storing evidence in `.backups/rollouts/<timestamp>/feishu-preflight/`. Rollout tooling reports rollout and preflight directories as repo-relative backup coordinates instead of full host filesystem paths, and its pre-rollout log snapshot is sanitized to allowlisted structured event/meta fields or redacted non-structured line summaries instead of raw runtime log text. A sanitized Slack/Feishu platform-health summary is recorded from `/admin/api/status` with only posture-safe enabled/state/degraded/permission status values and no recent broker logs or permission explanation text.

### Real-smoke evidence and setup proof

Real smoke evidence can be checked with `pnpm manual:feishu-smoke -- --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json --output-dir evidence/feishu-smoke` after the tenant actions are performed. The checker reads admin health, recent broker logs, and setup evidence; it does not replace the real Feishu group interaction. Phase 4 signoff needs both inbound rich/card/resource acceptance and outbound rich text, card, and file/image posting evidence from the same group session.

The setup evidence should start from `docs/feishu-setup-evidence.example.json` and be filled with exact real-tenant console labels before the checker is run. The checker requires `apiName=im:message.group_msg`, `status=approved`, approval evidence, and send/reply, card callback, and resource transfer permission posture evidence; it rejects the example's pending status and placeholder text such as "replace" or "approval ticket", and proves template evidence cannot satisfy the real-tenant gate.

Final smoke and saved `--status-file` verification require `--setup-evidence-file`; a saved admin status JSON alone is not enough for RFC signoff. Use `pnpm manual:feishu-smoke -- --status-file admin-status.json --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json` to re-verify saved rollout evidence.

### Sanitized status and failure evidence

If the checker cannot fetch `/admin/api/status`, it returns a machine-readable `admin.status_available` failure report whose evidence records only a sanitized base URL plus HTTP status or error class, omits query/hash values, and does not echo the response body. Output bundles also write `adminStatus.available=false` into `admin-status.json` so failed live fetches remain explicit, reusable evidence instead of a misleading empty status snapshot; saved `--status-file` rechecks replay that same `admin.status_available` failure.

Its output `admin-status.json` is sanitized to keep only allowlisted platform health, safe scalar session coordinates/timestamps, and recent structured logs needed for re-verification. account/auth/profile state and pending/inflight message previews are omitted, smoke report JSON, markdown summaries, and human-readable CLI output redact unsafe report text fields and evidence text before writing or printing, summary/source URLs omit query/hash values, bundle write notices print sanitized filenames only, early CLI errors redact unsafe text plus full filesystem paths, copied setup evidence omits forbidden fields and redacts secret-like strings, the live admin API redacts inbound message bodies and background job errors in status summaries, and active session summaries omit raw co-author candidate IDs.

live admin status and smoke evidence bundles allowlist `recentBrokerLogs` top-level event tokens plus metadata to RFC-safe scalar fields while reporting malformed broker log lines without echoing their raw text. Rollout metadata recursively redacts unsafe nested metadata string fields while preserving safe posture text such as `FEISHU_APP_SECRET=missing`. It also writes its pre-rollout broker log snapshot through the same evidence-safety posture: structured log events keep only allowed metadata, known startup markers are kept by name, and other raw lines are represented by redacted summaries. Ops tooling summarizes backup/data-root coordinates without full host filesystem paths and additionally summarizes active sessions, open inbound messages, and background jobs without raw message bodies, job tokens, or job scripts.

### Behavior coverage signoff

The checker enforces admin health coverage for independent Slack/Feishu state, current Feishu `state=ready`, connection modes, and Feishu permission posture (`bot_identity=configured`, `im:message.group_msg=verified`, `im:message:send_as_bot=configured`), and it requires `FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED=true` to be backed by same-session non-@ `msgType=text` follow-up transition evidence rather than by the admin flag alone. It also enforces Slack Socket Mode readiness through `chat.platform.ready source=socket_mode` or admin connection evidence, long-connection readiness through `chat.platform.ready source=long_connection` or admin connection evidence, observed Feishu log field coverage, duplicate replay evidence with no later same-message accepted/session/turn dispatch after `chat.message.deduped`, inbound rich/card/resource accepted logs matching the same group @ session, outbound `format=file|image` logs carrying uploaded `fileId`, card callback `sessionKey`, `conversationId`, and `rootMessageId` matching admin session state plus a same group @ session broker-posted card where the callback log occurs after the outbound card log, matching card `messageId` when Feishu supplies one and callback `eventId`/`payloadRef` otherwise, behavior coverage for accepted/ignored/deduped/degraded/failed/recovered outcomes, setup evidence safety for raw App Secret/access tokens/message bodies/user emails/raw bot IDs, and info/warn log safety for raw body or secret-like metadata.

Accepted and deduped behavior coverage require admin-session-matching accepted logs with no same-message ignored log. Ignored behavior coverage requires a private-chat ignored log with `conversationKind=direct` and no persisted Feishu session. Degraded behavior coverage requires a known Feishu `degradedReason`, with `permission` for permission-related degradation. Recovered behavior coverage requires `chat.history.recovered recoveredCount > 0` plus a `history_recovery` turn log, not only a degraded recovery attempt.

Happy-path tenant smoke can be combined with controlled replay/fault-injection logs or a saved incident bundle, but the combined evidence must pass `observability.behavior_coverage`.

Requirement evidence:

| Requirement                 | Minimum evidence before claiming complete                                                                                                                                                                                                                                                                                                                                                                                                                      |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| China Feishu first          | Config/docs/smoke target `open.feishu.cn` and defer Lark.                                                                                                                                                                                                                                                                                                                                                                                                      |
| No private chats            | Parser fixture, runtime/mock e2e, and logs prove ignored private events create no session.                                                                                                                                                                                                                                                                                                                                                                     |
| All group messages          | Permission docs name `im:message.group_msg`; admin exposes `all` vs `at_only`; admin health lists `im:message.group_msg=verified`; real non-@ smoke passes.                                                                                                                                                                                                                                                                                                    |
| Bounded history             | Recovery uses `FEISHU_INITIAL_THREAD_HISTORY_COUNT`, clamps explicit `/chat/thread-history` limits to `FEISHU_HISTORY_API_MAX_LIMIT`, and recovered logs match admin session state.                                                                                                                                                                                                                                                                            |
| Rich text + cards           | Raw `post`/`interactive` retention tests, outbound formatter/API tests, card callback mock e2e, real-smoke accepted payload logs matching the same group @ session, same-session outbound rich/card/file or image posting evidence with uploaded `fileId` for file/image logs, and real-smoke callback matching the same group @ session after a broker-posted card, using `messageId` when Feishu supplies one and callback `eventId`/`payloadRef` otherwise. |
| Simultaneous Slack + Feishu | Dual-platform mock e2e, Slack wrapper delegation tests, and Slack regression with independent health/logs.                                                                                                                                                                                                                                                                                                                                                     |
| TDD execution               | Each PR names RED, GREEN, OBSERVE, and REGRESSION.                                                                                                                                                                                                                                                                                                                                                                                                             |
| Self-iteration logs         | Required field tests, body leak tests, and playbook-to-fixture path.                                                                                                                                                                                                                                                                                                                                                                                           |

## Real Smoke Checklist

- [x] Broker connects via Feishu long connection.
- [x] Group @bot text emits an ordered `chat.message.accepted route=bot_mention msgType=text -> chat.session.created|resumed` transition that starts or resumes a Codex session whose `sessionKey`, `conversationId`, and `rootMessageId` match admin session state, with no same-message ignored log.
- [x] Bot/app/self sender fixture or captured event emits `chat.message.ignored ignoredReason=ignored_self` with no same-message accepted/session/turn dispatch log.
- [x] Non-@ group text follow-up enters the same active group @ session in `all` mode, proven by an ordered `chat.message.accepted route=group_message msgType=text -> chat.turn.steered|chat.session.resumed` transition with matching `messageId`, transition session coordinates matching admin session state, and no same-message ignored log; include a rootless group message if the Feishu client delivers that shape outside a message thread.
- [x] Bounded history is visible to Codex with same-session `chat.turn.steered source=history_recovery` for active turns or `chat.turn.started source=history_recovery` for recently active sessions, plus `chat.history.recovered recoveredCount > 0` and session coordinates matching admin session state; missing cursor evidence must be degraded instead of counted as recovered.
- [x] Final text reply posts to the same group @ session with `chat.outbound.posted format=text` and session coordinates matching admin session state.
- [x] Final Feishu turn completion emits an ordered same-session `chat.turn.started|steered -> chat.outbound.posted format=text -> chat.turn.completed` chain, with completion after the text reply and `turnId` / `batchId` matching the non-history-recovery turn start/steer log.
- [x] `-stop` proof targets the same group @ session and includes an ordered `chat.message.accepted -> chat.session.resumed -> chat.turn.stopped` chain with matching stop `messageId`, `hadActiveTurn=true`, an active `turnId`, no same-message ignored log, and session coordinates matching admin session state.
- [x] Feishu co-author confirmation card emits an ordered same-session `chat.outbound.posted format=card -> chat.card.callback.received -> chat.coauthor.confirmed` chain whose callback `messageId` matches the outbound card when Feishu supplies one, otherwise ordered same-session/root coordinates plus callback `eventId`/`payloadRef` prove the tie, callback and confirmation share `candidateRevision`, confirmation includes `confirmedCount > 0`, and session coordinates match admin session state.
- [x] Duplicate delivery/replay emits `chat.message.deduped` with the same `messageId` and `conversationId` as the original accepted event, with no later same-message accepted/session/turn dispatch.
- [x] Controlled degraded/failure evidence includes `chat.platform.degraded` and one failed-behavior event; coordinate-bearing send/download failures must match admin session state and carry their required failure fields, and detached handler failures must name a known Feishu handler (`message` or `interactive`) plus `errorClass`.
- [x] Slack still starts and responds in the same process with ordered Slack `chat.message.accepted -> chat.outbound.posted format=text` evidence that includes accepted/reply `messageId` values and shares the same `sessionKey`, `conversationId`, and `rootMessageId`.
- [x] Admin health shows both platform states.
- [x] `pnpm manual:feishu-smoke` passes against the rollout runtime.

</details>

<details>
<summary>Layer 5: Risks, open questions, and drift control</summary>

## Risks

- `im:message.group_msg` may be denied or delayed in other tenants.
- Long connection is cluster-style delivery, not broadcast.
- Feishu handlers must ack quickly and queue Codex work.
- Feishu message tree semantics may differ from Slack threads.
- Rich text/card payloads can hit size limits faster than plain text.
- Resource download does not support every message/resource type.
- Co-author identity may need extra identity permissions.

## Open Question Gates

| Question                             | Default until decided                                       | Must decide before                            | Non-blocked work                                                |
| ------------------------------------ | ----------------------------------------------------------- | --------------------------------------------- | --------------------------------------------------------------- |
| Feishu startup failure policy        | Production strict; development/limited rollout may degrade. | Production startup behavior and real rollout. | Phase 1 storage/API extraction; parser/API tests.               |
| Feishu co-author confirmation timing | Implemented after text MVP through Feishu cards.            | Production co-author parity claim.            | Real smoke, mapping admin docs, and callback fixture hardening. |
| Operational card style               | Minimal static cards first.                                 | Polished outbound cards.                      | Phase 3 text MVP; inbound card preservation.                    |
| `at_only` production allowance       | Limited pilot only.                                         | Any production parity claim.                  | Parser tests, degraded-mode tests, admin health.                |
| Global Lark                          | Out of scope.                                               | Any PR adding Lark domain/config/docs.        | China Feishu MVP phases.                                        |
| Exact permission labels              | Use stable API names until real setup.                      | Final setup docs and smoke signoff.           | Config, parser, API, mock e2e, admin health.                    |

## RFC Maintenance and Drift Control

Update this RFC set in the same PR when:

- A PR changes an invariant, API contract, session/storage shape, permission assumption, log event, required log field, phase gate, or completion evidence.
- A real fixture contradicts the content model, routing rules, history/recovery, or permission strategy.
- A tracer bullet needs to be split, reordered, or pointed at a different public interface.
- A degraded behavior becomes default beyond development or limited pilot.
- An open question is resolved or a default assumption changes.

Stop and re-review when:

- China Feishu is no longer first target.
- Private chat support becomes required for first implementation.
- `im:message.group_msg` is unavailable but production parity is still requested.
- Slack compatibility requires breaking `/slack/*`, persisted Slack sessions, or Slack mock e2e behavior.
- Raw rich/card/file payloads cannot be preserved.
- Deployment topology invalidates the one-process dual-platform assumption.

</details>
