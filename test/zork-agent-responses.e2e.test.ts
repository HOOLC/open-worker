/* eslint-disable max-lines -- the controlled provider keeps complete wire fixtures beside the protocol tests */

import type { ChildProcess } from "node:child_process";
import fs from "node:fs/promises";
import http, { type IncomingMessage, type ServerResponse } from "node:http";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { getFreePort, removeTempRoot, spawnAgent, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";

const agentToken = "zork-agent-responses-token";

type CapturedRequest = {
  method: string;
  url: string;
  authorization: string;
  body: Record<string, any>;
  response: ServerResponse;
};

class ControlledResponses {
  readonly requests: CapturedRequest[] = [];
  modelProbeCount = 0;
  readonly server = http.createServer((request, response) => this.capture(request, response));

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("responses provider did not bind");
    return `http://127.0.0.1:${address.port}/v1`;
  }

  async stop(): Promise<void> {
    for (const request of this.requests) {
      if (!request.response.writableEnded && !request.response.destroyed) request.response.destroy();
    }
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  private capture(request: IncomingMessage, response: ServerResponse): void {
    if (request.method === "GET" && request.url === "/v1/models") {
      this.modelProbeCount += 1;
      response.writeHead(200, { "content-type": "application/json" });
      response.end(
        JSON.stringify({
          object: "list",
          data: [{ id: "muse-spark-1.2-contributor", object: "model" }],
        }),
      );
      return;
    }
    const chunks: Buffer[] = [];
    request.on("data", (chunk) => chunks.push(chunk as Buffer));
    request.on("end", () => {
      const raw = Buffer.concat(chunks).toString("utf8");
      this.requests.push({
        method: request.method ?? "",
        url: request.url ?? "",
        authorization: String(request.headers.authorization ?? ""),
        body: raw ? (JSON.parse(raw) as Record<string, any>) : {},
        response,
      });
    });
  }

  respondWithText(index: number, text: string, inputTokens = 123, outputTokens = 45, cachedTokens = 100, reasoningTokens = 40): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const messageId = `msg_${index + 1}`;
    const responseId = `resp_${index + 1}`;
    const events = [
      {
        type: "response.created",
        response: { id: responseId, created_at: 1, model: "muse-spark-1.2-contributor" },
      },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: {
          id: messageId,
          type: "message",
          status: "in_progress",
          role: "assistant",
          content: [],
        },
      },
      { type: "response.output_text.delta", item_id: messageId, output_index: 0, delta: text },
      {
        type: "response.output_item.done",
        output_index: 0,
        item: {
          id: messageId,
          type: "message",
          status: "completed",
          role: "assistant",
          content: [{ type: "output_text", text, annotations: [] }],
        },
      },
      {
        type: "response.completed",
        response: {
          id: responseId,
          created_at: 1,
          model: "muse-spark-1.2-contributor",
          incomplete_details: null,
          usage: {
            input_tokens: inputTokens,
            input_tokens_details: { cached_tokens: cachedTokens },
            output_tokens: outputTokens,
            output_tokens_details: { reasoning_tokens: reasoningTokens },
          },
        },
      },
    ];
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.write("data: [DONE]\n\n");
    request.response.end();
  }

  respondWithToolCalls(index: number, calls: Array<{ name: string; arguments: Record<string, unknown> }>, text = "", inputTokens = 123, outputTokens = 45, cachedTokens = 100, reasoningTokens = 40): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const responseId = `resp_tools_${index + 1}`;
    const events: Array<Record<string, unknown>> = [
      {
        type: "response.created",
        response: { id: responseId, created_at: 1, model: "muse-spark-1.2-contributor" },
      },
    ];
    let outputIndex = 0;
    if (text) {
      const messageId = `msg_tools_${index + 1}`;
      events.push(
        {
          type: "response.output_item.added",
          output_index: outputIndex,
          item: {
            id: messageId,
            type: "message",
            status: "in_progress",
            role: "assistant",
            content: [],
          },
        },
        { type: "response.output_text.delta", item_id: messageId, output_index: outputIndex, delta: text },
        {
          type: "response.output_item.done",
          output_index: outputIndex,
          item: {
            id: messageId,
            type: "message",
            status: "completed",
            role: "assistant",
            content: [{ type: "output_text", text, annotations: [] }],
          },
        },
      );
      outputIndex += 1;
    }
    calls.forEach((call, callIndex) => {
      const itemId = `fc_tools_${index + 1}_${callIndex + 1}`;
      const callId = `call_tools_${index + 1}_${callIndex + 1}`;
      const argumentsJson = JSON.stringify(call.arguments);
      events.push(
        {
          type: "response.output_item.added",
          output_index: outputIndex,
          item: {
            id: itemId,
            type: "function_call",
            status: "in_progress",
            arguments: "",
            call_id: callId,
            name: call.name,
          },
        },
        {
          type: "response.function_call_arguments.delta",
          item_id: itemId,
          output_index: outputIndex,
          delta: argumentsJson,
        },
        {
          type: "response.function_call_arguments.done",
          item_id: itemId,
          output_index: outputIndex,
          arguments: argumentsJson,
        },
        {
          type: "response.output_item.done",
          output_index: outputIndex,
          item: {
            id: itemId,
            type: "function_call",
            status: "completed",
            arguments: argumentsJson,
            call_id: callId,
            name: call.name,
          },
        },
      );
      outputIndex += 1;
    });
    events.push({
      type: "response.completed",
      response: {
        id: responseId,
        created_at: 1,
        model: "muse-spark-1.2-contributor",
        incomplete_details: null,
        usage: {
          input_tokens: inputTokens,
          input_tokens_details: { cached_tokens: cachedTokens },
          output_tokens: outputTokens,
          output_tokens_details: { reasoning_tokens: reasoningTokens },
        },
      },
    });
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.write("data: [DONE]\n\n");
    request.response.end();
  }

  respondWithReasoningAndRead(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const events = [
      {
        type: "response.created",
        response: { id: "resp_tool_1", created_at: 1, model: "muse-spark-1.2-contributor" },
      },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: {
          id: "rs_1",
          type: "reasoning",
          status: "in_progress",
          summary: [],
        },
      },
      {
        type: "response.reasoning_summary_part.added",
        item_id: "rs_1",
        output_index: 0,
        summary_index: 0,
      },
      {
        type: "response.reasoning_summary_text.delta",
        item_id: "rs_1",
        output_index: 0,
        summary_index: 0,
        delta: "Inspect the file.",
      },
      {
        type: "response.reasoning_summary_part.done",
        item_id: "rs_1",
        output_index: 0,
        summary_index: 0,
      },
      {
        type: "response.output_item.done",
        output_index: 0,
        item: {
          id: "rs_1",
          type: "reasoning",
          status: "completed",
          encrypted_content: "encrypted-reasoning-1",
          summary: [{ type: "summary_text", text: "Inspect the file." }],
        },
      },
      {
        type: "response.output_item.added",
        output_index: 1,
        item: {
          id: "fc_1",
          type: "function_call",
          status: "in_progress",
          arguments: "",
          call_id: "call_read_1",
          name: "read",
        },
      },
      {
        type: "response.function_call_arguments.delta",
        item_id: "fc_1",
        output_index: 1,
        delta: '{"path":"README.md"}',
      },
      {
        type: "response.function_call_arguments.done",
        item_id: "fc_1",
        output_index: 1,
        arguments: '{"path":"README.md"}',
      },
      {
        type: "response.output_item.done",
        output_index: 1,
        item: {
          id: "fc_1",
          type: "function_call",
          status: "completed",
          arguments: '{"path":"README.md"}',
          call_id: "call_read_1",
          name: "read",
        },
      },
      {
        type: "response.completed",
        response: {
          id: "resp_tool_1",
          created_at: 1,
          model: "muse-spark-1.2-contributor",
          incomplete_details: null,
          usage: {
            input_tokens: 100,
            input_tokens_details: { cached_tokens: 0 },
            output_tokens: 50,
            output_tokens_details: { reasoning_tokens: 40 },
          },
        },
      },
    ];
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.write("data: [DONE]\n\n");
    request.response.end();
  }

  respondWithPlainReasoningAndRead(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const reasoning = {
      id: "rs_plain_1",
      type: "reasoning",
      status: null,
      summary: [],
      content: [{ type: "reasoning_text", text: "Inspect the file before answering.\n" }],
      encrypted_content: null,
    };
    const call = {
      id: "fc_plain_1",
      type: "function_call",
      status: "completed",
      arguments: '{"path":"README.md"}',
      call_id: "call_plain_read_1",
      name: "read",
    };
    const events = [
      {
        type: "response.created",
        response: { id: "resp_plain_1", created_at: 1, model: "muse-spark-1.2-contributor" },
      },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: { ...reasoning, status: "in_progress" },
      },
      { type: "response.output_item.done", output_index: 0, item: reasoning },
      {
        type: "response.output_item.added",
        output_index: 1,
        item: { ...call, status: "in_progress", arguments: "" },
      },
      {
        type: "response.function_call_arguments.delta",
        item_id: call.id,
        output_index: 1,
        delta: call.arguments,
      },
      {
        type: "response.function_call_arguments.done",
        item_id: call.id,
        output_index: 1,
        arguments: call.arguments,
      },
      { type: "response.output_item.done", output_index: 1, item: call },
      {
        type: "response.completed",
        response: {
          id: "resp_plain_1",
          created_at: 1,
          model: "muse-spark-1.2-contributor",
          incomplete_details: null,
          usage: {
            input_tokens: 100,
            input_tokens_details: { cached_tokens: 0 },
            output_tokens: 50,
            output_tokens_details: { reasoning_tokens: 40 },
          },
        },
      },
    ];
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.write("data: [DONE]\n\n");
    request.response.end();
  }

  respondWithEnd(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const responseId = `resp_end_${index + 1}`;
    const itemId = `fc_end_${index + 1}`;
    const callId = `call_end_${index + 1}`;
    const events = [
      {
        type: "response.created",
        response: { id: responseId, created_at: 1, model: "muse-spark-1.2-contributor" },
      },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: {
          id: itemId,
          type: "function_call",
          status: "in_progress",
          arguments: "",
          call_id: callId,
          name: "end",
        },
      },
      {
        type: "response.function_call_arguments.delta",
        item_id: itemId,
        output_index: 0,
        delta: "{}",
      },
      {
        type: "response.function_call_arguments.done",
        item_id: itemId,
        output_index: 0,
        arguments: "{}",
      },
      {
        type: "response.output_item.done",
        output_index: 0,
        item: {
          id: itemId,
          type: "function_call",
          status: "completed",
          arguments: "{}",
          call_id: callId,
          name: "end",
        },
      },
      {
        type: "response.completed",
        response: {
          id: responseId,
          created_at: 1,
          model: "muse-spark-1.2-contributor",
          incomplete_details: null,
          usage: {
            input_tokens: 1,
            input_tokens_details: { cached_tokens: 0 },
            output_tokens: 1,
            output_tokens_details: { reasoning_tokens: 0 },
          },
        },
      },
    ];
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.write("data: [DONE]\n\n");
    request.response.end();
  }

  respondNonStreamingWithReasoningAndRead(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    request.response.writeHead(200, { "content-type": "application/json" });
    request.response.end(
      JSON.stringify({
        id: "resp_non_streaming_tool_1",
        object: "response",
        created_at: 1,
        model: "muse-spark-1.2-contributor",
        status: "completed",
        incomplete_details: null,
        output: [
          {
            id: "rs_non_streaming_1",
            type: "reasoning",
            status: "completed",
            encrypted_content: "encrypted-non-streaming-reasoning-1",
            summary: [{ type: "summary_text", text: "Inspect the file." }],
          },
          {
            id: "fc_non_streaming_1",
            type: "function_call",
            status: "completed",
            arguments: '{"path":"README.md"}',
            call_id: "call_non_streaming_read_1",
            name: "read",
          },
        ],
        usage: {
          input_tokens: 200,
          input_tokens_details: { cached_tokens: 150 },
          output_tokens: 60,
          output_tokens_details: { reasoning_tokens: 50 },
        },
      }),
    );
  }

  respondNonStreamingWithEnd(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    request.response.writeHead(200, { "content-type": "application/json" });
    request.response.end(
      JSON.stringify({
        id: `resp_non_streaming_end_${index + 1}`,
        object: "response",
        created_at: 1,
        model: "muse-spark-1.2-contributor",
        status: "completed",
        incomplete_details: null,
        output: [
          {
            id: `fc_non_streaming_end_${index + 1}`,
            type: "function_call",
            status: "completed",
            arguments: "{}",
            call_id: `call_non_streaming_end_${index + 1}`,
            name: "end",
          },
        ],
        usage: {
          input_tokens: 10,
          input_tokens_details: { cached_tokens: 0 },
          output_tokens: 2,
          output_tokens_details: { reasoning_tokens: 0 },
        },
      }),
    );
  }

  respondWithReasoningThenEof(index: number): void {
    const request = this.requests[index];
    if (!request) throw new Error(`responses request ${index + 1} has not arrived`);
    const reasoningId = `rs_incomplete_${index + 1}`;
    const events = [
      {
        type: "response.created",
        response: {
          id: `resp_incomplete_${index + 1}`,
          created_at: 1,
          model: "muse-spark-1.2-contributor",
        },
      },
      {
        type: "response.output_item.added",
        output_index: 0,
        item: {
          id: reasoningId,
          type: "reasoning",
          status: "in_progress",
          summary: [],
        },
      },
      {
        type: "response.reasoning_summary_part.added",
        item_id: reasoningId,
        output_index: 0,
        summary_index: 0,
      },
      {
        type: "response.reasoning_summary_text.delta",
        item_id: reasoningId,
        output_index: 0,
        summary_index: 0,
        delta: "Still reasoning.",
      },
      {
        type: "response.reasoning_summary_part.done",
        item_id: reasoningId,
        output_index: 0,
        summary_index: 0,
      },
      {
        type: "response.output_item.done",
        output_index: 0,
        item: {
          id: reasoningId,
          type: "reasoning",
          status: "completed",
          encrypted_content: "not-durable",
          summary: [{ type: "summary_text", text: "Still reasoning." }],
        },
      },
    ];
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    for (const event of events) request.response.write(`data: ${JSON.stringify(event)}\n\n`);
    request.response.end();
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

async function writeOpenCodeProfile(dataRoot: string, baseUrl: string, streaming = true, parallelToolCalls = false): Promise<void> {
  await fs.mkdir(path.join(dataRoot, "profiles"), { recursive: true });
  await fs.writeFile(
    path.join(dataRoot, "profiles", "open-code-go.json"),
    `${JSON.stringify({
      provider: "opencode-go",
      billing: "subscription",
      base_url: baseUrl,
      auth: { type: "api_key", key: "test-open-code-go-key" },
      models: [
        {
          id: "muse-spark-1.2-contributor",
          api: "openai-responses",
          streaming,
          parallel_tool_calls: parallelToolCalls,
          thinking: ["xhigh"],
          default_thinking: "xhigh",
          capabilities: { input: ["text", "image"] },
          limits: { context_window_tokens: 1_048_576, max_output_tokens: 131_072 },
          default: true,
        },
      ],
    })}\n`,
  );
}

function openAiCompatibleProfile(baseUrl: string): Record<string, unknown> {
  return {
    provider: "openai-compatible",
    billing: "usage",
    base_url: baseUrl,
    headers: {},
    auth: { type: "api_key", key: "test-custom-key" },
    models: [
      {
        id: "/models/GT-NVFP4-5090",
        api: "openai-responses",
        streaming: true,
        parallel_tool_calls: false,
        thinking: ["low", "medium", "xhigh"],
        default_thinking: "xhigh",
        capabilities: { input: ["text"] },
        limits: { context_window_tokens: 256_000, max_output_tokens: 56_000 },
        default: true,
      },
    ],
  };
}

async function completedUsage(dataRoot: string, sessionId: string): Promise<Record<string, any> | undefined> {
  const segmentDir = path.join(dataRoot, "sessions", sessionId, "segments");
  const segment = (await fs.readdir(segmentDir)).find((name) => name.endsWith(".jsonl"));
  if (!segment) return undefined;
  const records = (await fs.readFile(path.join(segmentDir, segment), "utf8"))
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as Record<string, any>);
  return records.find((record) => record.kind === "domain" && record.event?.type === "model_request_completed")?.event?.usage;
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

describe.sequential("zork-agent OpenAI Responses transport", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) await cleanups.pop()?.();
  });

  it("uses a custom OpenAI-compatible profile without probing OpenAI or /models", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-openai-compatible-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "OpenAI-compatible agent readyz");

    const profileResponse = await fetch(`${baseUrl}/v1/profiles/qwen38`, {
      method: "PUT",
      headers: headers(),
      body: JSON.stringify(openAiCompatibleProfile(providerBaseUrl)),
    });
    expect(profileResponse.status).toBe(200);
    const profile = (await profileResponse.json()) as Record<string, any>;
    expect(profile.provider).toBe("openai-compatible");
    expect(profile.account).toEqual(
      expect.objectContaining({
        ok: true,
        account: expect.objectContaining({ type: "openai-compatible" }),
      }),
    );
    expect(profile.rateLimits).toEqual({ ok: false, error: "not_reported_by_provider" });
    expect(provider.modelProbeCount).toBe(0);

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "qwen38",
        model: "/models/GT-NVFP4-5090",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
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
    const request = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "custom Responses request",
    );
    expect(request.url).toBe("/v1/responses");
    expect(request.authorization).toBe("Bearer test-custom-key");
    expect(request.body.model).toBe("/models/GT-NVFP4-5090");
    expect(request.body.max_output_tokens).toBe(56_000);
    expect(provider.modelProbeCount).toBe(0);
    provider.respondWithEnd(0);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "custom Responses activation completion",
    );
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("records an early context_handoff call as a tool misuse without creating a handoff", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-handoff-misuse-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "handoff misuse agent readyz");

    expect(
      (
        await fetch(`${baseUrl}/v1/profiles/qwen38`, {
          method: "PUT",
          headers: headers(),
          body: JSON.stringify(openAiCompatibleProfile(providerBaseUrl)),
        })
      ).status,
    ).toBe(200);
    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "qwen38",
        model: "/models/GT-NVFP4-5090",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "continue working" }),
        })
      ).status,
    ).toBe(202);

    const ordinary = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "ordinary request exposing context_handoff",
    );
    expect(ordinary.body.tools).toEqual(expect.arrayContaining([expect.objectContaining({ name: "context_handoff" })]));
    provider.respondWithToolCalls(0, [
      {
        name: "context_handoff",
        arguments: { document: "premature document" },
      },
    ]);

    const afterMisuse = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "model follow-up after handoff misuse",
    );
    expect(JSON.stringify(afterMisuse.body.input)).toContain("context_handoff is only available when the runtime requests a context handoff");
    const eventsAfterMisuse = await domainEvents(dataRoot, sessionId);
    expect(eventsAfterMisuse.filter((event) => event.type === "context_handoff_created")).toHaveLength(0);
    expect(eventsAfterMisuse.some((event) => event.type === "message_appended" && event.message?.role === "tool" && event.message?.is_error === true && String(event.message?.content).includes("only available when the runtime requests"))).toBe(true);

    provider.respondWithEnd(1);
    const completed = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (events) => events.some((event) => event.type === "activation_finished"),
      "activation completion after observable handoff misuse",
    );
    expect(completed.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("rejects non-handoff tools and retries the exact cached handoff request", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-handoff-contract-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "OpenAI-compatible handoff agent readyz");

    const profile = openAiCompatibleProfile(providerBaseUrl) as Record<string, any>;
    profile.models[0].limits = { context_window_tokens: 30_000, max_output_tokens: 5_000 };
    expect(
      (
        await fetch(`${baseUrl}/v1/profiles/qwen38`, {
          method: "PUT",
          headers: headers(),
          body: JSON.stringify(profile),
        })
      ).status,
    ).toBe(200);

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "qwen38",
        model: "/models/GT-NVFP4-5090",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
        system_prompt: "While work remains, every response must include a tool call.",
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

    const ordinary = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "ordinary Responses request before handoff",
    );
    expect(ordinary.body.tool_choice).toBe("auto");
    expect(ordinary.body.tools).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          name: "context_handoff",
          parameters: {
            type: "object",
            properties: { document: expect.objectContaining({ type: "string" }) },
            required: ["document"],
            additionalProperties: false,
          },
        }),
      ]),
    );
    provider.respondWithText(0, "I will continue.", 24_900, 200, 24_800, 100);

    const handoff = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "OpenAI-compatible handoff request",
    );
    expect(handoff.body.tools).toEqual(ordinary.body.tools);
    expect(handoff.body.tool_choice).toBe(ordinary.body.tool_choice);
    expect(handoff.body.input.at(-1)).toEqual(
      expect.objectContaining({
        role: "user",
        content: expect.arrayContaining([expect.objectContaining({ type: "input_text", text: expect.stringContaining("context_handoff") })]),
      }),
    );
    expect(handoff.body.max_output_tokens).toBeLessThan(4_900);
    provider.respondWithToolCalls(
      1,
      [
        {
          name: "write",
          arguments: { path: "handoff-marker.txt", content: "tool result before handoff" },
        },
      ],
      "I need one more durable observation before the handoff.",
      25_300,
      120,
      25_200,
      40,
    );

    const retriedHandoff = await waitFor(
      () => provider.requests[2],
      (value) => value !== undefined,
      "retried handoff request after rejecting an ordinary tool call",
    );
    expect(retriedHandoff.body).toEqual(handoff.body);
    await expect(fs.readFile(path.join(tempRoot, "workspace", "handoff-marker.txt"), "utf8")).rejects.toMatchObject({ code: "ENOENT" });
    provider.respondWithToolCalls(
      2,
      [
        {
          name: "context_handoff",
          arguments: {
            document: "Long-term goal: finish the durable task. Current state: continue working.",
          },
        },
      ],
      "•",
      26_000,
      120,
      25_800,
      40,
    );

    const generation2 = await waitFor(
      () => provider.requests[3],
      (value) => value !== undefined,
      "first generation-2 Responses request",
    );
    expect(generation2.body.input).toEqual(expect.arrayContaining([expect.objectContaining({ type: "function_call", name: "read_context_handoff" }), expect.objectContaining({ type: "function_call_output", output: expect.stringContaining("Long-term goal") })]));
    provider.respondWithEnd(3);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "activation completion after OpenAI-compatible handoff",
    );
    const rejected = events.filter((event) => event.type === "context_handoff_rejected");
    expect(rejected).toHaveLength(1);
    expect(rejected[0]).toEqual(
      expect.objectContaining({
        assistant_content: "I need one more durable observation before the handoff.",
        tool_calls: [expect.objectContaining({ tool_name: "write" })],
      }),
    );
    expect(events.filter((event) => event.type === "context_handoff_created")).toHaveLength(1);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
  });

  it("uses /responses, sends xhigh, and durably records provider token usage", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl, true, true);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "responses agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "say OK" }),
        })
      ).status,
    ).toBe(202);

    const request = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "Responses request",
    );
    expect(request.method).toBe("POST");
    expect(request.url).toBe("/v1/responses");
    expect(request.authorization).toBe("Bearer test-open-code-go-key");
    expect(request.body.model).toBe("muse-spark-1.2-contributor");
    expect(request.body.stream).toBe(true);
    expect(request.body.reasoning).toEqual({ effort: "xhigh", summary: "detailed" });
    expect(request.body.store).toBe(false);
    expect(request.body.parallel_tool_calls).toBe(true);
    expect(request.body.include).toContain("reasoning.encrypted_content");
    const bashTool = request.body.tools.find((tool: any) => tool.name === "bash");
    expect(bashTool?.description).toContain("last 2000 lines or 50KB");
    expect(bashTool?.description).toContain("timeout in seconds");
    expect(bashTool?.parameters?.properties?.timeout?.description).toBe("Timeout in seconds (optional, no default timeout)");
    provider.respondWithText(0, "OK");

    const usage = await waitFor(
      () => completedUsage(dataRoot, sessionId),
      (value) => value !== undefined,
      "durable Responses usage",
    );
    expect(usage.input_tokens).toBe(123);
    expect(usage.output_tokens).toBe(45);
    await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "Responses end request",
    );
    provider.respondWithEnd(1);
  });

  it("uses complete Responses bodies for every profile when started with --no-streaming", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-non-streaming-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl, true);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken, ["--no-streaming"]);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "non-streaming Responses agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    await fs.writeFile(path.join(workspace, "README.md"), "hello from non-streaming\n");
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "read README.md, then finish" }),
        })
      ).status,
    ).toBe(202);

    const firstRequest = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "non-streaming Responses request",
    );
    expect(firstRequest.body.stream).toBeUndefined();
    expect(firstRequest.body.store).toBe(false);
    expect(firstRequest.body.include).toContain("reasoning.encrypted_content");
    provider.respondNonStreamingWithReasoningAndRead(0);

    const secondRequest = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "non-streaming Responses request after read",
    );
    expect(secondRequest.body.stream).toBeUndefined();
    expect(secondRequest.body.input.slice(1, 3)).toEqual([
      {
        id: "rs_non_streaming_1",
        type: "reasoning",
        status: "completed",
        encrypted_content: "encrypted-non-streaming-reasoning-1",
        summary: [{ type: "summary_text", text: "Inspect the file." }],
      },
      {
        id: "fc_non_streaming_1",
        type: "function_call",
        status: "completed",
        arguments: '{"path":"README.md"}',
        call_id: "call_non_streaming_read_1",
        name: "read",
      },
    ]);
    provider.respondNonStreamingWithEnd(1);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "non-streaming activation completion",
    );
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
    expect(events.filter((event) => event.type === "model_request_completed").map((event) => event.usage)).toEqual([expect.objectContaining({ input_tokens: 200, output_tokens: 60 }), expect.objectContaining({ input_tokens: 10, output_tokens: 2 })]);
  });

  it("keeps one non-streaming provider request alive for more than 30 seconds", { timeout: 50_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-slow-non-streaming-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl, true);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken, ["--no-streaming"]);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "slow non-streaming Responses agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "finish after thinking" }),
        })
      ).status,
    ).toBe(202);

    const request = await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "slow non-streaming Responses request",
    );
    expect(request.body.stream).toBeUndefined();
    await new Promise((resolve) => setTimeout(resolve, 32_000));
    expect(provider.requests).toHaveLength(1);
    provider.respondNonStreamingWithEnd(0);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "slow non-streaming activation completion",
    );
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");
    expect(provider.requests).toHaveLength(1);
  });

  it("keeps one provider request alive when the stream is silent for more than 30 seconds", { timeout: 45_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-silent-stream-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "silent-stream agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "say OK after thinking" }),
        })
      ).status,
    ).toBe(202);

    await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "silent Responses request",
    );
    await new Promise((resolve) => setTimeout(resolve, 31_000));
    expect(provider.requests).toHaveLength(1);
    provider.respondWithText(0, "OK");

    const usage = await waitFor(
      () => completedUsage(dataRoot, sessionId),
      (value) => value !== undefined,
      "durable usage after silent stream",
    );
    expect(usage.input_tokens).toBe(123);
    expect(usage.output_tokens).toBe(45);
    await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "Responses end after silent stream",
    );
    provider.respondWithEnd(1);
  });

  it("durably replays encrypted provider reasoning before the next tool round", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-reasoning-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl);
    let agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "responses reasoning agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    await fs.writeFile(path.join(workspace, "README.md"), "hello from the workspace\n");
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "read README.md, then answer" }),
        })
      ).status,
    ).toBe(202);

    await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "first Responses tool request",
    );
    provider.respondWithReasoningAndRead(0);

    const secondRequest = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "second Responses tool request",
    );
    expect(secondRequest.body.store).toBe(false);
    expect(secondRequest.body.include).toContain("reasoning.encrypted_content");
    const rawItems = [
      {
        id: "rs_1",
        type: "reasoning",
        status: "completed",
        encrypted_content: "encrypted-reasoning-1",
        summary: [{ type: "summary_text", text: "Inspect the file." }],
      },
      {
        id: "fc_1",
        type: "function_call",
        status: "completed",
        arguments: '{"path":"README.md"}',
        call_id: "call_read_1",
        name: "read",
      },
    ];
    expect(secondRequest.body.input.slice(1, 3)).toEqual(rawItems);
    expect(secondRequest.body.input).not.toEqual(expect.arrayContaining([expect.objectContaining({ type: "item_reference" })]));

    const durableEvents = await domainEvents(dataRoot, sessionId);
    const assistant = durableEvents.find((event) => event.type === "message_appended" && event.message?.role === "assistant");
    expect(assistant?.message?.provider_context).toEqual({
      profile_id: "open-code-go",
      provider: "opencode-go",
      model: "muse-spark-1.2-contributor",
      api: "openai-responses",
      output_items: rawItems,
    });

    await stopChild(agent);
    agent = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    await waitForReady(`${baseUrl}/readyz`, "restarted Responses reasoning agent readyz");
    const replayedRequest = await waitFor(
      () => provider.requests[2],
      (value) => value !== undefined,
      "replayed Responses request after restart",
    );
    expect(replayedRequest.body.input.slice(1, 3)).toEqual(rawItems);
    provider.respondWithText(2, "done");
    await waitFor(
      () => provider.requests[3],
      (value) => value !== undefined,
      "Responses end after tool result",
    );
    provider.respondWithEnd(3);
  });

  it("durably replays complete ordered plain reasoning output items across restart", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-plain-reasoning-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl);
    let agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "plain reasoning agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    const workspace = path.join(tempRoot, "workspace");
    await fs.writeFile(path.join(workspace, "README.md"), "plain reasoning replay\n");
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
      () => provider.requests[0],
      (value) => value !== undefined,
      "plain reasoning request",
    );
    provider.respondWithPlainReasoningAndRead(0);

    const secondRequest = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "request after plain reasoning tool call",
    );
    const rawItems = [
      {
        id: "rs_plain_1",
        type: "reasoning",
        status: null,
        summary: [],
        content: [{ type: "reasoning_text", text: "Inspect the file before answering.\n" }],
        encrypted_content: null,
      },
      {
        id: "fc_plain_1",
        type: "function_call",
        status: "completed",
        arguments: '{"path":"README.md"}',
        call_id: "call_plain_read_1",
        name: "read",
      },
    ];
    const rawStart = secondRequest.body.input.findIndex((item: Record<string, unknown>) => item.id === "rs_plain_1");
    expect(rawStart).toBeGreaterThanOrEqual(0);
    expect(secondRequest.body.input.slice(rawStart, rawStart + rawItems.length)).toEqual(rawItems);
    expect(secondRequest.body.input[rawStart + rawItems.length]).toEqual(expect.objectContaining({ type: "function_call_output", call_id: "call_plain_read_1" }));

    const durableEvents = await domainEvents(dataRoot, sessionId);
    const assistant = durableEvents.find((event) => event.type === "message_appended" && event.message?.role === "assistant");
    expect(assistant?.message?.provider_context?.output_items).toEqual(rawItems);

    await stopChild(agent);
    agent = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    await waitForReady(`${baseUrl}/readyz`, "restarted plain reasoning agent readyz");
    const replayedRequest = await waitFor(
      () => provider.requests[2],
      (value) => value !== undefined,
      "plain reasoning request after restart",
    );
    const replayedRawStart = replayedRequest.body.input.findIndex((item: Record<string, unknown>) => item.id === "rs_plain_1");
    expect(replayedRawStart).toBeGreaterThanOrEqual(0);
    expect(replayedRequest.body.input.slice(replayedRawStart, replayedRawStart + rawItems.length)).toEqual(rawItems);
    provider.respondWithEnd(2);
  });

  it("continues after a clean Responses EOF until the model explicitly calls end", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-responses-incomplete-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const provider = new ControlledResponses();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    await writeOpenCodeProfile(dataRoot, providerBaseUrl);
    const agent: ChildProcess = spawnAgent(dataRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const baseUrl = `http://127.0.0.1:${port}`;
    await waitForReady(`${baseUrl}/readyz`, "incomplete Responses agent readyz");

    const created = await fetch(`${baseUrl}/v1/sessions`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({
        profile_id: "open-code-go",
        model: "muse-spark-1.2-contributor",
        thinking: "xhigh",
        workspace: await testWorkspace(tempRoot),
      }),
    });
    expect(created.status).toBe(201);
    const sessionId = String(((await created.json()) as { session_id: string }).session_id);
    expect(
      (
        await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
          method: "POST",
          headers: headers(),
          body: JSON.stringify({ content: "finish the task" }),
        })
      ).status,
    ).toBe(202);

    await waitFor(
      () => provider.requests[0],
      (value) => value !== undefined,
      "Responses request before clean EOF",
    );
    provider.respondWithReasoningThenEof(0);

    const endRequest = await waitFor(
      () => provider.requests[1],
      (value) => value !== undefined,
      "next model round after clean Responses EOF",
    );
    expect(endRequest.body.tools).toEqual(expect.arrayContaining([expect.objectContaining({ name: "end" })]));
    provider.respondWithEnd(1);

    const events = await waitFor(
      () => domainEvents(dataRoot, sessionId),
      (items) => items.some((event) => event.type === "activation_finished"),
      "terminal activation after explicit Responses end",
    );
    expect(provider.requests).toHaveLength(2);
    expect(events.some((event) => event.type === "model_attempt_failed_fact")).toBe(false);
    expect(events.some((event) => event.type === "model_attempts_exhausted")).toBe(false);
    expect(events.filter((event) => event.type === "model_request_completed")).toHaveLength(2);
    expect(events.findLast((event) => event.type === "activation_finished")?.outcome).toBe("finished");

    const messages = await fetch(`${baseUrl}/v1/sessions/${sessionId}/messages?limit=200`, {
      headers: headers(),
    });
    expect(messages.status).toBe(200);
    const items = ((await messages.json()) as { items: Array<{ role?: string; content?: string }> }).items;
    expect(items).toEqual(expect.arrayContaining([expect.objectContaining({ role: "assistant", content: "" })]));
  });
});
