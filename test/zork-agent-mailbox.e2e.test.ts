import type { ChildProcess } from "node:child_process";
import fs from "node:fs/promises";
import http, { type ServerResponse } from "node:http";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { getFreePort, removeTempRoot, spawnAgent, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";

const internalToken = "zork-agent-mailbox-e2e-token";

type ProviderRequest = {
  body: {
    messages?: Array<{ role?: string; content?: string }>;
    stream?: boolean;
    tools?: Array<{ type?: string; function?: { name?: string; parameters?: unknown } }>;
  };
  response: ServerResponse;
};

class ControlledOpenAI {
  readonly requests: ProviderRequest[] = [];
  readonly server = http.createServer((request, response) => {
    if (request.method !== "POST" || !(request.url ?? "").includes("/chat/completions")) {
      response.statusCode = 404;
      response.end();
      return;
    }

    const chunks: Buffer[] = [];
    request.on("data", (chunk) => chunks.push(chunk as Buffer));
    request.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      this.requests.push({
        body: raw ? (JSON.parse(raw) as ProviderRequest["body"]) : {},
        response,
      });
      this.resolveWaiters();
    });
  });

  private waiters: Array<{ count: number; resolve: (request: ProviderRequest) => void }> = [];

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("controlled provider did not bind");
    return `http://127.0.0.1:${address.port}/v1`;
  }

  waitForRequest(count: number): Promise<ProviderRequest> {
    const existing = this.requests[count - 1];
    if (existing) return Promise.resolve(existing);
    return new Promise((resolve) => {
      this.waiters.push({ count, resolve });
    });
  }

  respond(count: number, content: string): void {
    const pending = this.requests[count - 1];
    if (!pending) throw new Error(`provider request ${count} has not arrived`);
    pending.response.writeHead(200, { "content-type": "text/event-stream" });
    pending.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [{ index: 0, delta: { role: "assistant", content } }],
      })}\n\n`,
    );
    pending.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      })}\n\n`,
    );
    pending.response.write("data: [DONE]\n\n");
    pending.response.end();
  }

  respondWithToolCall(count: number, name: string, args: Record<string, unknown>): void {
    this.respondWithToolCalls(count, [{ name, args }]);
  }

  respondWithToolCalls(count: number, calls: Array<{ name: string; args: Record<string, unknown> }>): void {
    const pending = this.requests[count - 1];
    if (!pending) throw new Error(`provider request ${count} has not arrived`);
    pending.response.writeHead(200, { "content-type": "text/event-stream" });
    pending.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [
          {
            index: 0,
            delta: {
              role: "assistant",
              tool_calls: calls.map(({ name, args }, index) => ({
                index,
                id: `call-${name}-${count}-${index}`,
                type: "function",
                function: { name, arguments: JSON.stringify(args) },
              })),
            },
          },
        ],
      })}\n\n`,
    );
    pending.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [{ index: 0, delta: {}, finish_reason: "tool_calls" }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      })}\n\n`,
    );
    pending.response.write("data: [DONE]\n\n");
    pending.response.end();
  }

  fail(count: number): void {
    const pending = this.requests[count - 1];
    if (!pending) throw new Error(`provider request ${count} has not arrived`);
    pending.response.writeHead(500, { "content-type": "application/json" });
    pending.response.end(
      JSON.stringify({
        error: {
          message: "controlled provider failure",
          type: "server_error",
        },
      }),
    );
  }

  async stop(): Promise<void> {
    for (let index = 0; index < this.requests.length; index += 1) {
      const response = this.requests[index]?.response;
      if (response && !response.writableEnded && !response.destroyed) this.respond(index + 1, "cleanup");
    }
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  private resolveWaiters(): void {
    this.waiters = this.waiters.filter((waiter) => {
      const request = this.requests[waiter.count - 1];
      if (!request) return true;
      waiter.resolve(request);
      return false;
    });
  }
}

function protectedHeaders(): Record<string, string> {
  return {
    authorization: `Bearer ${internalToken}`,
    "content-type": "application/json",
  };
}

async function appendMailbox(baseUrl: string, sessionId: string, content: string) {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
    method: "POST",
    headers: protectedHeaders(),
    body: JSON.stringify({ content }),
  });
  return {
    status: response.status,
    body: await response.text(),
  };
}

