import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { brokerRoot, getFreePort, removeTempRoot, spawnAgent, spawnBinary, stopChild, waitFor, waitForReady, writeConfig } from "./helpers.js";
import { MockSlackServer } from "./helpers/mock-slack-server.js";

const agentToken = "merged-mailbox-token";

describe.sequential("Gateway and Agent mailbox integration", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("routes Slack input to Agent without projecting assistant transcript", { timeout: 60_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "merged-mailbox-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    await writeFakeProfile(tempRoot);

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

    const agent = spawnAgent(tempRoot, true, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    const gateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(gateway));

    const agentBase = `http://127.0.0.1:${agentPort}`;
    await waitForReady(`${agentBase}/readyz`, "Agent readyz");
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "Gateway broker readyz");
    await waitForReady(`http://127.0.0.1:${gatewayPort}/readyz`, "Gateway Slack readyz");
    await slack.waitForSocket();

    const bot = await fetch(`http://127.0.0.1:${gatewayPort}/bot`);
    expect(bot.status).toBe(200);
    await expect(bot.json()).resolves.toMatchObject({ ok: true, self: { userId: "UBOT" } });

    await slack.sendEvent("evt-merged-1", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "100.200",
      ts: "100.201",
      text: "<@UBOT> hello mailbox",
    });

    await waitFor(
      () => readInboundMessages(stateDir, "C123:100.200"),
      (rows) => rows.some((row) => row.message_ts === "100.201" && row.status === "delivered"),
      "mailbox receipt",
    );
    const identity = readSessionIdentity(stateDir, "C123:100.200");
    const workspace = await fs.realpath(path.join(tempRoot, "workspaces", "slack", "C123", "100.200"));
    expect(identity.workspace_path).toBe(workspace);
    await waitFor(
      async () => readAgentStatus(agentBase, identity.id),
      (status) => status === "wait",
      "Agent session idle",
    );
    expect(slack.postedMessages).toHaveLength(0);

    const statusResponse = await fetch(`http://127.0.0.1:${gatewayPort}/threads/C123/100.200/status`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ status: "Running bash..." }),
    });
    expect(statusResponse.status).toBe(200);
    await waitFor(
      () => slack.assistantStatusUpdates,
      (updates) => updates.some((update) => update.channel === "C123" && update.status === "Running bash..."),
      "explicit Slack status",
    );

    const removedStateApi = await fetch(`http://127.0.0.1:${runtimePort}/chat/post-state`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({}),
    });
    expect(removedStateApi.status).toBe(404);

    const files = await fs.readdir(stateDir);
    expect(files.some((name) => name.startsWith("spool.sqlite"))).toBe(false);
    expect(files).toContain("gateway.sqlite");
    expect(files).not.toContain("runtime.sqlite");
    expect(files).not.toContain("control.sqlite");
    const gatewayDb = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
    const gatewayTables = gatewayDb
      .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('admin_operations', 'admin_audit_events') ORDER BY name")
      .all()
      .map((row) => String(row.name));
    gatewayDb.close();
    expect(gatewayTables).toEqual(["admin_audit_events", "admin_operations"]);

    const reset = await fetch(`http://127.0.0.1:${runtimePort}/slack/sessions/${encodeURIComponent("C123:100.200")}/reset`, {
      method: "POST",
    });
    expect(reset.status).toBe(200);
    expect(readInboundMessages(stateDir, "C123:100.200").some((row) => row.source === "admin_session_reset")).toBe(false);
    const resetIdentity = readSessionIdentity(stateDir, "C123:100.200");
    expect(resetIdentity.id).not.toBe(identity.id);
    expect(resetIdentity.workspace_path).toBe(workspace);
    expect(slack.postedMessages.some((message) => message.text.includes("admin_session_reset"))).toBe(false);

    await stopChild(agent);
    const failedDelete = await fetch(`http://127.0.0.1:${runtimePort}/slack/sessions/${encodeURIComponent("C123:100.200")}`, {
      method: "DELETE",
    });
    expect(failedDelete.status).toBe(500);
    expect(readSessionIdentity(stateDir, "C123:100.200")).toEqual(resetIdentity);

    const restartedAgent = spawnAgent(tempRoot, true, undefined, agentToken);
    cleanups.push(async () => stopChild(restartedAgent));
    await waitForReady(`${agentBase}/readyz`, "restarted Agent readyz");
    const deleted = await fetch(`http://127.0.0.1:${runtimePort}/slack/sessions/${encodeURIComponent("C123:100.200")}`, {
      method: "DELETE",
    });
    expect(deleted.status).toBe(200);
    expect(readOptionalSessionIdentity(stateDir, "C123:100.200")).toBeUndefined();
    expect((await fs.stat(workspace)).isDirectory()).toBe(true);
  });

  it("deduplicates a Slack redelivery across Gateway restart before appending a second mailbox message", { timeout: 60_000 }, async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "merged-mailbox-replay-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    await writeFakeProfile(tempRoot);

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

    const agent = spawnAgent(tempRoot, true, undefined, agentToken);
    cleanups.push(async () => stopChild(agent));
    await waitForReady(`http://127.0.0.1:${agentPort}/readyz`, "Agent readyz");

    const firstGateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "first Gateway readyz");
    await slack.waitForSocket();
    await slack.sendEvent("evt-replay-1", {
      type: "app_mention",
      user: "U123",
      channel: "C223",
      thread_ts: "200.200",
      ts: "200.201",
      text: "<@UBOT> replay me",
    });
    await waitFor(
      () => readInboundMessages(stateDir, "C223:200.200"),
      (rows) => rows.some((row) => row.message_ts === "200.201" && row.status === "delivered"),
      "first mailbox receipt",
    );
    const identity = readSessionIdentity(stateDir, "C223:200.200");
    firstGateway.kill("SIGKILL");
    await new Promise<void>((resolve) => firstGateway.once("exit", () => resolve()));

    const secondGateway = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", tempRoot, "--fake-agent", "--agent-token", agentToken],
    });
    cleanups.push(async () => stopChild(secondGateway));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`, "second Gateway readyz");
    await slack.waitForSocket();
    await slack.sendEvent("evt-replay-2", {
      type: "app_mention",
      user: "U123",
      channel: "C223",
      thread_ts: "200.200",
      ts: "200.201",
      text: "<@UBOT> replay me",
    });
    await waitFor(
      () => slack.acknowledgedEnvelopeIds,
      (ids) => ids.includes("env-evt-replay-2"),
      "redelivered Slack envelope mailbox receipt",
    );
    expect(readInboundMessages(stateDir, "C223:200.200").filter((row) => row.message_ts === "200.201")).toHaveLength(1);
    const agentMessages = (await fetch(`http://127.0.0.1:${agentPort}/v1/sessions/${identity.id}/messages`, {
      headers: { authorization: `Bearer ${agentToken}` },
    }).then((response) => response.json())) as { items?: Array<{ type?: string; role?: string; content?: string }> };
    expect(agentMessages.items?.filter((message) => message.role === "user" && message.content?.includes("replay me"))).toHaveLength(1);
    expect(slack.postedMessages).toHaveLength(0);
  });
});

