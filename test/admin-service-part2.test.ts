import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { loadConfig } from "../src/config.js";

import { AdminService } from "../src/services/admin-service.js";

describe("AdminService", () => {
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

  it("reads recent broker logs from a bounded tail instead of decoding whole files", async () => {
    const dataRoot = await fs.mkdtemp(path.join(os.tmpdir(), "admin-service-large-log-"));
    tempDirs.push(dataRoot);

    const config = loadConfig({
      SLACK_APP_TOKEN: "xapp-test",
      SLACK_BOT_TOKEN: "xoxb-test",
      DATA_ROOT: dataRoot,
    } as NodeJS.ProcessEnv);

    await fs.mkdir(config.codexHome, { recursive: true });
    await fs.mkdir(path.join(config.logDir, "broker"), { recursive: true });
    await fs.writeFile(path.join(config.logDir, "broker", "2026-03-19-00.jsonl"), `${"x".repeat(1024 * 1024)}\n{"message":"tail-1"}\n{"message":"tail-2"}\n`, "utf8");

    const service = new AdminService({
      config,
      startedAt: new Date("2026-03-19T00:00:00.000Z"),
      sessions: {
        listSessions: () => [],
        listInboundMessages: () => [],
        listBackgroundJobs: () => [],
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
        restartRuntime: async () => {},
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

    const status = await service.getStatus();
    expect((status.state as { recentBrokerLogs: unknown[] }).recentBrokerLogs).toEqual([{ message: "tail-1" }, { message: "tail-2" }]);
  });
});
