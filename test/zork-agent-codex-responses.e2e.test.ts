import type { ChildProcess } from "node:child_process";
import fs from "node:fs/promises";
import http, { type IncomingMessage, type ServerResponse } from "node:http";
import net, { type Socket } from "node:net";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";
import { type WebSocket, WebSocketServer } from "ws";

import { getFreePort, removeTempRoot, spawnAgent, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";

const agentToken = "zork-agent-responses-token";

type CapturedCodexHttpRequest = {
  method: string;
  url: string;
  authorization: string;
  headers: IncomingMessage["headers"];
  body: Record<string, any>;
};

type CapturedCodexWebSocketRequest = {
  body: Record<string, any>;
  headers: IncomingMessage["headers"];
  socket: WebSocket;
};

class ControlledConnectProxy {
  readonly connectTargets: string[] = [];
  readonly connectRequests: string[] = [];
  readonly sockets = new Set<Socket>();
  readonly server = net.createServer((socket) => this.accept(socket));

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("CONNECT proxy did not bind");
    return `http://127.0.0.1:${address.port}`;
  }

  async stop(): Promise<void> {
    for (const socket of this.sockets) socket.destroy();
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  private accept(client: Socket): void {
    this.sockets.add(client);
    client.once("close", () => this.sockets.delete(client));
    let buffered = Buffer.alloc(0);
    const readConnect = (chunk: Buffer): void => {
      buffered = Buffer.concat([buffered, chunk]);
      const headerEnd = buffered.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      client.off("data", readConnect);
      const request = buffered.subarray(0, headerEnd).toString("utf8");
      this.connectRequests.push(request);
      const match = /^CONNECT ([^: ]+):(\d+) HTTP\/1\.[01]\r\n/.exec(`${request}\r\n`);
      if (!match) {
        client.end("HTTP/1.1 400 Bad Request\r\n\r\n");
        return;
      }
      const target = `${match[1]}:${match[2]}`;
      this.connectTargets.push(target);
      const upstream = net.connect(Number(match[2]), "127.0.0.1", () => {
        client.write("HTTP/1.1 200 Connection Established\r\n\r\n");
        const remainder = buffered.subarray(headerEnd + 4);
        if (remainder.length > 0) upstream.write(remainder);
        client.pipe(upstream).pipe(client);
      });
      this.sockets.add(upstream);
      upstream.once("close", () => this.sockets.delete(upstream));
      upstream.once("error", () => client.destroy());
    };
    client.on("data", readConnect);
  }
}

class ControlledCodexResponses {
  readonly httpRequests: CapturedCodexHttpRequest[] = [];
  readonly webSocketRequests: CapturedCodexWebSocketRequest[] = [];
  readonly connections: WebSocket[] = [];
  readonly server = http.createServer((request, response) => this.captureHttp(request, response));
  readonly webSocketServer = new WebSocketServer({ noServer: true });

  constructor() {
    this.server.on("upgrade", (request, socket, head) => {
      this.webSocketServer.handleUpgrade(request, socket, head, (webSocket) => {
        this.connections.push(webSocket);
        webSocket.on("message", (data) => {
          this.webSocketRequests.push({
            body: JSON.parse(data.toString()) as Record<string, any>,
            headers: request.headers,
            socket: webSocket,
          });
        });
      });
    });
  }

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("codex responses provider did not bind");
    return `http://127.0.0.1:${address.port}/backend-api/codex`;
  }

  async stop(): Promise<void> {
    for (const connection of this.connections) connection.terminate();
    await new Promise<void>((resolve) => this.webSocketServer.close(() => resolve()));
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  reasoningAndReadItems(prefix: string): Array<Record<string, any>> {
    return [
      {
        id: `rs_${prefix}`,
        type: "reasoning",
        status: "completed",
        encrypted_content: `encrypted-${prefix}`,
        summary: [{ type: "summary_text", text: "Inspect the file." }],
        provider_extension: { preserve: true },
      },
      {
        id: `fc_${prefix}`,
        type: "function_call",
        status: "completed",
        arguments: '{"path":"README.md"}',
        call_id: `call_read_${prefix}`,
        name: "read",
        namespace: "functions",
        provider_extension: "keep-this-field",
      },
    ];
  }

  respondWebSocketWithReasoningAndRead(index: number): void {
    this.respondWebSocket(index, "resp_ws_1", this.reasoningAndReadItems("ws_1"), 200, 60, 150, 50);
  }

  respondWebSocketWithReasoningAndParallelReads(index: number): void {
    const [reasoning, read] = this.reasoningAndReadItems("ws_1");
    this.respondWebSocket(
      index,
      "resp_ws_1",
      [
        reasoning,
        read,
        {
          id: "fc_ws_1_notes",
          type: "function_call",
          status: "completed",
          arguments: '{"path":"NOTES.md"}',
          call_id: "call_read_ws_1_notes",
          name: "read",
        },
      ],
      200,
      60,
      150,
      50,
    );
  }

  respondWebSocketWithLongBash(index: number): void {
    this.respondWebSocket(
      index,
      "resp_ws_long_bash",
      [
        {
          id: "fc_ws_long_bash",
          type: "function_call",
          status: "completed",
          arguments: JSON.stringify({
            command: "printf started > websocket-ping-tool-started; sleep 1; printf finished > websocket-ping-tool-finished; printf done",
            timeout: 5,
          }),
          call_id: "call_ws_long_bash",
          name: "bash",
          namespace: "functions",
        },
      ],
      200,
      60,
      150,
      50,
    );
  }

  respondWebSocketWithEnd(index: number): void {
    this.respondWebSocket(index, `resp_ws_end_${index}`, [this.endItem(`ws_${index}`)], 10, 2, 8, 0);
  }

  respondWebSocketWithEmpty(index: number): void {
    this.respondWebSocket(index, `resp_ws_empty_${index}`, [], 10, 0, 8, 0);
  }

  respondWebSocketWithText(index: number, responseId: string, text: string, inputTokens: number, outputTokens: number): void {
    this.respondWebSocket(
      index,
      responseId,
      [
        {
          id: `msg_${responseId}`,
          type: "message",
          status: "completed",
          role: "assistant",
          content: [{ type: "output_text", text, annotations: [] }],
        },
      ],
      inputTokens,
      outputTokens,
      Math.max(0, inputTokens - 100),
      0,
    );
  }

  respondWebSocketWithHandoff(index: number, responseId: string, document: string, inputTokens: number, outputTokens: number): void {
    this.respondWebSocket(
      index,
      responseId,
      [
        {
          id: `msg_${responseId}`,
          type: "message",
          status: "completed",
          role: "assistant",
          content: [{ type: "output_text", text: "•", annotations: [] }],
        },
        {
          id: `fc_${responseId}`,
          type: "function_call",
          status: "completed",
          arguments: JSON.stringify({ document }),
          call_id: `call_${responseId}`,
          name: "context_handoff",
          namespace: "functions",
        },
      ],
      inputTokens,
      outputTokens,
      Math.max(0, inputTokens - 100),
      0,
    );
  }

  respondWebSocketWithFailure(index: number): void {
    const request = this.webSocketRequests[index];
    if (!request) throw new Error(`Codex WebSocket request ${index + 1} has not arrived`);
    request.socket.send(
      JSON.stringify({
        type: "response.failed",
        response: {
          id: `resp_ws_failed_${index}`,
          status: "failed",
          error: { status: 500, code: "controlled_failure", message: "controlled provider failure" },
        },
      }),
    );
  }

  private captureHttp(request: IncomingMessage, response: ServerResponse): void {
    const chunks: Buffer[] = [];
    request.on("data", (chunk) => chunks.push(chunk as Buffer));
    request.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      this.httpRequests.push({
        method: request.method ?? "",
        url: request.url ?? "",
        authorization: String(request.headers.authorization ?? ""),
        headers: request.headers,
        body: raw ? (JSON.parse(raw) as Record<string, any>) : {},
      });
      response.writeHead(400, { "content-type": "application/json" });
      response.end(JSON.stringify({ detail: "Stream must be set to true" }));
    });
  }

  private respondWebSocket(index: number, responseId: string, output: Array<Record<string, any>>, inputTokens: number, outputTokens: number, cachedTokens: number, reasoningTokens: number): void {
    const request = this.webSocketRequests[index];
    if (!request) throw new Error(`Codex WebSocket request ${index + 1} has not arrived`);
    request.socket.send(
      JSON.stringify({
        type: "response.created",
        response: { id: responseId, model: "gpt-5.6-luna" },
      }),
    );
    for (const [outputIndex, item] of output.entries()) {
      request.socket.send(JSON.stringify({ type: "response.output_item.done", output_index: outputIndex, item }));
    }
    request.socket.send(
      JSON.stringify({
        type: "response.completed",
        response: this.completedResponse(responseId, output, inputTokens, outputTokens, cachedTokens, reasoningTokens),
      }),
    );
  }

  private completedResponse(responseId: string, output: Array<Record<string, any>>, inputTokens: number, outputTokens: number, cachedTokens: number, reasoningTokens: number): Record<string, any> {
    return {
      id: responseId,
      object: "response",
      model: "gpt-5.6-luna",
      status: "completed",
      output,
      usage: {
        input_tokens: inputTokens,
        input_tokens_details: { cached_tokens: cachedTokens },
        output_tokens: outputTokens,
        output_tokens_details: { reasoning_tokens: reasoningTokens },
      },
    };
  }

  private endItem(suffix: string): Record<string, any> {
    return {
      id: `fc_end_${suffix}`,
      type: "function_call",
      status: "completed",
      arguments: "{}",
      call_id: `call_end_${suffix}`,
      name: "end",
      namespace: "functions",
    };
  }
}

