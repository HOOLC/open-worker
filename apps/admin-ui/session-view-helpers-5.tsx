import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { profileDisplayLabel, profileIsSelectable, profileOptionLabel, profileQuotaLabel, profileSessionActionLabel, profileTitle } from "./auth-profile-display";

import { applyAdminRealtimeEvent, getAdminStatusSnapshot, getTimelineSnapshot, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { agentTranscriptAvatar, agentTranscriptKind, agentTranscriptSpeaker } from "./agent-transcript-display";

import { requestCancelSessionJob } from "./session-job-actions";

import { stableSessionOrder } from "./session-order";

import { activeBackgroundJobCount, activeBackgroundJobs, buildChannelLabelById, renderSessionMeta, resolveSessionChannelLabel, sessionActivityAt, sessionActivityMs, sessionOperationalState, sessionSelectionBlocked, shouldShowSessionState } from "./session-row-display";

import type { SessionOperationalState } from "./session-row-display";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, statusLabel, type TimelineEvent } from "./timeline-display";

import { UiState, SessionRecord, TimelinePayload, timelinePayloadSession, mergeSessionRecords, sessionFilters, TIMELINE_PAGE_SIZE, TIMELINE_AUTO_LOAD_THRESHOLD, GitHubBindPage, SessionPermalinkView, SessionRow, SessionDetail, AgentSessionHero, SessionActions } from "./session-view-helpers-1.js";
import { GitHubIdentityPanel, GitHubBindingFlow, GitHubBindingIntro, SessionResetButton, SessionRuntimePanel, MetaLine, SessionDebugPanel, SessionTraceStats } from "./session-view-helpers-2.js";
import { SessionTimeline, SessionSelectionPanel, TimelinePayloadView, mergeTimelinePayloads, mergeTimelineEvents, TraceSummary, Timeline } from "./session-view-helpers-3.js";
import { TimelineRow, JobsTable, Badge, sessionMatchesFilter, resolveSelectedSession, sessionPrimaryText, sessionFirstText, messagePreview, summarizeSessionLead, compareSessionsForMode, requestJson } from "./session-view-helpers-4.js";

export function sessionTimelineApiPath(
  sessionKey: string,
  options: {
    readonly limit?: number | undefined;
    readonly beforeSequence?: number | undefined;
  } = {},
): string {
  const params = new URLSearchParams();
  if (options.limit) {
    params.set("limit", String(options.limit));
  }
  if (options.beforeSequence) {
    params.set("before_sequence", String(options.beforeSequence));
  }
  const query = params.toString();
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/timeline" + (query ? "?" + query : "");
}

export function sessionTimelineEventApiPath(sessionKey: string, eventId: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/timeline-events/" + encodeURIComponent(eventId);
}

export function slackThreadUrlApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/slack-thread-url";
}

export function githubIdentityApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/github-identity";
}

export function githubDeviceStartApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/github-oauth/device/start";
}

export function githubDevicePollApiPath(deviceAuthorizationId: string): string {
  return "/admin/api/github-oauth/device/" + encodeURIComponent(deviceAuthorizationId);
}

export function adminSessionPath(sessionKey: string): string {
  return "/admin/sessions/" + encodeURIComponent(sessionKey);
}

export function readGitHubBindSessionKey(): string | null {
  const match = window.location.pathname.match(/^\/admin\/sessions\/([^/]+)\/github\/bind\/?$/);
  if (!match?.[1]) {
    return null;
  }
  return decodePathSegment(match[1]);
}

export function readPermalinkSessionKey(): string | null {
  if (readGitHubBindSessionKey()) {
    return null;
  }
  const prefix = "/admin/sessions/";
  if (!window.location.pathname.startsWith(prefix)) {
    return null;
  }
  const encoded = window.location.pathname.slice(prefix.length).split("/")[0] || "";
  if (!encoded) {
    return null;
  }
  return decodePathSegment(encoded);
}

export function decodePathSegment(encoded: string): string {
  try {
    return decodeURIComponent(encoded);
  } catch {
    return encoded;
  }
}

export function loadUiState(): UiState {
  try {
    const raw = window.localStorage.getItem(uiStateStorageKey());
    return raw ? normalizeUiState(JSON.parse(raw)) : defaultUiState();
  } catch {
    return defaultUiState();
  }
}

export function persistUiState(next: UiState): void {
  try {
    window.localStorage.setItem(uiStateStorageKey(), JSON.stringify(next));
  } catch {}
}

export function uiStateStorageKey(): string {
  return "admin-ui-state:" + window.location.pathname;
}

export function defaultUiState(): UiState {
  return { adminView: "sessions", sessionFilter: "all", selectedSessionKey: null };
}

export function normalizeUiState(value: unknown): UiState {
  const next = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const adminView = ["sessions", "ops"].includes(String(next.adminView || "")) ? String(next.adminView) : "sessions";
  const sessionFilter = sessionFilters.includes(String(next.sessionFilter || "")) ? String(next.sessionFilter) : "all";
  const selectedSessionKey = typeof next.selectedSessionKey === "string" && next.selectedSessionKey ? next.selectedSessionKey : null;
  return { adminView, sessionFilter, selectedSessionKey };
}

