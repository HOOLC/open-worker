import fs from "node:fs/promises";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { postJson, readJson, requestJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

describe("admin routes e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("requires the configured admin token for admin api requests", async () => {
    const { baseUrl } = await startAdminFixture(cleanups, {
      extraEnv: {
        BROKER_ADMIN_TOKEN: "secret-token",
      },
    });

    const unauthorized = await requestJson(`${baseUrl}/admin/api/status`);
    expect(unauthorized.status).toBe(401);
    expect(unauthorized.payload).toMatchObject({
      ok: false,
      error: "admin_auth_required",
    });

    const authorized = await requestJson(`${baseUrl}/admin/api/status`, {
      headers: {
        "x-admin-token": "secret-token",
      },
    });
    expect(authorized.status).toBe(200);
    expect(authorized.payload).toMatchObject({
      account: {
        ok: true,
      },
    });
  });

  it("serves recent logs as a separate admin resource", async () => {
    const { baseUrl, config } = await startAdminFixture(cleanups);
    await fs.mkdir(path.join(config.logDir, "broker"), { recursive: true });
    await fs.writeFile(path.join(config.logDir, "broker", "2026-05-13-00.jsonl"), `${JSON.stringify({ ts: "2026-05-13T09:00:00.000Z", level: "info", message: "ready" })}\n`, "utf8");

    const logs = await readJson(`${baseUrl}/admin/api/logs?limit=3`);
    expect(logs).toMatchObject({
      ok: true,
      logs: [
        {
          level: "info",
          message: "ready",
        },
      ],
    });
  });

  it("creates an auth profile without an explicit name", async () => {
    const { baseUrl } = await startAdminFixture(cleanups, {
      useRealAuthProfiles: true,
    });

    const created = await postJson(`${baseUrl}/admin/api/auth-profiles`, {
      auth_json_content: '{"tokens":{"account_id":"acc-1"}}',
    });
    expect(created).toMatchObject({
      ok: true,
      profile: {
        name: "acc-1",
      },
      operation: {
        kind: "auth_profile_add",
        status: "succeeded",
      },
    });
  });

  it("forwards auth profile device-code start and completion to the admin service", async () => {
    const completeCalls: Array<Record<string, unknown>> = [];
    const { baseUrl } = await startAdminFixture(cleanups, {
      authProfiles: {
        listProfilesStatus: async () => ({
          managedRoot: "/tmp/auth-profiles",
          profilesRoot: "/tmp/auth-profiles/profiles",
          profiles: [],
        }),
        requestDeviceCodeAuth: async () => ({
          deviceAuthId: "device-1",
          userCode: "ABCD-EFGH",
        }),
        completeDeviceCodeAuth: async (payload: Record<string, unknown>) => {
          completeCalls.push(payload);
          return {
            status: "pending",
          };
        },
      },
    });

    const start = await postJson(`${baseUrl}/admin/api/auth-profiles/device-code/start`, {});
    expect(start).toMatchObject({
      ok: true,
      deviceCode: {
        deviceAuthId: "device-1",
        userCode: "ABCD-EFGH",
      },
    });

    const complete = await postJson(`${baseUrl}/admin/api/auth-profiles/device-code/complete`, {
      device_auth_id: "device-1",
      user_code: "ABCD-EFGH",
      retry_after_seconds: 8,
    });
    expect(complete).toMatchObject({
      ok: true,
      deviceCode: {
        status: "pending",
      },
    });
    expect(completeCalls).toEqual([
      {
        deviceAuthId: "device-1",
        userCode: "ABCD-EFGH",
        retryAfterSeconds: 8,
      },
    ]);
  });

  it("forwards GitHub author mapping upserts to the admin service", async () => {
    const { baseUrl, githubPrIdentity, sessions } = await startAdminFixture(cleanups, {
      extraEnv: {
        BROKER_DEFAULT_GITHUB_LOGIN: "legacy-bot",
        BROKER_DEFAULT_GITHUB_TOKEN: "legacy-token",
      },
      useRealGitHubServices: true,
    });
    if (!githubPrIdentity) {
      throw new Error("expected real GitHub PR identity service");
    }
    await githubPrIdentity.upsertBinding({
      slackUserId: "U123",
      githubLogin: "alice-gh",
      githubUserId: 101,
      token: "alice-token",
      scopes: ["repo"],
    });
    await sessions.ensureSession("C123", "111.222", { initiatorUserId: "U123" });

    const mapped = await postJson(`${baseUrl}/admin/api/github-authors`, {
      slack_user_id: "U123",
      github_author: "Alice Example <alice@example.com>",
    });
    expect(mapped).toMatchObject({
      ok: true,
      mapping: {
        userId: "U123",
        githubAuthor: "Alice Example <alice@example.com>",
      },
      operation: {
        kind: "github_author_upsert",
        status: "succeeded",
      },
    });

    const missing = await requestJson(`${baseUrl}/admin/api/github-accounts/default-pr`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({}),
    });
    expect(missing.status).toBe(400);

    const selected = await postJson(`${baseUrl}/admin/api/github-accounts/default-pr`, {
      slack_user_id: "U123",
    });
    expect(selected).toMatchObject({
      ok: true,
      defaultPrAccount: {
        available: true,
        slackUserId: "U123",
        githubLogin: "alice-gh",
      },
    });

    const deleted = await requestJson(`${baseUrl}/admin/api/github-authors/${encodeURIComponent("U999")}`, {
      method: "DELETE",
    }); // deleteGitHubAuthorMapping
    expect(deleted.status).toBe(200);
    expect(deleted.payload).toMatchObject({
      ok: true,
      slackUserId: "U999",
    });

    const identity = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/github-identity`);
    expect(identity).toMatchObject({
      ok: true,
      sessionKey: "C123:111.222",
      identity: {
        binding: {
          state: "bound",
          githubLogin: "alice-gh",
        },
        defaultAccount: {
          available: true,
          slackUserId: "U123",
          githubLogin: "alice-gh",
        },
      },
    });
  });

  it("records rollback requests as durable admin operations", async () => {
    const { baseUrl, deploymentCalls } = await startAdminFixture(cleanups);
    const rollback = await postJson(`${baseUrl}/admin/api/rollback`, {
      target: "admin",
      version: "0.1.0",
      allow_active: false,
    });
    expect(rollback).toMatchObject({
      ok: true,
      operation: {
        kind: "rollback",
        status: "succeeded",
        request: {
          target: "admin",
          version: "0.1.0",
        },
      },
    });
    expect(deploymentCalls).toEqual([{ kind: "rollback", target: "admin", version: "0.1.0" }]);
  });

  it("maps missing session delete failures to 404", async () => {
    const { baseUrl } = await startAdminFixture(cleanups);
    const response = await requestJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:missing")}`, {
      method: "DELETE",
    });
    expect(response.status).toBe(404);
    expect(response.payload).toMatchObject({
      ok: false,
      error: "Session not found: C123:missing",
    });
  });
});
