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

  it("shows Slack assistant thread status while a turn is running and clears it after replying", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    let brokerBaseUrl = "";
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
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        await waitFor(() => {
          return mockSlack.assistantStatusUpdates.some((update) => update.status === "Thinking...");
        }, "assistant thinking status");

        const response = await fetch(`${brokerBaseUrl}/slack/post-message`, {
          method: "POST",
          headers: {
            "content-type": "application/x-www-form-urlencoded; charset=utf-8",
          },
          body: new URLSearchParams({
            channel_id: "C123",
            thread_ts: "110.220",
            text: "STATUS_REPLY_OK",
            kind: "final",
          }).toString(),
        });
        if (!response.ok) {
          throw new Error(`Failed to post broker Slack reply: ${response.status}`);
        }

        context.complete("");
      },
    });
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
    brokerBaseUrl = broker.baseUrl;
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-status-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "110.220",
      ts: "110.221",
      text: "<@UBOT> status test",
    });

    await waitFor(() => {
      return mockSlack.assistantStatusUpdates.some((update) => update.status === "Thinking...");
    }, "assistant thinking status call");
    await waitFor(() => {
      return mockSlack.postedMessages.some((message) => message.text === "STATUS_REPLY_OK");
    }, "broker-posted Slack reply");
    await waitFor(() => {
      return mockSlack.assistantStatusUpdates.some((update) => update.status === "");
    }, "assistant status clear");

    expect(mockSlack.assistantStatusUpdates).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          channel: "C123",
          threadTs: "110.220",
          status: "Thinking...",
          loadingMessages: "Thinking...",
        }),
        expect.objectContaining({
          channel: "C123",
          threadTs: "110.220",
          status: "",
        }),
      ]),
    );
  }, 90_000);

  it("posts a session permalink when the bot starts processing a Slack thread", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
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
      extraEnv: {
        ADMIN_BASE_URL: "https://admin.example.test",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session-link", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "991.220",
      ts: "991.221",
      text: "<@UBOT> trace link",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "first turn start");
    await waitFor(() => mockSlack.postedMessages.some((message) => message.threadTs === "991.220" && message.text.includes("查看会话活动时间线") && message.text.includes("https://admin.example.test/admin/sessions/C123%3A991.220")), "session permalink startup message");
    await waitForSessionIdle(tempRoot, "C123:991.220");

    const postedLinks = mockSlack.postedMessages.filter((message) => message.threadTs === "991.220" && message.text.includes("/admin/sessions/C123%3A991.220"));
    expect(postedLinks).toHaveLength(1);
    expect(postedLinks[0]!.text).toContain("<https://admin.example.test/admin/sessions/C123%3A991.220|查看会话活动时间线>");
    expect(postedLinks[0]!.text).toContain("<https://admin.example.test/admin/sessions/C123%3A991.220/github/bind|绑定 GitHub>");
    expect(postedLinks[0]!.text).not.toContain("已开始处理");
    expect(postedLinks[0]!.text).not.toContain("Bot");
    const startupMessages = mockSlack.postedMessages.filter((message) => message.threadTs === "991.220");
    expect(startupMessages).toEqual([postedLinks[0]]);
    await expect(readSessionRecord(tempRoot, "C123:991.220")).resolves.toMatchObject({
      sessionPageLinkPostedAt: expect.any(String),
    });
  }, 60_000);

  it("records the session starter and warns about default GitHub PR fallback in the session link", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
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
      extraEnv: {
        ADMIN_BASE_URL: "https://admin.example.test",
        BROKER_DEFAULT_GITHUB_LOGIN: "default-bot",
        BROKER_DEFAULT_GITHUB_TOKEN: "default-token",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session-github-starter", {
      type: "app_mention",
      user: "U_STARTER",
      channel: "C123",
      thread_ts: "992.220",
      ts: "992.221",
      text: "<@UBOT> open a PR",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "first turn start");
    await waitFor(
      () =>
        mockSlack.postedMessages.some(
          (message) => message.threadTs === "992.220" && message.text.includes("查看会话活动时间线") && message.text.includes("当前发起人还没有绑定 GitHub 账号") && message.text.includes("默认账号 default-bot") && message.text.includes("https://admin.example.test/admin/sessions/C123%3A992.220/github/bind"),
        ),
      "session permalink GitHub fallback message",
    );
    await waitForSessionIdle(tempRoot, "C123:992.220");
    await expect(readSessionRecord(tempRoot, "C123:992.220")).resolves.toMatchObject({
      initiatorUserId: "U_STARTER",
      initiatorMessageTs: "992.221",
      initiatorCapturedAt: expect.any(String),
    });

    await mockSlack.sendEvent("evt-session-github-later", {
      type: "message",
      user: "U_LATER",
      channel: "C123",
      thread_ts: "992.220",
      ts: "992.222",
      text: "follow-up",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 2, "second turn start");
    await waitForSessionIdle(tempRoot, "C123:992.220");
    await expect(readSessionRecord(tempRoot, "C123:992.220")).resolves.toMatchObject({
      initiatorUserId: "U_STARTER",
      initiatorMessageTs: "992.221",
    });
  }, 60_000);

  it("asks the session starter to bind GitHub even when no default GitHub account is configured", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const brokerPort = await getFreePort();
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
      extraEnv: {
        ADMIN_BASE_URL: "https://admin.example.test",
      },
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-session-github-no-default", {
      type: "app_mention",
      user: "U_STARTER",
      channel: "C123",
      thread_ts: "993.220",
      ts: "993.221",
      text: "<@UBOT> open a PR",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "first turn start");
    await waitFor(
      () =>
        mockSlack.postedMessages.some(
          (message) => message.threadTs === "993.220" && message.text.includes("查看会话活动时间线") && message.text.includes("当前发起人还没有绑定 GitHub 账号") && message.text.includes("当前没有默认 GitHub PR 账号") && message.text.includes("https://admin.example.test/admin/sessions/C123%3A993.220/github/bind"),
        ),
      "session permalink GitHub bind message without default",
    );
    await waitForSessionIdle(tempRoot, "C123:993.220");
    await expect(readSessionRecord(tempRoot, "C123:993.220")).resolves.toMatchObject({
      initiatorUserId: "U_STARTER",
      initiatorMessageTs: "993.221",
    });
  }, 60_000);

  it("falls back to an eyes reaction when Slack assistant status is unavailable", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-broker-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    let brokerBaseUrl = "";
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
      assistantStatusError: "unknown_method",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        await waitFor(() => {
          return mockSlack.reactionOperations.some((operation) => operation.action === "add" && operation.channel === "C123" && operation.timestamp === "120.220" && operation.name === "eyes");
        }, "assistant fallback reaction add");

        const response = await fetch(`${brokerBaseUrl}/slack/post-message`, {
          method: "POST",
          headers: {
            "content-type": "application/x-www-form-urlencoded; charset=utf-8",
          },
          body: new URLSearchParams({
            channel_id: "C123",
            thread_ts: "120.220",
            text: "FALLBACK_REPLY_OK",
            kind: "final",
          }).toString(),
        });
        if (!response.ok) {
          throw new Error(`Failed to post broker Slack reply: ${response.status}`);
        }

        context.complete("");
      },
    });
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
    brokerBaseUrl = broker.baseUrl;
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-fallback-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "120.220",
      ts: "120.221",
      text: "<@UBOT> fallback test",
    });

    await waitFor(() => {
      return mockSlack.reactionOperations.some((operation) => operation.action === "add" && operation.channel === "C123" && operation.timestamp === "120.220" && operation.name === "eyes");
    }, "assistant fallback reaction add");
    await waitFor(() => {
      return mockSlack.postedMessages.some((message) => message.text === "FALLBACK_REPLY_OK");
    }, "fallback broker-posted Slack reply");
    await waitFor(() => {
      return mockSlack.reactionOperations.some((operation) => operation.action === "remove" && operation.channel === "C123" && operation.timestamp === "120.220" && operation.name === "eyes");
    }, "assistant fallback reaction clear");

    expect(mockSlack.assistantStatusUpdates).toHaveLength(0);
  }, 90_000);
});
