import { EventEmitter } from "node:events";

import { describe, expect, it, vi } from "vitest";

import { CodexAppServerRuntime } from "../src/services/agent-runtime/codex-app-server-runtime.js";
import type { AgentRuntimeEvent } from "../src/services/agent-runtime/types.js";
import type { SlackSessionRecord } from "../src/types.js";

const TEST_SESSION: SlackSessionRecord = {
  key: "C123:111.222",
  channelId: "C123",
  rootThreadTs: "111.222",
  workspacePath: "/tmp/workspace",
  agentSessionId: "thread-1",
  activeTurnId: "turn-1",
  createdAt: "2026-05-09T00:00:00.000Z",
  updatedAt: "2026-05-09T00:00:00.000Z",
};

describe("CodexAppServerRuntime", () => {
  it("emits assistant message content from response_item notifications", () => {
    const { codex, events } = createRuntimeFixture();

    codex.emit("notification", "codex/event", {
      thread_id: "thread-1",
      turn_id: "turn-1",
      msg: {
        type: "response_item",
        payload: {
          type: "message",
          id: "message-1",
          role: "assistant",
          content: [
            {
              type: "output_text",
              text: "我已经修好移动端布局。",
            },
          ],
        },
        timestamp: "2026-05-09T00:00:01.000Z",
      },
    });

    expect(events).toEqual([
      expect.objectContaining({
        type: "agent.message.completed",
        agentSessionId: "thread-1",
        brokerSessionKey: TEST_SESSION.key,
        turnId: "turn-1",
        messageId: "message-1",
        role: "assistant",
        text: "我已经修好移动端布局。",
      }),
    ]);
  });

  it("uses historical agent activity bindings after the session switches to a new runtime", () => {
    const switchedSession: SlackSessionRecord = {
      ...TEST_SESSION,
      agentSessionId: "thread-new",
      activeTurnId: "turn-new",
    };
    const { codex, events } = createRuntimeFixture({
      sessions: {
        findSessionByWorkspace: vi.fn(() => undefined),
        findSessionByAgentActivity: vi.fn(({ agentSessionId, turnId }) => (agentSessionId === "thread-old" || turnId === "turn-old" ? switchedSession : undefined)),
        listSessions: vi.fn(() => [switchedSession]),
      } as never,
    });

    codex.emit("notification", "codex/event", {
      thread_id: "thread-old",
      turn_id: "turn-old",
      msg: {
        type: "response_item",
        payload: {
          type: "message",
          id: "message-late",
          role: "assistant",
          content: [
            {
              type: "output_text",
              text: "旧 turn 的迟到事件仍然属于这个 Slack thread。",
            },
          ],
        },
      },
    });

    expect(events).toEqual([
      expect.objectContaining({
        type: "agent.message.completed",
        agentSessionId: "thread-old",
        brokerSessionKey: TEST_SESSION.key,
        turnId: "turn-old",
        messageId: "message-late",
        text: "旧 turn 的迟到事件仍然属于这个 Slack thread。",
      }),
    ]);
  });

  it("ignores empty assistant response_item notifications", () => {
    const { codex, events } = createRuntimeFixture();

    codex.emit("notification", "codex/event", {
      thread_id: "thread-1",
      turn_id: "turn-1",
      msg: {
        type: "response_item",
        payload: {
          type: "message",
          id: "message-empty",
          role: "assistant",
          content: [],
        },
      },
    });

    expect(events).toEqual([]);
  });
});

function createRuntimeFixture(options?: { readonly sessions?: unknown }): {
  readonly codex: EventEmitter;
  readonly events: AgentRuntimeEvent[];
} {
  const codex = Object.assign(new EventEmitter(), {
    start: vi.fn(async () => undefined),
    stop: vi.fn(async () => undefined),
    setSlackBotIdentity: vi.fn(),
    ensureThread: vi.fn(async () => "thread-1"),
    steer: vi.fn(async () => undefined),
    startTurn: vi.fn(async () => ({
      turnId: "turn-1",
      completion: Promise.resolve({
        threadId: "thread-1",
        turnId: "turn-1",
        finalMessage: "",
        aborted: false,
      }),
    })),
    interrupt: vi.fn(async () => undefined),
    readTurnResult: vi.fn(async () => null),
  });
  const runtime = new CodexAppServerRuntime({
    codex: codex as never,
    sessions: (options?.sessions ?? {
      findSessionByWorkspace: vi.fn(() => undefined),
      findSessionByAgentActivity: vi.fn(() => undefined),
      listSessions: vi.fn(() => [TEST_SESSION]),
    }) as never,
  });
  const events: AgentRuntimeEvent[] = [];
  runtime.on("event", (event: AgentRuntimeEvent) => {
    events.push(event);
  });
  return { codex, events };
}
