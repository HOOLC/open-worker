import fs from "node:fs/promises";
import http from "node:http";
import os from "node:os";
import path from "node:path";

import { expect } from "vitest";

import { loadConfig } from "../src/config.js";
import { createHttpHandler } from "../src/http/router.js";
import { AdminService } from "../src/services/admin-service.js";
import { AuthProfileService, type AuthProfilesStatus } from "../src/services/auth-profile-service.js";
import { GitHubAuthorMappingService } from "../src/services/github-author-mapping-service.js";
import { GitHubPrIdentityService } from "../src/services/github-pr-identity-service.js";
import { SessionManager } from "../src/services/session-manager.js";
import { StateStore } from "../src/store/state-store.js";
import type { AppConfig } from "../src/config.js";
import type { PersistedAgentTraceEvent, PersistedBackgroundJob, PersistedInboundMessage, SlackUserIdentity } from "../src/types.js";

export type AdminHarnessCleanup = () => Promise<void>;

export type AdminSlackConversations = {
  getConversationInfo?: (channelId: string) => Promise<{
    readonly channelId: string;
    readonly name?: string | undefined;
    readonly channelType?: string | undefined;
  } | null>;
  getUserIdentity?: (userId: string) => Promise<SlackUserIdentity | null>;
  getPermalink?: (options: { readonly channelId: string; readonly messageTs: string }) => Promise<string | null>;
};

export type StartAdminFixtureOptions = {
  readonly authProfilesStatus?: AuthProfilesStatus | undefined;
  readonly workerBaseUrl?: string | undefined;
  readonly extraEnv?: NodeJS.ProcessEnv | undefined;
  readonly tempPrefix?: string | undefined;
  readonly startedAt?: Date | undefined;
  readonly slackConversations?: AdminSlackConversations | undefined;
  readonly authProfiles?: unknown;
  readonly githubAuthorMappings?: unknown;
  readonly githubPrIdentity?: unknown;
  readonly runtime?: Record<string, unknown> | undefined;
  readonly deployment?: unknown;
  readonly useRealGitHubServices?: boolean | undefined;
  readonly useRealAuthProfiles?: boolean | undefined;
};

export type AdminFixture = {
  readonly baseUrl: string;
  readonly config: AppConfig;
  readonly sessions: SessionManager;
  readonly deploymentCalls: Array<Record<string, unknown>>;
  readonly dataRoot: string;
  readonly githubAuthorMappings?: GitHubAuthorMappingService | undefined;
  readonly githubPrIdentity?: GitHubPrIdentityService | undefined;
  readonly authProfiles?: AuthProfileService | undefined;
};

