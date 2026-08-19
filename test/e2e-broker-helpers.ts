import fs from "node:fs/promises";

import http from "node:http";

import path from "node:path";

import { once } from "node:events";

import { spawn } from "node:child_process";

import { fileURLToPath, pathToFileURL } from "node:url";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";
import type { ChatPlatform } from "../src/services/chat/chat-types.js";
import type { CodexInputItem } from "../src/services/codex/app-server-client.js";
import { StateStore } from "../src/store/state-store.js";
import type { PersistedAgentTraceEvent, PersistedAgentTurnUsage, PersistedBackgroundJob, PersistedInboundMessage, SlackSessionRecord } from "../src/types.js";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";

export const brokerRoot = path.dirname(fileURLToPath(new URL("../package.json", import.meta.url)));

export const DEFAULT_E2E_TIMEOUT_MS = 30_000;

export const DAY_MS = 24 * 60 * 60 * 1000;

export const FEISHU_E2E_APP_ID = "cli_0123456789abcdef";
export const FEISHU_E2E_APP_SECRET = "feishu-e2e-secret";
export const FEISHU_E2E_BOT_OPEN_ID = "ou_bot";

export function feishuE2eSdkRegisterUrl(): string {
  return pathToFileURL(path.join(brokerRoot, "test/helpers/mock-feishu-sdk-register.mjs")).href;
}

export function createFeishuE2eEnv(feishuPort: number, extra?: Record<string, string>): Record<string, string> {
  return {
    FEISHU_ENABLED: "true",
    FEISHU_APP_ID: FEISHU_E2E_APP_ID,
    FEISHU_APP_SECRET: FEISHU_E2E_APP_SECRET,
    FEISHU_BOT_OPEN_ID: FEISHU_E2E_BOT_OPEN_ID,
    FEISHU_GROUP_MESSAGE_MODE: "all",
    FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED: "true",
    FEISHU_STARTUP_REQUIRED: "true",
    LOG_LEVEL: "debug",
    FEISHU_MOCK_ORIGIN: `http://127.0.0.1:${feishuPort}`,
    FEISHU_MOCK_WS_URL: `ws://127.0.0.1:${feishuPort}/socket`,
    ...extra,
  };
}

export async function startBrokerProcess(options: { readonly port: number; readonly slackPort: number; readonly codexUrl: string; readonly tempRoot: string; readonly extraEnv?: Record<string, string> | undefined; readonly nodeImports?: readonly string[] | undefined }): Promise<{
  readonly baseUrl: string;
  readonly stop: () => Promise<void>;
  readonly logs: readonly string[];
}> {
  const logs: string[] = [];
  const importOption = (options.nodeImports ?? []).map((specifier) => `--import ${specifier}`).join(" ");
  const nodeOptions = [options.extraEnv?.NODE_OPTIONS ?? process.env.NODE_OPTIONS, importOption].filter((value) => Boolean(value)).join(" ");
  const child = spawn("pnpm", ["exec", "tsx", "src/index.ts"], {
    cwd: brokerRoot,
    env: {
      ...process.env,
      ...options.extraEnv,
      SLACK_APP_TOKEN: "xapp-test",
      SLACK_BOT_TOKEN: "xoxb-test",
      SLACK_API_BASE_URL: `http://127.0.0.1:${options.slackPort}/api`,
      SLACK_SOCKET_OPEN_URL: "apps.connections.open",
      SLACK_INITIAL_THREAD_HISTORY_COUNT: "8",
      SLACK_HISTORY_API_MAX_LIMIT: "50",
      FEISHU_ENABLED: options.extraEnv?.FEISHU_ENABLED ?? "false",
      STATE_DIR: path.join(options.tempRoot, "state"),
      SESSIONS_ROOT: path.join(options.tempRoot, "sessions"),
      REPOS_ROOT: path.join(options.tempRoot, "repos"),
      JOBS_ROOT: path.join(options.tempRoot, "jobs"),
      LOG_DIR: path.join(options.tempRoot, "logs"),
      CODEX_HOME: path.join(options.tempRoot, "codex-home"),
      PORT: String(options.port),
      BROKER_HTTP_BASE_URL: `http://127.0.0.1:${options.port}`,
      CODEX_APP_SERVER_URL: options.codexUrl,
      DEBUG: "1",
      ...(nodeOptions ? { NODE_OPTIONS: nodeOptions } : {}),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout.on("data", (chunk) => {
    logs.push(chunk.toString());
  });
  child.stderr.on("data", (chunk) => {
    logs.push(chunk.toString());
  });

  await waitForHttpReady(`http://127.0.0.1:${options.port}`, logs);

  return {
    baseUrl: `http://127.0.0.1:${options.port}`,
    logs,
    stop: async () => {
      if (child.exitCode !== null || child.signalCode !== null) {
        return;
      }

      child.kill("SIGTERM");
      const graceful = await Promise.race([once(child, "exit").then(() => true), delay(5_000).then(() => false)]);
      if (graceful) {
        return;
      }

      child.kill("SIGKILL");
      await once(child, "exit");
    },
  };
}

export async function waitForHttpReady(url: string, logs: readonly string[], timeoutMs = DEFAULT_E2E_TIMEOUT_MS): Promise<void> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) {
        return;
      }
    } catch {
      // ignore and retry
    }

    await delay(200);
  }

  throw new Error(`Timed out waiting for broker readiness: ${url}\n${logs.join("")}`);
}