function headers(): Record<string, string> {
  return { authorization: `Bearer ${agentToken}`, "content-type": "application/json" };
}

async function testWorkspace(tempRoot: string): Promise<string> {
  const workspace = path.join(tempRoot, "workspace");
  await fs.mkdir(workspace, { recursive: true });
  return workspace;
}

async function writeOpenAiCodexProfile(dataRoot: string, baseUrl: string, streaming = true, serviceTier?: string, parallelToolCalls?: boolean, contextWindowTokens = 872_000, maxOutputTokens = 128_000): Promise<void> {
  await fs.mkdir(path.join(dataRoot, "profiles"), { recursive: true });
  await fs.writeFile(
    path.join(dataRoot, "profiles", "openai-subscription.json"),
    `${JSON.stringify({
      provider: "openai",
      billing: "subscription",
      base_url: baseUrl,
      auth: { type: "api_key", key: "test-openai-subscription-key" },
      models: [
        {
          id: "gpt-5.6-luna",
          api: "openai-codex-responses",
          streaming,
          ...(serviceTier === undefined ? {} : { service_tier: serviceTier }),
          ...(parallelToolCalls === undefined ? {} : { parallel_tool_calls: parallelToolCalls }),
          thinking: ["low", "max"],
          default_thinking: "max",
          capabilities: { input: ["text", "image"] },
          limits: { context_window_tokens: contextWindowTokens, max_output_tokens: maxOutputTokens },
          default: true,
        },
      ],
    })}\n`,
  );
}

