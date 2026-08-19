import { afterEach, describe, expect, it } from "vitest";

import { AgentTraceRecorder } from "../src/services/agent-runtime/agent-trace-recorder.js";
import { readJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

describe("agent trace recorder e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("records visible assistant, input, and tool traces through admin timeline resources", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-recorder-",
    });
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-1");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-1");
    const recorder = new AgentTraceRecorder({ sessions });

    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-1",
      turnId: "turn-1",
      messageId: "message-empty",
      role: "assistant",
      text: "   \n  ",
      at: "2026-03-19T00:00:02.000Z",
    });
    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-1",
      turnId: "turn-1",
      messageId: "message-1",
      role: "assistant",
      text: "我会先检查状态。",
      at: "2026-03-19T00:00:03.000Z",
    });
    await recorder.record({
      type: "agent.input.received",
      inputId: "input-1",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      source: "slack_user",
      textPreview: "A newer Slack message arrived while the current turn is still active. Treat it as the latest instruction...",
      text: [
        "A newer Slack message arrived while the current turn is still active.",
        "Treat it as the latest instruction and adjust the ongoing work accordingly.",
        "",
        "A new message arrived in the active Slack thread. Carefully judge whether it requires a reply or action from you.",
        "structured_message_json:",
        "```json",
        JSON.stringify(
          {
            source: "app_mention",
            message_ts: "1778316208.809479",
            sender: {
              kind: "user",
              user_id: "U123",
              mention: "<@U123>",
              display_name: "Jc",
            },
            text: "<@U0ALY77RMJL> 结合 willow repo，分析图中问题",
            text_with_resolved_mentions: "@codex-3720 结合 willow repo，分析图中问题",
            images: [],
          },
          null,
          2,
        ),
        "```",
      ].join("\n"),
      at: "2026-03-19T00:00:03.100Z",
    });
    await recorder.record({
      type: "agent.input.received",
      inputId: "input-runtime-1",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      source: "background_job",
      textPreview: "A broker-managed background job reported a new asynchronous event for this session.",
      text: [
        "A broker-managed background job reported a new asynchronous event for this session.",
        "background_job_event_json:",
        "```json",
        JSON.stringify(
          {
            source: "background_job_event",
            message_ts: "1778316208.809479",
            job: {
              job_id: "job-1",
              job_kind: "watch_ci",
              event_kind: "job_completed",
            },
            summary: "PR #1873 checks 13 pass.",
          },
          null,
          2,
        ),
        "```",
      ].join("\n"),
      at: "2026-03-19T00:00:03.200Z",
    });
    await recorder.record({
      type: "agent.tool.started",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      callId: "call-1",
      name: "exec_command",
      input: {
        command: '/bin/zsh -lc "cd /tmp/workspace/app && pnpm test"',
        cwd: "/tmp/workspace",
        commandActions: [
          {
            type: "test",
            name: "unit",
          },
        ],
      },
      at: "2026-03-19T00:00:03.300Z",
    });

    const started = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect((started.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(["agent_assistant_message", "agent_input_received", "agent_input_received", "agent_tool_call"]);
    expect(started.events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          type: "agent_assistant_message",
          title: "Assistant 消息",
          summary: "我会先检查状态。",
          turnId: "turn-1",
        }),
        expect.objectContaining({
          type: "agent_input_received",
          title: "@codex-3720 结合 willow repo，分析图中问题",
          summary: "Jc · 提及",
          role: "user",
          metadata: expect.objectContaining({
            inputId: "input-1",
            source: "app_mention",
            sender: "Jc",
            messageTs: "1778316208.809479",
          }),
        }),
        expect.objectContaining({
          type: "agent_input_received",
          title: "PR #1873 checks 13 pass.",
          summary: "watch_ci · job_completed · Job job-1",
          role: "system",
          metadata: expect.objectContaining({
            inputId: "input-runtime-1",
            source: "background_job_event",
            jobKind: "watch_ci",
            eventKind: "job_completed",
          }),
        }),
        expect.objectContaining({
          type: "agent_tool_call",
          title: "pnpm test",
          summary: "测试 unit · cwd app · 运行中",
          metadata: expect.objectContaining({
            commandPreview: "pnpm test",
            cwdLabel: "app",
            actionSummary: "测试 unit",
          }),
        }),
      ]),
    );

    await recorder.record({
      type: "agent.tool.completed",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      callId: "call-1",
      name: "exec_command",
      output: {
        command: '/bin/zsh -lc "cd /tmp/workspace/app && pnpm test"',
        cwd: "/tmp/workspace",
        exitCode: 0,
        durationMs: 1200,
        aggregatedOutput: "PASS unit tests",
      },
      status: "completed",
      at: "2026-03-19T00:00:04.000Z",
    });

    const completed = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect((completed.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(["agent_assistant_message", "agent_input_received", "agent_input_received", "agent_tool_result"]);
    const toolResult = (completed.events as Array<Record<string, any>>).find((event) => event.type === "agent_tool_result");
    expect(toolResult).toMatchObject({
      title: "pnpm test",
      summary: "exit 0 · 1.2s · 输出 PASS unit tests",
      metadata: expect.objectContaining({
        exitCode: 0,
        durationMs: 1200,
        outputPreview: "PASS unit tests",
      }),
    });
    const detail = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline-events/${encodeURIComponent(String(toolResult?.id))}`);
    expect(detail).toMatchObject({
      ok: true,
      event: {
        type: "agent_tool_result",
        metadata: expect.objectContaining({
          outputPreview: "PASS unit tests",
        }),
      },
    });
  });

  it("records late events from an old agent turn after the session switches runtime ids", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-recorder-late-",
    });
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-old");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-old");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-new");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-new");
    const recorder = new AgentTraceRecorder({ sessions });
    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-old",
      turnId: "turn-old",
      messageId: "message-late",
      role: "assistant",
      text: "旧 session 的迟到事件不能断链。",
      at: "2026-03-19T00:00:03.000Z",
    });

    const timeline = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect(timeline.events).toEqual([
      expect.objectContaining({
        type: "agent_assistant_message",
        summary: "旧 session 的迟到事件不能断链。",
        turnId: "turn-old",
      }),
    ]);
  });
});
