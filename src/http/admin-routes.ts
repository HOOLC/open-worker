import http from "node:http";

import { URL } from "node:url";

import type { AppConfig } from "../config.js";

import type { AdminService } from "../services/admin-service.js";

import { readString, respondJson } from "./common.js";

import {
  matchSessionJobCancelPath,
  matchSessionTimelineEventPath,
  serveAdminSpaIndex,
  findAdminSpaIndex,
  serveAdminAsset,
  isAdminSpaRoute,
  contentTypeForAsset,
  readAdminBody,
  readPositiveNumber,
  readReleaseTarget,
  runAdminOperation,
  isSessionNotFoundError,
  streamAdminEvents,
  readEventCursor,
  respondTracedAdminJson,
  isAuthorizedAdminRequest,
} from "./admin-routes-helpers.js";

export async function handleAdminRequest(
  method: string,
  url: URL,
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly adminService: AdminService;
    readonly config: AppConfig;
  },
): Promise<boolean> {
  if (method === "GET" && isAdminSpaRoute(url.pathname)) {
    return serveAdminSpaIndex(response, options.config);
  }

  if (method === "GET" && url.pathname.startsWith("/admin/assets/")) {
    return serveAdminAsset(url, response);
  }

  if (!url.pathname.startsWith("/admin/api/")) {
    return false;
  }

  if (!isAuthorizedAdminRequest(request, options.config)) {
    respondJson(response, 401, {
      ok: false,
      error: "admin_auth_required",
    });
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/overview") {
    await respondTracedAdminJson(response, "overview", () => options.adminService.getOverview());
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/logs") {
    await respondTracedAdminJson(response, "logs", () =>
      options.adminService.getRecentLogs({
        limit: readPositiveNumber(url.searchParams.get("limit")),
      }),
    );
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/sessions") {
    await respondTracedAdminJson(response, "sessions", () => options.adminService.listSessionSummaries());
    return true;
  }

  if (method === "GET" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/timeline")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/timeline".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    await respondTracedAdminJson(response, "session-timeline", () =>
      options.adminService.getSessionTimeline(sessionKey, {
        limit: readPositiveNumber(url.searchParams.get("limit")),
        beforeSequence: readPositiveNumber(url.searchParams.get("before_sequence")),
      }),
    );
    return true;
  }

  const timelineEventMatch = matchSessionTimelineEventPath(url.pathname);
  if (method === "GET" && timelineEventMatch) {
    const result = await options.adminService.getSessionTimelineEvent(timelineEventMatch.sessionKey, timelineEventMatch.eventId);
    response.setHeader("server-timing", 'admin;desc="session-timeline-event";dur=0');
    respondJson(response, result.ok === false ? 404 : 200, result);
    return true;
  }

  if (method === "GET" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/slack-thread-url")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/slack-thread-url".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    const result = await options.adminService.getSessionSlackThreadUrl(sessionKey);
    respondJson(response, result.ok === false ? 404 : 200, result);
    return true;
  }

  if (method === "GET" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/github-identity")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/github-identity".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    const result = await options.adminService.getSessionGitHubIdentity(sessionKey);
    respondJson(response, result.ok === false ? 404 : 200, result);
    return true;
  }

  if (method === "POST" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/github-oauth/device/start")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/github-oauth/device/start".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () => options.adminService.startSessionGitHubDeviceAuthorization(sessionKey));
    return true;
  }

  if (method === "GET" && url.pathname.startsWith("/admin/api/github-oauth/device/")) {
    const deviceAuthorizationId = decodeURIComponent(url.pathname.slice("/admin/api/github-oauth/device/".length));
    if (!deviceAuthorizationId || deviceAuthorizationId.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () => options.adminService.pollGitHubDeviceAuthorization(deviceAuthorizationId));
    return true;
  }

  if (method === "POST" && url.pathname.startsWith("/admin/api/github-accounts/") && url.pathname.endsWith("/oauth/device/start")) {
    const slackUserId = decodeURIComponent(url.pathname.slice("/admin/api/github-accounts/".length, -"/oauth/device/start".length));
    if (!slackUserId || slackUserId.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () => options.adminService.startGitHubAccountDeviceAuthorization(slackUserId));
    return true;
  }

  if (method === "POST" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/auth-profile")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/auth-profile".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const name = readString(body.name);
    const mode = readString(body.mode);
    const autoMode = mode === "auto";
    if (!name && !autoMode) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["name", "mode=auto"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.switchSessionAuthProfile({
        sessionKey,
        ...(autoMode ? { mode: "auto" as const } : { name }),
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname.startsWith("/admin/api/sessions/") && url.pathname.endsWith("/reset")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length, -"/reset".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () =>
      options.adminService.resetSession({
        sessionKey,
      }),
    );
    return true;
  }

  if (method === "DELETE" && url.pathname.startsWith("/admin/api/sessions/")) {
    const sessionKey = decodeURIComponent(url.pathname.slice("/admin/api/sessions/".length));
    if (!sessionKey || sessionKey.includes("/")) {
      return false;
    }

    await runAdminOperation(
      response,
      () =>
        options.adminService.deleteSession({
          sessionKey,
        }),
      {
        errorStatus: (error) => (isSessionNotFoundError(error) ? 404 : 500),
      },
    );
    return true;
  }

  const sessionJobCancel = matchSessionJobCancelPath(url.pathname);
  if (method === "POST" && sessionJobCancel) {
    await runAdminOperation(response, () => options.adminService.cancelSessionJob(sessionJobCancel));
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/preflight") {
    respondJson(
      response,
      200,
      await options.adminService.getOperationPreflight({
        operation: readString(url.searchParams.get("operation")) ?? "unknown",
      }),
    );
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/operations") {
    respondJson(response, 200, await options.adminService.listAdminOperations());
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/usage") {
    await respondTracedAdminJson(response, "usage", () => options.adminService.getUsageOverview());
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/audit") {
    respondJson(
      response,
      200,
      await options.adminService.listAdminAuditEvents({
        operationId: readString(url.searchParams.get("operation_id")) ?? undefined,
      }),
    );
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/events") {
    streamAdminEvents(request, response, options.adminService, url);
    return true;
  }

  if (method === "GET" && url.pathname === "/admin/api/status") {
    await respondTracedAdminJson(response, "status", () => options.adminService.getStatus());
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/auth-profiles") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const name = readString(body.name) ?? undefined;
    const authJsonContent = readString(body.auth_json_content);
    if (!authJsonContent) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["auth_json_content"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.addAuthProfile({
        name,
        authJsonContent,
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/auth-profiles/device-code/start") {
    await runAdminOperation(response, () => options.adminService.startAuthProfileDeviceCode());
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/auth-profiles/device-code/complete") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const name = readString(body.name) ?? undefined;
    const deviceAuthId = readString(body.device_auth_id);
    const userCode = readString(body.user_code);
    const retryAfterSeconds = readPositiveNumber(body.retry_after_seconds);
    if (!deviceAuthId || !userCode) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["device_auth_id", "user_code"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.completeAuthProfileDeviceCode({
        name,
        deviceAuthId,
        userCode,
        retryAfterSeconds,
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/github-authors") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const slackUserId = readString(body.slack_user_id);
    const githubAuthor = readString(body.github_author);
    if (!slackUserId || !githubAuthor) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["slack_user_id", "github_author"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.upsertGitHubAuthorMapping({
        slackUserId,
        githubAuthor,
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/github-accounts/default-pr") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const slackUserId = readString(body.slack_user_id);
    if (!slackUserId) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["slack_user_id"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.setDefaultGitHubPrAccount({
        slackUserId,
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/deploy") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const target = readReleaseTarget(body.target);
    const version = readString(body.version);
    if (!target || !version) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["target", "version"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.deployRelease({
        target,
        version,
        allowActive: body.allow_active === true,
      }),
    );
    return true;
  }

  if (method === "POST" && url.pathname === "/admin/api/rollback") {
    const body = await readAdminBody(request, response);
    if (!body) {
      return true;
    }

    const target = readReleaseTarget(body.target);
    if (!target) {
      respondJson(response, 400, {
        ok: false,
        error: "missing_required_body",
        required: ["target"],
      });
      return true;
    }

    await runAdminOperation(response, () =>
      options.adminService.rollbackRelease({
        target,
        version: readString(body.version) ?? undefined,
        allowActive: body.allow_active === true,
      }),
    );
    return true;
  }

  if (method === "DELETE" && url.pathname.startsWith("/admin/api/auth-profiles/")) {
    const profileName = decodeURIComponent(url.pathname.slice("/admin/api/auth-profiles/".length));
    if (!profileName || profileName.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () =>
      options.adminService.deleteAuthProfile({
        name: profileName,
      }),
    );
    return true;
  }

  if (method === "DELETE" && url.pathname.startsWith("/admin/api/github-authors/")) {
    const slackUserId = decodeURIComponent(url.pathname.slice("/admin/api/github-authors/".length));
    if (!slackUserId || slackUserId.includes("/")) {
      return false;
    }

    await runAdminOperation(response, () =>
      options.adminService.deleteGitHubAuthorMapping({
        slackUserId,
      }),
    );
    return true;
  }

  return false;
}
