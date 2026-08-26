import { type ChildProcess } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { getFreePort, removeTempRoot, spawnAgent, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";

type SessionView = {
  session_id: string;
  profile_id: string;
  model: string;
  thinking: string;
  workspace: string;
  status: "working" | "wait";
};

type MessageItem = { type: "message"; role: "user" | "assistant" | "tool"; content: string } | { type: "wait"; reason: string };

const token = "agent-app-api-token";
const sessionWorkspaces = new Map<string, string>();

function headers(withToken = true): Record<string, string> {
  return {
    "content-type": "application/json",
    ...(withToken ? { authorization: `Bearer ${token}` } : {}),
  };
}

function profileDocument() {
  return {
    provider: "openai",
    billing: "usage",
    base_url: "http://127.0.0.1:9/v1",
    headers: { "x-profile-header": "fixture" },
    auth: { type: "api_key", key: "secret-that-must-not-be-returned" },
    models: [
      {
        id: "fixture-model",
        api: "openai-completions",
        streaming: true,
        parallel_tool_calls: false,
        thinking: ["low", "high"],
        default_thinking: "high",
        capabilities: { input: ["text", "image"] },
        default: true,
      },
    ],
  };
}

async function putProfile(baseUrl: string, withToken = true): Promise<void> {
  const response = await fetch(`${baseUrl}/v1/profiles/fixture`, {
    method: "PUT",
    headers: headers(withToken),
    body: JSON.stringify(profileDocument()),
  });
  expect(response.status).toBe(200);
}

async function createSession(baseUrl: string, withToken = true): Promise<SessionView> {
  const response = await fetch(`${baseUrl}/v1/sessions`, {
    method: "POST",
    headers: headers(withToken),
    body: JSON.stringify({
      profile_id: "fixture",
      model: "fixture-model",
      thinking: "high",
      workspace: sessionWorkspaces.get(baseUrl),
    }),
  });
  expect(response.status).toBe(201);
  return (await response.json()) as SessionView;
}

async function append(baseUrl: string, sessionId: string, content: string): Promise<void> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/mailbox`, {
    method: "POST",
    headers: headers(),
    body: JSON.stringify({ content }),
  });
  const body = await response.text();
  expect(response.status).toBe(202);
  expect(body).toBe("");
}

async function readMessages(baseUrl: string, sessionId: string, query = ""): Promise<{ items: MessageItem[]; older_cursor: string | null }> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}/messages${query}`, {
    headers: { authorization: `Bearer ${token}` },
  });
  expect(response.status).toBe(200);
  return (await response.json()) as { items: MessageItem[]; older_cursor: string | null };
}

