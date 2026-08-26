import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { getFreePort, removeTempRoot, spawnAgent, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";

const agentToken = "zork-agent-compliance-token";
const sessionWorkspaces = new Map<string, string>();

function authHeaders(json = false): Record<string, string> {
  return {
    authorization: `Bearer ${agentToken}`,
    ...(json ? { "content-type": "application/json" } : {}),
  };
}

async function writeProfile(dataRoot: string): Promise<void> {
  const profiles = path.join(dataRoot, "profiles");
  await fs.mkdir(profiles, { recursive: true });
  await fs.writeFile(
    path.join(profiles, "fixture.json"),
    `${JSON.stringify({
      provider: "openai",
      billing: "usage",
      base_url: "http://127.0.0.1:9/v1",
      auth: { type: "api_key", key: "sk-fixture" },
      models: [
        {
          id: "fixture-model",
          api: "openai-completions",
          streaming: true,
          thinking: ["off"],
          default_thinking: "off",
          capabilities: { input: ["text"] },
          default: true,
        },
      ],
    })}\n`,
  );
}

async function startAgent(env?: Record<string, string>): Promise<{
  tempRoot: string;
  dataRoot: string;
  baseUrl: string;
  child: ReturnType<typeof spawnAgent>;
  workspace: string;
}> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-compliance-"));
  const dataRoot = path.join(tempRoot, "data");
  const port = await getFreePort();
  await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
  await writeProfile(dataRoot);
  const child = spawnAgent(dataRoot, true, env, agentToken);
  const baseUrl = `http://127.0.0.1:${port}`;
  const workspace = path.join(tempRoot, "workspace");
  await fs.mkdir(workspace);
  sessionWorkspaces.set(baseUrl, workspace);
  await waitForReady(`${baseUrl}/readyz`, "zork-agent compliance readyz");
  return { tempRoot, dataRoot, baseUrl, child, workspace };
}

async function createSession(baseUrl: string, systemPrompt?: string): Promise<string> {
  const response = await fetch(`${baseUrl}/v1/sessions`, {
    method: "POST",
    headers: authHeaders(true),
    body: JSON.stringify({
      profile_id: "fixture",
      model: "fixture-model",
      thinking: "off",
      workspace: sessionWorkspaces.get(baseUrl),
      ...(systemPrompt === undefined ? {} : { system_prompt: systemPrompt }),
    }),
  });
  expect(response.status).toBe(201);
  const body = (await response.json()) as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(["model", "profile_id", "session_id", "status", "thinking", "workspace"]);
  return String(body.session_id);
}

async function postMessage(baseUrl: string, sessionId: string, content: string): Promise<void> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
    method: "POST",
    headers: authHeaders(true),
    body: JSON.stringify({ content }),
  });
  expect(response.status).toBe(202);
  expect(await response.text()).toBe("");
}

