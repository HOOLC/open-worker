import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { loadConfig } from "../src/config.js";

import { AdminService } from "../src/services/admin-service.js";

describe("AdminService", () => {
  const tempDirs: string[] = [];

  afterEach(async () => {
    vi.useRealTimers();
    await Promise.all(
      tempDirs.splice(0).map((directory) =>
        fs.rm(directory, {
          force: true,
          recursive: true,
        }),
      ),
    );
  });

  it("bounds slow runtime status probes so overview can still answer", async () => {
    vi.useFakeTimers();
    const dataRoot = await fs.mkdtemp(path.join(os.tmpdir(), "admin-service-runtime-timeout-"));
    tempDirs.push(dataRoot);

    const config = loadConfig({
      SLACK_APP_TOKEN: "xapp-test",
      SLACK_BOT_TOKEN: "xoxb-test",
      DATA_ROOT: dataRoot,
    } as NodeJS.ProcessEnv);
    const never = new Promise<never>(() => {});

    const service = new AdminService({
      config,
      startedAt: new Date("2026-03-19T00:00:00.000Z"),
      sessions: {
        listSessions: () => [],
        listInboundMessages: () => [],
        listBackgroundJobs: () => [],
      } as never,
      authProfiles: {
        listProfilesStatus: async () => never,
      } as never,
      githubAuthorMappings: {
        load: async () => {},
        listMappings: () => [],
      } as never,
      runtime: {
        readAccountSummary: async () => never,
        readAccountRateLimits: async () => never,
      } as never,
      deployment: {
        getStatus: async () => never,
      } as never,
    });

    const overviewPromise = service.getOverview();
    await vi.advanceTimersByTimeAsync(4_100);
    const overview = await overviewPromise;
    expect(overview).toMatchObject({
      ok: true,
      account: {
        ok: false,
        error: expect.stringContaining("account summary timed out"),
      },
      rateLimits: {
        ok: false,
        error: expect.stringContaining("account rate limits timed out"),
      },
      deployment: {
        ok: false,
        error: expect.stringContaining("deployment status timed out"),
      },
      authProfiles: {
        ok: false,
        error: expect.stringContaining("auth profiles timed out"),
        profiles: [],
      },
    });
  });
});
