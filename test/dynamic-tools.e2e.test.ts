import { existsSync, readFileSync } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { AppServerClient } from "../src/services/codex/app-server-client.js";
import { buildDynamicToolsDeclaration, handleToolCall, isReservedDynamicToolNamespace, type BrokerToolBackend, type DynamicToolBackend, type DynamicToolCallResult, type ThreadCoordinates } from "../src/services/codex/dynamic-tools.js";
import { brokerRoot, getFreePort, readBackgroundJobs, removeTempRoot, startBrokerProcess, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";
import type { MockTurnContext } from "./helpers/mock-codex-app-server.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";

const EXPECTED_NAMESPACES = ["chat", "coauthor", "job", "integration"] as const;
const FORBIDDEN_CURL_FRAGMENTS = ["/slack/post-message", "/chat/post-message", "/slack/thread-history", "git-coauthors/session-status", "/jobs/register", "/integrations/mcp-call"] as const;
const HISTORY_TEXT = "Alice: earlier thread history";
const brokerToolWiringPresent = isBrokerToolWiringPresent();

describe.sequential("dynamic tools e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("thread/start carries dynamicTools with all four namespaces and no reserved names", async () => {
    const harness = await startClientHarness(cleanups);
    await harness.client.ensureThread({
      channelId: "C123",
      rootThreadTs: "930.220",
      workspacePath: harness.workspace,
      sessionKey: "C123:930.220",
      platform: "slack",
    });

    const namespaces = readDeclaredNamespaces(harness.mock.dynamicTools ?? harness.mock.lastThreadStartParams?.dynamicTools);
    const names = namespaces.map((namespace) => namespace.name);
    expect(names).toEqual(expect.arrayContaining([...EXPECTED_NAMESPACES]));
    expect(new Set(names).size).toBe(names.length);
    expect(names).toHaveLength(EXPECTED_NAMESPACES.length);
    for (const name of names) {
      expect(isReservedDynamicToolNamespace(name)).toBe(false);
    }

    const catalog = buildDynamicToolsDeclaration();
    expect(catalog.map((namespace) => namespace.name)).toEqual(expect.arrayContaining([...EXPECTED_NAMESPACES]));
    expect(findNamespaceTools(namespaces, "chat")).toEqual(expect.arrayContaining(["post_message", "thread_history"]));
    expect(findNamespaceTools(namespaces, "job")).toEqual(expect.arrayContaining(["register"]));
    expect(findNamespaceTools(namespaces, "integration")).toEqual(expect.arrayContaining(["call"]));
    expect(findNamespaceTools(namespaces, "coauthor")).toEqual(expect.arrayContaining(["status"]));
  }, 30_000);

  it("chat.thread_history returns history text in contentItems", async () => {
    const recorded: Array<{ readonly method: string; readonly coords: ThreadCoordinates }> = [];
    const harness = await startClientHarness(cleanups, {
      backend: createStubBrokerBackend(recorded),
    });
    const threadId = await harness.client.ensureThread({
      channelId: "C123",
      rootThreadTs: "931.220",
      workspacePath: harness.workspace,
      sessionKey: "C123:931.220",
      platform: "slack",
    });

    const result = await harness.mock.invokeTool(threadId, "chat", "thread_history", {});
    expect(result.success).toBe(true);
    expect(contentText(result)).toContain(HISTORY_TEXT);
    expect(recorded.some((entry) => entry.method === "threadHistory" && entry.coords.threadId === threadId && entry.coords.channelId === "C123")).toBe(true);
  }, 30_000);

  it.skipIf(!brokerToolWiringPresent)(
    "chat.post_message posts to MockSlack with the session coordinates",
    async () => {
      const harness = await startBrokerHarness(cleanups);
      const threadTs = "932.220";
      await mention(harness, threadTs);
      const threadId = harness.mockCodex.threadStarts[0]?.threadId;
      expect(threadId).toBeTruthy();

      const result = await harness.mockCodex.invokeTool(threadId!, "chat", "post_message", {
        text: "hello from dynamic tool",
        kind: "progress",
      });
      expect(result.success).toBe(true);

      const posted = await harness.mockSlack.waitForPostedMessage((message) => message.text.includes("hello from dynamic tool"));
      expect(posted.channel).toBe("C123");
      expect(posted.threadTs).toBe(threadTs);
    },
    90_000,
  );

  it.skipIf(!brokerToolWiringPresent)(
    "job.register persists a background job",
    async () => {
      const harness = await startBrokerHarness(cleanups);
      const threadTs = "933.220";
      await mention(harness, threadTs);
      const threadId = harness.mockCodex.threadStarts[0]?.threadId;
      expect(threadId).toBeTruthy();

      const result = await harness.mockCodex.invokeTool(threadId!, "job", "register", {
        kind: "watch_ci",
        script: "#!/bin/sh\nsleep 30",
      });
      expect(result.success).toBe(true);

      await waitFor(async () => (await readBackgroundJobs(harness.tempRoot)).some((job) => job.kind === "watch_ci"), "persisted background job");
      expect(await readBackgroundJobs(harness.tempRoot)).toEqual(
        expect.arrayContaining([
          expect.objectContaining({
            kind: "watch_ci",
            sessionKey: `C123:${threadTs}`,
          }),
        ]),
      );
    },
    90_000,
  );

  it("hanging backend is answered success:false within ~35s and the turn continues", async () => {
    const releaseTurn = createDeferred();
    const harness = await startClientHarness(cleanups, {
      backend: createStubBrokerBackend([], {
        callIntegration: () => new Promise(() => {}),
      }),
      onTurnStart: async (context) => {
        await releaseTurn.promise;
        context.complete("continued after tool timeout");
      },
    });
    const threadId = await harness.client.ensureThread({
      channelId: "C123",
      rootThreadTs: "934.220",
      workspacePath: harness.workspace,
      sessionKey: "C123:934.220",
      platform: "slack",
    });
    const started = await harness.client.startTurn(threadId, harness.workspace, [
      {
        type: "text",
        text: "call hanging integration",
        text_elements: [],
      },
    ]);
    await waitFor(() => harness.mock.turnsStarted.length > 0, "held turn started");

    const startedAt = Date.now();
    const result = await harness.mock.invokeTool(threadId, "integration", "call", {
      server: "linear",
      name: "hang",
    });
    const elapsedMs = Date.now() - startedAt;
    expect(result.success).toBe(false);
    expect(contentText(result)).toMatch(/timed? ?out/i);
    expect(elapsedMs).toBeGreaterThan(20_000);
    expect(elapsedMs).toBeLessThan(35_000);

    releaseTurn.resolve();
    const turn = await started.completion;
    expect(turn.aborted).toBe(false);
    expect(turn.finalMessage).toContain("continued after tool timeout");
  }, 60_000);

  it("item/tool/call for an unknown threadId is rejected without leaking another session", async () => {
    const recorded: Array<{ readonly method: string; readonly args: unknown; readonly coords: ThreadCoordinates }> = [];
    const harness = await startClientHarness(cleanups, {
      backend: createStubBrokerBackend(recorded),
    });
    const threadA = await harness.client.ensureThread({
      channelId: "C123",
      rootThreadTs: "935.220",
      workspacePath: harness.workspace,
      sessionKey: "C123:935.220",
      platform: "slack",
    });
    const threadB = await harness.client.ensureThread({
      channelId: "C456",
      rootThreadTs: "935.221",
      workspacePath: harness.workspace,
      sessionKey: "C456:935.221",
      platform: "slack",
    });
    expect(threadA).not.toBe(threadB);
    expect(harness.client.readThreadCoordinates(threadA)?.channelId).toBe("C123");
    expect(harness.client.readThreadCoordinates(threadB)?.channelId).toBe("C456");
    expect(harness.client.readThreadCoordinates("missing-thread")).toBeUndefined();

    const outcome = await invokeToolOutcome(harness.mock, "missing-thread", "chat", "post_message", {
      text: "should not leak",
      kind: "progress",
    });
    if (outcome.ok) {
      expect(outcome.result.success).toBe(false);
      expect(contentText(outcome.result)).toMatch(/unknown thread/i);
    } else {
      expect(outcome.error.message.length).toBeGreaterThan(0);
    }

    expect(recorded).toEqual([]);
    expect(harness.client.readThreadCoordinates(threadA)?.sessionKey).toBe("C123:935.220");
    expect(harness.client.readThreadCoordinates(threadB)?.sessionKey).toBe("C456:935.221");
  }, 30_000);

  it.skipIf(!brokerToolWiringPresent)(
    "baseInstructions name the injected tools and omit migrated curl fragments",
    async () => {
      const harness = await startBrokerHarness(cleanups);
      await mention(harness, "936.220");
      const baseInstructions = String(harness.mockCodex.threadStarts[0]?.baseInstructions ?? harness.mockCodex.lastThreadStartParams?.baseInstructions ?? "");
      expect(baseInstructions).toContain("chat.post_message");
      expect(baseInstructions).toContain("chat.thread_history");
      expect(baseInstructions).toContain("job.register");
      expect(baseInstructions).toContain("integration.call");
      expect(baseInstructions).toContain("coauthor.status");
      for (const fragment of FORBIDDEN_CURL_FRAGMENTS) {
        expect(baseInstructions).not.toContain(fragment);
      }
      expect(baseInstructions).not.toContain("{{");
    },
    90_000,
  );

});