export async function waitFor(predicate: () => boolean | Promise<boolean>, label: string, timeoutMs = DEFAULT_E2E_TIMEOUT_MS): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if (await predicate()) {
        return;
      }
    } catch (error) {
      if (!isTransientSqliteLock(error)) {
        throw error;
      }
    }
    await delay(100);
  }

  throw new Error(`Timed out waiting for ${label}`);
}

export function isTransientSqliteLock(error: unknown): boolean {
  return error instanceof Error && /database is locked/i.test(error.message);
}

export async function waitForSessionIdle(tempRoot: string, sessionKey: string, timeoutMs = DEFAULT_E2E_TIMEOUT_MS): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastSession: SlackSessionRecord | undefined;

  while (Date.now() < deadline) {
    try {
      const session = await readSessionRecord(tempRoot, sessionKey);
      lastSession = session;
      if (!session.activeTurnId) {
        return;
      }
    } catch {
      // session file may not exist yet
    }

    await delay(100);
  }

  throw new Error(
    `Timed out waiting for session idle: ${sessionKey}; lastSession=${JSON.stringify({
      activeTurnId: lastSession?.activeTurnId ?? null,
      lastTurnSignalKind: lastSession?.lastTurnSignalKind ?? null,
      lastTurnSignalTurnId: lastSession?.lastTurnSignalTurnId ?? null,
    })}`,
  );
}

export async function waitForSessionActive(tempRoot: string, sessionKey: string, timeoutMs = DEFAULT_E2E_TIMEOUT_MS): Promise<void> {
  const deadline = Date.now() + timeoutMs;

  while (Date.now() < deadline) {
    try {
      const session = await readSessionRecord(tempRoot, sessionKey);
      if (session.activeTurnId) {
        return;
      }
    } catch {
      // session file may not exist yet
    }

    await delay(100);
  }

  throw new Error(`Timed out waiting for session active: ${sessionKey}`);
}

export async function readSessionRecord(tempRoot: string, sessionKey: string): Promise<SlackSessionRecord> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    const session = store.getSession(sessionKey);
    if (!session) {
      throw new Error(`Unknown session: ${sessionKey}`);
    }
    return session;
  } finally {
    store.close();
  }
}

export async function readInboundMessages(tempRoot: string, sessionKey: string): Promise<PersistedInboundMessage[]> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    return store.listInboundMessages({ sessionKey });
  } finally {
    store.close();
  }
}

export async function readAgentTraceEvents(tempRoot: string, sessionKey: string): Promise<PersistedAgentTraceEvent[]> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    return store.listAgentTraceEvents(sessionKey);
  } finally {
    store.close();
  }
}

export async function readAgentTurnUsage(tempRoot: string): Promise<PersistedAgentTurnUsage[]> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    return store.listAgentTurnUsage();
  } finally {
    store.close();
  }
}

export async function readBackgroundJobs(tempRoot: string, sessionKey?: string): Promise<PersistedBackgroundJob[]> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    return store.listBackgroundJobs(sessionKey ? { sessionKey } : undefined);
  } finally {
    store.close();
  }
}

export async function seedBrokerSessions(
  tempRoot: string,
  sessions: ReadonlyArray<{
    readonly platform?: ChatPlatform | undefined;
    readonly conversationId: string;
    readonly rootMessageId: string;
  }>,
): Promise<void> {
  await fs.mkdir(path.join(tempRoot, "state"), { recursive: true });
  await fs.mkdir(path.join(tempRoot, "sessions"), { recursive: true });
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  const manager = new SessionManager({
    stateStore: store,
    sessionsRoot: path.join(tempRoot, "sessions"),
  });
  await manager.load();
  try {
    for (const session of sessions) {
      const platform = session.platform ?? "slack";
      if (platform === "slack") {
        await manager.ensureSession(session.conversationId, session.rootMessageId);
      } else {
        await manager.ensureChatSession({
          platform,
          conversationId: session.conversationId,
          rootMessageId: session.rootMessageId,
        });
      }
    }
  } finally {
    store.close();
  }
}

