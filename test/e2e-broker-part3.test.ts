import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";

import { StateStore } from "../src/store/state-store.js";

import type { PersistedInboundMessage } from "../src/types.js";

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
  readBackgroundJobs,
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

  it("periodically recovers missed thread replies without requiring a socket reconnect", async () => {
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
      extraEnv: {
        SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS: "100",
        SLACK_MISSED_THREAD_RECOVERY_INTERVAL_MS: "100",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-periodic-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "333.220",
      ts: "333.221",
      text: "<@UBOT> 开个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "session bootstrap turn");
    await waitForSessionIdle(tempRoot, "C123:333.220");

    mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "333.220",
      ts: "333.222",
      text: "漏掉的周期性恢复消息",
      user: "U234",
    });

    await waitFor(() => {
      const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
      return deliveredTexts.some((text) => text.includes("漏掉的周期性恢复消息"));
    }, "periodic recovered thread reply");

    const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
    const recoveredText = deliveredTexts.find((text) => text.includes("漏掉的周期性恢复消息")) ?? "";
    expect(recoveredText).toContain("recovered_message_batch_json");
    expect(recoveredText).toContain('"recovery_kind": "missed_thread_messages"');
    expect(recoveredText).toContain("漏掉的周期性恢复消息");
  }, 90_000);

  it("recovers persisted pending backlog on startup when a session has no active turn", async () => {
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
      thread_ts: "666.220",
      ts: "666.221",
      text: "<@UBOT> 开个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "session bootstrap turn");
    await waitForSessionIdle(tempRoot, "C123:666.220");
    await broker.stop();
    cleanups.pop();

    const stateStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await sessions.load();
    const session = sessions.getSession("C123", "666.220");
    expect(session).toBeTruthy();

    const now = new Date().toISOString();
    const pendingMessage: PersistedInboundMessage = {
      key: `C123:666.220:666.222`,
      sessionKey: "C123:666.220",
      channelId: "C123",
      rootThreadTs: "666.220",
      messageTs: "666.222",
      source: "thread_reply",
      userId: "U234",
      text: "BOOT_PENDING_RECOVERY",
      senderKind: "user",
      mentionedUserIds: [],
      images: [],
      slackMessage: {
        type: "message",
        user: "U234",
        ts: "666.222",
        text: "BOOT_PENDING_RECOVERY",
        thread_ts: "666.220",
        channel: "C123",
      },
      status: "pending",
      createdAt: now,
      updatedAt: now,
    };
    await sessions.upsertInboundMessage(pendingMessage);

    const restarted = await startBrokerProcess({
      port,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => restarted.stop());

    await waitFor(() => {
      const deliveredTexts = mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input));
      return deliveredTexts.some((text) => text.includes("BOOT_PENDING_RECOVERY"));
    }, "startup recovery of persisted pending backlog");
  }, 90_000);

  it("reclaims sessions older than the hard protection window even when they still look active", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const stateStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await sessions.load();

    const oldAt = new Date(Date.now() - 72 * 60 * 60 * 1000).toISOString();
    const protectedAt = new Date(Date.now() - 36 * 60 * 60 * 1000).toISOString();
    const recentStateWriteAt = new Date().toISOString();
    const staleSession = await sessions.ensureSession("CSTALE", "777.100");
    await stateStore.upsertSession({
      ...staleSession,
      activeTurnId: "turn-stale",
      activeTurnStartedAt: oldAt,
      createdAt: oldAt,
      updatedAt: recentStateWriteAt,
    });
    await fs.writeFile(path.join(staleSession.workspacePath, "marker.txt"), "stale active session");

    const staleJobDir = path.join(tempRoot, "jobs", "job-stale-active");
    await fs.mkdir(staleJobDir, { recursive: true });
    const staleJobScript = path.join(staleJobDir, "run.sh");
    await fs.writeFile(staleJobScript, "#!/bin/sh\nsleep 300\n");
    await fs.chmod(staleJobScript, 0o755);
    await sessions.upsertBackgroundJob({
      id: "job-stale-active",
      token: "token-stale-active",
      sessionKey: staleSession.key,
      channelId: staleSession.channelId,
      rootThreadTs: staleSession.rootThreadTs,
      kind: "watch_ci",
      shell: "sh",
      cwd: staleSession.workspacePath,
      scriptPath: staleJobScript,
      restartOnBoot: true,
      status: "running",
      createdAt: oldAt,
      updatedAt: oldAt,
      startedAt: oldAt,
      heartbeatAt: oldAt,
    });

    const protectedSession = await sessions.ensureSession("CPROTECTED", "888.100");
    await stateStore.upsertSession({
      ...protectedSession,
      createdAt: protectedAt,
      updatedAt: protectedAt,
    });
    await fs.writeFile(path.join(protectedSession.workspacePath, "marker.txt"), "protected job session");

    const protectedJobDir = path.join(tempRoot, "jobs", "job-protected");
    await fs.mkdir(protectedJobDir, { recursive: true });
    const protectedJobScript = path.join(protectedJobDir, "run.sh");
    await fs.writeFile(protectedJobScript, "#!/bin/sh\nsleep 300\n");
    await fs.chmod(protectedJobScript, 0o755);
    await sessions.upsertBackgroundJob({
      id: "job-protected",
      token: "token-protected",
      sessionKey: protectedSession.key,
      channelId: protectedSession.channelId,
      rootThreadTs: protectedSession.rootThreadTs,
      kind: "watch_ci",
      shell: "sh",
      cwd: protectedSession.workspacePath,
      scriptPath: protectedJobScript,
      restartOnBoot: true,
      status: "running",
      createdAt: protectedAt,
      updatedAt: protectedAt,
      startedAt: protectedAt,
      heartbeatAt: protectedAt,
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
        DISK_CLEANUP_DRY_RUN: "false",
        DISK_CLEANUP_MIN_FREE_BYTES: "1000000000000000",
        DISK_CLEANUP_TARGET_FREE_BYTES: "1000000000000000",
        DISK_CLEANUP_INACTIVE_SESSION_MS: String(DAY_MS),
        DISK_CLEANUP_JOB_PROTECTION_MS: String(2 * DAY_MS),
        DISK_CLEANUP_OLD_LOG_MS: String(DAY_MS),
      },
    });
    cleanups.push(() => broker.stop());

    await waitFor(async () => !(await pathExists(staleSession.workspacePath)), "stale active session cleanup");
    await stateStore.load();

    expect(sessions.getSessionByKey(staleSession.key)).toBeUndefined();
    expect(sessions.getBackgroundJob("job-stale-active")).toBeUndefined();
    expect(await pathExists(staleJobDir)).toBe(false);
    expect(sessions.getSessionByKey(protectedSession.key)).toBeDefined();
    expect(await pathExists(protectedSession.workspacePath)).toBe(true);
    expect(await pathExists(protectedJobDir)).toBe(true);
  }, 90_000);

  it("dry-runs expired session cache cleanup on startup without deleting artifacts", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const stateStore = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot: path.join(tempRoot, "sessions"),
    });
    await sessions.load();

    const oldAt = new Date(Date.now() - 8 * DAY_MS).toISOString();
    const staleSession = await sessions.ensureSession("CCACHE", "999.100");
    await stateStore.upsertSession({
      ...staleSession,
      createdAt: oldAt,
      updatedAt: oldAt,
    });
    const nodeModulesPath = path.join(staleSession.workspacePath, "web/node_modules/pkg/index.js");
    await fs.mkdir(path.dirname(nodeModulesPath), { recursive: true });
    await fs.writeFile(nodeModulesPath, "cached dependency");

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
        DISK_CLEANUP_DRY_RUN: "true",
        DISK_CLEANUP_MIN_FREE_BYTES: "0",
        DISK_CLEANUP_TARGET_FREE_BYTES: "0",
        DISK_CLEANUP_SESSION_CACHE_TTL_MS: String(DAY_MS),
      },
    });
    cleanups.push(() => broker.stop());

    await waitFor(() => {
      return broker.logs.some((line) => line.includes("Disk cleanup session cache candidate") && line.includes(staleSession.key) && line.includes('"dryRun":true'));
    }, "session cache dry-run candidate log");

    expect(await pathExists(path.join(staleSession.workspacePath, "web/node_modules"))).toBe(true);
  }, 90_000);

  it("injects background job events back into the same session", async () => {
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

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "333.220",
      ts: "333.221",
      text: "<@UBOT> 先起一个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "initial turn");
    await waitForSessionIdle(tempRoot, "C123:333.220");

    const registerResponse = await fetch(`${broker.baseUrl}/jobs/register`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        channel_id: "C123",
        thread_ts: "333.220",
        kind: "watch_ci",
        script: "#!/bin/sh\nsleep 30",
      }),
    });
    const registerBody = (await registerResponse.json()) as {
      job?: { id: string; token: string };
    };
    expect(registerResponse.ok).toBe(true);
    expect(registerBody.job?.id).toBeTruthy();
    expect(registerBody.job?.token).toBeTruthy();
    await waitFor(async () => {
      const jobs = await readBackgroundJobs(tempRoot, "C123:333.220");
      return jobs.some((job) => job.id === registerBody.job!.id && job.kind === "watch_ci");
    }, "background job persisted");
    await expect(readBackgroundJobs(tempRoot, "C123:333.220")).resolves.toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: registerBody.job!.id,
          kind: "watch_ci",
          sessionKey: "C123:333.220",
        }),
      ]),
    );
    expect(["registered", "running"]).toContain((await readBackgroundJobs(tempRoot, "C123:333.220")).find((job) => job.id === registerBody.job!.id)?.status);

    await postJson(`${broker.baseUrl}/notify`, {
      platform: "slack",
      conversationId: "C123",
      rootMessageId: "333.220",
      text: "CI turned green.",
      jobId: registerBody.job!.id,
    });

    await waitFor(() => {
      const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
      return deliveredTexts.some((text) => text.includes("background_job_event_json"));
    }, "background job event delivery");
    const deliveredTexts = [...mockCodex.turnsStarted.slice(1).map((turn) => collectTextInput(turn.input)), ...mockCodex.steers.map((steer) => collectTextInput(steer.input))];
    expect(deliveredTexts.some((text) => text.includes("background_job_event_json"))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes("A broker-managed background job reported a new asynchronous event"))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes("CI turned green."))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes('"job_kind": "watch_ci"'))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes('"event_kind": "notify"'))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes(`"job_id": "${registerBody.job!.id}"`))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes("Most watcher events do not need a Slack reply"))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes("zork-call chat post-state"))).toBe(true);
    expect(deliveredTexts.some((text) => text.includes("silent final state"))).toBe(true);
    expect(deliveredTexts.every((text) => !text.includes("background_job_event_json") || !text.includes('"sender":'))).toBe(true);
  }, 60_000);
});