async function startClientHarness(
  cleanups: Array<() => Promise<void>>,
  options?: {
    readonly backend?: BrokerToolBackend;
    readonly onTurnStart?: (context: MockTurnContext) => Promise<void> | void;
  },
): Promise<{
  readonly mock: MockCodexAppServer;
  readonly client: AppServerClient;
  readonly workspace: string;
}> {
  const workspace = await fs.mkdtemp(path.join(os.tmpdir(), "dynamic-tools-client-"));
  cleanups.push(async () => {
    await removeTempRoot(workspace);
  });
  const mock = new MockCodexAppServer({
    ...(options?.onTurnStart ? { onTurnStart: options.onTurnStart } : {}),
  });
  const url = await mock.start();
  cleanups.push(async () => {
    await mock.stop();
  });
  const client = new AppServerClient({
    url,
    serviceName: "dynamic-tools-e2e",
    brokerHttpBaseUrl: "http://127.0.0.1:9",
    reposRoot: path.join(workspace, "repos"),
  });
  attachBrokerToolBackend(client, options?.backend ?? createStubBrokerBackend());
  cleanups.push(async () => {
    await client.close();
  });
  await client.connect();
  return { mock, client, workspace };
}

async function startBrokerHarness(
  cleanups: Array<() => Promise<void>>,
  options?: { readonly extraEnv?: Record<string, string> },
): Promise<{
  readonly baseUrl: string;
  readonly tempRoot: string;
  readonly mockCodex: MockCodexAppServer;
  readonly mockSlack: MockSlackServer;
}> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "dynamic-tools-broker-"));
  cleanups.push(async () => {
    await removeTempRoot(tempRoot);
  });
  const mockSlack = new MockSlackServer("UBOT", {
    botId: "BBOT",
    appId: "AAPP",
  });
  const mockCodex = new MockCodexAppServer();
  const slackPort = await mockSlack.start();
  const codexUrl = await mockCodex.start();
  cleanups.push(async () => {
    await mockCodex.stop();
    await mockSlack.stop();
  });
  const extraEnv = { ...brokerDynamicToolsEnv(), ...options?.extraEnv };
  const broker = await startBrokerProcess({
    port: await getFreePort(),
    slackPort,
    codexUrl,
    tempRoot,
    ...(Object.keys(extraEnv).length > 0 ? { extraEnv } : {}),
  });
  cleanups.push(() => broker.stop());
  return {
    baseUrl: broker.baseUrl,
    tempRoot,
    mockCodex,
    mockSlack,
  };
}

