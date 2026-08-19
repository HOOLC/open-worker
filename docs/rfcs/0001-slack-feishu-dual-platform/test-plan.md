# RFC 0001 Slack / Feishu parity test plan

This file contains the progressive test plan for [RFC 0001](../0001-slack-feishu-dual-platform.md). It starts from the Slack product baseline, maps Feishu parity, then names the automation and real-tenant gates that prove the feature is complete.

## One-Screen Summary

| Layer | Reader need                | Stop when                                                                               |
| ----- | -------------------------- | --------------------------------------------------------------------------------------- |
| 1     | Know what must work        | The Slack baseline and Feishu parity levels are clear.                                  |
| 2     | Run the fast local gate    | Unit, route, mock e2e, admin dashboard, and RFC audit commands pass.                    |
| 3     | Prove production readiness | Real Codex coding smoke plus Slack + Feishu evidence pass and admin health is readable. |
| 4     | Diagnose a parity gap      | The failed capability row points to the owning test slice and expected evidence.        |

The default acceptance command set is:

```sh
pnpm format:check
pnpm lint
pnpm build
pnpm test
pnpm test:e2e:feishu-mock
pnpm rfc:feishu-audit -- --json
pnpm rfc:feishu-test-plan -- --json
pnpm rfc:feishu-completion-audit -- --json
pnpm manual:codex-coding-smoke -- --json
pnpm manual:feishu-smoke -- --status-file evidence/feishu-smoke/admin-status.json --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json --output-dir evidence/feishu-smoke --json
```

## Read Layers

<details>
<summary>Layer 1: Product Capability Matrix</summary>

