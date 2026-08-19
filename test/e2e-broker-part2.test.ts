import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";

import { StateStore } from "../src/store/state-store.js";

import type { PersistedAgentTraceEvent, PersistedInboundMessage, SlackSessionRecord } from "../src/types.js";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";

import { MockSlackServer } from "./manual/mock-slack-server.js";

import {
  brokerRoot,
  DEFAULT_E2E_TIMEOUT_MS,
  DAY_MS,
  startBrokerProcess,
  waitForHttpReady,
  waitFor,
  isTransientSqliteLock,
  waitForSessionIdle,
  waitForSessionActive,
  readSessionRecord,
  readInboundMessages,
  readAgentTraceEvents,
  readHasProcessedEvent,
  readPendingSlackEvents,
  delay,
  pathExists,
  removeTempRoot,
  getFreePort,
  collectTextInput,
  findStartedTurnTextContaining,
  postJson,
  createDeferred,
} from "./e2e-broker-helpers.js";

describe.sequential("slack-codex-broker e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      const cleanup = cleanups.pop();
      await cleanup?.();
    }
  });

  it("starts a new session, backfills history, and forwards selected Slack card payloads", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
      channels: [
        {
          id: "C123",
          name: "deep-review",
          is_channel: true,
        },
      ],
    });
    const mockCodex = new MockCodexAppServer();
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-pre-root", {
      type: "message",
      user: "U123",
      channel: "C123",
      ts: "111.220",
      text: "ROOT_CONTEXT_ABC",
    });
    await mockSlack.sendEvent("evt-pre-recent", {
      type: "message",
      user: "U234",
      channel: "C123",
      thread_ts: "111.220",
      ts: "111.221",
      text: "RECENT_CONTEXT_DEF",
    });
    await mockSlack.sendEvent("evt-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "111.220",
      ts: "111.222",
      text: "<@UBOT> 看看 <@U234> 这条 thread",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "first turn start");
    await waitForSessionIdle(tempRoot, "C123:111.220");
    const session = await readSessionRecord(tempRoot, "C123:111.220");
    expect(session).toMatchObject({
      key: "C123:111.220",
      channelName: "deep-review",
      channelType: "channel",
      initiatorUserId: "U123",
      initiatorMessageTs: "111.222",
      workspacePath: path.join(tempRoot, "sessions", "C123-111-220", "workspace"),
    });
    expect(session.agentSessionId).toBeTruthy();
    expect(session.activeTurnId).toBeUndefined();
    expect(session.lastObservedMessageTs).toBeTruthy();
    expect(session.lastDeliveredMessageTs).toBeTruthy();
    await expect(pathExists(session.workspacePath)).resolves.toBe(true);
    await expect(readHasProcessedEvent(tempRoot, "evt-mention")).resolves.toBe(true);
    await expect(readPendingSlackEvents(tempRoot)).resolves.toEqual([]);
    const inbound = await readInboundMessages(tempRoot, "C123:111.220");
    expect(inbound).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          messageTs: "111.222",
          mentionedUserIds: expect.arrayContaining(["U234"]),
          mentionedUsers: expect.arrayContaining([
            expect.objectContaining({
              userId: "U234",
              displayName: "Mock Display 234",
            }),
          ]),
        }),
      ]),
    );
    const firstTurnText = collectTextInput(mockCodex.turnsStarted[0]!.input);
    expect(firstTurnText).toContain("ROOT_CONTEXT_ABC");
    expect(firstTurnText).toContain("RECENT_CONTEXT_DEF");
    expect(firstTurnText).toContain("structured_message_json");
    expect(firstTurnText).toContain('"source": "app_mention"');
    expect(firstTurnText).toContain('"user_id": "U123"');
    expect(firstTurnText).toContain('"text_with_resolved_mentions": "@Mock Bot 看看 @Mock Display 234 这条 thread"');

    const sessionListResponse = await fetch(`${broker.baseUrl}/admin/api/sessions`);
    expect(sessionListResponse.ok).toBe(true);
    const sessionList = (await sessionListResponse.json()) as {
      readonly sessions?: Array<{
        readonly key?: string;
        readonly firstUserMessage?: { readonly textPreview?: string };
        readonly lastUserMessage?: { readonly textPreview?: string };
      }>;
    };
    expect(sessionList.sessions?.find((session) => session.key === "C123:111.220")).toMatchObject({
      firstUserMessage: {
        textPreview: "@Mock Bot 看看 @Mock Display 234 这条 thread",
      },
      lastUserMessage: {
        textPreview: "@Mock Bot 看看 @Mock Display 234 这条 thread",
      },
    });

    await mockSlack.sendEvent("evt-linear-card", {
      type: "message",
      channel: "C123",
      thread_ts: "111.220",
      ts: "111.223",
      subtype: "bot_message",
      bot_id: "BLINEAR",
      app_id: "ALINEAR",
      username: "Linear",
      text: "",
      blocks: [
        {
          type: "section",
          text: {
            type: "mrkdwn",
            text: "*CUE-1180* 感觉 ai chat webview 帧率很低",
          },
        },
      ],
      attachments: [
        {
          title: "CUE-1180 感觉 ai chat webview 帧率很低",
          title_link: "https://linear.app/cue/issue/CUE-1180",
          text: "State: Backlog",
        },
      ],
    });

    await waitFor(() => {
      const deliveredTexts = [...mockCodex.turnsStarted.map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
      return deliveredTexts.some((text) => text.includes('"bot_id": "BLINEAR"'));
    }, "delivery of bot card payload");
    const deliveredTexts = [...mockCodex.turnsStarted.map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
    const botCardText = deliveredTexts.find((text) => text.includes('"bot_id": "BLINEAR"')) ?? "";
    expect(botCardText).toContain('"kind": "bot"');
    expect(botCardText).toContain('"app_id": "ALINEAR"');
    expect(botCardText).toContain('"username": "Linear"');
    expect(botCardText).toContain('"attachments"');
    expect(botCardText).toContain("https://linear.app/cue/issue/CUE-1180");
  }, 90_000);

  it("backfills Slack channel names for persisted sessions on startup", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const seedStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const seedSessions = new SessionManager({
      stateStore: seedStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await seedSessions.load();
    await seedSessions.ensureSession("CBACK", "222.333");
    const now = new Date().toISOString();
    await seedSessions.upsertInboundMessage({
      key: "CBACK:222.333:222.334",
      sessionKey: "CBACK:222.333",
      channelId: "CBACK",
      rootThreadTs: "222.333",
      messageTs: "222.334",
      source: "thread_reply",
      userId: "U123",
      text: "<@U234> 旧消息",
      senderKind: "user",
      mentionedUserIds: ["U234"],
      status: "pending",
      createdAt: now,
      updatedAt: now,
    });
    seedStore.close();

    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
      channels: [
        {
          id: "CBACK",
          name: "admin-trace",
          is_channel: true,
        },
      ],
    });
    const mockCodex = new MockCodexAppServer();
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await waitFor(async () => {
      const session = await readSessionRecord(tempRoot, "CBACK:222.333");
      return session.channelName === "admin-trace" && session.channelType === "channel";
    }, "persisted session channel metadata backfill");
    await waitFor(async () => {
      const inbound = await readInboundMessages(tempRoot, "CBACK:222.333");
      return inbound[0]?.mentionedUsers?.[0]?.displayName === "Mock Display 234";
    }, "persisted inbound mention identity backfill");
  }, 60_000);

  it("replays missed thread messages after restart as a single recovered batch", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer();
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const port = await getFreePort();
    const broker = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "222.220",
      ts: "222.221",
      text: "<@UBOT> 开个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "session bootstrap turn");
    await waitForSessionIdle(tempRoot, "C123:222.220");
    await broker.stop();
    cleanups.pop();

    mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "222.220",
      ts: "222.222",
      text: "漏掉的第一条",
      user: "U123",
    });
    mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "222.220",
      ts: "222.223",
      text: "漏掉的第二条",
      user: "U234",
    });

    const restarted = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => restarted.stop());

    await waitFor(() => {
      const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
      return deliveredTexts.some((text) => text.includes("recovered_message_batch_json"));
    }, "recovered batch turn");
    const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
    const recoveredText = deliveredTexts.find((text) => text.includes("recovered_message_batch_json")) ?? "";
    expect(recoveredText).toContain("recovered_message_batch_json");
    expect(recoveredText).toContain("The broker server restarted or reconnected.");
    expect(recoveredText).toContain('"source": "recovered_thread_batch"');
    expect(recoveredText).toContain('"recovery_kind": "missed_thread_messages"');
    expect(recoveredText).toContain("漏掉的第一条");
    expect(recoveredText).toContain("漏掉的第二条");
    expect(recoveredText).toContain('"batch_message_count": 2');
  }, 90_000);

  it("starts a fresh turn instead of resyncing back to an older active turn after a active input mismatch reset", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    let turnStartCount = 0;
    let releaseFirstTurn: (() => void) | undefined;
    const firstTurnGate = new Promise<void>((resolve) => {
      releaseFirstTurn = resolve;
    });
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnStartCount += 1;
        if (turnStartCount === 1) {
          await firstTurnGate;
          return;
        }
        if (turnStartCount >= 2) {
          context.complete("RECOVERED_AFTER_MISMATCH");
        }
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });
    cleanups.push(async () => {
      releaseFirstTurn?.();
    });

    const port = await getFreePort();
    const sessionKey = "C123:223.220";
    const broker = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: {
        SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS: "100",
        SLACK_MISSED_THREAD_RECOVERY_INTERVAL_MS: "100",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-active-input-mismatch-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "223.220",
      ts: "223.221",
      text: "<@UBOT> keep this turn running",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "initial active turn");
    await waitForSessionActive(tempRoot, sessionKey);
    await broker.stop();
    cleanups.pop();

    const writerStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const writerSessions = new SessionManager({
      stateStore: writerStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await writerSessions.load();
    const existingSession = writerSessions.getSession("C123", "223.220");
    expect(existingSession?.activeTurnId).toBeTruthy();

    const fakeTurnId = "turn-fake-new";
    const fakeActiveSession = await writerSessions.setActiveTurnId("C123", "223.220", fakeTurnId);
    expect(fakeActiveSession.activeTurnId).toBe(fakeTurnId);
    const inflightMessages = writerSessions.listInboundMessages({
      channelId: "C123",
      rootThreadTs: "223.220",
      status: "inflight",
    });
    expect(inflightMessages.length).toBeGreaterThan(0);
    await writerSessions.updateInboundMessagesForBatch(
      "C123",
      "223.220",
      inflightMessages.map((message) => message.messageTs),
      {
        status: "inflight",
        batchId: fakeTurnId,
      },
    );

    mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "223.220",
      ts: "223.222",
      text: "MISSED_AFTER_MISMATCH",
      user: "U234",
    });
    const codexThread = existingSession?.agentSessionId ? mockCodex.getThread(existingSession.agentSessionId) : undefined;
    if (codexThread) {
      codexThread.activeTurnId = undefined;
      for (const turn of codexThread.turns) {
        if (turn.status === "inProgress") {
          turn.status = "interrupted";
        }
      }
    }

    const restarted = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: {
        SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS: "100",
        SLACK_MISSED_THREAD_RECOVERY_INTERVAL_MS: "100",
      },
    });
    cleanups.push(() => restarted.stop());

    try {
      await waitFor(
        () => {
          const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
          return deliveredTexts.some((text) => text.includes("recovered_message_batch_json"));
        },
        "replacement turn after active input mismatch",
        60_000,
      );
    } catch (error) {
      console.error(restarted.logs.join("").slice(-8_000));
      throw error;
    }
    await waitForSessionIdle(tempRoot, sessionKey);

    const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
    const recoveredTurnText = deliveredTexts.find((text) => text.includes("recovered_message_batch_json")) ?? "";
    expect(recoveredTurnText).toContain("recovered_message_batch_json");
    expect(recoveredTurnText).toContain("MISSED_AFTER_MISMATCH");

    await waitFor(async () => {
      const session = await readSessionRecord(tempRoot, sessionKey);
      return !session.activeTurnId && session.lastDeliveredMessageTs === "223.222";
    }, "active-input-mismatch recovered delivery cursor");
    const finalSession = await readSessionRecord(tempRoot, sessionKey);
    expect(finalSession.activeTurnId).toBeUndefined();
    expect(finalSession.lastDeliveredMessageTs).toBe("223.222");

    await waitFor(async () => {
      const inbound = await readInboundMessages(tempRoot, sessionKey);
      return inbound.every((message) => message.status === "done");
    }, "all recovered active-input-mismatch inbound messages done");
    const finalInbound = await readInboundMessages(tempRoot, sessionKey);
    expect(finalInbound.filter((message) => message.status !== "done")).toHaveLength(0);
  }, 90_000);
});
