import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { profileDisplayLabel, profileIsSelectable, profileOptionLabel, profileQuotaLabel, profileSessionActionLabel, profileTitle } from "./auth-profile-display";

import { applyAdminRealtimeEvent, getAdminStatusSnapshot, getTimelineSnapshot, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { agentTranscriptAvatar, agentTranscriptKind, agentTranscriptSpeaker } from "./agent-transcript-display";

import { requestCancelSessionJob } from "./session-job-actions";

import { stableSessionOrder } from "./session-order";

import { activeBackgroundJobCount, activeBackgroundJobs, buildChannelLabelById, renderSessionMeta, resolveSessionChannelLabel, sessionActivityAt, sessionActivityMs, sessionDeliveryIssueIndicator, sessionOperationalState, shouldShowSessionState } from "./session-row-display";

import type { SessionOperationalState } from "./session-row-display";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, statusLabel, type TimelineEvent } from "./timeline-display";

import { GitHubIdentityPanel, GitHubBindingFlow, GitHubBindingIntro, SessionResetButton, SessionRuntimePanel, MetaLine, SessionDebugPanel, SessionTraceStats } from "./session-view-helpers-2.js";
import { SessionTimeline, SessionSelectionPanel, TimelinePayloadView, mergeTimelinePayloads, mergeTimelineEvents, TraceSummary, Timeline } from "./session-view-helpers-3.js";
import { TimelineRow, JobsTable, Badge, sessionMatchesFilter, resolveSelectedSession, sessionPrimaryText, sessionFirstText, messagePreview, summarizeSessionLead, compareSessionsForMode, requestJson } from "./session-view-helpers-4.js";
import {
  sessionTimelineApiPath,
  sessionTimelineEventApiPath,
  slackThreadUrlApiPath,
  githubIdentityApiPath,
  githubDeviceStartApiPath,
  githubDevicePollApiPath,
  adminSessionPath,
  readGitHubBindSessionKey,
  readPermalinkSessionKey,
  decodePathSegment,
  loadUiState,
  persistUiState,
  uiStateStorageKey,
  defaultUiState,
  normalizeUiState,
  classSafeValue,
  statusTone,
  toolTimelineStatusLabel,
  jobCancellable,
  sourceLabel,
  timelineEventKey,
  timelineEventIdentity,
  timestampMs,
  newestTimestamp,
  fmtTime,
  fmtDateTime,
  fmtRelativeTime,
  fmtPercent,
  shortValue,
} from "./session-view-helpers-5.js";

export type UiState = {
  readonly adminView: string;
  readonly sessionFilter: string;
  readonly selectedSessionKey: string | null;
};

export type SessionRecord = Record<string, any>;

export type TimelinePayload =
  | {
      readonly events?: TimelineEvent[];
      readonly trace?: Record<string, any>;
      readonly session?: SessionRecord;
      readonly page?: {
        readonly limit?: number;
        readonly hasMore?: boolean;
        readonly nextBeforeSequence?: number | null;
      };
    }
  | TimelineEvent[];

export function timelinePayloadSession(payload: TimelinePayload | null): SessionRecord | null {
  return payload && !Array.isArray(payload) && payload.session ? payload.session : null;
}

export function mergeSessionRecords(base: SessionRecord | null | undefined, detail: SessionRecord | null | undefined): SessionRecord | null {
  if (!base) return detail || null;
  if (!detail) return base;
  return {
    ...detail,
    ...base,
    backgroundJobs: Array.isArray(detail.backgroundJobs) ? detail.backgroundJobs : base.backgroundJobs,
    failedBackgroundJobs: Array.isArray(detail.failedBackgroundJobs) ? detail.failedBackgroundJobs : base.failedBackgroundJobs,
    workspacePath: detail.workspacePath ?? base.workspacePath,
    id: detail.id ?? base.id,
    sessionPageLinkPostedAt: detail.sessionPageLinkPostedAt ?? base.sessionPageLinkPostedAt,
    lastObservedMessageTs: detail.lastObservedMessageTs ?? base.lastObservedMessageTs,
    lastDeliveredMessageTs: detail.lastDeliveredMessageTs ?? base.lastDeliveredMessageTs,
  };
}