async function writeFakeProfile(dataRoot: string): Promise<void> {
  const profileDir = path.join(dataRoot, "profiles");
  await fs.mkdir(profileDir, { recursive: true });
  await fs.writeFile(
    path.join(profileDir, "test.json"),
    JSON.stringify({
      provider: "xai",
      billing: "subscription",
      models: [
        {
          id: "grok-4.6",
          api: "openai-completions",
          streaming: true,
          thinking: ["off"],
          default_thinking: "off",
          capabilities: { input: ["text"] },
          default: true,
        },
      ],
      auth: { type: "api_key", key: "test-key" },
    }),
  );
}

async function readAgentStatus(baseUrl: string, sessionId: string): Promise<string> {
  const response = await fetch(`${baseUrl}/v1/sessions/${sessionId}`, {
    headers: { authorization: `Bearer ${agentToken}` },
  });
  if (!response.ok) return "";
  const body = (await response.json()) as { status?: string };
  return String(body.status || "");
}

function readSessionIdentity(stateDir: string, key: string): { id: string; workspace_path: string } {
  const session = readOptionalSessionIdentity(stateDir, key);
  if (!session) throw new Error(`missing session ${key}`);
  return session;
}

function readOptionalSessionIdentity(stateDir: string, key: string): { id: string; workspace_path: string } | undefined {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    return db.prepare("SELECT id, workspace_path FROM sessions WHERE key = ?").get(key) as { id: string; workspace_path: string } | undefined;
  } finally {
    db.close();
  }
}

function readInboundMessages(stateDir: string, sessionKey: string): Array<{ message_ts: string; source: string; status: string }> {
  const db = new DatabaseSync(path.join(stateDir, "gateway.sqlite"), { readOnly: true });
  try {
    return db.prepare("SELECT message_ts, source, status FROM inbound_messages WHERE session_key = ? ORDER BY created_at, message_ts").all(sessionKey) as Array<{ message_ts: string; source: string; status: string }>;
  } finally {
    db.close();
  }
}
