import fs from "node:fs/promises";
import http, { type ServerResponse } from "node:http";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { brokerRoot, getFreePort, removeTempRoot, spawnAgent, spawnBinary, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";
import { MockSlackServer } from "./helpers/mock-slack-server.js";

const agentToken = "controlled-agent-token";

type PendingModelRequest = {
  body: { messages?: Array<{ role?: string; content?: unknown }> };
  response: ServerResponse;
};

type PendingMailboxAppend = {
  readonly sessionId: string;
  readonly body: { content?: string };
  readonly response: ServerResponse;
};

class ControlledMailboxAgent {
  readonly appends: PendingMailboxAppend[] = [];
  readonly creates: Array<Record<string, unknown>> = [];
  readonly selectionUpdates: Array<{ sessionId: string; selection: Record<string, unknown> }> = [];
  readonly profileWrites: Array<{ profileId: string; document: Record<string, unknown> }> = [];
  readonly profileDeletes: string[] = [];
  readonly sessions = new Map<string, Record<string, unknown>>();
  readonly profiles: Array<Record<string, unknown>>;
  createFailure: { readonly status: number; readonly message: string } | null = null;

  constructor(
    profiles: Array<Record<string, unknown>> = [
      {
        profile_id: "fixture",
        provider: "xai",
        auth_configured: true,
        models: [
          {
            id: "grok-4.6",
            api: "openai-completions",
            streaming: true,
            thinking: ["high", "xhigh"],
            default_thinking: "xhigh",
            capabilities: { input: ["text", "image"] },
            default: true,
          },
        ],
      },
    ],
  ) {
    this.profiles = profiles;
  }

  readonly server = http.createServer(async (request, response) => {
    if (request.headers.authorization !== `Bearer ${agentToken}`) {
      response.writeHead(401).end();
      return;
    }
    const url = request.url ?? "";
    if (request.method === "GET" && url === "/v1/profiles") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ items: this.profiles }));
      return;
    }
    const profilePath = /^\/v1\/profiles\/([^/]+)$/.exec(url);
    if (request.method === "PUT" && profilePath) {
      const profileId = decodeURIComponent(profilePath[1]);
      const document = (await readJsonRequest(request)) as Record<string, unknown>;
      if (profileId === "invalid") {
        response.writeHead(422, { "content-type": "application/json" });
        response.end(JSON.stringify({ error: { code: "invalid_request", message: "invalid profile" } }));
        return;
      }
      this.profileWrites.push({ profileId, document });
      const view = {
        profile_id: profileId,
        provider: document.provider,
        billing: document.billing,
        auth_configured: Boolean(document.auth && Object.keys(document.auth as object).length),
        models: document.models,
      };
      const existing = this.profiles.findIndex((profile) => profile.profile_id === profileId);
      if (existing >= 0) this.profiles.splice(existing, 1, view);
      else this.profiles.push(view);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify(view));
      return;
    }
    if (request.method === "DELETE" && profilePath) {
      const profileId = decodeURIComponent(profilePath[1]);
      this.profileDeletes.push(profileId);
      const existing = this.profiles.findIndex((profile) => profile.profile_id === profileId);
      if (existing >= 0) this.profiles.splice(existing, 1);
      response.writeHead(204).end();
      return;
    }
    if (request.method === "GET" && /^\/v1\/sessions\/[^/]+$/.test(url)) {
      const sessionId = decodeURIComponent(url.slice("/v1/sessions/".length));
      const session = this.sessions.get(sessionId);
      response.writeHead(session ? 200 : 404, { "content-type": "application/json" });
      response.end(JSON.stringify(session ?? { error: { message: "session not found" } }));
      return;
    }
    if (request.method === "POST" && url === "/v1/sessions") {
      const body = (await readJsonRequest(request)) as Record<string, unknown>;
      this.creates.push(body);
      if (this.createFailure) {
        response.writeHead(this.createFailure.status, { "content-type": "application/json" });
        response.end(JSON.stringify({ error: { code: "invalid_request", message: this.createFailure.message } }));
        return;
      }
      const sessionId = `01ARZ3NDEKTSV4RRFFQ69G5FA${this.creates.length.toString(32).toUpperCase()}`;
      const session = { session_id: sessionId, ...body, status: "wait" };
      this.sessions.set(sessionId, session);
      response.writeHead(201, { "content-type": "application/json" });
      response.end(JSON.stringify(session));
      return;
    }
    const selectionPath = /^\/v1\/sessions\/([^/]+)\/selection$/.exec(url);
    if (request.method === "PUT" && selectionPath) {
      const sessionId = decodeURIComponent(selectionPath[1]);
      const selection = (await readJsonRequest(request)) as Record<string, unknown>;
      const session = this.sessions.get(sessionId);
      if (!session) {
        response.writeHead(404, { "content-type": "application/json" });
        response.end(JSON.stringify({ error: { message: "session not found" } }));
        return;
      }
      this.selectionUpdates.push({ sessionId, selection });
      const updated = { ...session, ...selection };
      this.sessions.set(sessionId, updated);
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify(updated));
      return;
    }
    const mailbox = /^\/v1\/sessions\/([^/]+)\/mailbox$/.exec(url);
    if (request.method === "POST" && mailbox) {
      this.appends.push({
        sessionId: decodeURIComponent(mailbox[1]),
        body: (await readJsonRequest(request)) as { content?: string },
        response,
      });
      return;
    }
    response.writeHead(404).end();
  });

  async start(port: number): Promise<void> {
    await new Promise<void>((resolve) => this.server.listen(port, "127.0.0.1", resolve));
  }

  async waitForAppend(count: number): Promise<PendingMailboxAppend> {
    return await waitFor(
      () => this.appends[count - 1],
      (append): append is PendingMailboxAppend => Boolean(append),
      `Agent mailbox append ${count}`,
    );
  }

  async waitForCreate(count: number): Promise<Record<string, unknown>> {
    return await waitFor(
      () => this.creates[count - 1],
      (create): create is Record<string, unknown> => Boolean(create),
      `Agent session create ${count}`,
    );
  }

  accept(count: number): void {
    const append = this.appends[count - 1];
    if (!append) throw new Error(`missing mailbox append ${count}`);
    append.response.writeHead(202);
    append.response.end();
  }

  async stop(): Promise<void> {
    for (const [index, append] of this.appends.entries()) {
      if (!append.response.writableEnded) this.accept(index + 1);
    }
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }
}

