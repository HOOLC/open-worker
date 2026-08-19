import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

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

  it("does not wake a silent final turn or replay stale watcher events after completion", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    let turnCount = 0;
    let registeredJobId = "";
    let registeredJobToken = "";
    const firstTurnCompleted = createDeferred<void>();
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        turnCount += 1;
        if (turnCount === 1) {
          try {
            await waitForSessionActive(tempRoot, "C123:780.220");
            const registerResponse = await fetch(`${brokerBaseUrl}/jobs/register`, {
              method: "POST",
              headers: {
                "content-type": "application/json",
              },
              body: JSON.stringify({
                channel_id: "C123",
                thread_ts: "780.220",
                kind: "watch_ci",
                script: "#!/bin/sh\nsleep 30",
              }),
            });
            expect(registerResponse.ok).toBe(true);
            const registerJson = (await registerResponse.json()) as {
              job: { id: string; token: string };
            };
            registeredJobId = registerJson.job.id;
            registeredJobToken = registerJson.job.token;

            await postJson(`${brokerBaseUrl}/slack/post-state`, {
              channel_id: "C123",
              thread_ts: "780.220",
              kind: "final",
            });
            context.complete("");
            firstTurnCompleted.resolve();
          } catch (error) {
            firstTurnCompleted.reject(error);
            throw error;
          }
        }
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "780.220",
      ts: "780.221",
      text: "<@UBOT> 合并之后继续盯一下",
    });

    await firstTurnCompleted.promise;
    await waitForSessionIdle(tempRoot, "C123:780.220", 60_000);
    expect(turnCount).toBe(1);
    expect(registeredJobId).not.toBe("");
    expect(registeredJobToken).not.toBe("");

    const postedMessageCountAfterIdle = mockSlack.postedMessages.length;
    const eventResponse = await fetch(`${brokerBaseUrl}/notify`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "slack",
        conversationId: "C123",
        rootMessageId: "780.220",
        text: "PR merged on main",
        jobId: registeredJobId,
      }),
    });
    expect(eventResponse.ok).toBe(true);

    await delay(1_000);
    expect(turnCount).toBe(1);
    expect(mockSlack.postedMessages).toHaveLength(postedMessageCountAfterIdle);
  }, 90_000);

  it("does not recover the broker's own Slack messages as inbound work", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async () => {
        await postJson(`${brokerBaseUrl}/slack/post-message`, {
          channel_id: "C123",
          thread_ts: "555.220",
          text: "broker self reply",
        });
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "555.220",
      ts: "555.221",
      text: "<@UBOT> 触发一次回复",
    });
    await waitFor(() => mockSlack.postedMessages.some((message) => message.text === "broker self reply"), "bot reply", 30_000);
    await waitForSessionIdle(tempRoot, "C123:555.220", 30_000);
    const turnCountBeforeRestart = mockCodex.turnsStarted.length;

    await broker.stop();
    cleanups.pop();

    const restarted = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => restarted.stop());

    await delay(2_000);
    expect(mockCodex.turnsStarted).toHaveLength(turnCountBeforeRestart);
  }, 60_000);

  it("converts markdownish Slack posts to mrkdwn before delivery", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
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
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-format-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "777.220",
      ts: "777.221",
      text: "<@UBOT> 开个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "format session bootstrap turn");
    await waitForSessionIdle(tempRoot, "C123:777.220");

    await postJson(`${brokerBaseUrl}/slack/post-message`, {
      channel_id: "C123",
      thread_ts: "777.220",
      text: "## Summary\n- **done**\n- [docs](https://example.com)\n- `https://linear.app/settings/api`",
    });

    await waitFor(() => mockSlack.postedMessages.some((message) => message.threadTs === "777.220" && message.text.includes("*Summary*")), "converted slack markdown post");

    const posted = mockSlack.postedMessages.find((message) => message.threadTs === "777.220" && message.text.includes("*Summary*"));
    expect(posted?.text).toBe("*Summary*\n• *done*\n• <https://example.com|docs>\n• `https://linear.\u200Bapp/settings/api`");
  }, 60_000);

  it("chunks long Slack posts after markdownish conversion", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
    const brokerBaseUrl = `http://127.0.0.1:${brokerPort}`;
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
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-long-format-session", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "888.220",
      ts: "888.221",
      text: "<@UBOT> 开个 session",
    });
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "long format session bootstrap turn");
    await waitForSessionIdle(tempRoot, "C123:888.220");

    const markdownUnit = "1. **item**\n";
    const mrkdwnUnit = "1. *item*\n";
    const markdown = markdownUnit.repeat(400).trimEnd();

    await postJson(`${brokerBaseUrl}/slack/post-message`, {
      channel_id: "C123",
      thread_ts: "888.220",
      text: markdown,
    });

    await waitFor(() => mockSlack.postedMessages.filter((message) => message.threadTs === "888.220" && message.text.startsWith("1. *item*")).length >= 2, "multi-chunk converted slack post");

    const posted = mockSlack.postedMessages.filter((message) => message.threadTs === "888.220" && message.text.startsWith("1. *item*"));
    expect(posted).toHaveLength(2);
    expect(posted[0]?.text).toBe(mrkdwnUnit.repeat(350));
    expect(posted[1]?.text).toBe(mrkdwnUnit.repeat(49) + "1. *item*");
    expect(posted[0]?.text).not.toContain("**");
    expect(posted[1]?.text).not.toContain("**");
  }, 60_000);
});
