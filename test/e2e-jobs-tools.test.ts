import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { brokerRoot, collectTextInput, fetchJson, getFreePort, readBackgroundJobs, readSessionRecord, removeTempRoot, runTsxTool, startBrokerProcess, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";

describe.sequential("jobs and tools e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("registers a broker-managed script job and forwards helper-emitted events", async () => {
    const harness = await startHarness(cleanups);
    await mention(harness, "817.220");
    const registered = await fetchJson(`${harness.baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "817.220",
      kind: "watch_ci",
      script: "#!/bin/sh\nsleep 30",
    });
    expect(registered.status).toBe(200);
    const job = registered.body.job as { id: string; token: string; scriptPath: string };
    expect(job.scriptPath).toContain(job.id);

    const helperResult = await runJobHelper(harness, job, ["event", "--kind", "state_changed", "--summary", "CI turned green."]);
    expect(helperResult.status, `${helperResult.stdout}\n${helperResult.stderr}`).toBe(0);

    await waitFor(() => {
      const delivered = [...harness.mockCodex.turnsStarted.map((turn) => collectTextInput(turn.input)), ...harness.mockCodex.steers.map((steer) => collectTextInput(steer.input))];
      return delivered.some((text) => text.includes("CI turned green.") && text.includes("background_job_event_json"));
    }, "job helper event delivery");

    expect(await readBackgroundJobs(harness.tempRoot)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: job.id,
          lastEventKind: "state_changed",
          lastEventSummary: "CI turned green.",
        }),
      ]),
    );
  }, 90_000);

  it("does not cancel restartable jobs during broker shutdown", async () => {
    const harness = await startHarness(cleanups);
    await mention(harness, "818.220");
    const registered = await fetchJson(`${harness.baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "818.220",
      kind: "watch_ci",
      script: "#!/bin/sh\nsleep 30",
    });
    const job = registered.body.job as { id: string };
    expect(registered.status).toBe(200);

    await harness.stopBroker();
    expect(await readBackgroundJobs(harness.tempRoot)).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: job.id,
          status: "running",
          restartOnBoot: true,
        }),
      ]),
    );
  }, 90_000);

  it("lets admin cancel a session-owned job without exposing the job token", async () => {
    const harness = await startHarness(cleanups);
    await mention(harness, "819.220");
    const session = await readSessionRecord(harness.tempRoot, "C123:819.220");
    const childPidPath = path.join(session.workspacePath, "child.pid");
    const registered = await fetchJson(`${harness.baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "819.220",
      kind: "watch_ci",
      script: `#!/usr/bin/env bash\nsleep 30 &\necho $! > '${childPidPath}'\nwait`,
    });
    const job = registered.body.job as { id: string };
    const childPid = Number(await waitForFileContents(childPidPath));

    const mismatch = await fetchJson(`${harness.baseUrl}/jobs/${job.id}/admin-cancel`, {
      session_key: "C999:000.000",
    });
    expect(mismatch.status).toBe(400);
    expect(mismatch.body).toMatchObject({
      ok: false,
      error: "job_session_mismatch",
    });

    const cancelled = await fetchJson(`${harness.baseUrl}/jobs/${job.id}/admin-cancel`, {
      session_key: session.key,
    });
    expect(cancelled.status).toBe(200);
    expect(cancelled.body).toMatchObject({
      ok: true,
      job: {
        id: job.id,
        sessionKey: session.key,
        status: "cancelled",
      },
    });
    await waitForProcessExit(childPid);

    const again = await fetchJson(`${harness.baseUrl}/jobs/${job.id}/admin-cancel`, {
      session_key: session.key,
    });
    expect(again.status).toBe(500);
    expect(again.body).toMatchObject({
      ok: false,
      error: "job_not_cancellable:cancelled",
    });
  }, 90_000);

  it("injects a runtime-relative helper path into background jobs", async () => {
    const harness = await startHarness(cleanups);
    await mention(harness, "820.220");
    const session = await readSessionRecord(harness.tempRoot, "C123:820.220");
    const capturePath = path.join(session.workspacePath, "helper-path.txt");
    const registered = await fetchJson(`${harness.baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "820.220",
      kind: "watch_ci",
      script: `#!/usr/bin/env bash\nprintf '%s' "$BROKER_JOB_HELPER" > '${capturePath}'\nsleep 30`,
    });
    expect(registered.status).toBe(200);
    const helperPath = await waitForFileContents(capturePath);
    expect(helperPath.endsWith("job-callback.js") || helperPath.endsWith("job-callback.ts")).toBe(true);
    expect(helperPath.startsWith("/app/")).toBe(false);
  }, 90_000);

  it("rejects isolated MCP servers that are not marked as isolated", async () => {
    const harness = await startHarness(cleanups);
    const listed = await fetchJson(`${harness.baseUrl}/integrations/mcp-tools?server=github`);
    expect(listed.status).toBe(502);
    expect(listed.body).toMatchObject({
      ok: false,
      error: "unsupported_isolated_mcp_server:github",
    });
    const called = await fetchJson(`${harness.baseUrl}/integrations/mcp-call`, {
      server: "github",
      name: "search",
      arguments: {
        query: "docs",
      },
    });
    expect(called.status).toBe(502);
    expect(called.body).toMatchObject({
      ok: false,
      error: "unsupported_isolated_mcp_server:github",
    });
  }, 60_000);
});