class ControlledProvider {
  readonly requests: PendingModelRequest[] = [];
  readonly server = http.createServer(async (request, response) => {
    if (request.method !== "POST" || !(request.url ?? "").includes("/chat/completions")) {
      response.writeHead(404).end();
      return;
    }
    const chunks: Buffer[] = [];
    for await (const chunk of request) chunks.push(Buffer.from(chunk));
    this.requests.push({
      body: JSON.parse(Buffer.concat(chunks).toString("utf8")) as PendingModelRequest["body"],
      response,
    });
    this.flushWaiters();
  });
  private waiters: Array<{ count: number; resolve: (value: PendingModelRequest) => void }> = [];

  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.server.listen(0, "127.0.0.1", resolve));
    const address = this.server.address();
    if (!address || typeof address === "string") throw new Error("provider did not bind");
    return `http://127.0.0.1:${address.port}/v1`;
  }

  waitForRequest(count: number): Promise<PendingModelRequest> {
    const request = this.requests[count - 1];
    if (request) return Promise.resolve(request);
    return new Promise((resolve) => this.waiters.push({ count, resolve }));
  }

  respond(count: number, content: string): void {
    const request = this.requests[count - 1];
    if (!request) throw new Error(`missing provider request ${count}`);
    request.response.writeHead(200, { "content-type": "text/event-stream" });
    request.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [{ index: 0, delta: { role: "assistant", content } }],
      })}\n\n`,
    );
    request.response.write(
      `data: ${JSON.stringify({
        id: `chatcmpl-${count}`,
        object: "chat.completion.chunk",
        choices: [{ index: 0, delta: {}, finish_reason: "stop" }],
        usage: { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 },
      })}\n\n`,
    );
    request.response.end("data: [DONE]\n\n");
  }

  async stop(): Promise<void> {
    this.requests.forEach((request, index) => {
      if (!request.response.writableEnded) this.respond(index + 1, "cleanup");
    });
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
  }

  private flushWaiters(): void {
    this.waiters = this.waiters.filter((waiter) => {
      const request = this.requests[waiter.count - 1];
      if (!request) return true;
      waiter.resolve(request);
      return false;
    });
  }
}

describe.sequential("Gateway mailbox delivery", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) await cleanups.pop()?.();
  });

  it("exposes Agent-owned profiles to Admin without a Gateway profile store", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-profile-proxy-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());
    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const adminPort = await getFreePort();
    const agentPort = await getFreePort();
    const agent = new ControlledMailboxAgent();
    await agent.start(agentPort);
    cleanups.push(async () => agent.stop());
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        control: `127.0.0.1:${adminPort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));
    const adminBaseUrl = `http://127.0.0.1:${adminPort}`;
    await waitForReady(`${adminBaseUrl}/readyz`, "Gateway Admin readyz");

    const listed = await fetch(`${adminBaseUrl}/admin/api/profiles`);
    expect(listed.status).toBe(200);
    expect(await listed.json()).toEqual({ items: agent.profiles });

    const document = {
      provider: "openai",
      billing: "usage",
      auth: { type: "api_key", key: "sk-admin" },
      models: [
        {
          id: "gpt-admin",
          api: "openai-completions",
          streaming: true,
          thinking: ["off", "high"],
          default_thinking: "high",
          capabilities: { input: ["text"] },
          default: true,
        },
      ],
    };
    const written = await fetch(`${adminBaseUrl}/admin/api/profiles/admin`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(document),
    });
    expect(written.status).toBe(200);
    expect(agent.profileWrites).toEqual([{ profileId: "admin", document }]);

    const invalid = await fetch(`${adminBaseUrl}/admin/api/profiles/invalid`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(document),
    });
    expect(invalid.status).toBe(422);
    expect(await invalid.json()).toEqual({ ok: false, error: "invalid profile" });

    const deleted = await fetch(`${adminBaseUrl}/admin/api/profiles/admin`, { method: "DELETE" });
    expect(deleted.status).toBe(204);
    expect(agent.profileDeletes).toEqual(["admin"]);
    expect((await fetch(`${adminBaseUrl}/admin/api/auth-profiles`)).status).toBe(404);
    await expect(fs.access(path.join(tempRoot, "auth-profiles"))).rejects.toThrow();
  });

  it("updates model, thinking, and Profile independently and resolves only Profile auto", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-selection-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());
    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const adminPort = await getFreePort();
    const agentPort = await getFreePort();
    const models = [
      {
        id: "grok-4.6",
        api: "openai-completions",
        streaming: true,
        thinking: ["high", "xhigh"],
        default_thinking: "xhigh",
        capabilities: { input: ["text"] },
        default: true,
      },
    ];
    const agent = new ControlledMailboxAgent([
      {
        profile_id: "subscription",
        provider: "xai",
        billing: "subscription",
        auth_configured: true,
        account: { ok: true },
        rateLimits: { ok: true, rateLimits: { secondary: { usedPercent: 60 } } },
        models,
      },
      {
        profile_id: "usage",
        provider: "xai",
        billing: "usage",
        auth_configured: true,
        account: { ok: true },
        rateLimits: { ok: true, rateLimits: { credits: { balance: "100" } } },
        models,
      },
    ]);
    await agent.start(agentPort);
    cleanups.push(async () => agent.stop());
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        control: `127.0.0.1:${adminPort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "selection Gateway readyz");
    await waitForReady(`http://127.0.0.1:${adminPort}/readyz`, "selection Admin readyz");
    await slack.waitForSocket();

    await slack.sendEvent("evt-selection", {
      type: "app_mention",
      user: "U123",
      channel: "C-SELECTION",
      thread_ts: "820.100",
      ts: "820.101",
      text: "<@UBOT> create selection session",
    });
    await agent.waitForCreate(1);
    await agent.waitForAppend(1);
    agent.accept(1);

    const adminBaseUrl = `http://127.0.0.1:${adminPort}`;
    const explicit = await fetch(`${adminBaseUrl}/admin/api/sessions/${encodeURIComponent("C-SELECTION:820.100")}/selection`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ profile_id: "usage", model: "grok-4.6", thinking: "high" }),
    });
    expect(explicit.status).toBe(200);
    expect((await explicit.json()).selection).toEqual({ profile_id: "usage", model: "grok-4.6", thinking: "high" });

    const automatic = await fetch(`${adminBaseUrl}/admin/api/sessions/${encodeURIComponent("C-SELECTION:820.100")}/selection`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ profile_id: "auto", model: "grok-4.6", thinking: "high" }),
    });
    expect(automatic.status).toBe(200);
    expect((await automatic.json()).selection).toEqual({ profile_id: "subscription", model: "grok-4.6", thinking: "high" });
    expect(agent.selectionUpdates.map((update) => update.selection)).toEqual([
      { profile_id: "usage", model: "grok-4.6", thinking: "high" },
      { profile_id: "subscription", model: "grok-4.6", thinking: "high" },
    ]);

    const gatewayDb = new DatabaseSync(path.join(tempRoot, "state", "gateway.sqlite"));
    gatewayDb.prepare("UPDATE sessions SET id = NULL WHERE key = ?").run("C-SELECTION:820.100");
    gatewayDb.close();
    agent.createFailure = { status: 422, message: "selection disappeared" };
    const rejectedFreshSelection = await fetch(`${adminBaseUrl}/admin/api/sessions/${encodeURIComponent("C-SELECTION:820.100")}/selection`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ profile_id: "usage", model: "grok-4.6", thinking: "high" }),
    });
    expect(rejectedFreshSelection.status).toBe(422);
    expect(await rejectedFreshSelection.json()).toEqual({ ok: false, error: "selection disappeared" });
  });

  it("acknowledges a Slack envelope only after the Agent durably accepts its mailbox message", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-mailbox-ack-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());

    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const agentPort = await getFreePort();
    const agent = new ControlledMailboxAgent();
    await agent.start(agentPort);
    cleanups.push(async () => agent.stop());
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "Gateway broker readyz");
    await slack.waitForSocket();

    await slack.sendEvent("evt-mailbox-ack", {
      type: "app_mention",
      user: "U123",
      channel: "C-ACK",
      thread_ts: "800.100",
      ts: "800.101",
      text: "<@UBOT> append before ack",
    });
    expect(await agent.waitForCreate(1)).toEqual({
      profile_id: "fixture",
      model: "grok-4.6",
      thinking: "xhigh",
      system_prompt: expect.stringContaining("Slack"),
      workspace: path.join(tempRoot, "workspaces", "slack", "C-ACK", "800.100"),
    });
    const firstAppend = await agent.waitForAppend(1);
    expect(Object.keys(firstAppend.body)).toEqual(["content"]);
    expect(firstAppend.body.content).toContain("append before ack");
    expect(firstAppend.sessionId).toBe("01ARZ3NDEKTSV4RRFFQ69G5FA1");
    expect(slack.acknowledgedEnvelopeIds).not.toContain("env-evt-mailbox-ack");

    agent.accept(1);
    await waitFor(
      () => slack.acknowledgedEnvelopeIds,
      (ids) => ids.includes("env-evt-mailbox-ack"),
      "Slack acknowledgement after mailbox receipt",
    );

    let notifyResolved = false;
    const notify = fetch(`http://127.0.0.1:${runtimePort}/notify`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        conversationId: "C-ACK",
        rootMessageId: "800.100",
        text: "background result",
      }),
    }).then((response) => {
      notifyResolved = true;
      return response;
    });
    const pendingNotify = await agent.waitForAppend(2);
    expect(Object.keys(pendingNotify.body)).toEqual(["content"]);
    expect(pendingNotify.body.content).toContain("background result");
    expect(notifyResolved).toBe(false);
    agent.accept(2);
    expect((await notify).status).toBe(200);

    const secondNotify = fetch(`http://127.0.0.1:${runtimePort}/notify`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        conversationId: "C-ACK",
        rootMessageId: "800.100",
        text: "another background result",
      }),
    });
    const secondPendingNotify = await agent.waitForAppend(3);
    expect(Object.keys(secondPendingNotify.body)).toEqual(["content"]);
    expect(secondPendingNotify.body.content).toContain("another background result");
    agent.accept(3);
    expect((await secondNotify).status).toBe(200);
    expect(readInboundSources(stateDir, "C-ACK:800.100")).toEqual(["app_mention"]);

    const missingTarget = await fetch(`http://127.0.0.1:${runtimePort}/notify`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        conversationId: "C-MISSING",
        rootMessageId: "999.100",
        text: "must not be reported as delivered",
      }),
    });
    expect(missingTarget.status).toBe(404);

    const missingJob = await fetch(`http://127.0.0.1:${runtimePort}/notify`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        jobId: "job-that-does-not-exist",
        conversationId: "C-ACK",
        rootMessageId: "800.100",
        text: "must not be appended without its owning job",
      }),
    });
    expect(missingJob.status).toBe(404);
    expect(agent.appends).toHaveLength(3);

    insertBackgroundJob(stateDir, {
      id: "job-owned-by-ack-session",
      sessionKey: "C-ACK:800.100",
      channelId: "C-ACK",
      rootMessageId: "800.100",
      workspacePath: tempRoot,
    });
    const mismatchedJobTarget = await fetch(`http://127.0.0.1:${runtimePort}/notify`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        jobId: "job-owned-by-ack-session",
        conversationId: "C-OTHER",
        rootMessageId: "999.200",
        text: "must not escape the job-owned session",
      }),
    });
    expect(mismatchedJobTarget.status).toBe(400);
    expect(agent.appends).toHaveLength(3);

    expect(gatewayTableNames(stateDir)).not.toEqual(expect.arrayContaining(["inbound_events", "processed_events"]));
  });

  it("does not turn an auth-blocked input into an automatic Slack reply", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-mailbox-auth-block-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());

    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const agentPort = await getFreePort();
    const agent = new ControlledMailboxAgent([]);
    await agent.start(agentPort);
    cleanups.push(async () => agent.stop());
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "auth-blocked Gateway readyz");
    await slack.waitForSocket();

    await slack.sendEvent("evt-mailbox-auth-block", {
      type: "app_mention",
      user: "U123",
      channel: "C-AUTH-BLOCK",
      thread_ts: "810.100",
      ts: "810.101",
      text: "<@UBOT> this input has no auth profile",
    });
    await waitFor(
      () => readInboundStatus(stateDir, "C-AUTH-BLOCK:810.100", "810.101"),
      (status) => status === "blocked",
      "auth-blocked inbound audit",
    );

    expect(agent.appends).toHaveLength(0);
    expect(slack.acknowledgedEnvelopeIds).not.toContain("env-evt-mailbox-auth-block");
    expect(slack.postedMessages).toHaveLength(0);
  });

  it("ignores previous database names and delivers through a fresh Gateway database", { timeout: 30_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-mailbox-legacy-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    await fs.mkdir(stateDir, { recursive: true });
    const legacyWorkspace = path.join(tempRoot, "legacy-workspace");
    await fs.mkdir(legacyWorkspace, { recursive: true });
    createOriginBrokerDatabase(path.join(stateDir, "broker.sqlite"), legacyWorkspace);
    createObsoleteRuntimeDatabase(path.join(stateDir, "runtime.sqlite"));

    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());
    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const agentPort = await getFreePort();
    const agent = new ControlledMailboxAgent();
    await agent.start(agentPort);
    cleanups.push(async () => agent.stop());
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "legacy Gateway readyz");
    await slack.waitForSocket();

    await slack.sendEvent("evt-mailbox-legacy", {
      type: "app_mention",
      user: "U123",
      channel: "C-LEGACY",
      thread_ts: "900.100",
      ts: "900.101",
      text: "<@UBOT> deliver through the fresh application",
    });
    const append = await agent.waitForAppend(1);
    expect(Object.keys(append.body)).toEqual(["content"]);
    expect(append.body.content).toContain("deliver through the fresh application");
    expect(append.sessionId).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);
    agent.accept(1);
    await waitFor(
      () => slack.acknowledgedEnvelopeIds,
      (ids) => ids.includes("env-evt-mailbox-legacy"),
      "legacy Slack acknowledgement after mailbox receipt",
    );

    const current = readGatewaySession(path.join(stateDir, "gateway.sqlite"), "C-LEGACY:900.100");
    expect(current).toMatchObject({
      id: append.sessionId,
    });
    expect(current?.workspace_path).not.toBe(legacyWorkspace);
    expect(current?.workspace_path).toBe(path.join(tempRoot, "workspaces", "slack", "C-LEGACY", "900.100"));
    expect(agent.creates[0]?.workspace).toBe(current?.workspace_path);
    expect(gatewaySessionIdentityConstraints(stateDir, append.sessionId)).toEqual({
      nullIdRejected: false,
      duplicateIdRejected: true,
    });
    const columns = sessionColumns(stateDir);
    for (const forbiddenColumn of ["agent_session_id", "active_turn_id", "active_turn_started_at", "last_turn_signal_kind"]) {
      expect(columns).not.toContain(forbiddenColumn);
    }
    const tables = gatewayTableNames(stateDir);
    for (const forbiddenTable of ["schema_migrations", "agent_session_bindings", "inbound_events", "processed_events", "slack_events", "agent_turn_bindings", "agent_turn_usage"]) {
      expect(tables).not.toContain(forbiddenTable);
    }
    expect(readOriginBrokerSession(path.join(stateDir, "broker.sqlite"), "C-LEGACY:900.100")).toMatchObject({
      agent_session_id: "old-agent-session",
      workspace_path: legacyWorkspace,
    });
    expect(readObsoleteRuntimeTables(path.join(stateDir, "runtime.sqlite"))).toEqual(["obsolete_marker"]);
  });

  it("finishes each Slack delivery at mailbox receipt without tracking Agent execution", { timeout: 45_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-gateway-mailbox-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");

    const provider = new ControlledProvider();
    const providerBaseUrl = await provider.start();
    cleanups.push(async () => provider.stop());

    const slack = new MockSlackServer("UBOT");
    const slackPort = await slack.start();
    cleanups.push(async () => slack.stop());

    const gatewayPort = await getFreePort();
    const runtimePort = await getFreePort();
    const agentPort = await getFreePort();
    await writeConfig(tempRoot, {
      slack: {
        app_token: "xapp-test",
        bot_token: "xoxb-test",
        api_base_url: `http://127.0.0.1:${slackPort}/api`,
      },
      bind: {
        gateway: `127.0.0.1:${gatewayPort}`,
        runtime: `127.0.0.1:${runtimePort}`,
        agent: `127.0.0.1:${agentPort}`,
      },
    });
    await fs.mkdir(path.join(tempRoot, "profiles"), { recursive: true });
    await fs.writeFile(
      path.join(tempRoot, "profiles", "fixture.json"),
      `${JSON.stringify({
        provider: "openai",
        billing: "usage",
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
        base_url: providerBaseUrl,
        auth: { type: "api_key", key: "sk-test" },
      })}\n`,
    );

    const agent = spawnAgent(tempRoot, false, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));

    await waitForReady(`http://127.0.0.1:${agentPort}/readyz`, "mailbox Agent readyz");
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "mailbox Gateway readyz");
    await slack.waitForSocket();

    const firstSlackEvent = {
      type: "app_mention",
      user: "U123",
      channel: "C-MAILBOX",
      thread_ts: "700.100",
      ts: "700.101",
      text: "<@UBOT> first mailbox input",
    };
    await slack.sendEvent("evt-mailbox-1", firstSlackEvent);
    await provider.waitForRequest(1);
    await waitFor(
      () => readInboundStatus(stateDir, "C-MAILBOX:700.100", "700.101"),
      (status) => status === "delivered",
      "first mailbox receipt",
    );

    await slack.sendEvent("evt-mailbox-2", {
      ...firstSlackEvent,
      ts: "700.102",
      text: "<@UBOT> second mailbox input",
    });
    await waitFor(
      () => readInboundStatus(stateDir, "C-MAILBOX:700.100", "700.102"),
      (status) => status === "delivered",
      "second mailbox receipt while provider is blocked",
    );
    expect(provider.requests).toHaveLength(1);

    await slack.sendEvent("evt-mailbox-2-replay", {
      ...firstSlackEvent,
      ts: "700.102",
      text: "<@UBOT> second mailbox input",
    });
    await waitFor(
      () => slack.acknowledgedEnvelopeIds,
      (ids) => ids.includes("env-evt-mailbox-2-replay"),
      "duplicate Slack event mailbox receipt",
    );
    expect(provider.requests).toHaveLength(1);

    provider.respond(1, "internal first answer");
    const secondRequest = await provider.waitForRequest(2);
    const modelInput = JSON.stringify(secondRequest.body.messages ?? []);
    expect(modelInput).toContain("first mailbox input");
    expect(modelInput).toContain("second mailbox input");
    expect(modelInput.match(/second mailbox input/g)).toHaveLength(1);
    provider.respond(2, "internal second answer");

    expect(slack.postedMessages.some((message) => message.text.includes("internal first answer"))).toBe(false);
    expect(slack.postedMessages.some((message) => message.text.includes("internal second answer"))).toBe(false);

    const gatewaySession = readGatewaySession(path.join(stateDir, "gateway.sqlite"), "C-MAILBOX:700.100");
    const agentMessages = await waitFor(
      async () =>
        (await fetch(`http://127.0.0.1:${agentPort}/v1/sessions/${gatewaySession?.id}/messages`, {
          headers: { authorization: `Bearer ${agentToken}` },
        }).then((response) => response.json())) as {
          items?: Array<{ type?: string; role?: string; content?: string }>;
        },
      (payload) => payload.items?.some((item) => item.role === "assistant" && item.content === "internal second answer") === true,
      "Agent-owned message history",
    );
    expect(agentMessages.items?.filter((item) => item.role === "user" && item.content?.includes("second mailbox input"))).toHaveLength(1);

    const timeline = (await fetch(`http://127.0.0.1:${runtimePort}/internal/realtime/sessions/${encodeURIComponent("C-MAILBOX:700.100")}/timeline?limit=100`).then((response) => response.json())) as { events?: Array<{ type?: string; title?: string }> };
    expect((timeline.events ?? []).some((event) => event.type === "agent_assistant_message")).toBe(false);

    const columns = sessionColumns(stateDir);
    expect(columns).not.toContain("active_turn_id");
    expect(columns).not.toContain("active_turn_started_at");
    expect(columns.some((column) => column.includes("turn_signal"))).toBe(false);

    const adminProjection = await fetch(`http://127.0.0.1:${runtimePort}/internal/realtime/sessions`).then((response) => response.json());
    const serializedProjection = JSON.stringify(adminProjection);
    expect(serializedProjection).not.toContain("activeTurn");
    expect(serializedProjection).not.toContain("activation");
  });
});

