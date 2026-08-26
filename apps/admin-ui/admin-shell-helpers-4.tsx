import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { formatAuthQuotaDisplay, formatWeightedWeeklyQuotaScore, remainingPercent, weightedWeeklyQuotaScore, daysUntilReset } from "./auth-profile-quota";

import { profileAccountLabel, profilePlanLabel, profileTitle } from "./auth-profile-display";

import { connectAdminRealtime, getAdminStatusSnapshot, mergeAdminStatusSnapshot, publishAdminStatus, subscribeAdminStatus } from "./admin-status-store";

import { AdminSessionsView } from "./session-view";

import { statusLabel } from "./timeline-display";

import { AdminStatus, AdminView, Tone, OperationsView, DeployPanel, OperationRecords, ProfilesPanel, GitHubAccountsPanel } from "./admin-shell-helpers-1.js";
import { LogsPanel, ServicePanel, AddProfileDialog, GitHubAccountBindDialog, TopbarProfiles, RiskPanel, RiskCell, DeploymentPanel, ReleaseTargetPanel } from "./admin-shell-helpers-2.js";
import {
  ReleaseRow,
  DeployTargetOption,
  buildDeployTargetOptions,
  ProfileQuotaMetrics,
  ProfileQuotaSummary,
  profileQuotaSummary,
  Badge,
  loadAdminStatus,
  loadAdminSessionsStatus,
  loadAdminOverview,
  loadAdminLogs,
  mergeStatusOverview,
  mergeStatusLogs,
  summarizeSessionRows,
  AdminRequestInit,
  requestJson,
  githubAccountDeviceStartApiPath,
  githubDevicePollApiPath,
  confirmInterruptRisk,
  publishStatusFromPayload,
  loadAdminView,
  persistAdminView,
  uiStateStorageKey,
  profileQuotaItems,
  normalizeGitHubAccounts,
} from "./admin-shell-helpers-3.js";

export function buildFallbackGitHubAccounts(status: AdminStatus): Array<Record<string, any>> {
  const rows = new Map<string, Record<string, any>>();
  const bindings = Array.isArray(status.githubPrIdentities?.bindings) ? status.githubPrIdentities.bindings : [];
  const sessions = Array.isArray(status.state?.sessions) ? status.state.sessions : [];
  const defaultAccount = status.githubAccounts?.defaultPrAccount;
  const defaultSlackUserId = defaultAccount?.available === true && defaultAccount.source === "bound" ? String(defaultAccount.slackUserId || "") : "";

  function addSlackUser(userId: unknown, identity?: Record<string, any> | null): void {
    const slackUserId = String(userId || "").trim();
    if (!slackUserId || slackUserId.startsWith("username:")) return;
    const normalizedIdentity = normalizeSlackIdentity(slackUserId, identity);
    const existing = rows.get(slackUserId);
    if (!rows.has(slackUserId)) {
      rows.set(slackUserId, {
        slackUserId,
        slackIdentity: normalizedIdentity,
        isDefaultPrAccount: slackUserId === defaultSlackUserId,
        prBinding: {
          state: "unbound",
        },
      });
      return;
    }
    if (existing) {
      rows.set(slackUserId, {
        ...existing,
        slackIdentity: mergeSlackIdentity(existing.slackIdentity, normalizedIdentity),
      });
    }
  }

  for (const session of sessions) {
    addSlackUser(session.initiatorUserId);
    addSlackUser(session.firstUserMessage?.userId, session.firstUserMessage?.slackIdentity || identityFromSessionMessage(session.firstUserMessage));
    addSlackUser(session.lastUserMessage?.userId, session.lastUserMessage?.slackIdentity || identityFromSessionMessage(session.lastUserMessage));
  }

  for (const binding of bindings) {
    const slackUserId = String(binding.slackUserId || "").trim();
    if (!slackUserId) continue;
    rows.set(slackUserId, {
      ...(rows.get(slackUserId) || {
        slackUserId,
        slackIdentity: normalizeSlackIdentity(slackUserId),
      }),
      isDefaultPrAccount: slackUserId === defaultSlackUserId,
      prBinding: {
        state: binding.revokedAt ? "revoked" : "bound",
        githubLogin: binding.githubLogin,
        githubUserId: binding.githubUserId,
        githubEmail: binding.githubEmail ?? null,
        githubName: binding.githubName ?? null,
        scopes: binding.scopes || [],
        createdAt: binding.createdAt,
        updatedAt: binding.updatedAt,
        lastValidatedAt: binding.lastValidatedAt ?? null,
        revokedAt: binding.revokedAt ?? null,
      },
    });
  }

  return [...rows.values()].sort((left, right) => {
    if (Boolean(left.isDefaultPrAccount) !== Boolean(right.isDefaultPrAccount)) {
      return left.isDefaultPrAccount ? -1 : 1;
    }
    const leftBound = left.prBinding?.state === "bound";
    const rightBound = right.prBinding?.state === "bound";
    if (leftBound !== rightBound) return leftBound ? -1 : 1;
    return String(left.slackUserId).localeCompare(String(right.slackUserId));
  });
}