async function writeProfile(dataRoot: string, providerBaseUrl = "http://127.0.0.1:9/v1", limits?: { context_window_tokens: number; max_output_tokens: number }): Promise<void> {
  await fs.mkdir(path.join(dataRoot, "profiles"), { recursive: true });
  await fs.writeFile(
    path.join(dataRoot, "profiles", "fixture.json"),
    `${JSON.stringify({
      provider: "openai",
      billing: "usage",
      base_url: providerBaseUrl,
      auth: { type: "api_key", key: "sk-test" },
      models: [
        {
          id: "fixture-model",
          api: "openai-completions",
          streaming: true,
          thinking: ["off"],
          default_thinking: "off",
          capabilities: { input: ["text"] },
          ...(limits ? { limits } : {}),
          default: true,
        },
      ],
    })}\n`,
  );
}

async function createSession(baseUrl: string, workspace: string): Promise<string> {
  await fs.mkdir(workspace, { recursive: true });
  const response = await fetch(`${baseUrl}/v1/sessions`, {
    method: "POST",
    headers: protectedHeaders(),
    body: JSON.stringify({ profile_id: "fixture", model: "fixture-model", thinking: "off", workspace }),
  });
  expect(response.status).toBe(201);
  return String(((await response.json()) as { session_id?: string }).session_id);
}

async function readMessages(baseUrl: string, sessionId: string): Promise<Array<{ type?: string; role?: string; content?: string }>> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/messages?limit=200`, {
    headers: protectedHeaders(),
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

async function readStatus(baseUrl: string, sessionId: string): Promise<string> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}`, {
    headers: protectedHeaders(),
  });
  if (!response.ok) return "";
  return String(((await response.json()) as { status?: string }).status || "");
}

async function readEventTypes(dataRoot: string, sessionId: string): Promise<string[]> {
  return (await readDomainRecords(dataRoot, sessionId)).map((record) => String(record.event?.type || ""));
}

type DomainRecord = {
  kind?: string;
  batch_index?: number;
  batch_size?: number;
  event?: {
    type?: string;
    message?: { role?: string; content?: string; is_error?: boolean; tool_call_id?: string };
  };
};

async function readDomainRecords(dataRoot: string, sessionId: string): Promise<DomainRecord[]> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const current = (await fs.readdir(segmentDir)).find((name) => name.endsWith(".jsonl"));
  if (!current) throw new Error("current session segment is missing");
  const raw = await fs.readFile(path.join(segmentDir, current), "utf8");
  return raw
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as DomainRecord)
    .filter((record) => record.kind === "domain");
}

function userContents(request: ProviderRequest): string[] {
  return (request.body.messages ?? []).filter((message) => message.role === "user").map((message) => String(message.content));
}

function nextUlid(value: string): string {
  const alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
  const digits = [...value];
  for (let index = digits.length - 1; index >= 0; index -= 1) {
    const next = alphabet.indexOf(digits[index] ?? "") + 1;
    if (next < alphabet.length) {
      digits[index] = alphabet[next] ?? "";
      return digits.join("");
    }
    digits[index] = alphabet[0] ?? "0";
  }
  throw new Error("ULID overflow");
}

async function appendPendingToolBatch(dataRoot: string, sessionId: string, label: string, toolCalls: Array<{ tool_call_id: string; tool_name: string; arguments: Record<string, unknown> }>): Promise<void> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const segmentName = (await fs.readdir(segmentDir)).find((name) => name.endsWith(".jsonl"));
  if (!segmentName) throw new Error(`${label} session segment is missing`);
  const segmentPath = path.join(segmentDir, segmentName);
  const records = (await fs.readFile(segmentPath, "utf8"))
    .trim()
    .split("\n")
    .map(
      (line) =>
        JSON.parse(line) as {
          event_id: string;
          stream_version?: number;
          event_schema_version?: number;
          kind?: string;
        },
    );
  const latest = records.at(-1);
  if (!latest?.event_id || latest.kind !== "domain" || latest.stream_version === undefined || latest.event_schema_version === undefined) {
    throw new Error(`${label} session has no latest domain event`);
  }
  const activationEventId = nextUlid(latest.event_id);
  const assistantEventId = nextUlid(activationEventId);
  const selection = { profile_id: "fixture", model: "fixture-model", thinking: "off" };
  const injected = [
    {
      kind: "domain",
      event_id: activationEventId,
      stream_id: sessionId,
      stream_version: latest.stream_version + 1,
      event_schema_version: latest.event_schema_version,
      batch_index: 0,
      batch_size: 1,
      event: {
        type: "activation_started",
        activation_id: activationEventId,
        selection,
        started_at_ms: 2,
      },
    },
    {
      kind: "domain",
      event_id: assistantEventId,
      stream_id: sessionId,
      stream_version: latest.stream_version + 2,
      event_schema_version: latest.event_schema_version,
      batch_index: 0,
      batch_size: 1,
      event: {
        type: "message_appended",
        message: {
          message_id: assistantEventId,
          role: "assistant",
          content: "",
          is_error: false,
          tool_call_id: null,
          tool_calls: toolCalls,
          provider_context: null,
          source_mailbox_seq: null,
        },
        wake_wait: false,
      },
    },
  ];
  await fs.appendFile(segmentPath, `${injected.map((record) => JSON.stringify(record)).join("\n")}\n`);
}