| Capability                 | Slack baseline                                                                                                                       | Feishu target                                                                                                                                                     | Parity level             | Automated proof                                                                                                     |
| -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------- |
| Runtime readiness          | Socket Mode starts and reports ready.                                                                                                | Long connection starts and reports ready beside Slack.                                                                                                            | Parity                   | `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                                                         |
| Session start/resume       | App mention starts or resumes a thread session.                                                                                      | Group @bot starts or resumes a Feishu topic session; replies in that topic use the same lifecycle as Slack thread replies.                                        | Parity                   | `test/e2e-broker*.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-e2e.test.ts`                                    |
| Active follow-up           | Thread replies continue the active Slack session.                                                                                    | Feishu topic replies continue the same session; rootless non-@ group follow-up continues the active Feishu session only in `all` mode.                            | Parity when approved     | `test/feishu-e2e.test.ts`, real smoke `im:message.group_msg` evidence                                               |
| Degraded follow-up         | Not applicable for normal Slack thread replies.                                                                                      | `at_only` mode is explicitly degraded and must not claim non-@ parity.                                                                                            | Intentional difference   | `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                                                         |
| Private/direct chat        | Slack DMs are supported as broker sessions.                                                                                          | Feishu private chats are ignored before session creation.                                                                                                         | Product difference       | `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                                                         |
| Bot/self filtering         | Broker ignores its own Slack output and stale bot messages.                                                                          | Feishu ignores bot/app/self group senders before dispatch.                                                                                                        | Parity                   | `test/e2e-broker-part5.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                        |
| Stop command               | `-stop` interrupts an active Slack turn and replies status.                                                                          | `-stop` interrupts an active Feishu turn and replies status.                                                                                                      | Parity                   | `test/e2e-broker*.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                             |
| Real Codex coding task     | A real app-server turn edits files in an isolated workspace and proves a check passes.                                               | The same agent loop can be initiated from Feishu after bridge parity; the real coding gate must not rely on mock Codex.                                           | Parity plus Feishu gates | `test/manual/run-real-codex-coding-smoke.ts`, `pnpm manual:codex-coding-smoke -- --json`, `test/feishu-e2e.test.ts` |
| Work status / typing       | Slack shows assistant status through `assistant.threads.setStatus` and falls back to `eyes` reaction; `/slack/post-state` is silent. | Feishu must show a user-visible working indicator or progress card/message update while active; `/chat/post-state` is silent and must not create a terminal card. | Platform-adapted parity  | `test/slack-services.e2e.test.ts`, `test/feishu-e2e.test.ts`, `test/manual/run-real-feishu-smoke.ts`                |
| Read/unread visibility     | Slack client read/unread is outside bot control; product truth is broker open-inbound / handled state plus posted reply evidence.    | Feishu may add bot-message read receipt evidence when permissions are approved; otherwise dashboard open-inbound / handled state is the required product surface. | Platform-adapted parity  | `test/admin-service.e2e.test.ts`, `test/manual/run-real-feishu-smoke.ts`                                            |
| Markdown/text reply        | Markdownish output is converted to Slack mrkdwn and chunked.                                                                         | Text/markdown output is adapted to Feishu text or post/card-compatible output.                                                                                    | Platform-adapted parity  | `test/e2e-broker-part5.test.ts`, `test/e2e-broker-part5.test.ts`, `test/feishu-e2e.test.ts`                         |
| Rich/card inbound          | Slack card/block payloads are retained for agent context.                                                                            | Feishu post/interactive payloads are retained with readable summaries.                                                                                            | Parity                   | `test/e2e-broker-part2.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-e2e.test.ts`                               |
| Rich/card outbound         | Slack supports normal replies plus co-author interactive prompts.                                                                    | Feishu supports rich text, operational cards, and co-author confirmation cards.                                                                                   | Platform-adapted parity  | `test/slack-routes.e2e.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                        |
| File/image inbound         | Slack attachments are downloaded into the session workspace.                                                                         | Feishu image/file resources preserve metadata and download/transfer status.                                                                                       | Parity                   | `test/slack-services.e2e.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-e2e.test.ts`                             |
| File/image outbound        | `/slack/post-file` uploads file content to the thread.                                                                               | `/chat/post-file platform=feishu` uploads Feishu files/images with file ids in evidence.                                                                          | Platform-adapted parity  | `test/chat-routes.e2e.test.ts`, `test/feishu-e2e.test.ts`, `test/feishu-real-smoke.test.ts`                         |
| Bounded history recovery   | Slack missed thread messages are recovered and delivered as one batch.                                                               | Feishu history recovery rebuilds bounded context from message history when cursors exist.                                                                         | Parity                   | `test/e2e-broker-part2.test.ts`, `test/e2e-broker-part3.test.ts`, `test/feishu-e2e.test.ts`                         |
| Background jobs            | Async job events wake the same Slack session and admin can cancel jobs.                                                              | Platform-aware job events wake the same Feishu session and admin cancellation remains shared.                                                                     | Parity                   | `test/e2e-broker-part3.test.ts`, `test/job-routes.test.ts`, `test/feishu-e2e.test.ts`                               |
| Co-author / GitHub mapping | Slack identities drive GitHub author prompts, OAuth bindings, and resolution.                                                        | Feishu mappings use platform-scoped identities and card confirmation.                                                                                             | Platform-adapted parity  | `test/slack-routes.e2e.test.ts`, `test/admin-service.e2e.test.ts`, `test/feishu-e2e.test.ts`                        |
| Admin dashboard            | Slack sessions show channel labels, Slack permalink action, timeline, counts.                                                        | Feishu sessions show platform badge, readable safe coordinates, no Slack-only permalink.                                                                          | Platform-adapted parity  | `test/admin-service.e2e.test.ts`, `test/admin-session-performance.e2e.test.ts`                                      |
| Observability / redaction  | Structured events avoid leaking tokens or raw message bodies.                                                                        | Feishu logs add required platform fields and stricter setup/smoke redaction.                                                                                      | Parity plus Feishu gates | `test/http-request-log-redaction.test.ts`, `test/feishu-real-smoke.test.ts`, `pnpm rfc:feishu-audit -- --json`      |

