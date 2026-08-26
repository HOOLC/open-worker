import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { brokerRoot, getFreePort, removeTempRoot, writeConfig } from "./helpers.js";

describe.sequential("rust runtime", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("serves readyz, exposes an empty snapshot, and sends only explicit Slack messages", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "runtime-e2e-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const dataRoot = path.join(tempRoot, "data");

    const posts: Array<{ url: string; body: string }> = [];
    const gatewayPort = await getFreePort();
    const gateway = http.createServer((request, response) => {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      if (request.method === "POST" && url.pathname === "/api/auth.test") {
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify({ ok: true, user_id: "UBOT", user: "zork" }));
        return;
      }
      if (request.method === "GET" && url.pathname === "/bot") {
        response.setHeader("content-type", "application/json");
        response.end(
          JSON.stringify({
            ok: true,
            self: { userId: "UBOT", mention: "<@UBOT>", surface: "Slack", username: "zork" },
          }),
        );
        return;
      }
      if (request.method === "GET" && url.pathname.startsWith("/threads/")) {
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify({ ok: true, messages: [] }));
        return;
      }
      const chunks: Buffer[] = [];
      request.on("data", (chunk) => chunks.push(chunk as Buffer));
      request.on("end", () => {
        posts.push({ url: url.pathname, body: Buffer.concat(chunks).toString() });
        response.setHeader("content-type", "application/json");
        response.end(JSON.stringify({ ok: true, ts: "1.2" }));
      });
    });
    await new Promise<void>((resolve) => gateway.listen(gatewayPort, "127.0.0.1", resolve));
    cleanups.push(
      async () =>
        await new Promise<void>((resolve, reject) => {
          gateway.close((error) => (error ? reject(error) : resolve()));
        }),
    );

    const runtimePort = await getFreePort();
    await writeConfig(dataRoot, {
      bind: {
        runtime: `127.0.0.1:${runtimePort}`,
        gateway: `127.0.0.1:${await getFreePort()}`,
      },
      slack: { api_base_url: `http://127.0.0.1:${gatewayPort}/api` },
    });
    const child = spawnRuntime({
      cwd: brokerRoot,
      args: ["--data", dataRoot, "--fake-agent"],
      env: { RUST_LOG: "info" },
    });
    cleanups.push(async () => stopChild(child));
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`);

    const ready = await fetch(`http://127.0.0.1:${runtimePort}/readyz`);
    expect(ready.status).toBe(200);
    await expect(ready.json()).resolves.toMatchObject({ ok: true, service: "zork-gateway" });

    const snapshot = await fetch(`http://127.0.0.1:${runtimePort}/internal/realtime/snapshot`);
    expect(snapshot.status).toBe(200);
    await expect(snapshot.json()).resolves.toMatchObject({ state: { sessions: [] } });

    expect(posts.some((entry) => entry.url.includes("chat.postMessage"))).toBe(false);

    const posted = await fetch(`http://127.0.0.1:${runtimePort}/chat/post-message`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        platform: "slack",
        conversationId: "C123",
        rootMessageId: "100.200",
        text: "hello from test",
        kind: "final",
      }),
    });
    expect(posted.status).toBe(200);
    await expect(posted.json()).resolves.toMatchObject({ ok: true, conversationId: "C123" });
    expect(posts.some((entry) => entry.url.includes("chat.postMessage"))).toBe(true);

    const chinese = await fetch(`http://127.0.0.1:${runtimePort}/chat/post-message`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        platform: "slack",
        conversationId: "C123",
        rootMessageId: "100.200",
        text: "你好，这是中文回复 **加粗**",
        kind: "progress",
      }),
    });
    expect(chinese.status).toBe(200);
    await expect(chinese.json()).resolves.toMatchObject({ ok: true });
    expect(posts.some((entry) => entry.body.includes("%E4%BD%A0%E5%A5%BD") || decodeURIComponent(entry.body).includes("你好"))).toBe(true);

    const stillUp = await fetch(`http://127.0.0.1:${runtimePort}/readyz`);
    expect(stillUp.status).toBe(200);

    const zorkCall = path.join(dataRoot, "bin/zork-call");
    expect(existsSync(zorkCall)).toBe(true);
    expect(existsSync(path.join(dataRoot, "bin/gh"))).toBe(true);
    const cli = await runCommand(zorkCall, ["chat", "post-message", "--text", "from rust cli", "--kind", "progress"], {
      BROKER_API_BASE: `http://127.0.0.1:${runtimePort}`,
      CHAT_PLATFORM: "slack",
      CHAT_CONVERSATION_ID: "C123",
      CHAT_ROOT_MESSAGE_ID: "100.200",
    });
    expect(cli.status, `${cli.stdout}\n${cli.stderr}`).toBe(0);
  }, 60_000);
});

function spawnRuntime(options: { readonly cwd: string; readonly args: readonly string[]; readonly env: Record<string, string> }): ChildProcess {
  const binary = path.join(options.cwd, "target/debug/zork-gateway");
  const result = spawnSync("cargo", ["build", "-p", "zork-gateway", "-p", "zork-call"], {
    cwd: options.cwd,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`failed to build zork-gateway:\n${result.stderr || result.stdout}`);
  }
  if (!existsSync(binary)) {
    throw new Error(`zork-gateway missing at ${binary}`);
  }
  return spawn(binary, [...options.args], {
    cwd: options.cwd,
    env: {
      ...process.env,
      ...options.env,
    },
    stdio: ["ignore", "inherit", "inherit"],
  });
}

function runCommand(binary: string, args: readonly string[], env: Record<string, string>): Promise<{ status: number; stdout: string; stderr: string }> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, [...args], {
      env: { ...process.env, ...env },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout?.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr?.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      resolve({ status: code ?? 1, stdout, stderr });
    });
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
  const deadline = Date.now() + 20_000;
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
  throw new Error(`runtime readyz failed: ${lastError}`);
}
