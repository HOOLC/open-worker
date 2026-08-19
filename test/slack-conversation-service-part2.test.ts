import { EventEmitter } from "node:events";

import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { describe, expect, it, vi } from "vitest";

import { AuthProfileUnavailableError } from "../src/services/agent-runtime/session-auth-profile-runtime.js";

import { SlackConversationService } from "../src/services/slack/slack-conversation-service.js";

import { SessionManager } from "../src/services/session-manager.js";

import { StateStore } from "../src/store/state-store.js";

import { TEST_CONFIG } from "./slack-conversation-service-helpers.js";

describe("SlackConversationService", () => {
  it("keeps Slack input pending and posts one session link when auth profile is unavailable", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-conversation-auth-block-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-conversation-auth-block-sessions-"));
    const stateStore = new StateStore(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot,
    });
    await sessions.load();
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setSessionAuthProfile(session.key, "empty-profile", {
      boundAt: "2026-05-09T00:00:00.000Z",
    });
    const agentRuntime = Object.assign(new EventEmitter(), {
      start: vi.fn(async () => undefined),
      stop: vi.fn(async () => undefined),
      setSlackBotIdentity: vi.fn(),
      getCapabilities: vi.fn(),
      ensureSession: vi.fn(async () => {
        throw new AuthProfileUnavailableError({
          sessionKey: session.key,
          profileName: "empty-profile",
          reason: "primary_quota_exhausted",
        });
      }),
      submitInput: vi.fn(),
      interrupt: vi.fn(),
      readSession: vi.fn(),
      readTurn: vi.fn(),
    });
    const postThreadMessage = vi.fn(async () => "333.444");

    const service = new SlackConversationService({
      config: TEST_CONFIG,
      sessions,
      agentRuntime: agentRuntime as never,
      slackApi: {
        postThreadMessage,
        setAssistantThreadStatus: vi.fn(),
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
        getUserIdentity: vi.fn(async () => null),
      } as never,
      selfMessageFilter: {
        rememberPostedMessageTs: vi.fn(),
        shouldIgnoreThreadMessage: vi.fn(() => false),
      } as never,
    });

    await service.acceptInboundMessage(session, {
      source: "thread_reply",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.223",
      userId: "U123",
      text: "继续",
    });

    await vi.waitFor(() => {
      expect(postThreadMessage).toHaveBeenCalledTimes(2);
    });
    const postCalls = postThreadMessage.mock.calls as unknown as Array<[string, string, string]>;
    expect(postCalls[0]).toEqual(["C123", "111.222", "<https://admin.example/admin/sessions/C123%3A111.222|查看会话活动时间线>"]);
    expect(postCalls[1]?.[2]).toContain("账号额度不可用");
    expect(postCalls[1]?.[2]).toContain("https://admin.example/admin/sessions/C123%3A111.222");

    expect(
      sessions.listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        status: "pending",
      }),
    ).toHaveLength(1);
    expect(sessions.getSessionByKey(session.key)).toMatchObject({
      authProfileName: "empty-profile",
      authBlockReason: "primary_quota_exhausted",
      authBlockedNoticePostedAt: expect.any(String),
    });

    await service.acceptInboundMessage(sessions.getSessionByKey(session.key)!, {
      source: "thread_reply",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.224",
      userId: "U123",
      text: "再发一条",
    });
    await vi.waitFor(() => {
      expect(agentRuntime.ensureSession).toHaveBeenCalledTimes(2);
    });
    expect(postThreadMessage).toHaveBeenCalledTimes(2);

    await service.stop();
    stateStore.close();
    await fs.rm(stateDir, { force: true, recursive: true });
    await fs.rm(sessionsRoot, { force: true, recursive: true });
  });

  it("keeps Slack input pending without asking for manual switch when auth profile status reads fail", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-conversation-auth-probe-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-conversation-auth-probe-sessions-"));
    const stateStore = new StateStore(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot,
    });
    await sessions.load();
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setSessionAuthProfile(session.key, "bound-profile", {
      boundAt: "2026-05-09T00:00:00.000Z",
    });
    const agentRuntime = Object.assign(new EventEmitter(), {
      start: vi.fn(async () => undefined),
      stop: vi.fn(async () => undefined),
      setSlackBotIdentity: vi.fn(),
      getCapabilities: vi.fn(),
      ensureSession: vi.fn(async () => {
        throw new AuthProfileUnavailableError({
          sessionKey: session.key,
          profileName: "bound-profile",
          reason: "account_probe_failed",
        });
      }),
      submitInput: vi.fn(),
      interrupt: vi.fn(),
      readSession: vi.fn(),
      readTurn: vi.fn(),
    });
    const postThreadMessage = vi.fn(async () => "333.444");

    const service = new SlackConversationService({
      config: TEST_CONFIG,
      sessions,
      agentRuntime: agentRuntime as never,
      slackApi: {
        postThreadMessage,
        setAssistantThreadStatus: vi.fn(),
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
        getUserIdentity: vi.fn(async () => null),
      } as never,
      selfMessageFilter: {
        rememberPostedMessageTs: vi.fn(),
        shouldIgnoreThreadMessage: vi.fn(() => false),
      } as never,
    });

    await service.acceptInboundMessage(session, {
      source: "thread_reply",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.223",
      userId: "U123",
      text: "继续",
    });

    await vi.waitFor(() => {
      expect(agentRuntime.ensureSession).toHaveBeenCalledTimes(1);
    });
    const postedTexts = (postThreadMessage.mock.calls as unknown as Array<[string, string, string]>).map((call) => String(call[2] ?? ""));
    expect(postedTexts.some((text) => text.includes("账号额度不可用"))).toBe(false);
    expect(postedTexts.some((text) => text.includes("手动切换账号"))).toBe(false);
    expect(
      sessions.listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        status: "pending",
      }),
    ).toHaveLength(1);
    expect(sessions.getSessionByKey(session.key)).toMatchObject({
      authProfileName: "bound-profile",
      authBlockedAt: undefined,
      authBlockReason: undefined,
      authBlockedNoticePostedAt: undefined,
    });

    await service.stop();
    stateStore.close();
    await fs.rm(stateDir, { force: true, recursive: true });
    await fs.rm(sessionsRoot, { force: true, recursive: true });
  });
});