export const sessionFilters = ["all", "jobs", "issues"];

export const TIMELINE_PAGE_SIZE = 30;

export const TIMELINE_AUTO_LOAD_THRESHOLD = 32;

export function GitHubBindPage({ sessionKey }: { readonly sessionKey: string }): React.JSX.Element {
  return (
    <div className="github-bind-page">
      <section className="github-bind-card">
        <div className="github-bind-head">
          <div className="github-bind-copy">
            <div className="panel-title">绑定 GitHub 账号</div>
            <div className="summary-detail">这个页面只负责把当前 Slack 发起人绑定到 GitHub。绑定完成后，后续 PR 会使用这个 GitHub 账号。</div>
          </div>
          <a className="link-button" href={adminSessionPath(sessionKey)}>
            返回 Session
          </a>
        </div>
        <GitHubBindingFlow sessionKey={sessionKey} variant="page" autoStart />
      </section>
    </div>
  );
}

export function SessionPermalinkView({ sessionKey }: { readonly sessionKey: string }): React.JSX.Element {
  const snapshot = useSyncExternalStore(subscribeAdminStatus, getAdminStatusSnapshot, getAdminStatusSnapshot);
  const sessions = ((snapshot.status || {}) as Record<string, any>).state?.sessions || [];
  const realtimeSession = (sessions as SessionRecord[]).find((session) => session.key === sessionKey) || null;
  const timelineSnapshot = useSyncExternalStore(
    (listener) => subscribeTimeline(sessionKey, listener),
    () => getTimelineSnapshot(sessionKey),
    () => getTimelineSnapshot(sessionKey),
  );
  const timelinePayload = timelineSnapshot.payload as TimelinePayload | null;
  const timelineSession = timelinePayloadSession(timelinePayload);
  const [fetchedSession, setFetchedSession] = useState<SessionRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const session = mergeSessionRecords(realtimeSession || fetchedSession || timelineSession, fetchedSession || timelineSession);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    void requestJson(sessionTimelineApiPath(sessionKey, { limit: TIMELINE_PAGE_SIZE }))
      .then((nextPayload) => {
        if (cancelled) return;
        const payload = nextPayload as TimelinePayload;
        publishTimelinePayload(sessionKey, payload);
        if (!Array.isArray(payload) && payload.session) {
          setFetchedSession(payload.session);
        }
      })
      .catch((nextError: unknown) => {
        if (!cancelled) setError(nextError instanceof Error ? nextError.message : String(nextError));
      });
    return () => {
      cancelled = true;
    };
  }, [sessionKey]);

  return (
    <div className="session-permalink-layout">
      <section className="session-detail-panel session-permalink-panel">
        <div className="panel-body">{error ? <div className="empty-state">{error}</div> : session ? <SessionDetail key={session.key} session={session} isPermalink /> : <div className="empty-state">正在加载会话</div>}</div>
      </section>
    </div>
  );
}

export function SessionRow({ session, selected, channelLabelById, onSelect }: { readonly session: SessionRecord; readonly selected: boolean; readonly channelLabelById?: ReadonlyMap<string, string>; readonly onSelect: () => void }): React.JSX.Element {
  const state = sessionOperationalState(session);
  const activityAt = sessionActivityAt(session);
  const primary = sessionPrimaryText(session);
  const first = sessionFirstText(session);
  const stateBadge = shouldShowSessionState(state) ? <Badge label={state.label} tone={state.tone} title={state.detail} /> : null;
  return (
    <button type="button" className={"session-row-button session-card session-priority-" + classSafeValue(state.tone, "idle") + (selected ? " active" : "")} data-session-key={session.key} onClick={onSelect}>
      <div className="session-summary">
        <div className="session-line">
          <div className="session-lead" title={primary}>
            {primary}
          </div>
          {stateBadge}
          <div className="session-time" title={fmtDateTime(activityAt)}>
            {fmtRelativeTime(activityAt)}
          </div>
        </div>
        <div className="session-channel" title={first}>
          {first}
        </div>
        <div className="session-meta-line">
          {renderSessionMeta(session, channelLabelById).map((pill) => (
            <span key={pill.key} className={"session-meta-pill " + classSafeValue(pill.tone, "")} title={pill.title}>
              {pill.label}
            </span>
          ))}
        </div>
      </div>
    </button>
  );
}

