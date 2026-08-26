import { mergeAdminStatusSnapshot, publishAdminStatus } from "./admin-status-store";

import { AdminStatus } from "./admin-types.js";

export async function loadAdminStatus(): Promise<AdminStatus> {
  const sessionStatus = await loadAdminSessionsStatus();
  const [overviewResult, logsResult] = await Promise.allSettled([loadAdminOverview(), loadAdminLogs()]);
  const withOverview = overviewResult.status === "fulfilled" ? mergeStatusOverview(sessionStatus, overviewResult.value) : sessionStatus;
  return logsResult.status === "fulfilled" ? mergeStatusLogs(withOverview, logsResult.value.logs) : withOverview;
}

export async function loadAdminSessionsStatus(): Promise<AdminStatus> {
  const sessionsPayload = await requestJson("/admin/api/sessions", { timeoutMs: 45_000 });
  const sessions = Array.isArray(sessionsPayload.sessions) ? sessionsPayload.sessions : [];
  return {
    ok: true,
    realtime: sessionsPayload.realtime || {},
    state: {
      ...summarizeSessionRows(sessions),
      sessions,
    },
  };
}

export async function loadAdminOverview(): Promise<Record<string, any>> {
  return await requestJson("/admin/api/overview", { timeoutMs: 45_000 });
}

export async function loadAdminLogs(): Promise<Record<string, any>> {
  return await requestJson("/admin/api/logs?limit=40", { timeoutMs: 5_000 });
}

export function mergeStatusOverview(status: unknown, overview: unknown): AdminStatus {
  return mergeAdminStatusSnapshot(status, overview) as AdminStatus;
}

export function mergeStatusLogs(status: unknown, logs: unknown): AdminStatus {
  const current = status && typeof status === "object" && !Array.isArray(status) ? (status as AdminStatus) : {};
  return {
    ...current,
    state: {
      ...current.state,
      recentBrokerLogs: Array.isArray(logs) ? logs : [],
    },
  };
}

export function summarizeSessionRows(sessions: readonly Record<string, any>[]): Record<string, number> {
  return sessions.reduce(
    (summary, session) => {
      const blockedInboundCount = Number(session.blockedInboundCount || 0);
      const backgroundJobCount = Number(session.backgroundJobCount || 0);
      const runningBackgroundJobCount = Number(session.runningBackgroundJobCount || 0);
      const failedBackgroundJobCount = Number(session.failedBackgroundJobCount || 0);
      return {
        sessionCount: summary.sessionCount + 1,
        blockedInboundCount: summary.blockedInboundCount + blockedInboundCount,
        backgroundJobCount: summary.backgroundJobCount + backgroundJobCount,
        runningBackgroundJobCount: summary.runningBackgroundJobCount + runningBackgroundJobCount,
        failedBackgroundJobCount: summary.failedBackgroundJobCount + failedBackgroundJobCount,
      };
    },
    {
      sessionCount: 0,
      blockedInboundCount: 0,
      backgroundJobCount: 0,
      runningBackgroundJobCount: 0,
      failedBackgroundJobCount: 0,
    },
  );
}

export type AdminRequestInit = RequestInit & {
  readonly timeoutMs?: number | undefined;
};

export async function requestJson(path: string, init: AdminRequestInit = {}): Promise<Record<string, any>> {
  const { timeoutMs, ...fetchInit } = init;
  let timeout: number | null = null;
  const responsePromise = fetch(path, fetchInit).then(async (response) => {
    const payload = await response.json().catch(() => ({}));
    if (!response.ok || payload.ok === false) {
      throw new Error(payload.error || response.statusText || "请求失败");
    }
    return payload as Record<string, any>;
  });
  if (!timeoutMs) {
    return await responsePromise;
  }
  try {
    return await Promise.race([
      responsePromise,
      new Promise<Record<string, any>>((_, reject) => {
        timeout = window.setTimeout(() => reject(new Error(`请求超时：${path}`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timeout !== null) {
      window.clearTimeout(timeout);
    }
  }
}

export function githubAccountDeviceStartApiPath(slackUserId: string): string {
  return "/admin/api/github-accounts/" + encodeURIComponent(slackUserId) + "/oauth/device/start";
}

export function githubDevicePollApiPath(deviceAuthorizationId: string): string {
  return "/admin/api/github-oauth/device/" + encodeURIComponent(deviceAuthorizationId);
}

export async function confirmInterruptRisk(operation: string, verb: string): Promise<boolean | null> {
  const preflight = await requestJson("/admin/api/preflight?operation=" + encodeURIComponent(operation));
  if (preflight.safe) return false;
  const detail = "运行任务：" + (preflight.runningJobCount || 0);
  return window.confirm(`${verb} 会中断正在运行的后台任务。${detail}。继续？`) ? true : null;
}

export function publishStatusFromPayload(payload: Record<string, any>): void {
  if (payload.status) {
    publishAdminStatus(payload.status);
  }
}
