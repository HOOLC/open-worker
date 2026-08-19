import { describe, expect, it } from "vitest";

import { activeBackgroundJobCount, renderSessionMeta, sessionActivityAt, sessionInboundIndicator, sessionQueueState, sessionWorkIndicator, shouldShowSessionState } from "../src/admin-ui/session-row-display.js";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, isTimelineEventVisible } from "../src/admin-ui/timeline-display.js";

describe("admin session row display", () => {
  it("keeps common session list metadata out of the row", () => {
    const authProfiles = new Map<string, Record<string, any>>([
      [
        "profile-a",
        {
          name: "profile-a",
          account: {
            ok: true,
            account: {
              email: "operator@example.com",
              planType: "prolite",
            },
          },
          rateLimits: {
            ok: true,
            rateLimits: {
              primary: {
                usedPercent: 4,
              },
              secondary: {
                usedPercent: 36,
              },
            },
          },
        },
      ],
    ]);
    const meta = renderSessionMeta(
      {
        key: "C123:111.222",
        channelId: "C123",
        channelLabel: "C123",
        authProfileName: "profile-a",
        firstUserMessage: {
          textPreview: "@codex-3720 你好",
        },
        lastUserMessage: {
          textPreview: "后面 GPT 改了点",
        },
        usage: {
          turnCount: 3,
          totalTokens: 5120,
        },
        backgroundJobCount: 0,
        updatedAt: new Date().toISOString(),
      },
      authProfiles,
      new Map([["C123", "#ops"]]),
    );
    const labels = meta.map((item) => item.label);

    expect(labels).toEqual(["#ops", "7d 64% / 0.64", "Token 5.1K"]);
    expect(labels.join(" ")).not.toContain("Pro Lite");
    expect(shouldShowSessionState({ rank: 10 })).toBe(false);
  });

  it("marks Feishu sessions as a distinct dashboard platform", () => {
    const meta = renderSessionMeta(
      {
        key: "feishu:b2NfZ3JvdXA:b21fcm9vdA",
        platform: "feishu",
        channelId: "oc_group",
        channelLabel: "oc_group",
        conversationKind: "group",
        rootThreadTs: "om_root",
        usage: {
          turnCount: 1,
          totalTokens: 900,
        },
      },
      new Map(),
    );

    expect(meta.map((item) => item.label)).toEqual(["飞书", "oc_group", "Token 900"]);
    expect(meta.at(0)).toMatchObject({
      key: "platform",
      tone: "good",
      title: "Feishu session",
    });
  });

  it("only shows active job count and keeps distinct states visible", () => {
    const meta = renderSessionMeta(
      {
        key: "C123:111.222",
        channelId: "C123",
        channelName: "deep-review",
        firstUserMessage: {
          textPreview: "看一下",
        },
        lastUserMessage: {
          textPreview: "看一下",
        },
        openHumanInboundCount: 1,
        openInboundCount: 1,
        usage: {
          turnCount: 1,
          totalTokens: 1725,
        },
        backgroundJobCount: 2,
        runningBackgroundJobCount: 1,
        updatedAt: new Date().toISOString(),
      },
      new Map(),
    );
    const labels = meta.map((item) => item.label);

    expect(labels).toContain("#deep-review");
    expect(labels).toContain("未读/待处理 1 条");
    expect(labels).toContain("Jobs 1");
    expect(labels).not.toContain("Jobs 2");
    expect(shouldShowSessionState({ rank: 50 })).toBe(true);
  });

  it("does not promote historical jobs to current session state", () => {
    const session = {
      failedBackgroundJobCount: 2,
      backgroundJobCount: 2,
      runningBackgroundJobCount: 2,
      backgroundJobs: [
        {
          id: "completed-job",
          kind: "watch_ci",
          status: "completed",
        },
      ],
      failedBackgroundJobs: [
        {
          id: "failed-job",
          kind: "watch_ci",
          status: "failed",
          error: "PR #349 failed: CI Check failed",
          updatedAt: "2026-05-13T05:47:45.765Z",
        },
      ],
    };
    const state = sessionQueueState(session);
    const meta = renderSessionMeta(session, new Map());

    expect(activeBackgroundJobCount(session)).toBe(0);
    expect(state).toMatchObject({
      label: "空闲",
      tone: "",
      rank: 0,
    });
    expect(shouldShowSessionState(state)).toBe(false);
    expect(meta.find((item) => item.key === "jobs")).toBeUndefined();
    expect(meta.map((item) => item.label)).not.toContain("Jobs 2");
    expect(meta.map((item) => item.label)).not.toContain("失败 2");
  });

  it("treats registered and running jobs as active work", () => {
    const session = {
      backgroundJobCount: 3,
      runningBackgroundJobCount: 0,
      backgroundJobs: [
        {
          id: "registered-job",
          kind: "watch_ci",
          status: "registered",
        },
        {
          id: "completed-job",
          kind: "watch_ci",
          status: "completed",
        },
      ],
    };
    const state = sessionQueueState(session);
    const meta = renderSessionMeta(session, new Map());

    expect(activeBackgroundJobCount(session)).toBe(1);
    expect(state).toMatchObject({
      label: "后台任务",
      tone: "good",
      detail: "1 个运行任务",
    });
    expect(meta.find((item) => item.key === "jobs")).toMatchObject({
      label: "Jobs 1",
      tone: "good",
    });
  });

  it("uses plain pending labels for human Slack input", () => {
    const state = sessionQueueState({
      openInboundCount: 1,
      openHumanInboundCount: 1,
    });

    expect(state).toMatchObject({
      label: "待处理",
      tone: "warn",
      detail: "1 条未处理用户消息",
    });
    expect(state.label).not.toContain("待人处理");
  });

  it("exposes broker open-inbound as the product read/unread signal", () => {
    const slack = sessionInboundIndicator({
      platform: "slack",
      openInboundCount: 2,
      openHumanInboundCount: 1,
      openSystemInboundCount: 1,
    });
    const feishu = sessionInboundIndicator({
      platform: "feishu",
      openInboundCount: 1,
      openHumanInboundCount: 1,
      openSystemInboundCount: 0,
    });

    expect(slack).toMatchObject({
      value: "2 条",
      detail: "用户 1 / 系统 1",
      tone: "warn",
      title: expect.stringContaining("Slack thread/channel broker open-inbound state"),
    });
    expect(feishu).toMatchObject({
      value: "1 条",
      detail: "用户 1 / 系统 0",
      tone: "warn",
      title: expect.stringContaining("Feishu group broker open-inbound state"),
    });
    expect(slack?.title).toContain("not a native client unread counter");
  });

  it("names platform-adapted work indicators for active Slack and Feishu turns", () => {
    const slack = {
      platform: "slack",
      activeTurnId: "turn-slack-1234567890",
    };
    const feishu = {
      platform: "feishu",
      activeTurnId: "turn-feishu-1234567890",
    };

    expect(sessionQueueState(slack)).toMatchObject({
      label: "处理中",
      tone: "good",
      detail: "assistant status / eyes fallback · turn-slack-12...7890",
    });
    expect(sessionWorkIndicator(slack)).toMatchObject({
      value: "Slack 状态",
      title: expect.stringContaining("assistant.threads.setStatus"),
    });
    expect(sessionQueueState(feishu)).toMatchObject({
      label: "处理中",
      tone: "good",
      detail: "状态卡 / 消息更新 · turn-feishu-1...7890",
    });
    expect(sessionWorkIndicator(feishu)).toMatchObject({
      value: "飞书状态卡",
      title: expect.stringContaining("visible state card"),
    });
  });

  it("does not show account-switch state when the bound profile quota has recovered", () => {
    const profile = {
      name: "profile-a",
      account: {
        ok: true,
      },
      rateLimits: {
        ok: true,
        rateLimits: {
          primary: {
            usedPercent: 10,
            resetsAt: 1_779_000_000,
          },
          secondary: {
            usedPercent: 20,
            resetsAt: 1_780_000_000,
          },
        },
      },
    };
    const session = {
      key: "C123:111.222",
      channelId: "C123",
      authProfileName: "profile-a",
      authBlockedAt: "2026-05-09T01:00:00.000Z",
      authBlockReason: "primary_quota_exhausted",
      openInboundCount: 1,
      openSystemInboundCount: 1,
    };

    const state = sessionQueueState(session, profile);
    const meta = renderSessionMeta(session, new Map([["profile-a", profile]]));

    expect(state.label).toBe("待处理");
    expect(state.detail).toBe("1 条未处理系统消息");
    expect(meta.map((item) => item.key)).not.toContain("auth-blocked");
    expect(meta.map((item) => item.label)).not.toContain("账号待切换");
  });

  it("does not show account-switch state for probe-failure-only auth blocks", () => {
    const profile = {
      name: "profile-a",
      account: {
        ok: false,
        error: "account status read failed",
      },
      rateLimits: {
        ok: false,
        error: "rate limits read failed",
      },
    };
    const session = {
      key: "C123:111.222",
      channelId: "C123",
      authProfileName: "profile-a",
      authBlockedAt: "2026-05-09T01:00:00.000Z",
      authBlockReason: "account_probe_failed",
      openInboundCount: 1,
      openHumanInboundCount: 1,
    };

    const state = sessionQueueState(session, profile);
    const meta = renderSessionMeta(session, new Map([["profile-a", profile]]));

    expect(state.label).toBe("待处理");
    expect(state.detail).toBe("1 条未处理用户消息");
    expect(meta.map((item) => item.key)).not.toContain("auth-blocked");
    expect(meta.map((item) => item.label)).not.toContain("账号待切换");
  });

  it("uses semantic session activity time instead of metadata updatedAt", () => {
    expect(
      sessionActivityAt({
        key: "C123:111.222",
        createdAt: "2026-03-18T00:00:00.000Z",
        updatedAt: "2026-05-10T00:00:00.000Z",
        lastActivityAt: "2026-03-19T00:00:00.000Z",
        usage: {
          lastTurnAt: "2026-03-19T00:00:00.000Z",
        },
      }),
    ).toBe("2026-03-19T00:00:00.000Z");
  });
});
