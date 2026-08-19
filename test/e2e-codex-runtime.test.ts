import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import type { MockTurnContext } from "./helpers/mock-codex-app-server.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { fetchJson, getFreePort, readAgentTraceEvents, readAgentTurnUsage, removeTempRoot, startBrokerProcess, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";

describe.sequential("codex runtime e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("injects personal memory into thread/start base instructions only once", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "codex-runtime-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    await fs.mkdir(path.join(tempRoot, "codex-home"), { recursive: true });
    await fs.writeFile(path.join(tempRoot, "codex-home", "AGENT.md"), "remember this\n");

    const harness = await startHarness(tempRoot, cleanups);
    const sessionKey = "C123:810.220";

    await harness.mockSlack.sendEvent("evt-prompt-start", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "810.220",
      ts: "810.221",
      text: "<@UBOT> start with memory",
    });
    await waitForSessionIdle(tempRoot, sessionKey);

    expect(harness.mockCodex.threadStarts).toHaveLength(1);
    const baseInstructions = String(harness.mockCodex.threadStarts[0]?.baseInstructions ?? "");
    expect(harness.mockCodex.threadStarts[0]?.experimentalRawEvents).toBe(true);
    expect(baseInstructions).toContain("channel_id: C123");
    expect(baseInstructions).toContain("thread_ts: 810.220");
    expect(baseInstructions).toContain(`runtime_platform: ${process.platform}`);
    expect(baseInstructions).toContain("~/.codex/AGENT.md");
    expect(baseInstructions).toContain("remember this");
    expect(baseInstructions).toContain("zork-call");
    expect(baseInstructions).not.toContain("BROKER_JOB_HELPER");
    expect(baseInstructions).toContain("Turn stopping contract");
    expect(baseInstructions).toContain("Git commit co-author contract");
    expect(baseInstructions).toContain("zork-call chat post-message");
    expect(baseInstructions).toContain("zork-call integration call");
    expect(baseInstructions).not.toContain("{{");

    const promptEvents = (await readAgentTraceEvents(tempRoot, sessionKey)).filter((event) => event.type === "agent_system_prompt");
    expect(promptEvents).toHaveLength(1);
    expect(promptEvents[0]?.detail).toContain("remember this");

    await harness.mockSlack.sendEvent("evt-prompt-follow-up", {
      type: "message",
      user: "U123",
      channel: "C123",
      thread_ts: "810.220",
      ts: "810.222",
      text: "continue after resume",
    });
    await waitFor(() => harness.mockCodex.turnsStarted.length >= 2, "second turn after follow-up");
    await waitForSessionIdle(tempRoot, sessionKey);

    expect(harness.mockCodex.threadStarts).toHaveLength(1);
    expect(harness.mockCodex.threadResumes.every((resume) => resume.baseInstructions == null)).toBe(true);
    expect((await readAgentTraceEvents(tempRoot, sessionKey)).filter((event) => event.type === "agent_system_prompt")).toHaveLength(1);
  }, 90_000);

  it("captures exact token usage from turn completion notifications", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "codex-runtime-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    const harness = await startHarness(tempRoot, cleanups, {
      onTurnStart: (context) => {
        context.complete("USAGE_DONE", {
          input_tokens: 1200,
          cached_input_tokens: 300,
          output_tokens: 450,
          reasoning_tokens: 75,
          total_tokens: 1725,
          model: "gpt-5.5",
          effort: "xhigh",
        });
      },
    });

    await harness.mockSlack.sendEvent("evt-usage-complete", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "811.220",
      ts: "811.221",
      text: "<@UBOT> count tokens",
    });
    await waitForSessionIdle(tempRoot, "C123:811.220");
    await waitFor(async () => (await readAgentTurnUsage(tempRoot)).some((usage) => usage.totalTokens === 1725), "persisted turn usage");

    expect(await readAgentTurnUsage(tempRoot)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          sessionKey: "C123:811.220",
          source: "exact",
          inputTokens: 1200,
          cachedInputTokens: 300,
          outputTokens: 450,
          reasoningTokens: 75,
          totalTokens: 1725,
          model: "gpt-5.5",
          effort: "xhigh",
        }),
      ]),
    );
  }, 90_000);

  it("captures exact token usage from thread/tokenUsage/updated notifications", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "codex-runtime-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    const harness = await startHarness(tempRoot, cleanups, {
      emitThreadTokenUsage: true,
      onTurnStart: (context) => {
        context.complete("THREAD_USAGE_DONE", {
          input_tokens: 1500,
          cached_input_tokens: 250,
          output_tokens: 550,
          reasoning_tokens: 125,
          total_tokens: 2050,
          model: "gpt-5.5",
          effort: "xhigh",
        });
      },
    });

    await harness.mockSlack.sendEvent("evt-thread-usage", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "812.220",
      ts: "812.221",
      text: "<@UBOT> thread usage",
    });
    await waitForSessionIdle(tempRoot, "C123:812.220");
    await waitFor(async () => (await readAgentTurnUsage(tempRoot)).some((usage) => usage.totalTokens === 2050), "persisted thread token usage");

    expect(await readAgentTurnUsage(tempRoot)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          sessionKey: "C123:812.220",
          source: "exact",
          inputTokens: 1500,
          cachedInputTokens: 250,
          outputTokens: 550,
          reasoningTokens: 125,
          totalTokens: 2050,
        }),
      ]),
    );
  }, 90_000);

  it("records app-server commandExecution items as tool calls and failed results", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "codex-runtime-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    const harness = await startHarness(tempRoot, cleanups, {
      onTurnStart: (context) => {
        emitCommandExecution(context, {
          id: "call-1",
          command: '/bin/zsh -lc "pnpm test"',
          cwd: "/repo",
          status: "inProgress",
        });
        emitCommandExecution(
          context,
          {
            id: "call-1",
            command: '/bin/zsh -lc "pnpm test"',
            cwd: "/repo",
            status: "completed",
            aggregatedOutput: "PASS test",
            exitCode: 0,
            durationMs: 240,
          },
          "item/completed",
        );
        emitCommandExecution(
          context,
          {
            id: "call-2",
            command: '/bin/zsh -lc "pnpm lint"',
            status: "completed",
            aggregatedOutput: "lint failed",
            exitCode: 1,
          },
          "item/completed",
        );
        context.complete("TOOLS_DONE");
      },
    });

    await harness.mockSlack.sendEvent("evt-tools", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "813.220",
      ts: "813.221",
      text: "<@UBOT> run tests",
    });
    await waitForSessionIdle(tempRoot, "C123:813.220");
    await waitFor(async () => {
      const events = await readAgentTraceEvents(tempRoot, "C123:813.220");
      return events.some((event) => event.type === "agent_tool_result" && event.callId === "call-2");
    }, "tool traces persisted");

    const events = await readAgentTraceEvents(tempRoot, "C123:813.220");
    expect(events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          type: "agent_tool_call",
          callId: "call-1",
          toolName: "exec_command",
          status: "running",
        }),
        expect.objectContaining({
          type: "agent_tool_result",
          callId: "call-1",
          toolName: "exec_command",
          status: "completed",
        }),
        expect.objectContaining({
          type: "agent_tool_result",
          callId: "call-2",
          toolName: "exec_command",
          status: "failed",
        }),
      ]),
    );
    expect(events.find((event) => event.callId === "call-1" && event.type === "agent_tool_result")?.detail).toContain("PASS test");
    expect(events.find((event) => event.callId === "call-2")?.detail).toContain("lint failed");
  }, 90_000);

  it("reads account rate limits through the live app-server wire contract", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "codex-runtime-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    const harness = await startHarness(tempRoot, cleanups);

    await waitFor(async () => {
      const status = await fetchJson(`${harness.baseUrl}/admin/api/status`);
      const rateLimits = status.body.rateLimits as { ok?: boolean; rateLimits?: { usedPercent?: number } } | undefined;
      return status.status === 200 && rateLimits?.ok === true;
    }, "admin status rate limits");

    const status = await fetchJson(`${harness.baseUrl}/admin/api/status`);
    expect(status.body.rateLimits).toMatchObject({
      ok: true,
      rateLimits: {
        limitId: "codex",
        primary: {
          usedPercent: 12,
          windowDurationMins: 300,
          resetsAt: 1_777_777_777,
        },
        secondary: {
          usedPercent: 3,
          windowDurationMins: 10_080,
        },
        credits: {
          hasCredits: true,
          balance: "42.5",
        },
        planType: "team",
      },
    });
    expect(status.body.account).toMatchObject({
      ok: true,
      account: {
        type: "apiKey",
      },
    });
  }, 60_000);
});