async function appendDurableToolResult(dataRoot: string, sessionId: string, toolCallId: string, content: string): Promise<void> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const segmentName = (await fs.readdir(segmentDir)).find((name) => name.endsWith(".jsonl"));
  if (!segmentName) throw new Error("session segment is missing");
  const segmentPath = path.join(segmentDir, segmentName);
  const latest = JSON.parse((await fs.readFile(segmentPath, "utf8")).trim().split("\n").at(-1) ?? "") as {
    event_id?: string;
    stream_version?: number;
    event_schema_version?: number;
  };
  if (!latest.event_id || latest.stream_version === undefined || latest.event_schema_version === undefined) {
    throw new Error("session has no latest domain event");
  }
  const eventId = nextUlid(latest.event_id);
  const record = {
    kind: "domain",
    event_id: eventId,
    stream_id: sessionId,
    stream_version: latest.stream_version + 1,
    event_schema_version: latest.event_schema_version,
    batch_index: 0,
    batch_size: 1,
    event: {
      type: "message_appended",
      message: {
        message_id: eventId,
        role: "tool",
        content,
        is_error: false,
        tool_call_id: toolCallId,
        tool_calls: [],
        provider_context: null,
        source_mailbox_seq: null,
      },
      wake_wait: false,
    },
  };
  await fs.appendFile(segmentPath, `${JSON.stringify(record)}\n`);
}

