import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { AgentTraceRecorder } from "../src/services/agent-runtime/agent-trace-recorder.js";
import { SessionManager } from "../src/services/session-manager.js";
import { StateStore } from "../src/store/state-store.js";

describe("AgentTraceRecorder", () => {
  it("coalesces duplicate turn completion sources into one trace row", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "agent-trace-recorder-turn-complete-"));
    const sessionsRoot = path.join(stateDir, "sessions");
    const stateStore = new StateStore(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore,
      sessionsRoot,
    });

    await sessions.load();
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-1");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-1");

    const recorder = new AgentTraceRecorder({
      sessions,
    });
    await recorder.record({
      type: "agent.turn.completed",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      status: "completed",
      at: "2026-03-19T00:00:03.000Z",
    });
    await recorder.record({
      type: "agent.turn.completed",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      status: "completed",
      finalMessage: "处理完成，已回复 Slack。",
      at: "2026-03-19T00:00:03.020Z",
    });

    expect(sessions.listAgentTraceEvents(session.key)).toEqual([
      expect.objectContaining({
        type: "agent_turn_completed",
        title: "回合结束",
        summary: "回合已完成",
        detail: "处理完成，已回复 Slack。",
        status: "completed",
        turnId: "turn-1",
      }),
    ]);

    stateStore.close();
    await fs.rm(stateDir, {
      force: true,
      recursive: true,
    });
  });
});
