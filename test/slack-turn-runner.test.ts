import { EventEmitter } from "node:events";

import { afterEach, describe, expect, it, vi } from "vitest";

import type { AgentRuntime } from "../src/services/agent-runtime/types.js";
import { SlackTurnRunner } from "../src/services/slack/slack-turn-runner.js";
import type { SlackSessionRecord } from "../src/types.js";

afterEach(() => {
  vi.restoreAllMocks();
});

function createRuntime(overrides: Partial<AgentRuntime>): AgentRuntime {
  const runtime = new EventEmitter() as AgentRuntime;
  const capabilities = {
    submitWhileActive: true,
    interrupt: true,
    readTurn: true,
    readSession: true,
    rawEvents: true,
    tokenUsage: "exact",
    toolCalls: true,
    systemPromptEcho: true,
  } as const;
  runtime.getCapabilities = vi.fn(() => capabilities);
  runtime.start = vi.fn();
  runtime.stop = vi.fn();
  runtime.setSlackBotIdentity = vi.fn();
  runtime.ensureSession = vi.fn();
  runtime.submitInput = vi.fn();
  runtime.interrupt = vi.fn();
  runtime.readSession = vi.fn();
  runtime.readTurn = vi.fn();
  return Object.assign(runtime, overrides);
}

describe("SlackTurnRunner", () => {
  it("persists the active turn id before marking the Slack batch inflight", async () => {
    const calls: string[] = [];
    const session: SlackSessionRecord = {
      key: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      workspacePath: "/tmp/workspace",
      agentSessionId: "thread-1",
      createdAt: "2026-03-16T00:00:00.000Z",
      updatedAt: "2026-03-16T00:00:00.000Z",
    };
    const activeSession = {
      ...session,
      activeTurnId: "turn-1",
      activeTurnStartedAt: "2026-03-16T00:00:01.000Z",
    };

    const runner = new SlackTurnRunner({
      agentRuntime: createRuntime({
        submitInput: vi.fn(async () => ({
          receipt: {
            agentSessionId: "thread-1",
            turnId: "turn-1",
            inputId: "input-1",
            delivery: "started_turn" as const,
            deliveredAt: "2026-03-16T00:00:01.000Z",
          },
          completion: Promise.resolve({
            agentSessionId: "thread-1",
            turnId: "turn-1",
            finalMessage: "",
            aborted: false,
          }),
        })),
      }),
      slackApi: {
        getUserIdentity: vi.fn(),
        downloadFileAttachment: vi.fn(),
      } as never,
      sessions: {
        setActiveTurnId: vi.fn(async (_channelId: string, _rootThreadTs: string, turnId: string | undefined) => {
          calls.push(turnId ? "set-active" : "clear-active");
          return turnId ? activeSession : session;
        }),
        clearActiveTurnIdIfMatches: vi.fn(async () => {
          calls.push("clear-active");
          return session;
        }),
        setAgentSessionId: vi.fn(),
        upsertAgentTurnUsage: vi.fn(),
      } as never,
      inboundStore: {
        markMessagesInflightByTs: vi.fn(async () => {
          calls.push("mark-inflight");
        }),
        markTurnBatchDone: vi.fn(async () => {
          calls.push("mark-done");
          return activeSession;
        }),
        resetTurnBatchToPending: vi.fn(),
      } as never,
    });

    await runner.submitInputWithRecovery({
      session,
      sessionKey: session.key,
      senderUserId: "U123",
      input: [
        {
          type: "text",
          text: "hello",
          text_elements: [],
        },
      ],
      messageTsList: ["111.223"],
    });

    expect(calls.slice(0, 2)).toEqual(["set-active", "mark-inflight"]);
  });
});
