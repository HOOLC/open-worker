import { EventEmitter } from "node:events";

import { describe, expect, it, vi } from "vitest";

import { SlackConversationService } from "../src/services/slack/slack-conversation-service.js";
import { TEST_SESSION, TEST_CONFIG } from "./slack-conversation-service-helpers.js";
import { readCompanionSource } from "./source-helpers.js";

describe("SlackConversationService", () => {
  it("coalesces live active-turn reconcile timer ticks instead of overlapping passes", async () => {
    const source = await readCompanionSource(new URL("../src/services/slack/slack-conversation-service.ts", import.meta.url));

    expect(source).toContain("privateActiveTurnReconcilePromise");
    expect(source).toContain("privateRunLiveActiveTurnReconcileOnce");
    expect(source).toMatch(/if \(\s*this\.privateActiveTurnReconcilePromise\s*\)/);
  });

  it("removes the agent runtime event listener on stop", async () => {
    const agentRuntime = new EventEmitter();
    const getSessionByKey = vi.fn(() => TEST_SESSION);
    const setAssistantThreadStatus = vi.fn(async () => undefined);

    const service = new SlackConversationService({
      config: TEST_CONFIG,
      sessions: {
        getSessionByKey,
        upsertAgentTraceEvent: vi.fn(),
      } as never,
      agentRuntime: agentRuntime as never,
      slackApi: {
        setAssistantThreadStatus,
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
      } as never,
      selfMessageFilter: {} as never,
    });

    expect(agentRuntime.listenerCount("event")).toBe(1);

    agentRuntime.emit("event", {
      type: "agent.tool.started",
      agentSessionId: TEST_SESSION.agentSessionId,
      brokerSessionKey: TEST_SESSION.key,
      turnId: TEST_SESSION.activeTurnId,
      callId: "call-1",
      name: "exec_command",
      at: new Date().toISOString(),
    });

    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenCalledTimes(1);
    });

    await service.stop();

    expect(agentRuntime.listenerCount("event")).toBe(0);
    expect(setAssistantThreadStatus).toHaveBeenCalledTimes(2);

    agentRuntime.emit("event", {
      type: "agent.tool.started",
      agentSessionId: TEST_SESSION.agentSessionId,
      brokerSessionKey: TEST_SESSION.key,
      turnId: TEST_SESSION.activeTurnId,
      callId: "call-2",
      name: "exec_command",
      at: new Date().toISOString(),
    });

    await Promise.resolve();
    expect(setAssistantThreadStatus).toHaveBeenCalledTimes(2);
  });

  it("skips runtime events without a broker session key", async () => {
    const agentRuntime = new EventEmitter();
    const getSessionByKey = vi.fn(() => TEST_SESSION);
    const setAssistantThreadStatus = vi.fn(async () => undefined);

    const service = new SlackConversationService({
      config: TEST_CONFIG,
      sessions: {
        getSessionByKey,
        upsertAgentTraceEvent: vi.fn(),
      } as never,
      agentRuntime: agentRuntime as never,
      slackApi: {
        setAssistantThreadStatus,
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
      } as never,
      selfMessageFilter: {} as never,
    });

    agentRuntime.emit("event", {
      type: "agent.error",
      code: "runtime_error",
      message: "missing session",
      recoverable: false,
      at: new Date().toISOString(),
    });

    await Promise.resolve();

    expect(getSessionByKey).not.toHaveBeenCalled();
    expect(setAssistantThreadStatus).not.toHaveBeenCalled();

    await service.stop();
  });
});