describe.sequential("zork-agent app API", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  async function start(agentToken?: string): Promise<{ baseUrl: string; dataRoot: string; child: ChildProcess }> {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "zork-agent-app-api-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const dataRoot = path.join(tempRoot, "data");
    const port = await getFreePort();
    await writeConfig(dataRoot, { bind: { agent: `127.0.0.1:${port}` } });
    const child = spawnAgent(dataRoot, true, undefined, agentToken);
    cleanups.push(async () => stopChild(child));
    const baseUrl = `http://127.0.0.1:${port}`;
    const workspace = path.join(tempRoot, "workspace");
    await fs.mkdir(workspace);
    sessionWorkspaces.set(baseUrl, workspace);
    await waitForReady(`${baseUrl}/readyz`, "zork-agent readyz");
    return { baseUrl, dataRoot, child };
  }

  it("owns full profile resources and never returns profile secrets", async () => {
    const { baseUrl } = await start(token);
    await putProfile(baseUrl);

    const expected = {
      profile_id: "fixture",
      provider: "openai",
      billing: "usage",
      auth_configured: true,
      account: { ok: false, error: "not_probed" },
      rateLimits: { ok: false, error: "not_probed" },
      models: profileDocument().models,
    };
    const read = await fetch(`${baseUrl}/v1/profiles/fixture`, {
      headers: { authorization: `Bearer ${token}` },
    });
    expect(read.status).toBe(200);
    const readBody = await read.json();
    expect(readBody).toEqual(expected);
    expect(JSON.stringify(readBody)).not.toContain("secret-that-must-not-be-returned");

    const list = await fetch(`${baseUrl}/v1/profiles`, {
      headers: { authorization: `Bearer ${token}` },
    });
    expect(await list.json()).toEqual({ items: [expected] });

    const replacement = profileDocument();
    replacement.models[0].thinking = ["low"];
    replacement.models[0].default_thinking = "low";
    const replaced = await fetch(`${baseUrl}/v1/profiles/fixture`, {
      method: "PUT",
      headers: headers(),
      body: JSON.stringify(replacement),
    });
    expect(replaced.status).toBe(200);
    expect((await replaced.json()) as unknown).toEqual({ ...expected, models: replacement.models });

    const invalid = profileDocument();
    invalid.models[0].default_thinking = "xhigh";
    expect(
      (
        await fetch(`${baseUrl}/v1/profiles/invalid`, {
          method: "PUT",
          headers: headers(),
          body: JSON.stringify(invalid),
        })
      ).status,
    ).toBe(422);

    const deleted = await fetch(`${baseUrl}/v1/profiles/fixture`, {
      method: "DELETE",
      headers: { authorization: `Bearer ${token}` },
    });
    expect(deleted.status).toBe(204);
    expect((await fetch(`${baseUrl}/v1/profiles/fixture`, { headers: { authorization: `Bearer ${token}` } })).status).toBe(404);
  });

  it("declares the model API in the profile and accepts an OpenCode Go profile", async () => {
    const { baseUrl } = await start(token);
    const profile = {
      provider: "opencode-go",
      billing: "subscription",
      auth: { type: "api_key", key: "open-code-go-secret" },
      models: [
        {
          id: "muse-spark-1.2-contributor",
          api: "openai-responses",
          streaming: true,
          parallel_tool_calls: false,
          thinking: ["off", "minimal", "low", "medium", "high", "xhigh"],
          default_thinking: "xhigh",
          capabilities: { input: ["text", "image"] },
          limits: { context_window_tokens: 1_048_576, max_output_tokens: 131_072 },
          default: true,
        },
      ],
    };
    const response = await fetch(`${baseUrl}/v1/profiles/open-code-go`, {
      method: "PUT",
      headers: headers(),
      body: JSON.stringify(profile),
    });
    expect(response.status).toBe(200);
    const publicProfile = await response.json();
    expect(publicProfile).toEqual({
      profile_id: "open-code-go",
      provider: "opencode-go",
      billing: "subscription",
      auth_configured: true,
      account: { ok: false, error: "not_probed" },
      rateLimits: { ok: false, error: "not_probed" },
      models: profile.models,
    });
    expect(JSON.stringify(publicProfile)).not.toContain("open-code-go-secret");
  });

  it("accepts only the explicit profile, model, and thinking session selection", async () => {
    const { baseUrl } = await start(token);
    await putProfile(baseUrl);
    const created = await createSession(baseUrl);

    expect(created).toEqual({
      session_id: created.session_id,
      profile_id: "fixture",
      model: "fixture-model",
      thinking: "high",
      workspace: created.workspace,
      status: "wait",
    });
    expect(created.session_id).toMatch(/^[0-9A-HJKMNP-TV-Z]{26}$/);

    const read = (await fetch(`${baseUrl}/v1/sessions/${created.session_id}`, {
      headers: { authorization: `Bearer ${token}` },
    }).then((response) => response.json())) as SessionView;
    expect(read).toEqual(created);
    expect(Object.keys(read).sort()).toEqual(["model", "profile_id", "session_id", "status", "thinking", "workspace"]);

    const list = await fetch(`${baseUrl}/v1/sessions`, {
      headers: { authorization: `Bearer ${token}` },
    }).then((response) => response.json());
    expect(list).toEqual({ items: [created] });

    for (const body of [
      {},
      { profile_id: "fixture", model: "fixture-model", workspace: created.workspace },
      { profile_id: "fixture", model: "fixture-model", thinking: "xhigh", workspace: created.workspace },
      { profile_id: "missing", model: "fixture-model", thinking: "high", workspace: created.workspace },
      { profile_id: "fixture", model: "fixture-model", thinking: "high", workspace: path.join(created.workspace, "missing") },
      { profile_id: "fixture", model: "fixture-model", thinking: "high", workspace: created.workspace, session_id: "caller-owned" },
    ]) {
      const response = await fetch(`${baseUrl}/v1/sessions`, {
        method: "POST",
        headers: headers(),
        body: JSON.stringify(body),
      });
      expect(response.status).toBe(422);
    }

    const cancelled = await fetch(`${baseUrl}/v1/sessions/${created.session_id}/cancel`, {
      method: "POST",
      headers: { authorization: `Bearer ${token}` },
    });
    expect(cancelled.status).toBe(204);
    const cancelWithBody = await fetch(`${baseUrl}/v1/sessions/${created.session_id}/cancel`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({ reason: "legacy" }),
    });
    expect(cancelWithBody.status).toBe(422);
  });

  it("replaces an idle session selection as one validated triple", async () => {
    const { baseUrl } = await start(token);
    await putProfile(baseUrl);
    const alternate = profileDocument();
    alternate.models[0].default = false;
    const putAlternate = await fetch(`${baseUrl}/v1/profiles/alternate`, {
      method: "PUT",
      headers: headers(),
      body: JSON.stringify(alternate),
    });
    expect(putAlternate.status).toBe(200);
    const session = await createSession(baseUrl);

    const changed = await fetch(`${baseUrl}/v1/sessions/${session.session_id}/selection`, {
      method: "PUT",
      headers: headers(),
      body: JSON.stringify({ profile_id: "alternate", model: "fixture-model", thinking: "low" }),
    });
    expect(changed.status).toBe(200);
    expect(await changed.json()).toEqual({
      ...session,
      profile_id: "alternate",
      thinking: "low",
    });

    for (const body of [
      { profile_id: "alternate", model: "fixture-model", thinking: "xhigh" },
      { profile_id: "missing", model: "fixture-model", thinking: "low" },
      { profile_id: "alternate", model: "fixture-model", thinking: "low", automatic: true },
    ]) {
      const response = await fetch(`${baseUrl}/v1/sessions/${session.session_id}/selection`, {
        method: "PUT",
        headers: headers(),
        body: JSON.stringify(body),
      });
      expect(response.status).toBe(422);
    }
  });

  it("appends every content submission and reads durable messages from the tail backwards", { timeout: 20_000 }, async () => {
    const { baseUrl } = await start(token);
    await putProfile(baseUrl);
    const session = await createSession(baseUrl);

    const legacy = await fetch(`${baseUrl}/v1/sessions/${session.session_id}/mailbox`, {
      method: "POST",
      headers: headers(),
      body: JSON.stringify({ content: "same", message_id: "legacy-id" }),
    });
    expect(legacy.status).toBe(422);

    await append(baseUrl, session.session_id, "same");
    await append(baseUrl, session.session_id, "same");
    await append(baseUrl, session.session_id, "third");

    const settled = await waitFor(
      () => readMessages(baseUrl, session.session_id),
      (page) => page.items.filter((item) => item.type === "message" && item.role === "user").length === 3,
      "three independent mailbox messages are durable",
    );
    expect(settled.items.filter((item) => item.type === "message" && item.role === "user").map((item) => item.content)).toEqual(["same", "same", "third"]);
    expect(
      settled.items.every((item) => {
        if (item.type === "wait") return Object.keys(item).sort().join(",") === "reason,type";
        return Object.keys(item).sort().join(",") === "content,role,type";
      }),
    ).toBe(true);

    const all: MessageItem[] = [];
    let cursor: string | null = null;
    do {
      const query = `?limit=2${cursor === null ? "" : `&before=${encodeURIComponent(cursor)}`}`;
      const page = await readMessages(baseUrl, session.session_id, query);
      if (page.older_cursor !== null) {
        expect(page.older_cursor).toMatch(/^m\.[0-9A-HJKMNP-TV-Z]{26}$/);
      }
      all.unshift(...page.items);
      cursor = page.older_cursor;
    } while (cursor !== null);
    expect(all).toEqual(settled.items);
  });

  it("streams only public message items and transient assistant deltas", { timeout: 20_000 }, async () => {
    const { baseUrl } = await start(token);
    await putProfile(baseUrl);
    const session = await createSession(baseUrl);
    const response = await fetch(`${baseUrl}/v1/sessions/${session.session_id}/events`, {
      headers: { authorization: `Bearer ${token}`, accept: "text/event-stream" },
    });
    expect(response.status).toBe(200);

    const reader = response.body!.getReader();
    const frames: Array<{ event: string; data: Record<string, unknown> }> = [];
    const decoder = new TextDecoder();
    let buffer = "";
    const pump = (async () => {
      while (true) {
        const next = await reader.read();
        if (next.done) return;
        buffer += decoder.decode(next.value, { stream: true });
        let boundary = buffer.indexOf("\n\n");
        while (boundary >= 0) {
          const frame = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const event = /^event: (.+)$/m.exec(frame)?.[1] ?? "message";
          const data = /^data: (.+)$/m.exec(frame)?.[1];
          if (data) frames.push({ event, data: JSON.parse(data) as Record<string, unknown> });
          boundary = buffer.indexOf("\n\n");
        }
      }
    })();
    cleanups.push(async () => {
      await reader.cancel().catch(() => {});
      await pump.catch(() => {});
    });

    await append(baseUrl, session.session_id, "stream me");
    await waitFor(
      () => frames,
      (current) => current.some((frame) => frame.event === "assistant_delta") && current.some((frame) => frame.event === "message" && frame.data.role === "assistant"),
      "public assistant stream",
    );
    expect(frames.every((frame) => ["message", "wait", "assistant_delta"].includes(frame.event))).toBe(true);
    expect(frames.filter((frame) => frame.event === "assistant_delta").every((frame) => Object.keys(frame.data).join(",") === "text")).toBe(true);
    expect(JSON.stringify(frames)).not.toMatch(/activation|model_round|mailbox_seq|stream_version/);
  });

  it("has one readiness route and protects every application route only when a token was supplied", async () => {
    const protectedAgent = await start(token);
    expect((await fetch(`${protectedAgent.baseUrl}/readyz`)).status).toBe(200);
    expect((await fetch(`${protectedAgent.baseUrl}/v1/profiles`)).status).toBe(401);
    expect((await fetch(`${protectedAgent.baseUrl}/v1/sessions`)).status).toBe(401);

    for (const route of ["/healthz", "/v1/identity", "/v1/health", "/v1/capabilities", "/v1/auth-replicas", "/v1/models"] as const) {
      expect((await fetch(`${protectedAgent.baseUrl}${route}`)).status).toBe(404);
    }

    const openAgent = await start();
    const response = await fetch(`${openAgent.baseUrl}/v1/profiles`);
    expect(response.status).toBe(200);
  });
});