async function startHarness(cleanups: Array<() => Promise<void>>): Promise<{
  readonly baseUrl: string;
  readonly tempRoot: string;
  readonly mockCodex: MockCodexAppServer;
  readonly mockSlack: MockSlackServer;
  readonly stopBroker: () => Promise<void>;
}> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "jobs-tools-e2e-"));
  cleanups.push(async () => {
    await removeTempRoot(tempRoot);
  });
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
    port: await getFreePort(),
    slackPort,
    codexUrl,
    tempRoot,
  });
  cleanups.push(() => broker.stop());
  return {
    baseUrl: broker.baseUrl,
    tempRoot,
    mockCodex,
    mockSlack,
    stopBroker: () => broker.stop(),
  };
}

async function mention(
  harness: {
    readonly mockSlack: MockSlackServer;
    readonly tempRoot: string;
  },
  threadTs: string,
): Promise<void> {
  await harness.mockSlack.sendEvent(`evt-jobs-${threadTs}`, {
    type: "app_mention",
    user: "U123",
    channel: "C123",
    thread_ts: threadTs,
    ts: `${threadTs}1`,
    text: `<@UBOT> start session ${threadTs}`,
  });
  await waitForSessionIdle(harness.tempRoot, `C123:${threadTs}`);
}

async function runJobHelper(
  harness: { readonly baseUrl: string; readonly tempRoot: string },
  job: { readonly id: string; readonly token: string },
  args: readonly string[],
): Promise<{
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}> {
  const session = await readSessionRecord(harness.tempRoot, "C123:817.220");
  return await runTsxTool({
    script: path.join(brokerRoot, "src/tools/job-callback.ts"),
    cwd: session.workspacePath,
    args,
    env: {
      BROKER_API_BASE: harness.baseUrl,
      BROKER_JOB_ID: job.id,
      BROKER_JOB_TOKEN: job.token,
    },
  });
}

async function waitForFileContents(filePath: string): Promise<string> {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try {
      return await fs.readFile(filePath, "utf8");
    } catch {
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
  }
  throw new Error(`Timed out waiting for ${filePath}`);
}

async function waitForProcessExit(pid: number): Promise<void> {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    try {
      process.kill(pid, 0);
      await new Promise((resolve) => setTimeout(resolve, 25));
    } catch {
      return;
    }
  }
  throw new Error(`Timed out waiting for pid ${pid} to exit`);
}
