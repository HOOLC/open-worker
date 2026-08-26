import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { formatAuthQuotaDisplayParts, formatWeightedWeeklyQuotaScore, remainingPercent, weightedWeeklyQuotaScore, daysUntilReset } from "./auth-profile-quota";

import { formatUsageBalance, profileQuotaLabel, profileTitle, usageRemainingOf } from "./auth-profile-display";

import { connectAdminRealtime, getAdminStatusSnapshot, mergeAdminStatusSnapshot, publishAdminStatus, subscribeAdminStatus } from "./admin-status-store";

import { AdminSessionsView } from "./session-view";

import { statusLabel } from "./timeline-display";

import { AdminStatus, AdminView, Tone, OperationsView, DeployPanel, OperationRecords, ProfilesPanel, GitHubAccountsPanel } from "./admin-shell-helpers-1.js";
import { LogsPanel, ServicePanel, AddProfileDialog, GitHubAccountBindDialog, TopbarProfiles, RiskPanel, RiskCell, DeploymentPanel, ReleaseTargetPanel } from "./admin-shell-helpers-2.js";
import {
  buildFallbackGitHubAccounts,
  normalizeSlackIdentity,
  mergeSlackIdentity,
  identityFromSessionMessage,
  githubBindingLabel,
  githubBindingTone,
  githubAccountOptionLabel,
  quotaTone,
  statusTone,
  operationLabel,
  pickOperationLabel,
  fmtTime,
  fmtDateTime,
  shortRevision,
  formatRelativeDuration,
  formatResetTime,
  errorMessage,
} from "./admin-shell-helpers-4.js";

export function ReleaseRow({ label, release }: { readonly label: string; readonly release: any }): React.JSX.Element {
  if (!release?.targetPath) {
    return <div className="summary-detail">{label}：无</div>;
  }
  const metadata = release.metadata || {};
  const heading = metadata.packageVersion || metadata.shortRevision || metadata.revision || String(release.targetPath).split("/").pop() || "release";
  const detailTime = metadata.installedAt || metadata.builtAt;
  return (
    <div className="release-row">
      <div className="profile-line">
        <span className="profile-account">
          {label}：{heading}
        </span>
        <span className="profile-plan">{metadata.packageName || metadata.branch || "package"}</span>
      </div>
      <div className="summary-detail">{detailTime ? fmtDateTime(detailTime) : release.targetPath}</div>
    </div>
  );
}

export type DeployTargetOption = {
  readonly value: string;
  readonly label: string;
};

export function buildDeployTargetOptions(deployment: any): readonly DeployTargetOption[] {
  const versions = Array.isArray(deployment?.recentPackageVersions) ? deployment.recentPackageVersions : [];
  return versions
    .map((entry: Record<string, any>) => {
      const version = String(entry.version || "").trim();
      if (!version) return null;
      const spec = String(entry.packageSpec || "").trim();
      return {
        value: version,
        label: spec || version,
      };
    })
    .filter((option): option is DeployTargetOption => Boolean(option));
}

export function ProfileQuotaMetrics({ quota }: { readonly quota: ProfileQuotaSummary }): React.JSX.Element {
  if (quota.ok === false) {
    return <div className="profile-quota-error">{quota.error}</div>;
  }
  return (
    <div className="profile-quota-block" title={quota.fullLabel}>
      <div className="profile-quota-metrics">
        <div className={"profile-quota-metric " + quota.tone}>
          <span>{quota.remainingCaption}</span>
          <strong>{quota.remainingLabel}</strong>
        </div>
        <div className={"profile-quota-metric " + quota.tone}>
          <span>{quota.scoreCaption}</span>
          <strong>{quota.scoreLabel}</strong>
        </div>
        <div className="profile-quota-metric">
          <span>重置</span>
          <strong>{quota.resetLabel}</strong>
        </div>
      </div>
      {quota.shortLabel ? (
        <div className="profile-short-window">
          <span>短窗</span>
          <strong>{quota.shortLabel}</strong>
        </div>
      ) : null}
    </div>
  );
}

export type ProfileQuotaSummary =
  | {
      readonly ok: true;
      readonly fullLabel: string;
      readonly remainingCaption: string;
      readonly remainingLabel: string;
      readonly scoreCaption: string;
      readonly scoreLabel: string;
      readonly resetLabel: string;
      readonly shortLabel: string | null;
      readonly tone: Tone;
    }
  | {
      readonly ok: false;
      readonly error: string;
      readonly tone: Tone;
    };