export function normalizeSlackIdentity(slackUserId: string, identity?: Record<string, any> | null): Record<string, any> {
  return {
    userId: slackUserId,
    mention: `<@${slackUserId}>`,
    ...(identity?.username ? { username: identity.username } : {}),
    ...(identity?.displayName ? { displayName: identity.displayName } : {}),
    ...(identity?.realName ? { realName: identity.realName } : {}),
    ...(identity?.email ? { email: identity.email } : {}),
  };
}

export function mergeSlackIdentity(previous: Record<string, any> | undefined, next: Record<string, any>): Record<string, any> {
  return {
    ...normalizeSlackIdentity(String(next.userId || previous?.userId || "")),
    ...previous,
    ...Object.fromEntries(Object.entries(next).filter(([, value]) => value !== undefined && value !== null && value !== "")),
  };
}

export function identityFromSessionMessage(message: Record<string, any> | null | undefined): Record<string, any> | null {
  if (!message) return null;
  return normalizeSlackIdentity(String(message.userId || ""), message.senderUsername ? { username: message.senderUsername } : {});
}

export function githubBindingLabel(binding: Record<string, any>): string {
  if (binding.state === "bound") return "已绑定 " + (binding.githubLogin || "");
  if (binding.state === "revoked") return "绑定失效";
  return "未绑定";
}

export function githubBindingTone(binding: Record<string, any>): Tone {
  if (binding.state === "bound") return "good";
  if (binding.state === "revoked") return "danger";
  return "warn";
}

export function githubAccountOptionLabel(account: Record<string, any>): string {
  const identity = account.slackIdentity || {};
  const binding = account.prBinding || {};
  const slackLabel = identity.realName || identity.displayName || identity.username || account.slackUserId;
  const githubLabel = binding.githubLogin || "GitHub";
  return String(slackLabel) + " · " + String(githubLabel);
}

export function quotaTone(remaining: number): Tone {
  if (remaining < 10) return "danger";
  if (remaining < 30) return "warn";
  return "";
}

export function statusTone(status: unknown): Tone {
  const value = String(status || "").toLowerCase();
  if (["succeeded", "running", "active", "ok", "completed", "done"].includes(value)) return "good";
  if (["pending", "inflight", "registered", "starting", "idle", "started", "wait"].includes(value)) return "warn";
  if (["failed", "error", "stopped", "cancelled", "blocked"].includes(value)) return "danger";
  if (["agent_system_prompt", "agent_memory", "agent_runtime_instruction"].includes(value)) return "purple";
  if (value.startsWith("agent_")) return "info";
  if (["deploy", "rollback"].includes(value)) return "info";
  return "";
}

export function operationLabel(value: unknown): string {
  const labels: Record<string, string> = {
    deploy: "发布",
    rollback: "回滚",
    github_author_upsert: "保存 GitHub 作者",
    github_author_delete: "删除 GitHub 作者",
    github_pr_default_set: "设置默认 PR 账号",
  };
  return labels[String(value || "")] || String(value || "");
}

export function pickOperationLabel(operation: Record<string, any>): string {
  return operation?.request?.version || operation?.request?.ref || operation?.request?.name || operation?.request?.slackUserId || operation?.id || "-";
}

export function fmtTime(value: unknown): string {
  if (!value) return "--";
  const date = new Date(String(value));
  if (!Number.isFinite(date.getTime())) return String(value);
  return [String(date.getHours()).padStart(2, "0"), String(date.getMinutes()).padStart(2, "0"), String(date.getSeconds()).padStart(2, "0")].join(":");
}

export function fmtDateTime(value: unknown): string {
  if (!value) return "--";
  const date = new Date(String(value));
  if (!Number.isFinite(date.getTime())) return String(value);
  return [date.getFullYear(), String(date.getMonth() + 1).padStart(2, "0"), String(date.getDate()).padStart(2, "0")].join("-") + " " + fmtTime(value);
}

export function shortRevision(value: unknown): string {
  const text = String(value || "").trim();
  return text.length > 12 ? text.slice(0, 12) : text;
}

export function formatRelativeDuration(ms: number): string {
  const absMs = Math.abs(ms);
  const minutes = Math.round(absMs / 60_000);
  if (minutes < 60) return minutes + " 分钟";
  const hours = Math.round(absMs / 3_600_000);
  if (hours < 48) return hours + " 小时";
  return Math.round(absMs / 86_400_000) + " 天";
}

export function formatResetTime(seconds: unknown): string {
  const value = Number(seconds);
  if (!Number.isFinite(value)) return "未知";
  const delta = value * 1000 - Date.now();
  const relative = formatRelativeDuration(delta);
  return delta > 0 ? relative + "后" : relative + "前";
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