function readInboundStatus(stateDir: string, sessionKey: string, messageTs: string): string | undefined {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    const row = db.prepare("SELECT status FROM inbound_messages WHERE session_key = ? AND message_ts = ?").get(sessionKey, messageTs) as { status?: string } | undefined;
    return row?.status;
  } finally {
    db.close();
  }
}

function readInboundSources(stateDir: string, sessionKey: string): string[] {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    return (db.prepare("SELECT source FROM inbound_messages WHERE session_key = ? ORDER BY created_at, message_ts").all(sessionKey) as Array<{ source: string }>).map((row) => row.source);
  } finally {
    db.close();
  }
}

function sessionColumns(stateDir: string): string[] {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    return (db.prepare("PRAGMA table_info(sessions)").all() as Array<{ name: string }>).map((column) => column.name);
  } finally {
    db.close();
  }
}

async function readJsonRequest(request: http.IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) chunks.push(Buffer.from(chunk));
  return JSON.parse(Buffer.concat(chunks).toString("utf8")) as Record<string, unknown>;
}

function gatewayTableNames(stateDir: string): string[] {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    return (db.prepare("SELECT name FROM sqlite_master WHERE type = 'table'").all() as Array<{ name: string }>).map((row) => row.name);
  } finally {
    db.close();
  }
}