export function profileQuotaSummary(profile: any): ProfileQuotaSummary {
  const rateLimits = profile?.rateLimits ?? profile;
  if (!rateLimits || rateLimits.ok === false) {
    return {
      ok: false,
      error: rateLimits?.error || "额度不可用",
      tone: "danger",
    };
  }

  if (profile?.billing === "usage") {
    const remaining = usageRemainingOf(profile);
    const remainingLabel = remaining === undefined ? "--" : remaining === Number.POSITIVE_INFINITY ? "无限" : `$${formatUsageBalance(remaining)}`;
    return {
      ok: true,
      fullLabel: profileQuotaLabel(profile),
      remainingCaption: "余额",
      remainingLabel,
      scoreCaption: "加权",
      scoreLabel: "—",
      resetLabel: "按量",
      shortLabel: null,
      tone: quotaTone(remaining && remaining > 0 ? 100 : 0),
    };
  }

  const snapshot = rateLimits.rateLimits || {};
  const display = formatAuthQuotaDisplayParts({
    primary: snapshot.primary,
    secondary: snapshot.secondary,
  });
  const weekly = display.windows.weekly;
  const remaining = remainingPercent(weekly?.usedPercent);
  const score = weightedWeeklyQuotaScore(remaining, daysUntilReset(weekly?.resetsAt));
  return {
    ok: true,
    fullLabel: display.fullLabel || "额度未知",
    remainingCaption: "7d 剩余",
    remainingLabel: remaining === undefined ? "--" : `${Math.round(remaining)}%`,
    scoreCaption: "加权",
    scoreLabel: formatWeightedWeeklyQuotaScore(score),
    resetLabel: formatResetTime(weekly?.resetsAt),
    shortLabel: display.shortLabel,
    tone: quotaTone(remaining ?? 100) || (display.weeklyLabel ? "" : "warn"),
  };
}

export function Badge({ label, tone = "" }: { readonly label: string; readonly tone?: Tone }): React.JSX.Element {
  return <span className={"badge " + (tone || statusTone(label))}>{statusLabel(label)}</span>;
}

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

export function loadAdminView(): AdminView {
  try {
    const raw = window.localStorage.getItem(uiStateStorageKey());
    const parsed = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
    return parsed.adminView === "ops" ? "ops" : "sessions";
  } catch {
    return "sessions";
  }
}

export function persistAdminView(adminView: AdminView): void {
  try {
    const raw = window.localStorage.getItem(uiStateStorageKey());
    const previous = raw ? (JSON.parse(raw) as Record<string, unknown>) : {};
    window.localStorage.setItem(uiStateStorageKey(), JSON.stringify({ ...previous, adminView }));
  } catch {}
}

export function uiStateStorageKey(): string {
  return "admin-ui-state:" + window.location.pathname;
}

export function profileQuotaItems(profiles: readonly Record<string, any>[]): Array<{
  readonly label: string;
  readonly title: string;
  readonly billing: "subscription" | "usage";
  readonly score: number;
  readonly remaining: number;
}> {
  return profiles
    .map((profile) => {
      const billing = profile.billing === "usage" ? ("usage" as const) : ("subscription" as const);
      const rateLimits = profile.rateLimits || {};
      if (rateLimits.ok === false) return null;
      if (billing === "usage") {
        const remaining = usageRemainingOf(profile);
        if (remaining === undefined) return null;
        return {
          label: profileQuotaLabel(profile),
          title: profileTitle(profile),
          billing,
          score: remaining === Number.POSITIVE_INFINITY ? Number.POSITIVE_INFINITY : remaining,
          remaining: remaining > 0 ? 100 : 0,
        };
      }
      const limits = rateLimits.rateLimits || {};
      const display = formatAuthQuotaDisplayParts({
        primary: limits.primary,
        secondary: limits.secondary,
      });
      if (!display.fullLabel) return null;
      const weekly = display.windows.weekly;
      const remaining = remainingPercent(weekly?.usedPercent);
      const score = weightedWeeklyQuotaScore(remaining, daysUntilReset(weekly?.resetsAt));
      return {
        label: display.fullLabel,
        title: profileTitle(profile),
        billing,
        score: score ?? -1,
        remaining: remaining ?? 0,
      };
    })
    .filter(
      (
        item,
      ): item is {
        readonly label: string;
        readonly title: string;
        readonly billing: "subscription" | "usage";
        readonly score: number;
        readonly remaining: number;
      } => Boolean(item),
    )
    .sort((left, right) => {
      if (left.billing !== right.billing) {
        return left.billing === "subscription" ? -1 : 1;
      }
      return right.score - left.score || right.remaining - left.remaining || left.title.localeCompare(right.title);
    });
}

export function normalizeGitHubAccounts(status: AdminStatus): Array<Record<string, any>> {
  const accounts = status.githubAccounts?.accounts;
  if (Array.isArray(accounts) && accounts.length > 0) return accounts;
  const fallback = buildFallbackGitHubAccounts(status);
  return fallback;
}
