import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { loadConfig } from "../src/config.js";
import { readCompanionSource } from "./source-helpers.js";
import { AdminService } from "../src/services/admin-service.js";

describe("admin session performance contract", () => {
  const tempDirs: string[] = [];

  afterEach(async () => {
    await Promise.all(
      tempDirs.splice(0).map((directory) =>
        fs.rm(directory, {
          force: true,
          recursive: true,
        }),
      ),
    );
  });

  it("documents the summary and timeline pagination contract", async () => {
    const doc = await fs.readFile(new URL("../docs/admin-session-performance.md", import.meta.url), "utf8");
    expect(doc).toContain("/admin/api/sessions");
    expect(doc).toContain("before_sequence");
    expect(doc).toContain("per-session redundant");
    expect(doc).toContain("加载更早活动");
    expect(doc).toContain("visible-event contract");
    expect(doc).toContain("scroll container reaches the top");
  });

  it("keeps session summaries off raw trace and turn-usage scans", async () => {
    const dataRoot = await fs.mkdtemp(path.join(os.tmpdir(), "admin-session-summary-fast-"));
    tempDirs.push(dataRoot);
    const config = loadConfig({
      SLACK_APP_TOKEN: "xapp-test",
      SLACK_BOT_TOKEN: "xoxb-test",
      DATA_ROOT: dataRoot,
    } as NodeJS.ProcessEnv);

    const service = new AdminService({
      config,
      startedAt: new Date("2026-03-19T00:00:00.000Z"),
      sessions: {
        listSessions: () => [
          {
            key: "C123:111.222",
            channelId: "C123",
            rootThreadTs: "111.222",
            workspacePath: "/tmp/session",
            createdAt: "2026-03-19T00:00:00.000Z",
            updatedAt: "2026-03-19T00:00:00.000Z",
          },
        ],
        listInboundMessages: () => [],
        listBackgroundJobs: () => [],
        listAgentSessionUsageSummaries: () => [
          {
            sessionKey: "C123:111.222",
            channelId: "C123",
            rootThreadTs: "111.222",
            turnCount: 1,
            exactTurns: 1,
            estimatedTurns: 0,
            missingTurns: 0,
            inputTokens: 10,
            cachedInputTokens: 4,
            outputTokens: 5,
            reasoningTokens: 1,
            totalTokens: 16,
            updatedAt: "2026-03-19T00:00:02.000Z",
            lastTurnAt: "2026-03-19T00:00:02.000Z",
            model: "test-model",
            effort: "low",
          },
        ],
        listAgentTurnUsage: () => {
          throw new Error("session summaries must not scan raw turn usage");
        },
        listAgentTraceEvents: () => {
          throw new Error("session summaries must not read trace events");
        },
        load: async () => {
          throw new Error("session summaries must not refresh session directories");
        },
      } as never,
      authProfiles: {
        listProfilesStatus: async () => ({
          managedRoot: path.join(dataRoot, "auth-profiles"),
          profilesRoot: path.join(dataRoot, "auth-profiles", "profiles"),
          profiles: [],
        }),
      } as never,
      githubAuthorMappings: {
        load: async () => {},
        listMappings: () => [],
      } as never,
      runtime: {
        readAccountSummary: async () => ({
          account: null,
          requiresOpenaiAuth: true,
        }),
        readAccountRateLimits: async () => ({
          rateLimits: null,
          rateLimitsByLimitId: {},
        }),
      } as never,
    });

    const summaries = await service.listSessionSummaries();
    expect(summaries).toMatchObject({
      sessions: [
        {
          key: "C123:111.222",
          usage: {
            totalTokens: 16,
            turnCount: 1,
          },
        },
      ],
    });
    const session = (summaries.sessions as Array<Record<string, unknown>>)[0]!;
    expect(session).not.toHaveProperty("workspacePath");
    expect(session).not.toHaveProperty("backgroundJobs");
    expect(session).not.toHaveProperty("failedBackgroundJobs");
    expect(session.usage).not.toHaveProperty("inputTokens");
  });

  it("keeps the React timeline request bounded and exposes load-older UI", async () => {
    const source = await readCompanionSource(new URL("../src/admin-ui/session-view.tsx", import.meta.url));
    expect(source).toContain("TIMELINE_PAGE_SIZE");
    expect(source).toContain("before_sequence");
    expect(source).toContain("加载更早活动");
    expect(source).toContain("onLoadOlder");
    expect(source).toContain("scrollTop <= TIMELINE_AUTO_LOAD_THRESHOLD");
    expect(source).toContain("pendingPrependAnchorRef");
    expect(source).toContain("scrollHeight - anchor.scrollHeight");
    expect(source).toContain("container.scrollTop = anchor.scrollTop + insertedHeight");
  });
});