function insertBackgroundJob(
  stateDir: string,
  job: {
    id: string;
    sessionKey: string;
    channelId: string;
    rootMessageId: string;
    workspacePath: string;
  },
): void {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"));
  try {
    db.prepare(
      `INSERT INTO background_jobs (
         id, token, session_key, channel_id, root_thread_ts, kind, shell, cwd, script_path,
         restart_on_boot, status, created_at, updated_at
       ) VALUES (?, ?, ?, ?, ?, 'test', 'sh', ?, ?, 0, 'running', ?, ?)`,
    ).run(job.id, `token-${job.id}`, job.sessionKey, job.channelId, job.rootMessageId, job.workspacePath, path.join(job.workspacePath, `${job.id}.sh`), "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z");
  } finally {
    db.close();
  }
}

function createOriginBrokerDatabase(databasePath: string, workspacePath: string): void {
  const db = new DatabaseSync(databasePath);
  try {
    db.exec(`
      CREATE TABLE sessions (
        key TEXT PRIMARY KEY,
        platform TEXT,
        conversation_id TEXT,
        conversation_kind TEXT,
        root_message_id TEXT,
        platform_thread_id TEXT,
        channel_id TEXT NOT NULL,
        channel_name TEXT,
        channel_type TEXT,
        root_thread_ts TEXT NOT NULL,
        workspace_path TEXT NOT NULL,
        initiator_user_id TEXT,
        initiator_message_ts TEXT,
        initiator_captured_at TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        agent_session_id TEXT,
        active_turn_id TEXT,
        active_turn_started_at TEXT,
        last_observed_message_ts TEXT,
        last_delivered_message_ts TEXT,
        last_slack_reply_at TEXT,
        session_page_link_posted_at TEXT,
        auth_profile_name TEXT,
        auth_profile_bound_at TEXT,
        auth_blocked_at TEXT,
        auth_block_reason TEXT,
        last_turn_signal_turn_id TEXT,
        last_turn_signal_kind TEXT,
        last_turn_signal_reason TEXT,
        last_turn_signal_at TEXT,
        UNIQUE(channel_id, root_thread_ts)
      );
      CREATE TABLE processed_events (sequence INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE);
      CREATE TABLE slack_events (event_id TEXT PRIMARY KEY, payload TEXT NOT NULL, status TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
      CREATE TABLE agent_turn_bindings (turn_id TEXT PRIMARY KEY, session_key TEXT NOT NULL);
      CREATE TABLE agent_turn_usage (turn_id TEXT PRIMARY KEY, session_key TEXT NOT NULL);
    `);
    db.prepare(
      `INSERT INTO sessions (
         key, platform, conversation_id, conversation_kind, root_message_id, platform_thread_id,
         channel_id, channel_type, root_thread_ts, workspace_path, initiator_user_id,
         initiator_message_ts, initiator_captured_at, created_at, updated_at, agent_session_id,
         active_turn_id, active_turn_started_at, auth_profile_name, auth_profile_bound_at,
         last_turn_signal_turn_id, last_turn_signal_kind, last_turn_signal_at
       ) VALUES (?, 'slack', ?, 'channel', ?, ?, ?, 'channel', ?, ?, 'U123', ?, ?, ?, ?, ?, ?, ?, 'fixture', ?, ?, 'started', ?)`,
    ).run("C-LEGACY:900.100", "C-LEGACY", "900.100", "900.100", "C-LEGACY", "900.100", workspacePath, "900.100", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z", "old-agent-session", "old-turn", "2026-01-01T00:00:00Z", "2026-01-01T00:00:00Z", "old-turn", "2026-01-01T00:00:00Z");
  } finally {
    db.close();
  }
}

function readGatewaySession(databasePath: string, key: string): { id: string; workspace_path: string } | undefined {
  const db = new DatabaseSync(databasePath, { readOnly: true });
  try {
    return db.prepare("SELECT id, workspace_path FROM sessions WHERE key = ?").get(key) as { id: string; workspace_path: string } | undefined;
  } finally {
    db.close();
  }
}

function createObsoleteRuntimeDatabase(databasePath: string): void {
  const db = new DatabaseSync(databasePath);
  try {
    db.exec("CREATE TABLE obsolete_marker (value TEXT NOT NULL)");
    db.prepare("INSERT INTO obsolete_marker (value) VALUES (?)").run("must remain untouched");
  } finally {
    db.close();
  }
}

function readObsoleteRuntimeTables(databasePath: string): string[] {
  const db = new DatabaseSync(databasePath, { readOnly: true });
  try {
    return (db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all() as Array<{ name: string }>).map((row) => row.name);
  } finally {
    db.close();
  }
}

function readOriginBrokerSession(databasePath: string, key: string): { agent_session_id?: string; workspace_path?: string } | undefined {
  const db = new DatabaseSync(databasePath, { readOnly: true });
  try {
    return db.prepare("SELECT agent_session_id, workspace_path FROM sessions WHERE key = ?").get(key) as { agent_session_id?: string; workspace_path?: string } | undefined;
  } finally {
    db.close();
  }
}

function gatewaySessionIdentityConstraints(stateDir: string, existingId: string): { nullIdRejected: boolean; duplicateIdRejected: boolean } {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"));
  const insert = db.prepare(
    `INSERT INTO sessions (key, id, channel_id, root_thread_ts, workspace_path, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')`,
  );
  db.exec("BEGIN");
  try {
    let nullIdRejected = false;
    let duplicateIdRejected = false;
    try {
      insert.run("C-NULL:1.0", null, "C-NULL", "1.0", path.join(stateDir, "null-id"));
    } catch {
      nullIdRejected = true;
    }
    try {
      insert.run("C-DUPLICATE:2.0", existingId, "C-DUPLICATE", "2.0", path.join(stateDir, "duplicate-id"));
    } catch {
      duplicateIdRejected = true;
    }
    return { nullIdRejected, duplicateIdRejected };
  } finally {
    db.exec("ROLLBACK");
    db.close();
  }
}
