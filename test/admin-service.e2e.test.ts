import fs from "node:fs/promises";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";
import { StateStore } from "../src/store/state-store.js";
import { inboundMessage, readJson, requestJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

const hanging = () => new Promise<never>(() => {});

const quotaRuntime = {
  readAccountSummary: async () => ({
    account: {
      email: "quota@example.com",
      type: "chatgpt",
      planType: "team",
    },
    requiresOpenaiAuth: false,
  }),
  readAccountRateLimits: async () => ({
    rateLimits: {
      limitId: "codex",
      limitName: "Codex",
      primary: {
        usedPercent: 42,
        windowDurationMins: 300,
        resetsAt: 1_735_692_000,
      },
      secondary: {
        usedPercent: 7,
        windowDurationMins: 10_080,
        resetsAt: 1_735_999_999,
      },
      credits: {
        hasCredits: true,
        unlimited: false,
        balance: "18.75",
      },
      planType: "team",
    },
    rateLimitsByLimitId: {
      codex: {
        limitId: "codex",
        limitName: "Codex",
        primary: {
          usedPercent: 42,
          windowDurationMins: 300,
          resetsAt: 1_735_692_000,
        },
        secondary: {
          usedPercent: 7,
          windowDurationMins: 10_080,
          resetsAt: 1_735_999_999,
        },
        credits: {
          hasCredits: true,
          unlimited: false,
          balance: "18.75",
        },
        planType: "team",
      },
    },
  }),
};

describe("admin service e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("bounds slow runtime status probes so overview can still answer", { timeout: 15_000 }, async () => {
    const { baseUrl } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-runtime-timeout-",
      authProfiles: {
        listProfilesStatus: hanging,
      },
      runtime: {
        readAccountSummary: hanging,
        readAccountRateLimits: hanging,
      },
      deployment: {
        getStatus: hanging,
      },
    });

    const overview = await readJson(`${baseUrl}/admin/api/overview`);
    expect(overview).toMatchObject({
      ok: true,
      account: {
        ok: false,
        error: expect.stringContaining("account summary timed out"),
      },
      rateLimits: {
        ok: false,
        error: expect.stringContaining("account rate limits timed out"),
      },
      deployment: {
        ok: false,
        error: expect.stringContaining("deployment status timed out"),
      },
      authProfiles: {
        ok: false,
        error: expect.stringContaining("auth profiles timed out"),
        profiles: [],
      },
    });
  });

  it("keeps overview off inbound history and usage while sessions still report them", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-overview-inbound-",
    });
    await sessions.ensureSession("C123", "111.222", { initiatorUserId: "U0BOB" });
    await sessions.upsertInboundMessage(
      inboundMessage({
        status: "pending",
        text: "follow up",
      }),
    );

    const overview = await readJson(`${baseUrl}/admin/api/overview`);
    expect(overview).not.toHaveProperty("usage");
    expect(overview).toMatchObject({
      ok: true,
      state: {
        sessionCount: 1,
        openInboundCount: 0,
        openHumanInboundCount: 0,
        openSystemInboundCount: 0,
      },
      githubAccounts: {
        accounts: [
          {
            slackUserId: "U0BOB",
          },
        ],
      },
    });
    expect((overview.state as Record<string, unknown>).sessions).toBeUndefined();

    const sessionList = await readJson(`${baseUrl}/admin/api/sessions`);
    expect(sessionList).toMatchObject({
      ok: true,
      sessions: [
        {
          key: "C123:111.222",
          openInboundCount: 1,
          openHumanInboundCount: 1,
        },
      ],
    });
  });

  it("resolves Slack permalinks and keeps Feishu sessions off Slack thread links", async () => {
    const permalinkCalls: Array<Record<string, string>> = [];
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-thread-link-",
      slackConversations: {
        getConversationInfo: async () => null,
        getPermalink: async (options) => {
          permalinkCalls.push(options);
          return "https://workspace.slack.com/archives/C123/p111222?thread_ts=111.222&cid=C123";
        },
      },
    });
    await sessions.ensureSession("C123", "111.222");
    const feishuSession = await sessions.ensureChatSession(
      {
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
      },
      {
        conversationKind: "group",
        platformThreadId: "om_root",
      },
    );

    const slackLink = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/slack-thread-url`);
    expect(slackLink).toEqual({
      ok: true,
      sessionKey: "C123:111.222",
      url: "https://workspace.slack.com/archives/C123/p111222?thread_ts=111.222&cid=C123",
    });
    expect(permalinkCalls).toEqual([{ channelId: "C123", messageTs: "111.222" }]);

    const sessionList = await readJson(`${baseUrl}/admin/api/sessions`);
    expect((sessionList.sessions as Array<Record<string, unknown>>).find((session) => session.key === feishuSession.key)).toMatchObject({
      key: feishuSession.key,
      platform: "feishu",
      conversationId: "oc_group",
      conversationKind: "group",
      rootMessageId: "om_root",
      platformThreadId: "om_root",
      channelId: "oc_group",
      rootThreadTs: "om_root",
      threadUrl: null,
    });

    const feishuLink = await requestJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(feishuSession.key)}/slack-thread-url`);
    expect(feishuLink.status).toBe(404);
    expect(feishuLink.payload).toMatchObject({
      ok: false,
      error: "platform_permalink_unavailable",
      platform: "feishu",
      sessionKey: feishuSession.key,
    });
    expect(permalinkCalls).toEqual([{ channelId: "C123", messageTs: "111.222" }]);
  });

  it("exposes GitHub author mappings and OAuth bindings as unified GitHub accounts", async () => {
    const { baseUrl, sessions, githubAuthorMappings, githubPrIdentity } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-github-accounts-",
      extraEnv: {
        BROKER_DEFAULT_GITHUB_LOGIN: "legacy-bot",
        BROKER_DEFAULT_GITHUB_TOKEN: "legacy-token",
      },
      useRealGitHubServices: true,
      slackConversations: {
        getUserIdentity: async (userId: string) => {
          if (userId !== "U0BOB") return null;
          return {
            userId,
            mention: `<@${userId}>`,
            username: "bob",
            displayName: "Bob Slack",
            realName: "Bob Example",
            email: "bob@example.com",
          };
        },
      },
    });
    if (!githubAuthorMappings || !githubPrIdentity) {
      throw new Error("expected real GitHub services");
    }

    await githubAuthorMappings.upsertManualMapping({
      slackUserId: "U_ALICE",
      githubAuthor: "Alice Example <alice@example.com>",
      slackIdentity: {
        userId: "U_ALICE",
        mention: "<@U_ALICE>",
        displayName: "Alice",
        email: "alice@example.com",
      },
    });
    await githubPrIdentity.upsertBinding({
      slackUserId: "U_ALICE",
      githubLogin: "alice-gh",
      githubUserId: 101,
      token: "alice-token",
      scopes: ["repo", "read:user", "user:email"],
      githubEmail: "alice@github.example",
      githubName: "Alice GitHub",
    });
    await githubPrIdentity.setDefaultBinding("U_ALICE");
    await sessions.ensureSession("C123", "111.222", { initiatorUserId: "U0BOB" });
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "m2",
        messageTs: "111.333",
        userId: "U_CAROL",
        text: "please review this too",
        senderKind: "user",
        senderUsername: "carol",
      }),
    );
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "m3",
        messageTs: "111.444",
        userId: "U_BOT",
        text: "bot message",
        senderKind: "bot",
      }),
    );
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "m4",
        messageTs: "111.555",
        userId: "username:legacy-bot",
        text: "legacy sender",
        senderKind: "user",
      }),
    );

    const overview = await readJson(`${baseUrl}/admin/api/overview`);
    expect(overview.githubAccounts).toMatchObject({
      count: 2,
      defaultPrAccount: {
        available: true,
        source: "bound",
        slackUserId: "U_ALICE",
        githubLogin: "alice-gh",
      },
      accounts: [
        {
          slackUserId: "U_ALICE",
          isDefaultPrAccount: true,
          slackIdentity: {
            userId: "U_ALICE",
            mention: "<@U_ALICE>",
          },
          prBinding: {
            state: "bound",
            githubLogin: "alice-gh",
            githubUserId: 101,
            githubEmail: "alice@github.example",
            githubName: "Alice GitHub",
            scopes: ["repo", "read:user", "user:email"],
          },
        },
        {
          slackUserId: "U0BOB",
          slackIdentity: {
            userId: "U0BOB",
            mention: "<@U0BOB>",
            username: "bob",
            displayName: "Bob Slack",
            realName: "Bob Example",
            email: "bob@example.com",
          },
          prBinding: {
            state: "unbound",
          },
        },
      ],
    });
    expect(JSON.stringify(overview.githubAccounts)).not.toContain("U_CAROL");
    expect(JSON.stringify(overview.githubAccounts)).not.toContain("U_BOT");
    expect(JSON.stringify(overview.githubAccounts)).not.toContain("username:legacy-bot");
    expect(JSON.stringify(overview.githubAccounts)).not.toContain("githubAuthor");
    expect(JSON.stringify(overview.githubAccounts)).not.toContain("Alice Example <alice@example.com>");
  });

  it("includes account rate limits, reads recent broker logs from a bounded tail instead of decoding whole files, and reports platform health", async () => {
    const { baseUrl, config } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-status-",
      extraEnv: {
        FEISHU_ENABLED: "true",
        FEISHU_APP_ID: "cli_test",
        FEISHU_APP_SECRET: "secret-test",
        FEISHU_BOT_OPEN_ID: "ou_test",
        FEISHU_GROUP_MESSAGE_MODE: "all",
        FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED: "true",
      },
      runtime: quotaRuntime,
    });
    await fs.mkdir(path.join(config.logDir, "broker"), { recursive: true });
    await fs.writeFile(path.join(config.logDir, "broker", "2026-03-19-00.jsonl"), `${"x".repeat(1024 * 1024)}\n{"message":"tail-1"}\n{"message":"tail-2"}\n`, "utf8");
    await fs.writeFile(
      path.join(config.logDir, "broker", "2026-03-19-01.jsonl"),
      [JSON.stringify({ ts: "2026-03-19T00:00:01.000Z", message: "chat.platform.ready", meta: { platform: "slack", source: "socket_mode" } }), JSON.stringify({ ts: "2026-03-19T00:00:02.000Z", message: "chat.platform.ready", meta: { platform: "feishu", source: "long_connection", groupMessageMode: "all" } }), ""].join(
        "\n",
      ),
      "utf8",
    );

    const status = await readJson(`${baseUrl}/admin/api/status`);
    expect((status.state as { recentBrokerLogs: unknown[] }).recentBrokerLogs).toEqual([
      { message: "tail-1" },
      { message: "tail-2" },
      { ts: "2026-03-19T00:00:01.000Z", message: "chat.platform.ready", meta: { platform: "slack", source: "socket_mode" } },
      { ts: "2026-03-19T00:00:02.000Z", message: "chat.platform.ready", meta: { platform: "feishu", source: "long_connection", groupMessageMode: "all" } },
    ]);
    expect(status).toMatchObject({
      account: {
        ok: true,
        account: {
          email: "quota@example.com",
          type: "chatgpt",
          planType: "team",
        },
      },
      rateLimits: {
        ok: true,
        rateLimits: {
          limitId: "codex",
          planType: "team",
          credits: {
            balance: "18.75",
            hasCredits: true,
            unlimited: false,
          },
        },
        rateLimitsByLimitId: {
          codex: {
            limitName: "Codex",
          },
        },
      },
      platforms: {
        slack: {
          platform: "slack",
          enabled: true,
          state: "ready",
          connection: {
            mode: "socket_mode",
            connected: true,
            lastConnectedAt: "2026-03-19T00:00:01.000Z",
          },
        },
        feishu: {
          platform: "feishu",
          enabled: true,
          state: "ready",
          groupMessageMode: "all",
          allMessageDeliveryVerified: true,
          connection: {
            mode: "long_connection",
            connected: true,
            lastConnectedAt: "2026-03-19T00:00:02.000Z",
          },
          permissions: expect.arrayContaining([expect.objectContaining({ name: "bot_identity", status: "configured" }), expect.objectContaining({ name: "im:message.group_msg", status: "verified" }), expect.objectContaining({ name: "im:message:send_as_bot", status: "configured" })]),
        },
      },
    });

    const logs = await readJson(`${baseUrl}/admin/api/logs?limit=3`);
    expect(logs).toMatchObject({
      ok: true,
      logs: [{ message: "tail-2" }, { ts: "2026-03-19T00:00:01.000Z", message: "chat.platform.ready" }, { ts: "2026-03-19T00:00:02.000Z", message: "chat.platform.ready" }],
    });
  });

  it("reloads persisted session state and resolves shared channel labels", async () => {
    const lookupCalls: string[] = [];
    const { baseUrl, config, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-state-refresh-",
      slackConversations: {
        getConversationInfo: async (channelId) => {
          lookupCalls.push(channelId);
          return {
            channelId,
            name: "ops",
            channelType: "channel",
          };
        },
      },
    });

    let status = await readJson(`${baseUrl}/admin/api/status`);
    expect(status).toMatchObject({
      state: {
        sessionCount: 0,
        activeCount: 0,
      },
    });

    const writerStore = new StateStore(config.stateDir, config.sessionsRoot);
    const writerSessions = new SessionManager({
      stateStore: writerStore,
      sessionsRoot: config.sessionsRoot,
    });
    await writerSessions.load();
    cleanups.push(async () => {
      writerStore.close();
    });
    await writerSessions.ensureSession("C123", "111.222", {
      channelName: "deep-review",
      channelType: "channel",
    });
    await writerSessions.setActiveTurnId("C123", "111.222", "turn-1");
    await writerSessions.upsertInboundMessage({
      key: "C123:111.222:111.223",
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.223",
      source: "thread_reply",
      userId: "U123",
      text: "<@U234> follow up",
      senderUsername: "starter",
      mentionedUserIds: ["U234"],
      mentionedUsers: [
        {
          userId: "U234",
          mention: "<@U234>",
          username: "mock-user-234",
          displayName: "Mock Display 234",
          realName: "Mock User 234",
        },
      ],
      status: "pending",
      createdAt: "2026-03-19T00:00:00.000Z",
      updatedAt: "2026-03-19T00:00:00.000Z",
    });

    status = await readJson(`${baseUrl}/admin/api/status`);
    expect(status).toMatchObject({
      state: {
        sessionCount: 1,
        activeCount: 1,
        openInboundCount: 1,
        openHumanInboundCount: 1,
        openSystemInboundCount: 0,
        sessions: [
          {
            channelId: "C123",
            channelName: "deep-review",
            channelType: "channel",
            channelLabel: "#deep-review",
            firstUserMessage: {
              userId: "U123",
              senderUsername: "starter",
              slackIdentity: {
                userId: "U123",
                username: "starter",
              },
              textPreview: "@Mock Display 234 follow up",
            },
            lastUserMessage: {
              userId: "U123",
              senderUsername: "starter",
              slackIdentity: {
                userId: "U123",
                username: "starter",
              },
              textPreview: "@Mock Display 234 follow up",
            },
          },
        ],
      },
    });

    await writerSessions.ensureSession("C123", "222.333");
    status = await readJson(`${baseUrl}/admin/api/status`);
    const legacySession = ((status as Record<string, any>).state.sessions as Record<string, any>[]).find((session) => session.key === "C123:222.333");
    expect(legacySession).toMatchObject({
      channelId: "C123",
      channelName: null,
      channelLabel: "#deep-review",
    });

    await sessions.ensureSession("C456", "333.444");
    const timeline = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C456:333.444")}/timeline`);
    expect((timeline as Record<string, any>).session).toMatchObject({
      key: "C456:333.444",
      channelName: null,
      channelLabel: "#ops",
    });
    const summaries = await readJson(`${baseUrl}/admin/api/sessions`);
    expect((summaries as Record<string, any>).sessions).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          key: "C456:333.444",
          channelLabel: "#ops",
        }),
      ]),
    );
    expect(lookupCalls).toEqual(["C456"]);
  });

  it("orders sessions by real activity and splits human vs system inbound counts", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-service-activity-",
    });
    await sessions.ensureSession("C123", "111.222");
    await sessions.ensureSession("C123", "222.333");
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "C123:111.222:111.223",
        sessionKey: "C123:111.222",
        rootThreadTs: "111.222",
        messageTs: "111.223",
        text: "old activity",
        status: "done",
        createdAt: "2026-03-19T00:00:00.000Z",
        updatedAt: "2026-03-19T00:00:00.000Z",
      }),
    );
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "C123:222.333:222.334",
        sessionKey: "C123:222.333",
        rootThreadTs: "222.333",
        messageTs: "222.334",
        text: "new activity",
        status: "done",
        createdAt: "2026-03-20T00:00:00.000Z",
        updatedAt: "2026-03-20T00:00:00.000Z",
      }),
    );
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "C123:111.222:111.224",
        sessionKey: "C123:111.222",
        rootThreadTs: "111.222",
        messageTs: "111.224",
        source: "background_job_event",
        userId: "U0ALY77RMJL",
        text: "job update",
        status: "pending",
        createdAt: "2026-03-19T00:00:01.000Z",
        updatedAt: "2026-03-19T00:00:01.000Z",
      }),
    );
    await sessions.upsertInboundMessage(
      inboundMessage({
        key: "C123:111.222:111.225",
        sessionKey: "C123:111.222",
        rootThreadTs: "111.222",
        messageTs: "111.225",
        text: "follow up",
        status: "pending",
        createdAt: "2026-03-19T00:00:02.000Z",
        updatedAt: "2026-03-19T00:00:02.000Z",
      }),
    );

    const status = await readJson(`${baseUrl}/admin/api/status`);
    const summaries = (status as Record<string, any>).state.sessions as Record<string, any>[];
    expect(summaries.map((session) => session.key).slice(0, 2)).toEqual(["C123:222.333", "C123:111.222"]);
    expect(summaries.find((session) => session.key === "C123:111.222")).toMatchObject({
      updatedAt: expect.any(String),
      lastActivityAt: "2026-03-19T00:00:02.000Z",
      openInboundCount: 2,
      openHumanInboundCount: 1,
      openSystemInboundCount: 1,
    });
    expect(status).toMatchObject({
      state: {
        openInboundCount: 2,
        openHumanInboundCount: 1,
        openSystemInboundCount: 1,
      },
    });
  });
});