async function mention(
  harness: {
    readonly mockSlack: MockSlackServer;
    readonly tempRoot: string;
    readonly mockCodex: MockCodexAppServer;
  },
  threadTs: string,
): Promise<void> {
  await harness.mockSlack.sendEvent(`evt-dynamic-tools-${threadTs}`, {
    type: "app_mention",
    user: "U123",
    channel: "C123",
    thread_ts: threadTs,
    ts: `${threadTs}1`,
    text: `<@UBOT> start dynamic tools ${threadTs}`,
  });
  await waitFor(() => harness.mockCodex.threadStarts.length >= 1, "codex thread/start");
  await waitForSessionIdle(harness.tempRoot, `C123:${threadTs}`);
}

function attachBrokerToolBackend(client: AppServerClient, backend: BrokerToolBackend): void {
  const dynamicBackend: DynamicToolBackend = {
    listDeclarations: () => buildDynamicToolsDeclaration(),
    handleCall: async (call) => await handleToolCall(call, (threadId) => client.readThreadCoordinates(threadId), backend),
  };
  client.setDynamicToolBackend(dynamicBackend);
  if (hasServerRequestHandler(client, "item/tool/call")) {
    return;
  }

  client.setServerRequestHandler("item/tool/call", async (params) => {
    const result = await handleToolCall(params, (threadId) => client.readThreadCoordinates(threadId), backend);
    return {
      contentItems: [...result.contentItems],
      success: result.success,
      ...(result.reason ? { reason: result.reason } : {}),
    };
  });
}

function hasServerRequestHandler(client: AppServerClient, method: string): boolean {
  const handlers = (client as unknown as { privateServerRequestHandlers?: Map<string, unknown> }).privateServerRequestHandlers;
  return Boolean(handlers?.has(method));
}