describe.sequential("zork-agent mailbox", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) await cleanups.pop()?.();
  });

  it("continues assistant-only model rounds until the model explicitly calls end", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-explicit-end-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "explicit-end Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await appendMailbox(baseUrl, sessionId, "finish only by calling end");

    const firstRequest = await provider.waitForRequest(1);
    expect(firstRequest.body.tools?.find((tool) => tool.function?.name === "end")?.function?.parameters).toEqual({
      type: "object",
      properties: {},
      additionalProperties: false,
    });
    provider.respond(1, "I am not finished until I call end.");

    const secondRequest = await provider.waitForRequest(2);
    expect(secondRequest.body.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          role: "assistant",
          content: "I am not finished until I call end.",
        }),
      ]),
    );
    expect(await readStatus(baseUrl, sessionId)).toBe("working");
    provider.respond(2, "");

    const thirdRequest = await provider.waitForRequest(3);
    expect(thirdRequest.body.messages).toEqual(expect.arrayContaining([expect.objectContaining({ role: "assistant", content: "" })]));
    expect(await readStatus(baseUrl, sessionId)).toBe("working");

    provider.respondWithToolCall(3, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "activation completion after explicit end",
    );
    const records = await readDomainRecords(dataRoot, sessionId);
    const eventTypes = records.map((record) => record.event?.type);
    expect(eventTypes.filter((type) => type === "activation_finished")).toHaveLength(1);
    const finishIndex = records.findIndex((record) => record.event?.type === "activation_finished");
    expect(records[finishIndex - 1]).toMatchObject({
      batch_index: 0,
      batch_size: 2,
      event: {
        type: "message_appended",
        message: { role: "tool", content: "end accepted", tool_call_id: "call-end-3-0" },
      },
    });
    expect(records[finishIndex]).toMatchObject({ batch_index: 1, batch_size: 2 });
  });

  it("rejects end when it shares a model response with another tool call", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-exclusive-end-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "exclusive-end Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await appendMailbox(baseUrl, sessionId, "do not accept a batched end");
    await provider.waitForRequest(1);
    provider.respondWithToolCalls(1, [
      { name: "wait_for", args: {} },
      { name: "end", args: {} },
    ]);

    const followup = await provider.waitForRequest(2);
    expect(followup.body.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ role: "tool", content: "Invalid arguments for tool wait_for" }),
        expect.objectContaining({
          role: "tool",
          content: "end must be the only tool call in its model response",
        }),
      ]),
    );
    expect(await readStatus(baseUrl, sessionId)).toBe("working");

    provider.respondWithToolCall(2, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "activation completion after a later exclusive end",
    );
  });

  it("resumes an assistant-only activation after restart and still requires end", { timeout: 45_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-explicit-end-restart-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "explicit-end restart Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await appendMailbox(baseUrl, sessionId, "continue this activation across restart");
    await provider.waitForRequest(1);
    provider.respond(1, "durable assistant before restart");
    await provider.waitForRequest(2);

    await stopChild(firstAgent);
    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "restarted explicit-end Agent readyz");

    const resumedRequest = await provider.waitForRequest(3);
    expect(resumedRequest.body.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          role: "assistant",
          content: "durable assistant before restart",
        }),
      ]),
    );
    expect(await readStatus(baseUrl, sessionId)).toBe("working");
    provider.respondWithToolCall(3, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after assistant-only restart recovery",
    );
  });

  it("reports a pending end as interrupted and keeps the activation running", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-pending-end-recovery-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "pending-end setup Agent readyz");
    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await stopChild(firstAgent);
    await appendPendingToolBatch(dataRoot, sessionId, "pending-end", [{ tool_call_id: "pending-end-call", tool_name: "end", arguments: {} }]);

    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "pending-end recovery Agent readyz");
    const resumed = await provider.waitForRequest(1);
    expect(resumed.body.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          role: "tool",
          content: "Agent runtime was interrupted before a durable result was recorded. This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover.",
        }),
      ]),
    );
    expect(await readStatus(baseUrl, sessionId)).toBe("working");

    const recovered = await readDomainRecords(dataRoot, sessionId);
    expect(recovered.some((record) => record.event?.type === "activation_finished")).toBe(false);
    expect(recovered.find((record) => record.event?.message?.tool_call_id === "pending-end-call")).toMatchObject({
      batch_index: 0,
      batch_size: 1,
      event: {
        type: "message_appended",
        message: {
          role: "tool",
          is_error: true,
          tool_call_id: "pending-end-call",
        },
      },
    });
    provider.respondWithToolCall(1, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after interrupted pending end",
    );
  });

  it("reports a pending wait_for as interrupted without creating a wait", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-pending-wait-recovery-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "pending-wait setup Agent readyz");
    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await stopChild(firstAgent);
    await appendPendingToolBatch(dataRoot, sessionId, "pending-wait", [
      {
        tool_call_id: "pending-wait-call",
        tool_name: "wait_for",
        arguments: { reason: "wait for a callback", timeout_seconds: 60 },
      },
    ]);

    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "pending-wait recovery Agent readyz");
    const resumed = await provider.waitForRequest(1);
    expect(resumed.body.messages).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          role: "tool",
          content: "Agent runtime was interrupted before a durable result was recorded. This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover.",
        }),
      ]),
    );
    expect(await readStatus(baseUrl, sessionId)).toBe("working");
    const eventTypes = await readEventTypes(dataRoot, sessionId);
    expect(eventTypes).not.toContain("wait_set");
    expect(eventTypes).not.toContain("wait_timer_scheduled");

    provider.respondWithToolCall(1, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after interrupted pending wait",
    );
  });

  it("atomically interrupts every unpaired tool call without replaying any effect", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-tool-batch-recovery-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "tool recovery setup Agent readyz");
    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await stopChild(firstAgent);
    const workspace = path.join(tempRoot, "workspace");
    await appendPendingToolBatch(dataRoot, sessionId, "pending-writes", [
      {
        tool_call_id: "pending-write-a",
        tool_name: "write",
        arguments: { path: "must-not-exist-a.txt", content: "a" },
      },
      {
        tool_call_id: "pending-write-b",
        tool_name: "write",
        arguments: { path: "must-not-exist-b.txt", content: "b" },
      },
    ]);

    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "tool batch recovery Agent readyz");
    const resumed = await provider.waitForRequest(1);
    const interrupted = (resumed.body.messages ?? []).filter(
      (message) => message.role === "tool" && message.content === "Agent runtime was interrupted before a durable result was recorded. This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover.",
    );
    expect(interrupted).toHaveLength(2);
    await expect(fs.stat(path.join(workspace, "must-not-exist-a.txt"))).rejects.toMatchObject({
      code: "ENOENT",
    });
    await expect(fs.stat(path.join(workspace, "must-not-exist-b.txt"))).rejects.toMatchObject({
      code: "ENOENT",
    });

    const recovered = (await readDomainRecords(dataRoot, sessionId)).filter((record) => record.event?.message?.tool_call_id?.startsWith("pending-write-"));
    expect(recovered).toHaveLength(2);
    expect(recovered.map((record) => record.batch_index)).toEqual([0, 1]);
    expect(recovered.map((record) => record.batch_size)).toEqual([2, 2]);
    expect(recovered.map((record) => record.event?.message?.is_error)).toEqual([true, true]);

    provider.respondWithToolCall(1, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after batch recovery",
    );
  });

  it("keeps committed tool results and interrupts only the remaining calls", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mixed-tool-recovery-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "mixed recovery setup Agent readyz");
    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    await stopChild(firstAgent);
    const workspace = path.join(tempRoot, "workspace");
    await appendPendingToolBatch(dataRoot, sessionId, "mixed-results", [
      {
        tool_call_id: "committed-write",
        tool_name: "write",
        arguments: { path: "already-recorded.txt", content: "recorded" },
      },
      {
        tool_call_id: "unpaired-write",
        tool_name: "write",
        arguments: { path: "must-not-be-replayed.txt", content: "unknown" },
      },
    ]);
    await appendDurableToolResult(dataRoot, sessionId, "committed-write", "already durable");

    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "mixed tool recovery Agent readyz");
    const resumed = await provider.waitForRequest(1);
    const toolMessages = (resumed.body.messages ?? []).filter((message) => message.role === "tool");
    expect(toolMessages.map((message) => message.content)).toEqual(["already durable", "Agent runtime was interrupted before a durable result was recorded. This tool call may have completed, partially completed, or not started. Inspect the current state before deciding whether or how to recover."]);
    await expect(fs.stat(path.join(workspace, "must-not-be-replayed.txt"))).rejects.toMatchObject({ code: "ENOENT" });
    expect((await readDomainRecords(dataRoot, sessionId)).filter((record) => record.event?.message?.tool_call_id === "committed-write")).toHaveLength(1);

    provider.respondWithToolCall(1, "end", {});
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after mixed tool recovery",
    );
  });

  it("persists one ordered mailbox and drains every available message before each model request", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "mailbox agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));

    const first = await appendMailbox(baseUrl, sessionId, "first");
    expect(first).toEqual({ status: 202, body: "" });
    const firstRequest = await provider.waitForRequest(1);
    expect(userContents(firstRequest)).toEqual(["first"]);

    for (const content of ["second", "third", "fourth", "runtime notice", "third"]) {
      expect(await appendMailbox(baseUrl, sessionId, content)).toEqual({ status: 202, body: "" });
    }

    provider.respond(1, "first answer");
    const secondRequest = await provider.waitForRequest(2);
    expect(userContents(secondRequest)).toEqual(["first", "second", "third", "fourth", "runtime notice", "third"]);
    provider.respond(2, "second answer");
    await provider.waitForRequest(3);
    provider.respondWithToolCall(3, "end", {});
  });

  it("drains mailbox input before the provider invocation after a failed attempt", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-retry-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl);

    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "mailbox retry agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));

    await appendMailbox(baseUrl, sessionId, "before failure");
    await provider.waitForRequest(1);
    await appendMailbox(baseUrl, sessionId, "arrived during failed request");
    provider.fail(1);

    const requestAfterFailure = await provider.waitForRequest(2);
    expect(userContents(requestAfterFailure)).toEqual(["before failure", "arrived during failed request"]);
    provider.respond(2, "answer after refreshed context");
    await provider.waitForRequest(3);
    provider.respondWithToolCall(3, "end", {});
  });

  it("returns from an abandoned context handoff to drain mailbox before rebuilding", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-handoff-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl, {
      context_window_tokens: 40_000,
      max_output_tokens: 4_096,
    });

    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "mailbox handoff agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));

    await appendMailbox(baseUrl, sessionId, `large context ${"x".repeat(20 * 1024)}`);
    const firstHandoff = await provider.waitForRequest(1);
    expect(JSON.stringify(firstHandoff.body.messages)).toContain("large context");

    await appendMailbox(baseUrl, sessionId, "arrived while handoff was in flight");
    provider.fail(1);

    const rebuiltHandoff = await provider.waitForRequest(2);
    expect(JSON.stringify(rebuiltHandoff.body.messages)).toContain("arrived while handoff was in flight");
    provider.respond(
      2,
      JSON.stringify({
        schema: "zork.context-handoff-document.v1",
        document: "Continue after the mailbox-aware handoff.",
      }),
    );

    await provider.waitForRequest(3);
    provider.respond(3, "continued after handoff");
    await provider.waitForRequest(4);
    provider.respondWithToolCall(4, "end", {});
    const messages = await waitFor(
      () => readMessages(baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content === "continued after handoff"),
      "mailbox-aware handoff completion",
    );
    expect(messages.filter((item) => item.role === "user").map((item) => item.content)).toEqual([expect.stringContaining("large context"), "arrived while handoff was in flight"]);
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after mailbox-aware handoff",
    );
  });

  it("resumes a durable context handoff plan after an in-flight restart", { timeout: 45_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-handoff-restart-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl, {
      context_window_tokens: 40_000,
      max_output_tokens: 4_096,
    });

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "handoff restart Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));

    await appendMailbox(baseUrl, sessionId, `restart context ${"x".repeat(20 * 1024)}`);
    const interruptedHandoff = await provider.waitForRequest(1);
    expect(JSON.stringify(interruptedHandoff.body.messages)).toContain("restart context");

    await stopChild(firstAgent);
    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "restarted handoff Agent readyz");

    await waitFor(
      () => provider.requests.length,
      (count) => count >= 2,
      "provider request resumed from the durable handoff plan",
    );
    const resumedHandoff = provider.requests[1];
    expect(JSON.stringify(resumedHandoff?.body.messages)).toContain("restart context");
    provider.respond(
      2,
      JSON.stringify({
        schema: "zork.context-handoff-document.v1",
        document: "Continue after recovering the durable handoff plan.",
      }),
    );

    await provider.waitForRequest(3);
    provider.respond(3, "continued after restart");
    await provider.waitForRequest(4);
    provider.respondWithToolCall(4, "end", {});
    const recovered = await waitFor(
      () => readMessages(baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content === "continued after restart"),
      "context handoff completion after restart",
    );
    expect(recovered.filter((item) => item.role === "user")).toHaveLength(1);
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after recovered handoff",
    );
  });

  it("rebuilds a context handoff after mailbox input precedes restart recovery", { timeout: 45_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-handoff-restart-input-"));
    cleanups.push(async () => removeTempRoot(tempRoot));

    const provider = new ControlledOpenAI();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot, providerBaseUrl, {
      context_window_tokens: 40_000,
      max_output_tokens: 4_096,
    });

    const firstAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "handoff restart-with-input Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));

    await appendMailbox(baseUrl, sessionId, `restart input context ${"x".repeat(20 * 1024)}`);
    const interruptedHandoff = await provider.waitForRequest(1);
    expect(JSON.stringify(interruptedHandoff.body.messages)).toContain("restart input context");
    await appendMailbox(baseUrl, sessionId, "mailbox input persisted before restart");

    await stopChild(firstAgent);
    const restartedAgent = spawnAgent(dataRoot, false, undefined, internalToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${baseUrl}/readyz`, "restarted handoff-with-input Agent readyz");

    const rebuiltHandoff = await provider.waitForRequest(2);
    expect(JSON.stringify(rebuiltHandoff.body.messages)).toContain("mailbox input persisted before restart");
    provider.respond(
      2,
      JSON.stringify({
        schema: "zork.context-handoff-document.v1",
        document: "Continue after rebuilding from recovered mailbox input.",
      }),
    );

    await provider.waitForRequest(3);
    provider.respond(3, "continued after restart and mailbox drain");
    await provider.waitForRequest(4);
    provider.respondWithToolCall(4, "end", {});
    const recovered = await waitFor(
      () => readMessages(baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content === "continued after restart and mailbox drain"),
      "context handoff completion after restart with mailbox input",
    );
    expect(recovered.filter((item) => item.role === "user").map((item) => item.content)).toEqual([expect.stringContaining("restart input context"), "mailbox input persisted before restart"]);
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after rebuilt handoff",
    );
  });

  it("drains messages received during a tool effect before the following model request", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-tool-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot);

    const agent = spawnAgent(dataRoot, true, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "tool mailbox Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    const workspace = path.join(tempRoot, "workspace");

    await appendMailbox(
      baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "bash",
          input: {
            command: "touch tool-started; while [ ! -f tool-release ]; do sleep 0.02; done; printf tool-finished",
          },
        },
      }),
    );
    await waitFor(
      () =>
        fs
          .stat(path.join(workspace, "tool-started"))
          .then(() => true)
          .catch(() => false),
      Boolean,
      "tool effect start",
    );

    expect(await appendMailbox(baseUrl, sessionId, "during tool 1")).toEqual({
      status: 202,
      body: "",
    });
    expect(await appendMailbox(baseUrl, sessionId, "during tool 2")).toEqual({
      status: 202,
      body: "",
    });
    await fs.writeFile(path.join(workspace, "tool-release"), "release");

    const messages = await waitFor(
      () => readMessages(baseUrl, sessionId),
      (items) => items.some((item) => item.role === "assistant" && item.content === "during tool 2"),
      "model request after tool mailbox drain",
    );
    expect(messages.some((message) => message.role === "tool" && message.content?.includes("tool-finished"))).toBe(true);
    expect(messages.filter((message) => message.role === "user").map((message) => message.content)).toEqual([expect.stringContaining("fake_tool"), "during tool 1", "during tool 2"]);
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "explicit end after tool mailbox drain",
    );
  });

  it("keeps a long tool in the active session until its terminal Tool result", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-mailbox-long-tool-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const dataRoot = path.join(tempRoot, "data");
    const agentPort = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${agentPort}` } });
    await writeProfile(dataRoot);

    const agent = spawnAgent(dataRoot, true, undefined, internalToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${baseUrl}/readyz`, "long tool Agent readyz");

    const sessionId = await createSession(baseUrl, path.join(tempRoot, "workspace"));
    const workspace = path.join(tempRoot, "workspace");

    await appendMailbox(
      baseUrl,
      sessionId,
      JSON.stringify({
        fake_tool: {
          name: "bash",
          input: {
            command: "touch long-tool-started; while [ ! -f long-tool-release ]; do sleep 0.02; done; printf long-tool-finished",
          },
        },
      }),
    );
    await waitFor(
      () =>
        fs
          .stat(path.join(workspace, "long-tool-started"))
          .then(() => true)
          .catch(() => false),
      Boolean,
      "long tool start",
    );

    expect(await readStatus(baseUrl, sessionId)).toBe("working");
    const runningTypes = await readEventTypes(dataRoot, sessionId);
    expect(runningTypes).toContain("activation_started");
    expect(runningTypes).not.toContain("activation_finished");
    expect(runningTypes.some((type) => type.startsWith("async_tool_call_"))).toBe(false);
    expect((await readMessages(baseUrl, sessionId)).some((message) => message.role === "tool")).toBe(false);

    await fs.writeFile(path.join(workspace, "long-tool-release"), "release");
    const completed = await waitFor(
      () => readMessages(baseUrl, sessionId),
      (items) => items.some((message) => message.role === "tool" && message.content === "long-tool-finished"),
      "durable long-tool terminal result",
    );
    expect(completed.some((message) => message.content === "async_running")).toBe(false);
    expect(completed.find((message) => message.content === "long-tool-finished")).toEqual({
      type: "message",
      role: "tool",
      content: "long-tool-finished",
    });
    await waitFor(
      () => readStatus(baseUrl, sessionId),
      (status) => status === "wait",
      "activation reaches durable wait after the tool follow-up",
    );
  });
});
