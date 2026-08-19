import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";
import { StateStore } from "../src/store/state-store.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { collectTextInput, createDeferred, delay, getFreePort, readInboundMessages, readSessionRecord, removeTempRoot, startBrokerProcess, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";

describe.sequential("slack services e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      const cleanup = cleanups.pop();
      await cleanup?.();
    }
  });

  it("reconciles orphaned inflight messages and starts a fresh agent session when the stored thread is gone", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-services-e2e-"));
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

    await mockSlack.sendEvent("evt-orphan-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "201.220",
      ts: "201.221",
      text: "<@UBOT> open orphan session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "orphan session bootstrap");
    await waitForSessionIdle(tempRoot, "C123:201.220");
    const original = await readSessionRecord(tempRoot, "C123:201.220");
    expect(original.agentSessionId).toBeTruthy();
    await broker.stop();
    cleanups.pop();

    const writerStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const writerSessions = new SessionManager({
      stateStore: writerStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await writerSessions.load();
    const session = writerSessions.getSession("C123", "201.220");
    expect(session).toBeTruthy();
    const idleSession = await writerSessions.setLastDeliveredMessageTs("C123", "201.220", "201.400");
    await writerSessions.setActiveTurnId("C123", "201.220", undefined);
    await writerSessions.setAgentSessionId("C123", "201.220", "thread-missing");
    await writerSessions.upsertInboundMessage({
      key: `${idleSession.key}:201.300`,
      sessionKey: idleSession.key,
      channelId: idleSession.channelId,
      rootThreadTs: idleSession.rootThreadTs,
      messageTs: "201.300",
      source: "thread_reply",
      userId: "U123",
      text: "already delivered",
      status: "inflight",
      batchId: "turn-old",
      createdAt: "2026-03-17T00:00:00.000Z",
      updatedAt: "2026-03-17T00:00:00.000Z",
    });
    await writerSessions.upsertInboundMessage({
      key: `${idleSession.key}:201.301`,
      sessionKey: idleSession.key,
      channelId: idleSession.channelId,
      rootThreadTs: idleSession.rootThreadTs,
      messageTs: "201.301",
      source: "background_job_event",
      userId: "BOT",
      text: "same old batch",
      status: "inflight",
      batchId: "turn-old",
      createdAt: "2026-03-17T00:00:00.000Z",
      updatedAt: "2026-03-17T00:00:00.000Z",
    });
    await writerSessions.upsertInboundMessage({
      key: `${idleSession.key}:201.500`,
      sessionKey: idleSession.key,
      channelId: idleSession.channelId,
      rootThreadTs: idleSession.rootThreadTs,
      messageTs: "201.500",
      source: "thread_reply",
      userId: "U123",
      text: "NEEDS_REPLAY_AFTER_ORPHAN",
      status: "inflight",
      batchId: "turn-new",
      createdAt: "2026-03-17T00:00:00.000Z",
      updatedAt: "2026-03-17T00:00:00.000Z",
    });
    writerStore.close();

    const restarted = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => restarted.stop());

    await waitFor(() => {
      return mockCodex.turnsStarted.slice(1).some((turn) => collectTextInput(turn.input).includes("NEEDS_REPLAY_AFTER_ORPHAN"));
    }, "orphaned inflight replayed after missing thread reset");
    await waitForSessionIdle(tempRoot, "C123:201.220");

    const inbound = await readInboundMessages(tempRoot, "C123:201.220");
    expect(inbound.find((message) => message.messageTs === "201.300")?.status).toBe("done");
    expect(inbound.find((message) => message.messageTs === "201.301")?.status).toBe("done");
    expect(inbound.find((message) => message.messageTs === "201.500")?.status).toBe("done");
    const recovered = await readSessionRecord(tempRoot, "C123:201.220");
    expect(recovered.agentSessionId).toBeTruthy();
    expect(recovered.agentSessionId).not.toBe("thread-missing");
    expect(recovered.agentSessionId).not.toBe(original.agentSessionId);
    expect(mockCodex.threadResumes.some((resume) => resume.threadId === "thread-missing")).toBe(true);
  }, 90_000);

  it("downloads Slack attachments into the workspace and maps tool names to assistant status", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-services-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const sawToolStatus = createDeferred<void>();
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        if (collectTextInput(context.input).includes("use the attached icon")) {
          context.notify("codex/event/tool_start", {
            threadId: context.threadId,
            turnId: context.turnId,
            callId: "fresh-tool",
            name: "apply_patch",
          });
          await waitFor(() => mockSlack.assistantStatusUpdates.some((update) => update.status === "Updating files..."), "apply_patch assistant status");
          sawToolStatus.resolve();
          context.notify("codex/event/tool_end", {
            threadId: context.threadId,
            turnId: context.turnId,
            callId: "fresh-tool",
          });
        }
        context.complete("");
      },
    });
    const slackPort = await mockSlack.start();
    const fileUrl = mockSlack.addDownloadableFile("F123", Buffer.from("<svg/>"), "image/svg+xml");
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      sawToolStatus.resolve();
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

    await mockSlack.sendEvent("evt-attach-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "301.220",
      ts: "301.221",
      text: "<@UBOT> use the attached icon",
      files: [
        {
          id: "F123",
          name: "../screen.svg",
          mimetype: "image/svg+xml",
          url_private_download: fileUrl,
        },
      ],
    });

    await waitFor(() => mockCodex.turnsStarted.some((turn) => collectTextInput(turn.input).includes("use the attached icon")), "attachment turn start");
    await sawToolStatus.promise;
    await waitForSessionIdle(tempRoot, "C123:301.220");

    const turnText = collectTextInput(mockCodex.turnsStarted.find((turn) => collectTextInput(turn.input).includes("use the attached icon"))!.input);
    expect(turnText).toContain('"attachments": [');
    expect(turnText).toContain("F123-screen.svg");
    expect(turnText).not.toMatch(/"type": "image"/);
    const session = await readSessionRecord(tempRoot, "C123:301.220");
    const expectedPath = path.join(session.workspacePath, ".slack-attachments", "301.221", "F123-screen.svg");
    expect(turnText).toContain(`"local_path": "${expectedPath}"`);
    expect(await fs.readFile(expectedPath, "utf8")).toBe("<svg/>");
    expect(mockSlack.assistantStatusUpdates).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          channel: "C123",
          threadTs: "301.220",
          status: "Updating files...",
        }),
      ]),
    );

    await Promise.all([
      mockSlack.sendEvent("evt-race-a", {
        type: "app_mention",
        user: "U123",
        channel: "C123",
        thread_ts: "302.220",
        ts: "302.221",
        text: "<@UBOT> first race",
      }),
      mockSlack.sendEvent("evt-race-b", {
        type: "message",
        user: "U234",
        channel: "C123",
        thread_ts: "302.220",
        ts: "302.222",
        text: "second race",
      }),
    ]);
    await waitFor(() => mockCodex.turnsStarted.some((turn) => collectTextInput(turn.input).includes("first race") || collectTextInput(turn.input).includes("second race")), "race session turn");
    await waitFor(() => mockSlack.postedMessages.filter((message) => message.threadTs === "302.220" && message.text.includes("查看会话活动时间线")).length === 1, "session permalink posted once during inbound race");
    await waitForSessionIdle(tempRoot, "C123:302.220");
    expect(mockSlack.postedMessages.filter((message) => message.threadTs === "302.220" && message.text.includes("查看会话活动时间线"))).toHaveLength(1);
  }, 90_000);

  it("backs off missed-thread recovery after Slack rate limits replies", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-services-e2e-"));
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

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: {
        SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS: "100",
        SLACK_MISSED_THREAD_RECOVERY_INTERVAL_MS: "100",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-rate-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "401.220",
      ts: "401.221",
      text: "<@UBOT> rate limit session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "rate-limit session bootstrap");
    await waitForSessionIdle(tempRoot, "C123:401.220");

    const repliesBeforeFailure = mockSlack.conversationsRepliesCalls;
    mockSlack.conversationsRepliesFailure = {
      status: 429,
      retryAfterSec: 120,
    };
    mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "401.220",
      ts: "401.222",
      text: "missed while rate limited",
      user: "U234",
    });

    await waitFor(() => mockSlack.conversationsRepliesCalls > repliesBeforeFailure, "rate-limited replies call");
    const callsAfterFirstFailure = mockSlack.conversationsRepliesCalls;
    await delay(1_500);
    expect(mockSlack.conversationsRepliesCalls).toBe(callsAfterFirstFailure);
    expect(mockCodex.turnsStarted.slice(1).some((turn) => collectTextInput(turn.input).includes("missed while rate limited"))).toBe(false);
    expect(broker.logs.some((line) => line.includes("Paused Slack missed-message recovery after Slack rate limit"))).toBe(true);
  }, 90_000);

  it("does not block HTTP readiness on persisted active-turn reconciliation and clears stale snapshot turns", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-services-e2e-"));
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

    await mockSlack.sendEvent("evt-stale-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "501.220",
      ts: "501.221",
      text: "<@UBOT> stale turn session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "stale-turn session bootstrap");
    await waitForSessionIdle(tempRoot, "C123:501.220");
    const liveSession = await readSessionRecord(tempRoot, "C123:501.220");
    expect(liveSession.agentSessionId).toBeTruthy();
    await broker.stop();
    cleanups.pop();

    const writerStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const writerSessions = new SessionManager({
      stateStore: writerStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await writerSessions.load();
    await writerSessions.setActiveTurnId("C123", "501.220", "turn-missing-from-snapshot");
    const inflight = writerSessions.listInboundMessages({
      channelId: "C123",
      rootThreadTs: "501.220",
      status: "done",
    });
    if (inflight[0]) {
      await writerSessions.updateInboundMessagesForBatch("C123", "501.220", [inflight[0].messageTs], {
        status: "inflight",
        batchId: "turn-missing-from-snapshot",
      });
    }
    writerStore.close();

    mockCodex.delayThreadReadMs = 10_000;
    const startedAt = Date.now();
    const restarted = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => restarted.stop());
    expect(Date.now() - startedAt).toBeLessThan(6_000);

    mockCodex.delayThreadReadMs = 0;
    await waitFor(async () => {
      const session = await readSessionRecord(tempRoot, "C123:501.220");
      return !session.activeTurnId;
    }, "stale snapshot turn cleared after delayed thread/read");
  }, 90_000);
});