export async function writeGitHubPrBinding(
  tempRoot: string,
  binding: {
    readonly slackUserId: string;
    readonly githubLogin: string;
    readonly githubUserId: number;
    readonly token: string;
    readonly githubEmail?: string | undefined;
    readonly githubName?: string | undefined;
    readonly scopes?: readonly string[] | undefined;
    readonly revokedAt?: string | undefined;
  },
): Promise<void> {
  const bindingsDir = path.join(tempRoot, "state", "github-pr-identities", "bindings");
  await fs.mkdir(bindingsDir, { recursive: true });
  const now = new Date().toISOString();
  const fileName = `${encodeURIComponent(binding.slackUserId).replaceAll("%", "_")}.json`;
  await fs.writeFile(
    path.join(bindingsDir, fileName),
    `${JSON.stringify(
      {
        slackUserId: binding.slackUserId,
        githubLogin: binding.githubLogin,
        githubUserId: binding.githubUserId,
        token: binding.token,
        ...(binding.githubEmail ? { githubEmail: binding.githubEmail } : {}),
        ...(binding.githubName ? { githubName: binding.githubName } : {}),
        scopes: binding.scopes ?? ["repo"],
        createdAt: now,
        updatedAt: now,
        ...(binding.revokedAt ? { revokedAt: binding.revokedAt } : {}),
      },
      null,
      2,
    )}\n`,
    { mode: 0o600 },
  );
}

export async function withStateStore<T>(tempRoot: string, fn: (store: StateStore) => Promise<T> | T): Promise<T> {
  const store = new StateStore(path.join(tempRoot, "state"), path.join(tempRoot, "sessions"));
  await store.load();
  try {
    return await fn(store);
  } finally {
    store.close();
  }
}

export async function readHasProcessedEvent(tempRoot: string, eventId: string): Promise<boolean> {
  return await withStateStore(tempRoot, (store) => store.hasProcessedEvent(eventId));
}

export async function readPendingSlackEvents(tempRoot: string) {
  return await withStateStore(tempRoot, (store) => store.listPendingSlackEvents());
}

export async function delay(timeoutMs: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, timeoutMs));
}

export async function pathExists(targetPath: string): Promise<boolean> {
  try {
    await fs.access(targetPath);
    return true;
  } catch {
    return false;
  }
}

export async function removeTempRoot(tempRoot: string): Promise<void> {
  let lastError: unknown = undefined;
  for (let attempt = 0; attempt < 5; attempt += 1) {
    try {
      await fs.rm(tempRoot, { force: true, recursive: true });
      return;
    } catch (error) {
      lastError = error;
      await delay(100 * (attempt + 1));
    }
  }

  throw lastError;
}

export async function getFreePort(): Promise<number> {
  const server = http.createServer();
  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });

  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to allocate free port");
  }

  const port = address.port;
  await new Promise<void>((resolve) => {
    server.close(() => resolve());
  });
  return port;
}

export function collectTextInput(input: readonly CodexInputItem[]): string {
  return input
    .filter((item): item is Extract<CodexInputItem, { type: "text" }> => item.type === "text")
    .map((item) => item.text)
    .join("\n");
}

export function findStartedTurnTextContaining(mockCodex: MockCodexAppServer, needle: string): string | undefined {
  return mockCodex.turnsStarted.map((turn) => collectTextInput(turn.input)).find((text) => text.includes(needle));
}

export async function postJson(url: string, payload: Record<string, unknown>): Promise<void> {
  const response = await fetchJson(url, payload);
  if (response.status < 200 || response.status >= 300) {
    throw new Error(`HTTP ${response.status} for ${url}: ${JSON.stringify(response.body)}`);
  }
}

export async function fetchJson(
  url: string,
  payload?: Record<string, unknown>,
  options?: {
    readonly method?: string;
    readonly headers?: Record<string, string>;
  },
): Promise<{
  readonly status: number;
  readonly body: Record<string, unknown>;
}> {
  const response = await fetch(url, {
    method: options?.method ?? (payload ? "POST" : "GET"),
    headers: {
      ...(payload ? { "content-type": "application/json" } : {}),
      ...options?.headers,
    },
    ...(payload ? { body: JSON.stringify(payload) } : {}),
  });
  const text = await response.text();
  return {
    status: response.status,
    body: text ? (JSON.parse(text) as Record<string, unknown>) : {},
  };
}

export async function runTsxTool(options: { readonly script: string; readonly cwd: string; readonly args?: readonly string[]; readonly env?: Record<string, string> }): Promise<{
  readonly status: number;
  readonly stdout: string;
  readonly stderr: string;
}> {
  return await new Promise((resolve, reject) => {
    const child = spawn(path.join(brokerRoot, "node_modules/.bin/tsx"), [options.script, ...(options.args ?? [])], {
      cwd: options.cwd,
      env: {
        ...process.env,
        ...options.env,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code) => {
      resolve({
        status: code ?? 1,
        stdout,
        stderr,
      });
    });
  });
}

export function createDeferred<T>(): {
  readonly promise: Promise<T>;
  readonly resolve: (value: T | PromiseLike<T>) => void;
  readonly reject: (reason?: unknown) => void;
} {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((innerResolve, innerReject) => {
    resolve = innerResolve;
    reject = innerReject;
  });

  return {
    promise,
    resolve,
    reject,
  };
}