export async function seedAgentTraceFixture(sessions: SessionManager, sessionKey: string): Promise<void> {
  const baseInstructions = ["System core instruction", "", "Personal long-lived memory from ~/.codex/AGENT.md:", "- prefer Chinese admin pages", "- preserve Slack context", "", "Slack thread message model:", "The following messages are the live Slack thread."].join("\n");
  const records: Array<Omit<PersistedAgentTraceEvent, "sessionKey" | "createdAt" | "updatedAt">> = [
    {
      id: `${sessionKey}:broker:system_prompt`,
      source: "broker",
      type: "agent_system_prompt",
      at: "2026-03-19T00:00:00.000Z",
      sequence: 0,
      title: "系统 Prompt",
      summary: "Codex 线程启动指令",
      detail: baseInstructions,
      status: "loaded",
      role: "system",
    },
    {
      id: `${sessionKey}:broker:memory`,
      source: "broker",
      type: "agent_memory",
      at: "2026-03-19T00:00:00.000Z",
      sequence: 1,
      title: "记忆",
      summary: "- prefer Chinese admin pages - preserve Slack context",
      detail: "- prefer Chinese admin pages\n- preserve Slack context",
      status: "loaded",
      role: "system",
    },
    {
      id: `${sessionKey}:broker:user`,
      source: "broker",
      type: "agent_user_message",
      at: "2026-03-19T00:00:01.000Z",
      sequence: 1000,
      title: "用户消息",
      summary: "请检查发布状态",
      detail: "请检查发布状态",
      status: "received",
      role: "user",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:broker:runtime-reminder`,
      source: "broker",
      type: "agent_runtime_reminder",
      at: "2026-03-19T00:00:02.000Z",
      sequence: 2000,
      title: "Runtime 提醒",
      summary: "Runtime reminder: 你已经工作了一段时间，请发进展。",
      detail: "Runtime reminder: 你已经工作了一段时间，请发进展。",
      status: "sent",
      role: "system",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:input-delivered`,
      source: "agent_runtime",
      type: "agent_input_delivered",
      at: "2026-03-19T00:00:02.100Z",
      sequence: 2100,
      title: "输入已送达",
      summary: "启动新回合",
      status: "started_turn",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:turn-started`,
      source: "agent_runtime",
      type: "agent_turn_started",
      at: "2026-03-19T00:00:02.200Z",
      sequence: 2200,
      title: "回合开始",
      summary: "开始处理输入",
      status: "running",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:assistant`,
      source: "agent_runtime",
      type: "agent_assistant_message",
      at: "2026-03-19T00:00:03.000Z",
      sequence: 3000,
      title: "Assistant 消息",
      summary: "我会先检查状态。",
      detail: "我会先检查状态。",
      status: "completed",
      role: "assistant",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:tool-call`,
      source: "agent_runtime",
      type: "agent_tool_call",
      at: "2026-03-19T00:00:04.000Z",
      sequence: 4000,
      title: "工具调用",
      summary: "exec_command",
      detail: '{"cmd":"pnpm test"}',
      status: "running",
      role: "assistant",
      toolName: "exec_command",
      callId: "call-1",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:running-tool-call`,
      source: "agent_runtime",
      type: "agent_tool_call",
      at: "2026-03-19T00:00:04.100Z",
      sequence: 4100,
      title: "工具调用",
      summary: "exec_command",
      detail: '{"cmd":"pnpm lint"}',
      status: "running",
      role: "assistant",
      toolName: "exec_command",
      callId: "call-2",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:token-count`,
      source: "agent_runtime",
      type: "agent_token_count",
      at: "2026-03-19T00:00:04.500Z",
      sequence: 4500,
      title: "Token 用量",
      summary: "180704 tokens",
      detail: JSON.stringify({
        tokenUsage: {
          last: {
            totalTokens: 180704,
            inputTokens: 180698,
            cachedInputTokens: 180096,
            outputTokens: 6,
          },
          total: {
            totalTokens: 16720576,
          },
        },
      }),
      status: "completed",
      turnId: "turn-1",
      metadata: {
        totalTokens: 180704,
        inputTokens: 180698,
        outputTokens: 6,
        reasoningTokens: 0,
        source: "exact",
      },
    },
    {
      id: `${sessionKey}:runtime:turn-completed`,
      source: "agent_runtime",
      type: "agent_turn_completed",
      at: "2026-03-19T00:00:04.600Z",
      sequence: 4600,
      title: "回合结束",
      summary: "回合已完成",
      detail: "我会先检查状态。",
      status: "completed",
      turnId: "turn-1",
    },
    {
      id: `${sessionKey}:runtime:tool-result`,
      source: "agent_runtime",
      type: "agent_tool_result",
      at: "2026-03-19T00:00:05.000Z",
      sequence: 5000,
      title: "工具结果",
      summary: "PASS admin-control-plane",
      detail: "PASS admin-control-plane",
      status: "completed",
      role: "tool",
      callId: "call-1",
      turnId: "turn-1",
    },
  ];
  const now = "2026-03-19T00:00:06.000Z";
  for (const record of records) {
    await sessions.upsertAgentTraceEvent({
      ...record,
      sessionKey,
      createdAt: now,
      updatedAt: now,
    });
  }
}

export async function seedActiveSession(sessions: SessionManager): Promise<void> {
  const session = await sessions.ensureSession("C123", "111.222");
  await sessions.setActiveTurnId("C123", "111.222", "turn-1");
  await sessions.recordTurnSignal("C123", "111.222", {
    turnId: "turn-1",
    kind: "wait",
    reason: "waiting on CI",
    occurredAt: "2026-03-19T00:00:04.000Z",
  });
  await sessions.upsertInboundMessage(
    inboundMessage({
      sessionKey: session.key,
      key: "C123:111.222:111.222",
      messageTs: "111.222",
      status: "done",
      text: "initial request",
      createdAt: "2026-03-19T00:00:00.000Z",
      updatedAt: "2026-03-19T00:00:00.000Z",
    }),
  );
  await sessions.upsertInboundMessage(
    inboundMessage({
      sessionKey: session.key,
      key: "C123:111.222:111.223",
      messageTs: "111.223",
      status: "pending",
      text: "follow up",
      updatedAt: "2026-03-19T00:00:01.000Z",
    }),
  );
  await sessions.upsertBackgroundJob(
    backgroundJob({
      sessionKey: session.key,
      status: "running",
      updatedAt: "2026-03-19T00:00:02.000Z",
      startedAt: "2026-03-19T00:00:02.000Z",
      heartbeatAt: "2026-03-19T00:00:03.000Z",
    }),
  );
  await sessions.upsertBackgroundJob(
    backgroundJob({
      sessionKey: session.key,
      id: "completed-job",
      token: "completed-token",
      status: "completed",
      updatedAt: "2026-03-19T00:00:02.500Z",
      startedAt: "2026-03-19T00:00:02.000Z",
      completedAt: "2026-03-19T00:00:02.500Z",
    }),
  );
  await sessions.upsertBackgroundJob(
    backgroundJob({
      sessionKey: session.key,
      id: "failed-job",
      token: "failed-token",
      status: "failed",
      error: "PR #349 failed: CI Check failed",
      updatedAt: "2026-03-19T00:00:02.750Z",
      startedAt: "2026-03-19T00:00:02.000Z",
      completedAt: "2026-03-19T00:00:02.750Z",
    }),
  );
  await sessions.recordTurnSignal("C123", "111.222", {
    turnId: "turn-1",
    kind: "final",
    reason: "final",
    occurredAt: "2026-03-19T00:00:05.000Z",
  });
  await sessions.recordTurnSignal("C123", "111.222", {
    turnId: "turn-1",
    kind: "wait",
    reason: "waiting on CI",
    occurredAt: "2026-03-19T00:00:06.000Z",
  });
}

export function inboundMessage(patch: Partial<PersistedInboundMessage>): PersistedInboundMessage {
  return {
    key: "C123:111.222:111.223",
    sessionKey: "C123:111.222",
    channelId: "C123",
    rootThreadTs: "111.222",
    messageTs: "111.223",
    source: "thread_reply",
    userId: "U123",
    text: "hello",
    status: "done",
    createdAt: "2026-03-19T00:00:01.000Z",
    updatedAt: "2026-03-19T00:00:01.000Z",
    ...patch,
  };
}

export function backgroundJob(patch: Partial<PersistedBackgroundJob>): PersistedBackgroundJob {
  return {
    id: "job-1",
    token: "job-token",
    sessionKey: "C123:111.222",
    channelId: "C123",
    rootThreadTs: "111.222",
    kind: "watch_ci",
    shell: "/bin/sh",
    cwd: "/tmp/workspace",
    scriptPath: "/tmp/job.sh",
    restartOnBoot: true,
    status: "registered",
    createdAt: "2026-03-19T00:00:02.000Z",
    updatedAt: "2026-03-19T00:00:02.000Z",
    ...patch,
  };
}

export function deploymentStatus(config: AppConfig): Record<string, unknown> {
  const adminPackageName = "@agent-session-broker/admin";
  const workerPackageName = "@agent-session-broker/worker";
  return {
    serviceRoot: config.serviceRoot ?? "",
    npmRegistryUrl: null,
    targets: {
      admin: {
        target: "admin",
        packageName: adminPackageName,
        currentRelease: {
          linkPath: config.currentAdminReleasePath ?? "",
          targetPath: null,
          exists: false,
          metadata: null,
        },
        previousRelease: {
          linkPath: config.previousAdminReleasePath ?? "",
          targetPath: null,
          exists: false,
          metadata: null,
        },
        failedRelease: {
          linkPath: config.failedAdminReleasePath ?? "",
          targetPath: null,
          exists: false,
          metadata: null,
        },
        recentReleases: [],
        recentPackageVersions: [
          {
            version: "0.2.0",
            packageSpec: `${adminPackageName}@0.2.0`,
          },
        ],
      },
      worker: {
        target: "worker",
        packageName: workerPackageName,
        currentRelease: {
          linkPath: config.currentWorkerReleasePath ?? "",
          targetPath: path.join(config.serviceRoot ?? "", "releases", "worker", "npm-0.2.0", "node_modules", "@agent-session-broker", "worker"),
          exists: true,
          metadata: {
            revision: null,
            shortRevision: null,
            branch: null,
            target: "worker",
            packageName: workerPackageName,
            packageVersion: "0.2.0",
            packageSpec: `${workerPackageName}@0.2.0`,
            installedAt: "2026-03-19T00:00:00.000Z",
            installedBy: "test",
            installedFromHost: "test-host",
            requestedVersion: "0.2.0",
            stateSchemaVersion: 3,
          },
        },
        previousRelease: {
          linkPath: config.previousWorkerReleasePath ?? "",
          targetPath: null,
          exists: false,
          metadata: null,
        },
        failedRelease: {
          linkPath: config.failedWorkerReleasePath ?? "",
          targetPath: null,
          exists: false,
          metadata: null,
        },
        recentReleases: [],
        recentPackageVersions: [
          {
            version: "0.2.0",
            packageSpec: `${workerPackageName}@0.2.0`,
          },
        ],
      },
    },
    admin: {
      launchdLoaded: true,
      healthOk: true,
      healthBody: '{"ok":true}',
    },
    worker: {
      launchdLoaded: true,
      healthOk: true,
      readyOk: true,
      healthBody: '{"ok":true}',
      readyError: null,
    },
  };
}

export function authProfilesStatusFixture(): AuthProfilesStatus {
  return {
    managedRoot: "/tmp/auth-profiles",
    profilesRoot: "/tmp/auth-profiles/profiles",
    profiles: [authProfileFixture("empty-profile", 100, 20), authProfileFixture("usable-profile", 10, 15)],
  };
}

export function authProfileFixture(name: string, primaryUsed: number, secondaryUsed: number): AuthProfilesStatus["profiles"][number] {
  return {
    name,
    path: `/tmp/auth-profiles/profiles/${name}.json`,
    source: "probe",
    checkedAt: "2026-05-09T00:00:00.000Z",
    account: {
      ok: true,
      account: {
        email: `${name}@example.com`,
        type: "chatgpt",
        planType: "pro",
      },
      requiresOpenaiAuth: false,
    },
    rateLimits: {
      ok: true,
      rateLimits: {
        limitId: "codex",
        limitName: "Codex",
        primary: {
          usedPercent: primaryUsed,
          windowDurationMins: 300,
          resetsAt: 1_779_000_000,
        },
        secondary: {
          usedPercent: secondaryUsed,
          windowDurationMins: 10_080,
          resetsAt: 1_780_000_000,
        },
        credits: null,
        planType: "pro",
      },
      rateLimitsByLimitId: {},
    },
  };
}

export async function readJson(url: string): Promise<Record<string, unknown>> {
  const response = await fetch(url);
  const payload = (await response.json()) as Record<string, unknown>;
  expect(response.status).toBe(200);
  return payload;
}

export async function postJson(url: string, body: Record<string, unknown>, headers: Record<string, string> = {}): Promise<Record<string, any>> {
  const response = await fetch(url, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      ...headers,
    },
    body: JSON.stringify(body),
  });
  const payload = (await response.json()) as Record<string, any>;
  expect(response.status).toBe(200);
  return payload;
}

export async function requestJson(
  url: string,
  init: RequestInit = {},
): Promise<{
  readonly status: number;
  readonly payload: Record<string, any>;
  readonly headers: Headers;
}> {
  const response = await fetch(url, init);
  const payload = (await response.json()) as Record<string, any>;
  return {
    status: response.status,
    payload,
    headers: response.headers,
  };
}

export async function startAdminFixture(cleanups: AdminHarnessCleanup[], options: StartAdminFixtureOptions = {}): Promise<AdminFixture> {
  const dataRoot = await fs.mkdtemp(path.join(os.tmpdir(), options.tempPrefix ?? "admin-control-plane-"));
  cleanups.push(async () => {
    await fs.rm(dataRoot, { force: true, recursive: true });
  });

  const config = loadConfig({
    SLACK_APP_TOKEN: "xapp-test",
    SLACK_BOT_TOKEN: "xoxb-test",
    DATA_ROOT: dataRoot,
    SERVICE_ROOT: dataRoot,
    ADMIN_LAUNCHD_LABEL: "admin.test",
    WORKER_LAUNCHD_LABEL: "worker.test",
    ADMIN_PLIST_PATH: path.join(dataRoot, "admin.plist"),
    WORKER_PLIST_PATH: path.join(dataRoot, "worker.plist"),
    ...(options.workerBaseUrl ? { WORKER_BASE_URL: options.workerBaseUrl } : {}),
    ...options.extraEnv,
  } as NodeJS.ProcessEnv);
  await fs.mkdir(config.codexHome, { recursive: true });
  await fs.mkdir(config.logDir, { recursive: true });

  const stateStore = new StateStore(config.stateDir, config.sessionsRoot);
  const sessions = new SessionManager({
    stateStore,
    sessionsRoot: config.sessionsRoot,
  });
  await sessions.load();
  cleanups.push(async () => {
    stateStore.close();
  });

  const githubAuthorMappings = options.useRealGitHubServices ? new GitHubAuthorMappingService({ stateDir: config.stateDir }) : undefined;
  if (githubAuthorMappings) {
    await githubAuthorMappings.load();
  }
  const githubPrIdentity = options.useRealGitHubServices
    ? new GitHubPrIdentityService({
        stateDir: config.stateDir,
        defaultGitHubLogin: config.defaultGitHubLogin,
        defaultGitHubToken: config.defaultGitHubToken,
      })
    : undefined;
  if (githubPrIdentity) {
    await githubPrIdentity.load();
  }
  const authProfiles = options.useRealAuthProfiles
    ? new AuthProfileService({
        config,
        probeProfile: async (profileName) => ({
          source: "probe",
          checkedAt: "2026-03-19T00:00:00.000Z",
          account: {
            ok: true,
            account: {
              email: `${profileName}@example.com`,
              type: "chatgpt",
              planType: "pro",
            },
            requiresOpenaiAuth: false,
          },
          rateLimits: {
            ok: true,
            rateLimits: {
              limitId: "codex",
              limitName: "Codex",
              primary: {
                usedPercent: 10,
                windowDurationMins: 300,
                resetsAt: 1_743_307_200,
              },
              secondary: {
                usedPercent: 20,
                windowDurationMins: 10_080,
                resetsAt: 1_743_912_000,
              },
              credits: null,
              planType: "pro",
            },
            rateLimitsByLimitId: {},
          },
        }),
      })
    : undefined;

  const deploymentCalls: Array<Record<string, unknown>> = [];
  const defaultDeployment = {
    getStatus: async () => deploymentStatus(config),
    deploy: async ({ target, version }: { readonly target: "admin" | "worker"; readonly version: string }) => {
      deploymentCalls.push({ kind: "deploy", target, version });
      return deploymentStatus(config);
    },
    rollback: async ({ target, version }: { readonly target: "admin" | "worker"; readonly version?: string | undefined }) => {
      deploymentCalls.push({ kind: "rollback", target, version: version ?? null });
      return deploymentStatus(config);
    },
    restartWorker: async () => {},
  };

  const adminService = new AdminService({
    config,
    sessions,
    startedAt: options.startedAt ?? new Date("2026-03-19T00:00:00.000Z"),
    authProfiles: (options.authProfiles ??
      authProfiles ?? {
        listProfilesStatus: async () =>
          options.authProfilesStatus ?? {
            managedRoot: path.join(dataRoot, "auth-profiles"),
            profilesRoot: path.join(dataRoot, "auth-profiles", "profiles"),
            profiles: [],
          },
        addProfile: async () => ({ name: "profile" }),
        deleteProfile: async () => {},
        requestDeviceCodeAuth: async () => ({
          deviceAuthId: "device-1",
          userCode: "ABCD-EFGH",
        }),
        completeDeviceCodeAuth: async () => ({
          status: "pending",
        }),
      }) as never,
    githubAuthorMappings: (options.githubAuthorMappings ??
      githubAuthorMappings ?? {
        load: async () => {},
        listMappings: () => [],
        upsertManualMapping: async () => ({}),
        deleteMapping: async () => {},
      }) as never,
    ...(options.githubPrIdentity || githubPrIdentity ? { githubPrIdentity: (options.githubPrIdentity ?? githubPrIdentity) as never } : {}),
    runtime: {
      restartRuntime: async () => {},
      readAccountSummary: async () => ({
        account: {
          email: "admin@example.com",
          type: "chatgpt",
          planType: "team",
        },
        requiresOpenaiAuth: false,
      }),
      readAccountRateLimits: async () => ({
        rateLimits: null,
        rateLimitsByLimitId: {},
      }),
      ...options.runtime,
    } as never,
    deployment: (options.deployment ?? defaultDeployment) as never,
    ...(options.slackConversations ? { slackConversations: options.slackConversations as never } : {}),
  });

  const server = http.createServer(
    createHttpHandler({
      adminService,
      config,
    }),
  );
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  cleanups.push(async () => {
    await new Promise<void>((resolve, reject) => {
      server.close((error) => {
        if (error) {
          reject(error);
          return;
        }
        resolve();
      });
    });
  });

  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to start admin fixture");
  }

  return {
    baseUrl: `http://127.0.0.1:${address.port}`,
    config,
    sessions,
    deploymentCalls,
    dataRoot,
    ...(githubAuthorMappings ? { githubAuthorMappings } : {}),
    ...(githubPrIdentity ? { githubPrIdentity } : {}),
    ...(authProfiles ? { authProfiles } : {}),
  };
}