async function startHarness(
  tempRoot: string,
  cleanups: Array<() => Promise<void>>,
  options?: {
    readonly onTurnStart?: (context: MockTurnContext) => Promise<void> | void;
    readonly emitThreadTokenUsage?: boolean;
    readonly extraEnv?: Record<string, string>;
  },
): Promise<{
  readonly baseUrl: string;
  readonly mockCodex: MockCodexAppServer;
  readonly mockSlack: MockSlackServer;
}> {
  const mockSlack = new MockSlackServer("UBOT", {
    botId: "BBOT",
    appId: "AAPP",
  });
  const mockCodex = new MockCodexAppServer({
    ...(options?.onTurnStart ? { onTurnStart: options.onTurnStart } : {}),
    ...(options?.emitThreadTokenUsage != null ? { emitThreadTokenUsage: options.emitThreadTokenUsage } : {}),
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
    extraEnv: options?.extraEnv,
  });
  cleanups.push(() => broker.stop());
  return {
    baseUrl: broker.baseUrl,
    mockCodex,
    mockSlack,
  };
}

function emitCommandExecution(context: MockTurnContext, item: Record<string, unknown>, method: "item/started" | "item/completed" = "item/started"): void {
  context.notify(method, {
    threadId: context.threadId,
    turnId: context.turnId,
    item: {
      type: "commandExecution",
      ...item,
    },
  });
}