</details>

<details>
<summary>Layer 2: Fast Local Automation</summary>

- [ ] Run `pnpm format:check`; complete when `oxfmt --check .` reports all matched files formatted.
- [ ] Run `pnpm lint`; complete when `oxlint . --deny-warnings` exits successfully.
- [ ] Run `pnpm build`; complete when admin UI, TypeScript, and static asset copy all pass.
- [ ] Run `pnpm test`; complete when the full Vitest suite passes.
- [ ] Run `pnpm test:e2e:feishu-mock`; complete when Feishu bridge, adapter, fixture replay, and dual runtime tests pass.
- [ ] Run `pnpm rfc:feishu-audit -- --json`; complete when `ok`, `localOk`, and `realTenantOk` are true for the saved evidence bundle.
- [ ] Run `pnpm rfc:feishu-test-plan -- --json`; complete when the capability matrix, proof links, Layer 2 commands, saved smoke report, and RFC audit are all verified.

</details>

<details>
<summary>Layer 3: Real Tenant Smoke</summary>

- [ ] Confirm Slack Socket Mode and Feishu long connection are both ready in one rollout runtime.
- [ ] Run `pnpm manual:codex-coding-smoke -- --json`; complete when a real Codex app-server turn edits `target.txt`, runs `node check.mjs`, and reports `ok=true`.
- [ ] Run `pnpm rfc:feishu-completion-audit -- --json`; complete only when Slack self-regression drive, Feishu self-regression observe, and real Codex coding smoke bundles are all present and sanitized.
- [ ] Confirm Feishu setup evidence records China Feishu, bot identity, `im:message.group_msg=approved`, send-as-bot, card callback, and resource transfer posture without secrets.
- [ ] In the real Feishu group, @bot starts or resumes a session and the broker posts a final text reply.
- [ ] In the same active Feishu session, send a non-@ follow-up and prove it reaches the same session.
- [ ] During an active turn, verify a user-visible working indicator exists: Slack `assistant.threads.setStatus` or fallback reaction, and Feishu typing-equivalent progress card/message evidence.
- [ ] Verify silent terminal state parity: Slack `/slack/post-state final` and Feishu `/chat/post-state final` record lifecycle state without posting a visible terminal-only message/card.
- [ ] Verify read/unread semantics at the product level: inbound follow-up appears as open/pending broker state until dispatched/handled, then clears or is superseded by final reply evidence; Feishu read receipts are optional only if explicitly implemented and permissioned.
- [ ] In the same group session, exercise rich text, card, image/file inbound, outbound rich/card/file, co-author card callback, `-stop`, duplicate replay, and bounded history recovery.
- [ ] Run `pnpm manual:feishu-smoke -- --base-url <admin-url> --setup-evidence-file evidence/feishu-smoke/feishu-setup-evidence.json --output-dir evidence/feishu-smoke --json`; complete when `ok=true`.

</details>

<details>
<summary>Layer 4: Dashboard Acceptance</summary>

- [ ] Slack session rows show human channel labels when available and can resolve a backend Slack permalink.
- [ ] Feishu session rows show a `飞书` platform pill and safe `conversationId/rootMessageId` coordinates.
- [ ] Feishu session details do not show the Slack permalink action until a real Feishu permalink/deep-link contract exists.
- [ ] Mixed Slack + Feishu sessions keep queue state, open inbound counts, auth profile state, token usage, background jobs, and timeline events comparable.
- [ ] Dashboard rows/details expose work status or typing-equivalent state for active sessions, with terminal states clearing the indicator.
- [ ] Dashboard rows/details expose read/unread or broker open-inbound state for unhandled follow-ups, separately from platform-native client unread counts.
- [ ] Admin platform health shows Slack Socket Mode and Feishu long connection independently, including degraded permission posture.

</details>