export function SessionDetail({ session: providedSession, isPermalink = false }: { readonly session: SessionRecord; readonly isPermalink?: boolean }): React.JSX.Element {
  const sessionKey = String(providedSession.key || "");
  const timelineSnapshot = useSyncExternalStore(
    (listener) => subscribeTimeline(sessionKey, listener),
    () => getTimelineSnapshot(sessionKey),
    () => getTimelineSnapshot(sessionKey),
  );
  const session = mergeSessionRecords(providedSession, timelinePayloadSession(timelineSnapshot.payload as TimelinePayload | null)) || providedSession;
  const snapshot = useSyncExternalStore(subscribeAdminStatus, getAdminStatusSnapshot, getAdminStatusSnapshot);
  const sessions = (((snapshot.status || {}) as Record<string, any>).state?.sessions || []) as SessionRecord[];
  const channelLabelById = buildChannelLabelById([...sessions, session]);
  const channelLabel = resolveSessionChannelLabel(session, channelLabelById);
  const state = sessionOperationalState(session);
  const activityAt = sessionActivityAt(session);
  const primary = sessionPrimaryText(session);
  const first = sessionFirstText(session);
  const blockedInbound = Number(session.blockedInboundCount || 0);
  const currentJobs = activeBackgroundJobs(session);
  const runningJobs = activeBackgroundJobCount(session);
  const totalJobs = Number(session.backgroundJobCount || (Array.isArray(session.backgroundJobs) ? session.backgroundJobs.length : 0));
  const hasJobs = runningJobs > 0;
  return (
    <>
      <AgentSessionHero title={primary} request={first} state={state} channelLabel={channelLabel} activityAt={activityAt} blockedInbound={blockedInbound} runningJobs={runningJobs} totalJobs={totalJobs} />
      <div className="session-body">
        <div className="session-inspector">
          <div className="mini-panel trace-panel session-timeline-panel">
            <div className="mini-title">工作时间线</div>
            <div className="mini-body">
              <SessionTimeline session={session} />
            </div>
          </div>
          <div className="session-side-column">
            <div className="mini-panel">
              <div className="mini-title">接管 / 链接</div>
              <div className="mini-body">
                <SessionActions session={session} isPermalink={isPermalink} />
              </div>
            </div>
            <SessionRuntimePanel session={session} state={state} blockedInbound={blockedInbound} totalJobs={totalJobs} runningJobs={runningJobs} />
            {hasJobs ? (
              <div className="mini-panel">
                <div className="mini-title">后台任务</div>
                <div className="mini-body">{runningJobs > 0 ? <JobsTable session={session} jobs={currentJobs} expectedCount={runningJobs} /> : null}</div>
              </div>
            ) : null}
            <div className="mini-panel">
              <div className="mini-title">时间线统计</div>
              <div className="mini-body">
                <SessionTraceStats sessionKey={String(session.key || "")} />
              </div>
            </div>
            <div className="mini-panel">
              <div className="mini-title">技术上下文</div>
              <div className="mini-body">
                <SessionDebugPanel session={session} channelLabel={channelLabel} channelTitle={String(session.channelId || "")} activityAt={activityAt} />
              </div>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}

export function AgentSessionHero({
  title,
  request,
  state,
  channelLabel,
  activityAt,
  blockedInbound,
  runningJobs,
  totalJobs,
}: {
  readonly title: string;
  readonly request: string;
  readonly state: SessionOperationalState;
  readonly channelLabel: string;
  readonly activityAt: unknown;
  readonly blockedInbound: number;
  readonly runningJobs: number;
  readonly totalJobs: number;
}): React.JSX.Element {
  const deliveryIssue = sessionDeliveryIssueIndicator({ blockedInboundCount: blockedInbound });
  const visibleState = shouldShowSessionState(state) ? { label: state.label, tone: state.tone, title: state.detail } : { label: "Agent 状态未知", tone: "", title: "Gateway 不持有 Agent 执行状态" };
  const stats = [
    { label: "频道", value: channelLabel, title: channelLabel },
    {
      label: "最近",
      value: fmtRelativeTime(activityAt),
      detail: fmtDateTime(activityAt),
      title: fmtDateTime(activityAt),
    },
    runningJobs > 0
      ? {
          label: "任务",
          value: runningJobs + " 运行",
          detail: totalJobs > runningJobs ? "历史共 " + totalJobs : undefined,
          tone: "good",
        }
      : null,
    deliveryIssue
      ? {
          label: "投递失败",
          value: deliveryIssue.value,
          title: deliveryIssue.title,
          tone: deliveryIssue.tone,
        }
      : null,
  ].filter((item): item is { label: string; value: string; detail?: string; tone?: string; title?: string } => Boolean(item));

  return (
    <div className={"agent-session-hero " + classSafeValue(visibleState.tone, "neutral")}>
      <div className="agent-session-copy">
        <div className="agent-session-kicker">
          <span>Agent Session</span>
          <Badge label={visibleState.label} tone={visibleState.tone} title={visibleState.title} />
        </div>
        <h1 className="agent-session-title" title={title}>
          {title}
        </h1>
        <div className="agent-session-request" title={request}>
          {request}
        </div>
      </div>
      <div className="agent-session-stat-grid">
        {stats.map((item) => (
          <div key={item.label} className={"agent-session-stat " + classSafeValue(item.tone, "")} title={item.title || item.detail || item.value}>
            <span>{item.label}</span>
            <strong>{item.value}</strong>
            {item.detail ? <em>{item.detail}</em> : null}
          </div>
        ))}
      </div>
    </div>
  );
}

export function SessionActions({ session, isPermalink }: { readonly session: SessionRecord; readonly isPermalink: boolean }): React.JSX.Element {
  const sessionKey = String(session.key || "");
  const isSlackSession = String(session.platform || "slack") === "slack";
  const [threadBusy, setThreadBusy] = useState(false);
  const [threadError, setThreadError] = useState<string | null>(null);

  async function openSlackThread(): Promise<void> {
    if (!sessionKey || threadBusy) {
      return;
    }
    const opened = window.open("", "_blank");
    setThreadBusy(true);
    setThreadError(null);
    try {
      const payload = (await requestJson(slackThreadUrlApiPath(sessionKey))) as Record<string, any>;
      const url = typeof payload.url === "string" ? payload.url : "";
      if (!url) {
        throw new Error("Slack permalink missing");
      }
      if (opened) {
        try {
          opened.opener = null;
        } catch {}
        opened.location.href = url;
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch (error) {
      if (opened) {
        opened.close();
      }
      setThreadError("Slack Thread 跳转失败：" + (error instanceof Error ? error.message : String(error)));
    } finally {
      setThreadBusy(false);
    }
  }

  return (
    <div className="side-action-stack">
      <SessionSelectionPanel session={session} />
      <GitHubIdentityPanel session={session} />
      <SessionResetButton session={session} />
      <div className="side-link-grid">
        {!isPermalink ? (
          <a className="link-button" href={adminSessionPath(String(session.key || ""))}>
            打开独立视图
          </a>
        ) : (
          <a className="link-button" href="/admin">
            返回会话列表
          </a>
        )}
        {isSlackSession ? (
          <button
            type="button"
            className="link-button"
            disabled={threadBusy || !sessionKey}
            onClick={() => {
              void openSlackThread();
            }}
          >
            {threadBusy ? "正在打开 Slack 线程" : "打开 Slack 线程"}
          </button>
        ) : null}
      </div>
      {threadError ? <div className="summary-detail">{threadError}</div> : null}
    </div>
  );
}
