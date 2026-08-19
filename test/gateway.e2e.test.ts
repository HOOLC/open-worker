import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { SpoolStore } from "../src/store/spool-store.js";
import { brokerRoot, getFreePort, removeTempRoot } from "./e2e-broker-helpers.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";

describe.sequential("rust gateway", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("enqueues slack events and proxies outbound Slack API calls", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "gateway-e2e-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const stateDir = path.join(tempRoot, "state");
    await fs.mkdir(stateDir, { recursive: true });

    const mockSlack = new MockSlackServer("UBOT");
    const slackPort = await mockSlack.start();
    cleanups.push(async () => mockSlack.stop());

    const gatewayPort = await getFreePort();
    const child = spawnGateway({
      cwd: brokerRoot,
      env: {
        STATE_DIR: stateDir,
        PORT: String(gatewayPort),
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
        SLACK_API_BASE_URL: `http://127.0.0.1:${slackPort}/api`,
        RUST_LOG: "info",
      },
    });
    cleanups.push(async () => stopChild(child));

    await waitForReady(`http://127.0.0.1:${gatewayPort}/readyz`);
    await mockSlack.waitForSocket();
    await mockSlack.sendEvent("evt-gateway-1", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "100.200",
      ts: "100.201",
      text: "<@UBOT> hello gateway",
    });

    const store = new SpoolStore(stateDir);
    await store.load();
    const inbound = await waitFor(
      () => store.claim({ direction: "inbound", owner: "e2e-worker", leaseMs: 30_000 }),
      (rows) => rows.length > 0,
      "inbound spool",
    );
    expect(inbound.map((row) => row.id)).toEqual(["evt-gateway-1"]);
    const payload = JSON.parse(inbound[0]!.payload) as { event_id?: string; event?: { text?: string } };
    expect(payload.event_id).toBe("evt-gateway-1");
    expect(payload.event?.text).toContain("hello gateway");
    expect(existsSync(path.join(stateDir, "spool.sqlite"))).toBe(true);
    expect(existsSync(path.join(stateDir, "broker.sqlite"))).toBe(false);
    store.ack("evt-gateway-1");
    store.close();

    const posted = await fetch(`http://127.0.0.1:${gatewayPort}/slack/chat.postMessage`, {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded; charset=utf-8" },
      body: new URLSearchParams({
        channel: "C123",
        thread_ts: "100.200",
        text: "hello from gateway",
      }).toString(),
    });
    expect(posted.status).toBe(200);
    const postedPayload = (await posted.json()) as { ok?: boolean };
    expect(postedPayload.ok).toBe(true);
    const slackMessage = await mockSlack.waitForPostedMessage((message) => message.text.includes("hello from gateway"));
    expect(slackMessage.channel).toBe("C123");
    expect(slackMessage.threadTs).toBe("100.200");
  }, 60_000);
});

function spawnGateway(options: { readonly cwd: string; readonly env: Record<string, string> }): ChildProcess {
  const binary = path.join(options.cwd, "target/debug/zork-gateway");
  if (!existsSync(binary)) {
    throw new Error("zork-gateway debug binary is missing; run cargo build -p zork-gateway");
  }
  return spawn(binary, [], {
    cwd: options.cwd,
    env: {
      ...process.env,
      ...options.env,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
}

async function stopChild(child: ChildProcess): Promise<void> {
  if (child.exitCode != null || child.signalCode != null) {
    return;
  }
  child.kill("SIGTERM");
  await new Promise<void>((resolve) => {
    const timer = setTimeout(() => {
      child.kill("SIGKILL");
      resolve();
    }, 2_000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function waitForReady(url: string): Promise<void> {
  const deadline = Date.now() + 15_000;
  let lastError = "not ready";
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
      lastError = `status ${response.status}`;
    } catch (error) {
      lastError = error instanceof Error ? error.message : String(error);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`gateway readyz failed: ${lastError}`);
}

async function waitFor<T>(read: () => T, predicate: (value: T) => boolean, label: string): Promise<T> {
  const deadline = Date.now() + 10_000;
  let last: T | undefined;
  while (Date.now() < deadline) {
    last = read();
    if (predicate(last)) {
      return last;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`timed out waiting for ${label}`);
}