function createStubBrokerBackend(recorded: Array<{ readonly method: string; readonly args?: unknown; readonly coords: ThreadCoordinates }> = [], overrides?: Partial<BrokerToolBackend>): BrokerToolBackend {
  return {
    postMessage: async (args, coords) => {
      recorded.push({ method: "postMessage", args, coords });
      return { ok: true };
    },
    postState: async (args, coords) => {
      recorded.push({ method: "postState", args, coords });
      return { ok: true };
    },
    postFile: async (args, coords) => {
      recorded.push({ method: "postFile", args, coords });
      return { ok: true };
    },
    threadHistory: async (args, coords) => {
      recorded.push({ method: "threadHistory", args, coords });
      return { formattedText: HISTORY_TEXT };
    },
    coauthorStatus: async (args, coords) => {
      recorded.push({ method: "coauthorStatus", args, coords });
      return { ok: true };
    },
    coauthorConfigure: async (args, coords) => {
      recorded.push({ method: "coauthorConfigure", args, coords });
      return { ok: true };
    },
    registerJob: async (args, coords) => {
      recorded.push({ method: "registerJob", args, coords });
      return { ok: true, id: "job-stub" };
    },
    listIntegrationTools: async (args, coords) => {
      recorded.push({ method: "listIntegrationTools", args, coords });
      return { tools: [] };
    },
    callIntegration: async (args, coords) => {
      recorded.push({ method: "callIntegration", args, coords });
      return { ok: true };
    },
    ...overrides,
  };
}

function contentText(result: DynamicToolCallResult): string {
  return result.contentItems.map((item) => item.text).join("\n");
}

async function invokeToolOutcome(mock: MockCodexAppServer, threadId: string, namespace: string, tool: string, args: Record<string, unknown>): Promise<{ readonly ok: true; readonly result: DynamicToolCallResult } | { readonly ok: false; readonly error: Error }> {
  try {
    return { ok: true, result: await mock.invokeTool(threadId, namespace, tool, args) };
  } catch (error) {
    return { ok: false, error: error instanceof Error ? error : new Error(String(error)) };
  }
}

function readDeclaredNamespaces(value: unknown): Array<{ readonly name: string; readonly tools?: ReadonlyArray<{ readonly name?: string }> }> {
  if (!Array.isArray(value)) {
    return [];
  }

  return value.flatMap((entry) => {
    if (!entry || typeof entry !== "object" || typeof (entry as { name?: unknown }).name !== "string") {
      return [];
    }
    return [entry as { readonly name: string; readonly tools?: ReadonlyArray<{ readonly name?: string }> }];
  });
}

function findNamespaceTools(namespaces: ReadonlyArray<{ readonly name: string; readonly tools?: ReadonlyArray<{ readonly name?: string }> }>, name: string): string[] {
  return (namespaces.find((namespace) => namespace.name === name)?.tools ?? []).flatMap((tool) => (typeof tool.name === "string" ? [tool.name] : []));
}

function createDeferred(): {
  readonly promise: Promise<void>;
  readonly resolve: () => void;
} {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

function isBrokerToolWiringPresent(): boolean {
  return isBrokerToolBackendWired() && isPromptRewritten();
}

function isBrokerToolBackendWired(): boolean {
  if (existsSync(path.join(brokerRoot, "src/services/broker-tool-backend.ts")) || existsSync(path.join(brokerRoot, "src/services/codex/broker-tool-backend.ts"))) {
    return true;
  }

  const needles = ["setToolBackend", "createBrokerToolBackend"];
  for (const rel of ["src/services/codex/codex-broker.ts", "src/services/service-components.ts", "src/index.ts", "src/worker-index.ts", "src/services/agent-runtime/session-auth-profile-runtime.ts"]) {
    const source = readRepoSource(rel);
    if (needles.some((needle) => source.includes(needle)) || source.includes("setDynamicToolBackend(")) {
      return true;
    }
  }
  return false;
}

function isPromptRewritten(): boolean {
  return readRepoSource("src/services/codex/prompts/slack-thread-base-instructions.md").includes("chat.post_message") || readRepoSource("src/services/codex/slack-thread-base-instructions.ts").includes("chat.post_message");
}

function readRepoSource(rel: string): string {
  try {
    return readFileSync(path.join(brokerRoot, rel), "utf8");
  } catch {
    return "";
  }
}

function brokerDynamicToolsEnv(): Record<string, string> {
  const configSrc = readRepoSource("src/config.ts");
  if (configSrc.includes("AGENT_DYNAMIC_TOOLS") || configSrc.includes("agentDynamicTools")) {
    return { AGENT_DYNAMIC_TOOLS: "true" };
  }
  return {};
}