export function classSafeValue(value: unknown, fallback: string): string {
  const text = String(value || fallback || "").replace(/[^a-z0-9_-]/gi, "");
  return text || fallback || "";
}

export function statusTone(status: unknown): string {
  const value = String(status || "").toLowerCase();
  if (["succeeded", "running", "active", "ok", "completed", "done"].includes(value)) return "good";
  if (["pending", "inflight", "registered", "starting", "idle", "started", "wait"].includes(value)) return "warn";
  if (["failed", "error", "stopped", "cancelled", "blocked"].includes(value)) return "danger";
  if (["agent_system_prompt", "agent_memory", "agent_runtime_instruction"].includes(value)) return "purple";
  if (["agent_user_message", "agent_assistant_message", "agent_tool_result", "agent_token_count"].includes(value)) return "good";
  if (["agent_runtime_reminder", "agent_tool_call"].includes(value)) return "warn";
  if (value.startsWith("agent_")) return "info";
  if (["deploy", "rollback"].includes(value)) return "info";
  return "";
}

export function toolTimelineStatusLabel(event: TimelineEvent): string {
  if (event.status) {
    return statusLabel(event.status);
  }
  const type = String(event.type || "").toLowerCase();
  if (type === "agent_tool_call") {
    return "运行中";
  }
  if (type === "agent_tool_result") {
    return "完成";
  }
  return "工具";
}

export function jobCancellable(job: Record<string, any>): boolean {
  const status = String(job.status || "").toLowerCase();
  return status === "registered" || status === "running";
}

export function sourceLabel(value: unknown): string {
  const labels: Record<string, string> = {
    app_mention: "提及",
    direct_message: "私信",
    thread_reply: "线程回复",
    background_job_event: "后台任务事件",
    admin_session_reset: "Session 重置",
  };
  return labels[String(value || "")] || String(value || "");
}

export function timelineEventKey(event: TimelineEvent, index: number): string {
  return timelineEventIdentity(event) || "event-" + index;
}

export function timelineEventIdentity(event: TimelineEvent): string {
  return [event.id, event.sequence, event.at, event.type, event.callId, event.toolName, event.title, event.summary].filter(Boolean).join("\u001f");
}

export function timestampMs(value: unknown): number {
  const parsed = Date.parse(String(value || ""));
  return Number.isFinite(parsed) ? parsed : 0;
}

export function newestTimestamp(values: readonly unknown[]): number {
  return values.reduce((latest, value) => Math.max(latest, timestampMs(value)), 0);
}

export function fmtTime(value: unknown): string {
  if (!value) return "--";
  try {
    const date = new Date(String(value));
    const hours = String(date.getHours()).padStart(2, "0");
    const minutes = String(date.getMinutes()).padStart(2, "0");
    const seconds = String(date.getSeconds()).padStart(2, "0");
    return hours + ":" + minutes + ":" + seconds;
  } catch {
    return String(value);
  }
}

export function fmtDateTime(value: unknown): string {
  if (!value) return "--";
  try {
    const date = new Date(String(value));
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    const hours = String(date.getHours()).padStart(2, "0");
    const minutes = String(date.getMinutes()).padStart(2, "0");
    const seconds = String(date.getSeconds()).padStart(2, "0");
    return year + "-" + month + "-" + day + " " + hours + ":" + minutes + ":" + seconds;
  } catch {
    return String(value);
  }
}

export function fmtRelativeTime(value: unknown): string {
  const ms = timestampMs(value);
  if (!ms) return "--";
  const delta = Date.now() - ms;
  if (delta < 0) return fmtTime(value);
  if (delta < 45000) return "刚刚";
  if (delta < 3600000) return Math.max(1, Math.round(delta / 60000)) + " 分钟前";
  if (delta < 86400000) return Math.round(delta / 3600000) + " 小时前";
  if (delta < 604800000) return Math.round(delta / 86400000) + " 天前";
  return fmtDateTime(value);
}

export function fmtTokens(value: unknown): string {
  const count = Math.max(0, Number(value || 0));
  if (count >= 1000000) return (count / 1000000).toFixed(2).replace(/\.00$/, "") + "M";
  if (count >= 1000) return (count / 1000).toFixed(1).replace(/\.0$/, "") + "K";
  return String(Math.round(count));
}

export function fmtPercent(value: number): string {
  if (!Number.isFinite(value)) return "0%";
  const percent = Math.max(0, Math.min(999, value * 100));
  if (percent >= 10) return Math.round(percent) + "%";
  return percent.toFixed(1).replace(/\.0$/, "") + "%";
}

export function shortValue(value: unknown, maxLength: number): string {
  const text = String(value || "");
  if (text.length <= maxLength) return text;
  return text.slice(0, Math.max(4, maxLength - 5)) + "..." + text.slice(-4);
}