async function readMessages(baseUrl: string, sessionId: string): Promise<Array<{ type?: string; role?: string; content?: string }>> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/messages?limit=200`, {
    headers: authHeaders(),
  });
  expect(response.status).toBe(200);
  return (
    (
      (await response.json()) as {
        items?: Array<{ type?: string; role?: string; content?: string }>;
      }
    ).items ?? []
  );
}

describe.sequential("zork-agent application boundaries", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) await cleanups.pop()?.();
  });

  it("protects only the application APIs and leaves removed endpoint contracts absent", async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));

    expect((await fetch(`${runtime.baseUrl}/readyz`)).status).toBe(200);
    expect((await fetch(`${runtime.baseUrl}/v1/sessions`)).status).toBe(401);
    expect((await fetch(`${runtime.baseUrl}/v1/profiles`)).status).toBe(401);
    expect((await fetch(`${runtime.baseUrl}/v1/sessions`, { headers: authHeaders() })).status).toBe(200);
    for (const removed of ["/healthz", "/v1/identity", "/v1/health", "/v1/capabilities", "/v1/auth-replicas", "/v1/callbacks/old"]) {
      expect((await fetch(runtime.baseUrl + removed, { headers: authHeaders() })).status).toBe(404);
    }
  });

  it("persists and uses the caller-owned workspace without placing it in Agent session storage", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const workspace = path.join(runtime.tempRoot, "caller-workspace");
    await fs.mkdir(workspace);

    const created = await fetch(`${runtime.baseUrl}/v1/sessions`, {
      method: "POST",
      headers: authHeaders(true),
      body: JSON.stringify({
        profile_id: "fixture",
        model: "fixture-model",
        thinking: "off",
        workspace,
      }),
    });
    expect(created.status).toBe(201);
    const body = (await created.json()) as Record<string, unknown>;
    const sessionId = String(body.session_id);
    expect(body.workspace).toBe(await fs.realpath(workspace));

    const sessionDir = path.join(runtime.dataRoot, "sessions", sessionId);
    expect(await fs.readdir(sessionDir)).toEqual(["segments"]);
    const [segment] = await fs.readdir(path.join(sessionDir, "segments"));
    const [creationLine] = (await fs.readFile(path.join(sessionDir, "segments", segment), "utf8")).trim().split("\n");
    expect((JSON.parse(creationLine) as { event: { workspace?: string } }).event.workspace).toBe(await fs.realpath(workspace));

    await postMessage(runtime.baseUrl, sessionId, JSON.stringify({ fake_tool: { name: "write", input: { path: "owned-by-caller.txt", content: "kept" } } }));
    await waitFor(
      () => fs.readFile(path.join(workspace, "owned-by-caller.txt"), "utf8").catch(() => ""),
      (content) => content === "kept",
      "tool uses caller workspace",
    );

    const deleted = await fetch(`${runtime.baseUrl}/v1/sessions/${sessionId}`, {
      method: "DELETE",
      headers: authHeaders(),
    });
    expect(deleted.status).toBe(204);
    await expect(fs.stat(sessionDir)).rejects.toMatchObject({ code: "ENOENT" });
    expect(await fs.readFile(path.join(workspace, "owned-by-caller.txt"), "utf8")).toBe("kept");
  });

  it("uses one Agent-generated session ID for the durable stream and repairs a torn suffix", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    expect(sessionId).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);

    const sessionDir = path.join(runtime.dataRoot, "sessions", sessionId);
    expect(await fs.readdir(sessionDir)).toEqual(["segments"]);
    const segmentNames = await fs.readdir(path.join(sessionDir, "segments"));
    expect(segmentNames).toHaveLength(1);
    expect(segmentNames[0]).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}\.jsonl$/);
    const segmentPath = path.join(sessionDir, "segments", segmentNames[0]);
    const creationEvent = JSON.parse((await fs.readFile(segmentPath, "utf8")).trim()) as {
      kind?: string;
      event_id?: string;
      batch_index?: number;
      batch_size?: number;
    };
    expect(creationEvent).toMatchObject({ kind: "domain", batch_index: 0, batch_size: 1 });
    expect(segmentNames[0]).toBe(`${creationEvent.event_id}.jsonl`);
    if (process.platform !== "win32") {
      expect((await fs.stat(segmentPath)).mode & 0o777).toBe(0o600);
    }

    await postMessage(runtime.baseUrl, sessionId, "durable before torn tail");
    await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content === "durable before torn tail"),
      "durable assistant result",
    );
    await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content === "end accepted"),
      "durable explicit end result",
    );
    await stopChild(runtime.child);

    const jsonlPath = segmentPath;
    const committed = await fs.readFile(jsonlPath, "utf8");
    expect(committed).not.toContain("global_position");
    expect(committed).not.toContain("status_changed");
    await fs.appendFile(jsonlPath, '{"kind":"domain"');

    const restarted = spawnAgent(runtime.dataRoot, true, undefined, agentToken);
    cleanups.push(async () => stopChild(restarted));
    await waitForReady(`${runtime.baseUrl}/readyz`, "zork-agent torn-tail restart");
    const restartedSession = await fetch(`${runtime.baseUrl}/v1/sessions/${sessionId}`, {
      headers: authHeaders(),
    });
    expect(restartedSession.status).toBe(200);
    expect((await restartedSession.json()) as Record<string, unknown>).toMatchObject({
      workspace: await fs.realpath(runtime.workspace),
    });
    expect((await readMessages(runtime.baseUrl, sessionId)).some((item) => item.content === "durable before torn tail")).toBe(true);
    expect(await fs.readFile(jsonlPath, "utf8")).toBe(committed);

    const deleted = await fetch(`${runtime.baseUrl}/v1/sessions/${sessionId}`, {
      method: "DELETE",
      headers: authHeaders(),
    });
    expect(deleted.status).toBe(204);
    await expect(fs.stat(sessionDir)).rejects.toMatchObject({ code: "ENOENT" });
    expect((await fs.stat(runtime.workspace)).isDirectory()).toBe(true);
  });

  it("persists the optional session system prompt in SessionCreated and has no implicit prompt", async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));

    const explicitPrompt = "fixed coding prompt for this session";
    const explicitId = await createSession(runtime.baseUrl, explicitPrompt);
    const noPromptId = await createSession(runtime.baseUrl);

    const readCreation = async (sessionId: string): Promise<Record<string, unknown>> => {
      const segments = path.join(runtime.dataRoot, "sessions", sessionId, "segments");
      const [segment] = await fs.readdir(segments);
      const [line] = (await fs.readFile(path.join(segments, segment), "utf8")).trim().split("\n");
      return (JSON.parse(line) as { event: Record<string, unknown> }).event;
    };
    expect(await readCreation(explicitId)).toMatchObject({ system_prompt: explicitPrompt });
    expect(await readCreation(noPromptId)).toMatchObject({ system_prompt: null });
  });

  it("does not impose infrastructure byte limits on mailbox, tool arguments, or tool results", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    const workspace = runtime.workspace;
    const content = "x\n".repeat(160 * 1024);

    await postMessage(runtime.baseUrl, sessionId, JSON.stringify({ fake_tool: { name: "write", input: { path: "large.txt", content } } }));
    await waitFor(
      () => fs.readFile(path.join(workspace, "large.txt"), "utf8").catch(() => ""),
      (value) => value === content,
      "large tool argument is executed",
    );

    await postMessage(runtime.baseUrl, sessionId, JSON.stringify({ fake_tool: { name: "read", input: { path: "large.txt" } } }));
    const messages = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content?.includes("Use offset=")),
      "large read returns a tool-owned bounded page",
    );
    const latestTool = messages.findLast((item) => item.role === "tool" && item.content?.includes("Use offset="));
    expect(latestTool?.content?.length ?? 0).toBeLessThan(60 * 1024);
  });

  it("uses Pi tail truncation for long bash output and exposes the complete workspace file to read", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    const workspace = runtime.workspace;

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "bash",
          input: {
            command: 'i=1; while [ "$i" -le 2400 ]; do printf \'line-%04d:%032d\\n\' "$i" "$i"; i=$((i + 1)); done',
          },
        },
      }),
    );
    const messages = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content?.includes("line-2400")),
      "bounded bash result",
    );
    const bashResult = messages.findLast((item) => item.role === "tool" && item.content?.includes("line-2400"))?.content ?? "";
    expect(bashResult).not.toContain("line-0001");
    expect(bashResult).toContain("line-2400");
    expect(bashResult).not.toContain("line-1200");
    expect(bashResult).toContain("Showing lines");
    expect(bashResult).toContain("50.0KB limit");
    expect(bashResult).toContain("Use read with offset and limit");
    expect(bashResult.length).toBeLessThan(60 * 1024);

    const outputPath = bashResult.match(/Full output: (\.zork\/bash-output-[0-9A-HJKMNP-TV-Z]{26}\.log)/)?.[1];
    expect(outputPath).toBeDefined();
    const fullOutput = await fs.readFile(path.join(workspace, outputPath!), "utf8");
    expect(fullOutput).toContain("line-0001");
    expect(fullOutput).toContain("line-1200");
    expect(fullOutput).toContain("line-2400");
    expect(fullOutput.split("\n")).toHaveLength(2401);

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: { name: "read", input: { path: outputPath, offset: 1200, limit: 1 } },
      }),
    );
    const paged = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content?.startsWith("line-1200")),
      "read page from complete bash output",
    );
    expect(paged.findLast((item) => item.role === "tool" && item.content?.startsWith("line-1200"))?.content).toContain("Use offset=1201");
  });

  it("overlaps independent tools but durably commits results in declaration order", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tools: [
          {
            name: "bash",
            input: {
              command: "touch first-started; i=0; while [ ! -f second-started ] && [ $i -lt 100 ]; do sleep 0.02; i=$((i + 1)); done; test -f second-started; sleep 0.25; printf first-parallel",
            },
          },
          {
            name: "bash",
            input: {
              command: "touch second-started; i=0; while [ ! -f first-started ] && [ $i -lt 100 ]; do sleep 0.02; i=$((i + 1)); done; test -f first-started; printf second-parallel",
            },
          },
        ],
      }),
    );
    const messages = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content === "first-parallel") && items.some((item) => item.role === "tool" && item.content === "second-parallel"),
      "both independent tools complete after overlapping",
    );
    const parallelResults = messages.filter((item) => item.role === "tool" && (item.content === "first-parallel" || item.content === "second-parallel"));
    expect(parallelResults.map((item) => item.content)).toEqual(["first-parallel", "second-parallel"]);
  });

  it("preserves declaration order for conflicting access to the same path", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tools: [
          { name: "write", input: { path: "ordered.txt", content: "ordered" } },
          { name: "read", input: { path: "ordered.txt" } },
        ],
      }),
    );
    const messages = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content === "ordered"),
      "same-path read observes the prior write",
    );
    expect(messages.some((item) => item.role === "tool" && item.content === "ordered")).toBe(true);
  });

  it("provides Pi-style read pagination and multi-block edit", { timeout: 30_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    const workspace = runtime.workspace;
    await fs.writeFile(path.join(workspace, "edit.txt"), "one\ntwo\nthree\nfour\n");

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: { name: "read", input: { path: "edit.txt", offset: 2, limit: 2 } },
      }),
    );
    const paged = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "tool" && item.content?.startsWith("two\nthree")),
      "read offset and limit",
    );
    expect(paged.findLast((item) => item.role === "tool" && item.content?.startsWith("two\nthree"))?.content).toContain("Use offset=4");

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "edit",
          input: {
            path: "edit.txt",
            edits: [
              { oldText: "one", newText: "ONE" },
              { oldText: "four", newText: "FOUR" },
            ],
          },
        },
      }),
    );
    await waitFor(
      () => fs.readFile(path.join(workspace, "edit.txt"), "utf8"),
      (value) => value === "ONE\ntwo\nthree\nFOUR\n",
      "multi-block edit",
    );
  });

  it("runs the application tools inside the caller-owned workspace without inheriting parent secrets", { timeout: 30_000 }, async () => {
    const runtime = await startAgent({ ZORK_UNSAFE_PARENT_SECRET: "must-not-leak" });
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    const workspace = runtime.workspace;

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: { name: "write", input: { path: "hello.txt", content: "hello tools" } },
      }),
    );
    await waitFor(
      () => fs.readFile(path.join(workspace, "hello.txt"), "utf8").catch(() => ""),
      (content) => content === "hello tools",
      "write tool output",
    );

    await postMessage(runtime.baseUrl, sessionId, JSON.stringify({ fake_tool: { name: "read", input: { path: "hello.txt" } } }));
    await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content?.includes("hello tools")),
      "read tool result",
    );

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "bash",
          input: { command: 'pwd; printf "%s" "${ZORK_UNSAFE_PARENT_SECRET-unset}" > env.txt' },
        },
      }),
    );
    await waitFor(
      () => fs.readFile(path.join(workspace, "env.txt"), "utf8").catch(() => ""),
      (content) => content === "unset",
      "sanitized tool environment",
    );
    const messages = await waitFor(
      () => readMessages(runtime.baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content?.includes(workspace)),
      "bash workspace cwd",
    );
    expect(messages.some((item) => item.content?.includes("must-not-leak"))).toBe(false);
  });

  it("cancels the complete active bash process group with an empty cancel request", { timeout: 20_000 }, async () => {
    const runtime = await startAgent();
    cleanups.push(async () => removeTempRoot(runtime.tempRoot));
    cleanups.push(async () => stopChild(runtime.child));
    const sessionId = await createSession(runtime.baseUrl);
    const workspace = runtime.workspace;

    await postMessage(
      runtime.baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "bash",
          input: { command: "trap '' HUP; sleep 60 & echo $! > background.pid; wait" },
        },
      }),
    );
    const pid = Number(
      await waitFor(
        () => fs.readFile(path.join(workspace, "background.pid"), "utf8").catch(() => ""),
        (value) => /^\d+\s*$/.test(value),
        "background child pid",
      ),
    );
    cleanups.push(async () => {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        // The cancelled process is already gone.
      }
    });

    const cancelled = await fetch(`${runtime.baseUrl}/v1/sessions/${sessionId}/cancel`, {
      method: "POST",
      headers: authHeaders(),
    });
    expect(cancelled.status).toBe(204);
    await waitFor(
      () => processExists(pid),
      (exists) => !exists,
      "cancelled bash process group",
    );
  });
});

function processExists(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
