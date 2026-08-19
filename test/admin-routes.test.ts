import http from "node:http";

import fs from "node:fs/promises";

import { afterEach, describe, expect, it } from "vitest";

import { loadConfig } from "../src/config.js";

import { deferUntilResponseFinished } from "../src/http/response-deferred-tasks.js";

import { createHttpHandler } from "../src/http/router.js";

import { waitFor } from "./admin-routes-helpers.js";
import { normalizeSourceWhitespace, readCompanionSource } from "./source-helpers.js";

describe("admin routes", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  async function startAdminServer(configEnv: NodeJS.ProcessEnv, adminService: Record<string, unknown>): Promise<string> {
    const config = loadConfig(configEnv);
    const server = http.createServer(
      createHttpHandler({
        adminService: adminService as never,
        bridge: {} as never,
        isolatedMcp: {} as never,
        jobManager: {} as never,
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
      throw new Error("failed to start test server");
    }
    return `http://127.0.0.1:${address.port}`;
  }

  it("runs deploy restart callbacks only after the deploy response is finished", async () => {
    const restartCalls: string[] = [];
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        deployRelease: async () => {
          const deferred = deferUntilResponseFinished(async () => {
            restartCalls.push("restart");
          });
          return {
            ok: true,
            deferred,
            restartCount: restartCalls.length,
          };
        },
      },
    );

    const response = await fetch(`${baseUrl}/admin/api/deploy`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        target: "worker",
        version: "0.2.0",
        allow_active: true,
      }),
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({
      ok: true,
      deferred: true,
      restartCount: 0,
    });

    await waitFor(() => restartCalls.length === 1, "deferred restart callback");
  });

  it("renders auth profile management and session console sections in the admin page", async () => {
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        getStatus: async () => ({ ok: true, status: "admin-ok" }),
        addAuthProfile: async () => ({ ok: true }),
        upsertGitHubAuthorMapping: async () => ({ ok: true }),
        deleteGitHubAuthorMapping: async () => ({ ok: true }),
        deleteAuthProfile: async () => ({ ok: true }),
        deployRelease: async () => ({ ok: true }),
        rollbackRelease: async () => ({ ok: true }),
      },
    );

    const page = await fetch(`${baseUrl}/admin`);
    expect(page.status).toBe(200);
    const html = await page.text();
    const adminIndexSource = await fs.readFile(new URL("../src/admin-ui/index.html", import.meta.url), "utf8");
    const adminMainSource = await fs.readFile(new URL("../src/admin-ui/main.tsx", import.meta.url), "utf8");
    const adminShellSource = await readCompanionSource(new URL("../src/admin-ui/admin-shell.tsx", import.meta.url));
    const viteConfigSource = await fs.readFile(new URL("../vite.config.ts", import.meta.url), "utf8");
    const sessionViewSource = await readCompanionSource(new URL("../src/admin-ui/session-view.tsx", import.meta.url));

    expect(html).toContain('id="admin-root"');
    expect(html).toContain('id="admin-config"');
    expect(html).toContain("/admin/assets/admin-ui.css");
    expect(html).toContain("/admin/assets/admin-ui.js");
    expect(html).not.toContain("switchAdminView");
    expect(adminIndexSource).toContain('id="admin-root"');
    expect(adminIndexSource).toContain('id="admin-config"');
    expect(adminIndexSource).toContain('src="/main.tsx"');
    expect(viteConfigSource).toContain('root: "src/admin-ui"');
    expect(viteConfigSource).toContain('base: "/admin/"');
    expect(viteConfigSource).toContain('input: "index.html"');
    expect(adminMainSource).toContain("<AdminShell");
    expect(adminMainSource).not.toContain("initAdminPage");
    expect(adminMainSource).not.toContain("dangerouslySetInnerHTML");
    expect(adminShellSource).toContain("export function AdminShell");
    expect(adminShellSource).toContain("admin-nav");
    expect(adminShellSource).toContain('data-admin-view="sessions"');
    expect(adminShellSource).toContain('data-admin-view="ops"');
    expect(adminShellSource).not.toContain("top-actions");
    expect(adminShellSource).not.toContain("refresh-button");
    expect(adminShellSource).not.toContain("last-refresh");
    expect(adminShellSource).not.toContain("实时");
    expect(adminShellSource).not.toContain("刷新");
    expect(adminShellSource).toContain("账号池");
    expect(adminShellSource).toContain("GitHub 账号");
    expect(adminShellSource).not.toContain("GitHub 作者映射");
    expect(adminShellSource).toContain("发布");
    expect(adminShellSource).toContain("推荐使用设备码 OAuth");
    expect(adminShellSource).toContain("备用：导入 auth.json");
    expect(adminShellSource).toContain("绑定 GitHub");
    expect(adminShellSource).not.toContain("Slack 用户 ID（U123...）");
    expect(adminShellSource).not.toContain("Commit 作者：姓名 <email@example.com>");
    expect(adminShellSource).not.toContain("编辑作者");
    expect(adminShellSource).not.toContain("历史 Commit 作者");
    expect(adminShellSource).not.toContain("session-react-root");
    expect(sessionViewSource).not.toContain("session-search");
    expect(sessionViewSource).not.toContain("sessionSearch");
    expect(sessionViewSource).not.toContain('type="search"');
    expect(adminShellSource).not.toContain('type="search"');
    expect(adminShellSource).not.toContain("筛选 Slack / GitHub 账号");
    expect(sessionViewSource).toContain("session-detail-panel");
    expect(sessionViewSource).not.toContain("Agent 工作台");
    expect(adminShellSource).not.toContain("Session Inspector");
    expect(sessionViewSource).not.toContain("session-table-header");
    expect(adminShellSource).toContain("系统日志");
    expect(sessionViewSource).toContain("待处理");
    expect(sessionViewSource).toContain("openHumanInboundCount");
    expect(sessionViewSource).toContain("openSystemInboundCount");
    expect(adminShellSource).not.toContain("status-strip");
    expect(adminShellSource).not.toContain("command-grid");
    expect(adminShellSource).not.toContain("MSG: ");
    expect(adminShellSource).not.toContain("profile-name-input");
    expect(adminShellSource).not.toContain("Account Quota");
    expect(adminShellSource).not.toContain("Control");
    expect(adminShellSource).not.toContain("ADMIN TOKEN");
    expect(adminShellSource).not.toContain("/admin/api/runtime-files");
  });

  it("serves a deep-linkable admin session page", async () => {
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        getStatus: async () => ({ ok: true, status: "admin-ok" }),
        addAuthProfile: async () => ({ ok: true }),
        upsertGitHubAuthorMapping: async () => ({ ok: true }),
        deleteGitHubAuthorMapping: async () => ({ ok: true }),
        deleteAuthProfile: async () => ({ ok: true }),
        deployRelease: async () => ({ ok: true }),
        rollbackRelease: async () => ({ ok: true }),
      },
    );

    const page = await fetch(`${baseUrl}/admin/sessions/${encodeURIComponent("C123:111.222")}`);
    expect(page.status).toBe(200);
    const html = await page.text();
    const adminMainSource = await fs.readFile(new URL("../src/admin-ui/main.tsx", import.meta.url), "utf8");
    const adminCssSource = normalizeSourceWhitespace(await fs.readFile(new URL("../src/admin-ui/admin.css", import.meta.url), "utf8"));
    const sessionViewSource = await readCompanionSource(new URL("../src/admin-ui/session-view.tsx", import.meta.url));

    expect(html).toContain('id="admin-root"');
    expect(html).toContain("/admin/assets/admin-ui.js");
    expect(adminMainSource).toContain("isSessionPermalinkPath");
    expect(adminMainSource).toContain("session-permalink-page");
    expect(adminCssSource).toContain("body.session-permalink-page .topbar");
    expect(sessionViewSource).toContain("readPermalinkSessionKey");
    expect(sessionViewSource).toContain("SessionPermalinkView");
    expect(sessionViewSource).toContain('/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/timeline');
  });

  it("serves the GitHub bind session deep link and routes device OAuth api calls", async () => {
    const calls: string[] = [];
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        getSessionGitHubIdentity: async (sessionKey: string) => {
          calls.push(`identity:${sessionKey}`);
          return {
            ok: true,
            sessionKey,
            identity: {
              binding: { state: "unbound" },
              defaultAccount: { available: true, githubLogin: "default-bot" },
            },
          };
        },
        startSessionGitHubDeviceAuthorization: async (sessionKey: string) => {
          calls.push(`start:${sessionKey}`);
          return {
            ok: true,
            device: {
              id: "device-1",
              userCode: "ABCD-EFGH",
            },
          };
        },
        pollGitHubDeviceAuthorization: async (deviceAuthorizationId: string) => {
          calls.push(`poll:${deviceAuthorizationId}`);
          return {
            ok: true,
            result: { status: "pending" },
          };
        },
      },
    );
    const sessionKey = "C123:111.222";

    const page = await fetch(`${baseUrl}/admin/sessions/${encodeURIComponent(sessionKey)}/github/bind`);
    expect(page.status).toBe(200);
    await expect(page.text()).resolves.toContain('id="admin-root"');
    const sessionViewSource = await readCompanionSource(new URL("../src/admin-ui/session-view.tsx", import.meta.url));
    expect(sessionViewSource).toContain("function GitHubBindPage");
    expect(sessionViewSource).toContain("github-bind-page");
    expect(sessionViewSource).toContain("github-bind-card");
    expect(sessionViewSource).toContain("GitHubBindingFlow");
    expect(sessionViewSource).not.toContain('const shouldAutoStart = typeof window !== "undefined" && window.location.pathname.endsWith("/github/bind")');

    const identity = await fetch(`${baseUrl}/admin/api/sessions/${encodeURIComponent(sessionKey)}/github-identity`);
    expect(identity.status).toBe(200);
    await expect(identity.json()).resolves.toMatchObject({
      ok: true,
      identity: {
        binding: { state: "unbound" },
        defaultAccount: { githubLogin: "default-bot" },
      },
    });

    const started = await fetch(`${baseUrl}/admin/api/sessions/${encodeURIComponent(sessionKey)}/github-oauth/device/start`, {
      method: "POST",
    });
    expect(started.status).toBe(200);
    await expect(started.json()).resolves.toMatchObject({
      ok: true,
      device: {
        id: "device-1",
        userCode: "ABCD-EFGH",
      },
    });

    const polled = await fetch(`${baseUrl}/admin/api/github-oauth/device/device-1`);
    expect(polled.status).toBe(200);
    await expect(polled.json()).resolves.toMatchObject({
      ok: true,
      result: { status: "pending" },
    });
    expect(calls).toEqual(["identity:C123:111.222", "start:C123:111.222", "poll:device-1"]);
  });

  it("routes admin GitHub account OAuth start by existing Slack user id", async () => {
    const calls: string[] = [];
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        startGitHubAccountDeviceAuthorization: async (slackUserId: string) => {
          calls.push(slackUserId);
          return {
            ok: true,
            device: {
              id: "device-1",
              slackUserId,
              userCode: "ABCD-EFGH",
            },
          };
        },
      },
    );

    const started = await fetch(`${baseUrl}/admin/api/github-accounts/${encodeURIComponent("U123")}/oauth/device/start`, {
      method: "POST",
    });
    expect(started.status).toBe(200);
    await expect(started.json()).resolves.toMatchObject({
      ok: true,
      device: {
        id: "device-1",
        slackUserId: "U123",
      },
    });
    expect(calls).toEqual(["U123"]);
  });

  it("persists session ui state in the admin page script", async () => {
    const baseUrl = await startAdminServer(
      {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
      } as NodeJS.ProcessEnv,
      {
        getStatus: async () => ({ ok: true, status: "admin-ok" }),
        addAuthProfile: async () => ({ ok: true }),
        upsertGitHubAuthorMapping: async () => ({ ok: true }),
        deleteGitHubAuthorMapping: async () => ({ ok: true }),
        deleteAuthProfile: async () => ({ ok: true }),
        deployRelease: async () => ({ ok: true }),
        rollbackRelease: async () => ({ ok: true }),
      },
    );

    const page = await fetch(`${baseUrl}/admin`);
    expect(page.status).toBe(200);
    const html = await page.text();
    const adminMainSource = await fs.readFile(new URL("../src/admin-ui/main.tsx", import.meta.url), "utf8");
    const adminShellSource = await readCompanionSource(new URL("../src/admin-ui/admin-shell.tsx", import.meta.url));
    const sessionViewSource = await readCompanionSource(new URL("../src/admin-ui/session-view.tsx", import.meta.url));
    const sessionRowDisplaySource = await fs.readFile(new URL("../src/admin-ui/session-row-display.ts", import.meta.url), "utf8");
    const adminCssSource = normalizeSourceWhitespace(await fs.readFile(new URL("../src/admin-ui/admin.css", import.meta.url), "utf8"));

    expect(adminMainSource).not.toContain("admin-legacy");
    expect(sessionViewSource).toContain("admin-ui-state:");
    expect(sessionViewSource).toContain("selectedSessionKey");
    expect(sessionViewSource).toContain("data-session-key");
    expect(adminShellSource).toContain("AdminSessionsView");
    expect(adminShellSource).toContain("publishAdminStatus");
    expect(sessionViewSource).toContain("useSyncExternalStore");
    expect(sessionViewSource).toContain("orderRef");
    expect(sessionViewSource).toContain("key={session.key}");
    expect(sessionViewSource).not.toContain("innerHTML");
    expect(sessionViewSource).not.toContain("dangerouslySetInnerHTML");
    expect(adminShellSource).toContain("window.localStorage.getItem");
    expect(sessionViewSource).not.toContain("expandedSessionKeys");
    expect(sessionViewSource).toContain("ongoing");
    expect(adminShellSource).toContain("authProfileQuotaItems");
    expect(adminShellSource).toContain("profileTitle");
    expect(sessionViewSource).toContain("自动分配");
    expect(sessionViewSource).toContain('mode: "auto"');
    expect(adminShellSource).not.toContain("renderAccountChip");
    expect(adminShellSource).not.toContain("refreshButton");
    expect(adminShellSource).not.toContain("lastRefresh");
    expect(adminShellSource).not.toContain(" 活跃 · ");
    expect(adminShellSource).not.toContain(" 待处理 · ");
    expect(sessionViewSource).toContain("sessionQueueState");
    expect(sessionViewSource).toContain("compareSessionsForMode");
    expect(sessionViewSource).toContain("session-card");
    expect(sessionViewSource).toContain("session-meta-pill");
    expect(sessionRowDisplaySource).not.toContain("待人处理");
    expect(sessionRowDisplaySource).toContain("待处理");
    expect(sessionRowDisplaySource).not.toContain("任务失败");
    expect(sessionViewSource).toContain('mode === "issues" && !sessionAuthBlockActive');
    expect(sessionViewSource).toContain('mode === "usage"');
    expect(sessionViewSource).toContain("fmtRelativeTime");
    expect(adminCssSource).toContain("text-overflow: ellipsis");
    expect(adminCssSource).toContain("overflow-x: auto");
    expect(adminCssSource).toContain("flex: 0 0 auto");
    expect(adminCssSource).toContain("grid-auto-rows: max-content");
    expect(adminCssSource).toContain("align-content: start");
    expect(adminCssSource).toContain("html, body { width: 100%; height: 100%; overflow: hidden; }");
    expect(adminCssSource).toContain(".shell { width: 100%; height: 100dvh;");
    expect(adminCssSource).toContain("grid-template-columns: minmax(320px, 420px)");
    expect(adminCssSource).toContain(".session-detail-panel > .panel-body");
    expect(adminCssSource).toContain(".session-body { flex: 1; min-height: 0; overflow: hidden;");
    expect(adminCssSource).toContain(".session-timeline-panel .mini-body { flex: 1; min-height: 0; overflow: hidden;");
    expect(adminCssSource).toContain(".timeline { flex: 1; min-height: 0; display: flex; flex-direction: column;");
    expect(adminCssSource).toContain(".agent-transcript");
    expect(adminCssSource).toContain(".agent-message-body");
    expect(adminCssSource).toContain(".agent-tool-step");
    expect(adminCssSource).toContain(".session-card { display: block; overflow: hidden; }");
    expect(adminCssSource).toContain(".session-meta-line { display: flex; gap: 4px; align-items: center; flex-wrap: nowrap;");
    expect(adminCssSource).toContain(".session-meta-pill { min-width: 0; max-width: 100%; flex: 0 1 auto;");
    expect(adminCssSource).toContain(".session-card");
    expect(adminCssSource).toContain(".session-priority-danger");
    expect(adminCssSource).toContain("dialog[open]");
    expect(adminCssSource).toContain("position: fixed");
    expect(adminCssSource).toContain("z-index: 1000");
    expect(adminCssSource).not.toContain(".top-actions");
    expect(adminCssSource).not.toContain(".admin-nav { grid-template-columns: 1fr; }");
    expect(html).toContain("/admin/assets/admin-ui.js");
  });
});