async function domainEvents(dataRoot: string, sessionId: string): Promise<Array<Record<string, any>>> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const segmentNames = (await fs.readdir(segmentDir)).filter((name) => name.endsWith(".jsonl")).sort();
  const events: Array<Record<string, any>> = [];
  for (const segmentName of segmentNames) {
    const records = (await fs.readFile(path.join(segmentDir, segmentName), "utf8"))
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => JSON.parse(line) as Record<string, any>);
    events.push(...records.filter((record) => record.kind === "domain").map((record) => record.event));
  }
  return events;
}

async function domainEventRecords(dataRoot: string, sessionId: string): Promise<Array<Record<string, any>>> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const segmentNames = (await fs.readdir(segmentDir)).filter((name) => name.endsWith(".jsonl")).sort();
  const records: Array<Record<string, any>> = [];
  for (const segmentName of segmentNames) {
    records.push(
      ...(await fs.readFile(path.join(segmentDir, segmentName), "utf8"))
        .trim()
        .split("\n")
        .filter(Boolean)
        .map((line) => JSON.parse(line) as Record<string, any>)
        .filter((record) => record.kind === "domain"),
    );
  }
  return records;
}

describe.sequential("zork-agent OpenAI Codex Responses WebSocket transport", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) await cleanups.pop()?.();
  });

  it("rejects --no-streaming for Codex before issuing any provider request", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-http-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true, "priority");
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken, ["--no-streaming"]);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "Codex HTTP agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "You are the test coding agent.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "finish" }),
        })
      ).status,
    ).toBe(202);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex non-streaming rejection",
    );
    expect(provider.httpRequests).toHaveLength(0);
    expect(provider.webSocketRequests).toHaveLength(0);
    const failures = events.filter((event) => event.type === "model_attempt_failed_fact");
    expect(failures).toHaveLength(1);
    expect(failures[0]).toEqual(expect.objectContaining({ error_class: "invalid_selection", retryable: false }));
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("failed");
  });

  it("allows parallel tool calls and sends their outputs as one continuation suffix", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = new URL(await provider.start());
    cleanups.push(async () => provider.stop());
    const proxy = new ControlledConnectProxy();
    const proxyUrl = await proxy.start();
    cleanups.push(async () => proxy.stop());
    providerBaseUrl.hostname = "codex.invalid";

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl.toString(), true, "priority", false);
    const agent: ChildProcess = spawnAgent(
      dataRoot,
      false,
      {
        HTTP_PROXY: proxyUrl,
        http_proxy: proxyUrl,
        HTTPS_PROXY: proxyUrl,
        https_proxy: proxyUrl,
        NO_PROXY: "",
        no_proxy: "",
      },
      agentToken,
    );
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "Codex WebSocket agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "You are the WebSocket test agent.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    await fs.writeFile(path.join(workspace, "README.md"), "websocket continuation\n");
    await fs.writeFile(path.join(workspace, "NOTES.md"), "parallel tool call\n");
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "read README.md and NOTES.md, then finish" }),
        })
      ).status,
    ).toBe(202);

    const first = await waitFor(
      () => provider.webSocketRequests[0],
      (value) => value !== undefined,
      "first Codex WebSocket request",
    );
    expect(provider.httpRequests).toHaveLength(0);
    expect(provider.connections).toHaveLength(1);
    expect(proxy.connectTargets).toContain(`codex.invalid:${providerBaseUrl.port}`);
    expect(proxy.connectRequests.join("\n")).not.toContain("test-openai-subscription-key");
    expect(first.headers["openai-beta"]).toBe("responses_websockets=2026-02-06");
    expect(first.headers["x-openai-internal-codex-responses-lite"]).toBeUndefined();
    expect(first.headers["session-id"]).toBe(sessionId);
    expect(first.body.type).toBe("response.create");
    expect(first.body.service_tier).toBe("priority");
    expect(first.body.parallel_tool_calls).toBe(true);
    expect(first.body.previous_response_id).toBeUndefined();
    expect(first.body.instructions).toBeUndefined();
    expect(first.body.tools).toBeUndefined();
    expect(first.body.input[0]).toEqual(expect.objectContaining({ type: "additional_tools", role: "developer" }));
    expect(first.body.client_metadata).toBeUndefined();
    provider.respondWebSocketWithReasoningAndParallelReads(0);

    const second = await waitFor(
      () => provider.webSocketRequests[1],
      (value) => value !== undefined,
      "incremental Codex WebSocket request",
    );
    expect(provider.connections).toHaveLength(1);
    expect(second.socket).toBe(first.socket);
    expect(second.body.service_tier).toBe("priority");
    expect(second.body.parallel_tool_calls).toBe(true);
    expect(second.body.previous_response_id).toBe("resp_ws_1");
    expect(second.body.input).toHaveLength(2);
    expect(second.body.input.map((item: Record<string, any>) => item.call_id)).toEqual(["call_read_ws_1", "call_read_ws_1_notes"]);
    expect(JSON.stringify(second.body.input)).not.toContain("encrypted-ws_1");
    provider.respondWebSocketWithEnd(1);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex WebSocket activation completion",
    );
    const completions = events.filter((event) => event.type === "model_request_completed");
    expect(completions).toHaveLength(2);
    expect(completions[0]?.provider_input).toEqual({
      mode: "full",
      logical_input_items: first.body.input.length,
      sent_input_items: first.body.input.length,
      response_id: "resp_ws_1",
    });
    expect(completions[1]?.provider_input).toEqual({
      mode: "delta",
      logical_input_items: expect.any(Number),
      sent_input_items: second.body.input.length,
      previous_response_id: "resp_ws_1",
      response_id: "resp_ws_end_1",
    });
    expect(completions[1]?.provider_input.logical_input_items).toBeGreaterThan(completions[1]?.provider_input.sent_input_items);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
    await waitFor(
      () => first.socket.readyState,
      (readyState) => readyState === first.socket.CLOSED,
      "Codex WebSocket release at activation boundary",
    );
  });

  it("answers provider Ping while a tool is still executing", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-ping-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true, "priority", false);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "Codex WebSocket Ping agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "Run the requested tool, then finish.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "run the tool, then finish" }),
        })
      ).status,
    ).toBe(202);

    const first = await waitFor(
      () => provider.webSocketRequests[0],
      (value) => value !== undefined,
      "Codex request before long tool",
    );
    provider.respondWebSocketWithLongBash(0);
    await waitFor(
      async () =>
        fs
          .access(path.join(workspace, "websocket-ping-tool-started"))
          .then(() => true)
          .catch(() => false),
      Boolean,
      "long tool start marker",
    );

    const pingPayload = Buffer.from("ping-during-tool");
    const pongPayload = new Promise<Buffer>((resolve) => {
      first.socket.once("pong", (data) => resolve(Buffer.from(data)));
    });
    first.socket.ping(pingPayload);
    expect(await pongPayload).toEqual(pingPayload);
    await expect(fs.access(path.join(workspace, "websocket-ping-tool-finished"))).rejects.toThrow();

    const second = await waitFor(
      () => provider.webSocketRequests[1],
      (value) => value !== undefined,
      "Codex request after long tool",
    );
    expect(provider.connections).toHaveLength(1);
    expect(second.socket).toBe(first.socket);
    expect(second.body.previous_response_id).toBe("resp_ws_long_bash");
    expect(second.body.input).toEqual([expect.objectContaining({ type: "function_call_output", call_id: "call_ws_long_bash" })]);
    provider.respondWebSocketWithEnd(1);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex activation completion after Ping during tool",
    );
    expect(provider.httpRequests).toHaveLength(0);
    expect(events.filter((event) => event.type === "model_attempt_failed_fact")).toHaveLength(0);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("starts generation 2 with the durable handoff event ULID as the synthetic call id", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-handoff-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true, "priority", false, 20_000, 1_000);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "handoff Codex WebSocket agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "Keep working until you call end.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "continue the durable task" }),
        })
      ).status,
    ).toBe(202);

    const beforeHandoff = await waitFor(
      () => provider.webSocketRequests[0],
      (value) => value !== undefined,
      "Codex request before handoff",
    );
    expect(JSON.stringify(beforeHandoff.body.input[0])).toContain("context_handoff");
    provider.respondWebSocketWithText(0, "resp_before_handoff", "I will continue.", 18_900, 200);

    const handoffRequest = await waitFor(
      () => provider.webSocketRequests[1],
      (value) => value !== undefined,
      "Codex handoff request",
    );
    expect(JSON.stringify(handoffRequest.body.input)).toContain("context_handoff");
    expect(handoffRequest.body.tool_choice).toBe(beforeHandoff.body.tool_choice);
    expect(handoffRequest.body.input.at(-1)).toEqual(
      expect.objectContaining({
        role: "user",
        content: expect.arrayContaining([expect.objectContaining({ type: "input_text", text: expect.stringContaining("context_handoff") })]),
      }),
    );
    provider.respondWebSocketWithHandoff(1, "resp_handoff", "Long-term goal: finish the task. Current state: continue and call end when complete.", 19_300, 120);

    const generation2 = await waitFor(
      () => provider.webSocketRequests[2],
      (value) => value !== undefined,
      "first generation-2 Codex request",
    );
    expect(generation2.body.previous_response_id).toBeUndefined();
    const handoffCall = generation2.body.input.find((item: Record<string, any>) => item.type === "function_call" && item.name === "read_context_handoff");
    const handoffResult = generation2.body.input.find((item: Record<string, any>) => item.type === "function_call_output" && item.call_id === handoffCall?.call_id);
    const records = await domainEventRecords(dataRoot, sessionId);
    const handoffRecord = records.find((record) => record.event?.type === "context_handoff_created");
    expect(handoffRecord).toBeDefined();
    expect(handoffCall?.call_id).toBe(`call_${handoffRecord?.event_id}`);
    expect(String(handoffCall?.call_id).length).toBeLessThanOrEqual(64);
    expect(handoffResult).toEqual(
      expect.objectContaining({
        call_id: handoffCall?.call_id,
        output: expect.stringContaining("Long-term goal: finish the task"),
      }),
    );
    provider.respondWebSocketWithEnd(2);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex activation completion after handoff",
    );
    expect(events.filter((event) => event.type === "context_handoff_created")).toHaveLength(1);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("opens a new WebSocket and sends durable full context after agent restart", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-restart-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());
    const proxy = new ControlledConnectProxy();
    const proxyUrl = await proxy.start();
    cleanups.push(async () => proxy.stop());
    const proxyEnvironment = {
      HTTP_PROXY: proxyUrl,
      http_proxy: proxyUrl,
      HTTPS_PROXY: proxyUrl,
      https_proxy: proxyUrl,
      NO_PROXY: "127.0.0.1",
      no_proxy: "127.0.0.1",
    };

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true);
    const firstAgent: ChildProcess = spawnAgent(dataRoot, false, proxyEnvironment, agentToken);
    cleanups.push(async () => stopChild(firstAgent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "restart Codex WebSocket agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "You are the restart test agent.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    await fs.writeFile(path.join(workspace, "README.md"), "durable restart\n");
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "read README.md, then finish" }),
        })
      ).status,
    ).toBe(202);

    await waitFor(
      () => provider.webSocketRequests[0],
      (value) => value !== undefined,
      "pre-restart full WebSocket request",
    );
    expect(proxy.connectTargets).toHaveLength(0);
    provider.respondWebSocketWithReasoningAndRead(0);
    await waitFor(
      () => provider.webSocketRequests[1],
      (value) => value !== undefined,
      "pre-restart incremental WebSocket request",
    );
    await stopChild(firstAgent);

    const secondAgent: ChildProcess = spawnAgent(dataRoot, false, proxyEnvironment, agentToken);
    cleanups.push(async () => stopChild(secondAgent));
    await waitForReady(`${baseUrl}/readyz`, "restarted Codex WebSocket agent readyz");
    const restarted = await waitFor(
      () => provider.webSocketRequests[2],
      (value) => value !== undefined,
      "full WebSocket request after restart",
    );
    expect(proxy.connectTargets).toHaveLength(0);
    expect(provider.connections).toHaveLength(2);
    expect(restarted.socket).not.toBe(provider.webSocketRequests[1].socket);
    expect(restarted.body.previous_response_id).toBeUndefined();
    const expectedOutputItems = provider.reasoningAndReadItems("ws_1");
    const rawOutputIndex = restarted.body.input.findIndex((item: any) => item.id === "rs_ws_1");
    expect(restarted.body.input.slice(rawOutputIndex, rawOutputIndex + 2)).toEqual(expectedOutputItems);
    expect(restarted.body.input).toEqual(expect.arrayContaining([expect.objectContaining({ type: "function_call_output", call_id: "call_read_ws_1" })]));
    provider.respondWebSocketWithEnd(2);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "restarted Codex activation completion",
    );
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("keeps an empty completed response as a successful round without projecting an assistant item", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-empty-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "empty Codex WebSocket agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "You are the empty response test agent.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "finish only by calling end" }),
        })
      ).status,
    ).toBe(202);

    const first = await waitFor(
      () => provider.webSocketRequests[0],
      (value) => value !== undefined,
      "Codex request before empty completion",
    );
    expect(first.body.service_tier).toBeUndefined();
    provider.respondWebSocketWithEmpty(0);
    const afterEmpty = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "message_appended" && event.message?.role === "assistant" && event.message?.provider_context !== undefined),
      "durable empty Codex assistant",
    );
    const durableEmptyAssistant = afterEmpty.find((event) => event.type === "message_appended" && event.message?.role === "assistant" && event.message?.provider_context !== undefined);
    expect(durableEmptyAssistant?.message?.provider_context).toEqual({
      profile_id: "openai-subscription",
      provider: "openai",
      model: "gpt-5.6-luna",
      api: "openai-codex-responses",
      output_items: [],
    });
    const second = await waitFor(
      () => provider.webSocketRequests[1],
      (value) => value !== undefined,
      "Codex request after empty completion",
    );
    expect(second.socket).toBe(first.socket);
    expect(second.body.previous_response_id).toBe("resp_ws_empty_0");
    expect(second.body.input).toEqual([]);
    provider.respondWebSocketWithEnd(1);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex activation after empty completion",
    );
    expect(events.filter((event) => event.type === "model_attempt_failed")).toHaveLength(0);
    expect(events.filter((event) => event.type === "model_request_completed")).toHaveLength(2);
    const emptyAssistant = events.find((event) => event.type === "message_appended" && event.message?.role === "assistant" && event.message?.provider_context?.output_items?.length === 0);
    expect(emptyAssistant?.message?.content).toBe("");
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("maps each failed WebSocket response to exactly one durable Agent attempt without fallback", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-codex-ws-failed-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledCodexResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenAiCodexProfile(dataRoot, providerBaseUrl, true);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    let agentStderr = "";
    agent.stderr?.setEncoding("utf8");
    agent.stderr?.on("data", (chunk: string) => {
      agentStderr += chunk;
    });
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "failed Codex WebSocket agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "openai-subscription",
        model: "gpt-5.6-luna",
        thinking: "max",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "You are the provider failure test agent.",
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "exercise provider failure handling" }),
        })
      ).status,
    ).toBe(202);

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await waitFor(
        () => provider.webSocketRequests[attempt],
        (value) => value !== undefined,
        `Codex failed request ${attempt + 1}`,
      );
      provider.respondWebSocketWithFailure(attempt);
    }

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "Codex terminal provider failure",
    );
    expect(provider.httpRequests).toHaveLength(0);
    expect(provider.webSocketRequests).toHaveLength(3);
    expect(provider.connections).toHaveLength(3);
    expect(events.filter((event) => event.type === "model_attempt_started")).toHaveLength(3);
    const failedAttempts = events.filter((event) => event.type === "model_attempt_failed_fact");
    expect(failedAttempts).toHaveLength(3);
    for (const [index, failure] of failedAttempts.entries()) {
      expect(failure.provider_input).toEqual({
        mode: "full",
        logical_input_items: provider.webSocketRequests[index]?.body.input.length,
        sent_input_items: provider.webSocketRequests[index]?.body.input.length,
      });
    }
    expect(events.filter((event) => event.type === "model_attempt_failed")).toHaveLength(1);
    expect(events.filter((event) => event.type === "model_attempts_exhausted")).toHaveLength(1);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("failed");
    expect(agentStderr).toContain("model provider attempt failed");
    expect(agentStderr).toContain("codex.websocket.provider_event");
    expect(agentStderr).toContain("retryable=true");
    expect(agentStderr).toContain("status_code=Some(500)");
    expect(agentStderr).toContain("controlled_failure");
    expect(agentStderr).toContain("controlled provider failure");
    expect(agentStderr).not.toContain("test-openai-subscription-key");
  });
});
