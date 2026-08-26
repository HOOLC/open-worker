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
  fmtTokens,
  fmtPercent,
  shortValue,
} from "./session-view-helpers-5.js";

export function TimelineRow({ event }: { readonly event: TimelineEvent }): React.JSX.Element {
  const [detail, setDetail] = useState<string | null>(typeof event.detail === "string" ? event.detail : null);
  const [detailStatus, setDetailStatus] = useState<string | null>(null);
  const [detailOpen, setDetailOpen] = useState(false);
  const display = getTimelineEventDisplay(event);
  const badgeTone = statusTone(event.status === "failed" || event.status === "error" ? event.status : event.type);
  const kind = agentTranscriptKind(event);
  const toolTone = kind === "tool" ? statusTone(event.status || event.type) || badgeTone || "info" : "";
  const rowTone = kind === "tool" ? toolTone : badgeTone;
  const speaker = agentTranscriptSpeaker(kind, event);
  const isNotice = kind === "system" || kind === "session";
  const isCommandEvent = event.toolName === "exec_command";
  const canLoadDetail = Boolean(event.detailAvailable && event.id && event.sessionKey);
  const meta = [kind !== "tool" && kind !== "user" && kind !== "assistant" && kind !== "bot" && event.status ? statusLabel(event.status) : "", !isCommandEvent && event.toolName ? "工具 " + event.toolName : "", event.detailTruncated ? "内容已截断" : ""].filter(Boolean).join(" · ");

  useEffect(() => {
    setDetail(typeof event.detail === "string" ? event.detail : null);
    setDetailStatus(null);
    setDetailOpen(false);
  }, [event.id, event.detail]);

  async function loadDetail(): Promise<void> {
    if (detail || detailStatus === "loading" || !canLoadDetail) {
      return;
    }
    setDetailStatus("loading");
    try {
      const payload = (await requestJson(sessionTimelineEventApiPath(String(event.sessionKey), String(event.id)))) as Record<string, any>;
      const nextDetail = typeof payload.event?.detail === "string" ? payload.event.detail : "";
      setDetail(nextDetail || "没有详情");
      setDetailStatus(null);
    } catch (error) {
      setDetailStatus(error instanceof Error ? error.message : String(error));
    }
  }

  async function toggleDetail(): Promise<void> {
    const nextOpen = !detailOpen;
    setDetailOpen(nextOpen);
    if (nextOpen) {
      await loadDetail();
    }
  }

  function renderTraceDetails(): React.JSX.Element | null {
    if (!detail && !canLoadDetail) {
      return null;
    }
    return (
      <button
        type="button"
        className={"trace-details-button" + (detailOpen ? " open" : "")}
        aria-label="查看详情"
        title={detailOpen ? "收起详情" : "查看详情"}
        onClick={() => {
          void toggleDetail();
        }}
      >
        <span aria-hidden="true" className="trace-details-icon">
          i
        </span>
      </button>
    );
  }

  function renderTraceDetailPanel(): React.JSX.Element | null {
    if (!detailOpen) {
      return null;
    }
    return <pre className="trace-detail-panel">{detail || (detailStatus === "loading" ? "正在加载" : detailStatus || "")}</pre>;
  }

  return (
    <div className={"agent-message agent-message-" + kind + " " + rowTone}>
      <div className="agent-message-avatar" aria-hidden="true">
        {agentTranscriptAvatar(kind)}
      </div>
      <article className="agent-message-body">
        {isNotice ? (
          <div className="agent-notice">
            <span className="agent-notice-kind">{speaker}</span>
            <time dateTime={String(event.at || "")} title={fmtDateTime(event.at)}>
              {fmtTime(event.at)}
            </time>
            <Badge label={display.badgeLabel} tone={badgeTone} />
            <strong title={display.title}>{display.title}</strong>
            {display.summary ? <span title={display.summary}>{display.summary}</span> : null}
            {meta ? (
              <em className="trace-meta" title={meta}>
                {meta}
              </em>
            ) : null}
            {renderTraceDetails()}
          </div>
        ) : (
          <div className="agent-message-head">
            <strong className="agent-speaker">{speaker}</strong>
            <time dateTime={String(event.at || "")} title={fmtDateTime(event.at)}>
              {fmtTime(event.at)}
            </time>
            {kind === "tool" ? <Badge label={display.badgeLabel} tone={badgeTone} /> : null}
            {meta ? (
              <span className="trace-meta" title={meta}>
                {meta}
              </span>
            ) : null}
            {kind === "tool" ? null : renderTraceDetails()}
          </div>
        )}
        {!isNotice && kind === "tool" ? (
          <div className={"agent-tool-step " + toolTone}>
            <div>
              <strong title={display.title}>{display.title}</strong>
              {display.summary ? <em title={display.summary}>{display.summary}</em> : null}
            </div>
            <span className="agent-tool-status">{toolTimelineStatusLabel(event)}</span>
            {renderTraceDetails()}
          </div>
        ) : !isNotice ? (
          <div className="agent-message-content">
            <p title={display.title}>{display.title}</p>
            {display.summary ? <span title={display.summary}>{display.summary}</span> : null}
          </div>
        ) : null}
        {renderTraceDetailPanel()}
      </article>
    </div>
  );
}

export function JobsTable({ session, jobs, expectedCount }: { readonly session: SessionRecord; readonly jobs: readonly Record<string, any>[]; readonly expectedCount?: number }): React.JSX.Element {
  const [busyJobId, setBusyJobId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const sessionKey = String(session.key || "");

  async function cancelJob(job: Record<string, any>): Promise<void> {
    const jobId = String(job.id || "");
    if (!sessionKey || !jobId || !jobCancellable(job)) {
      return;
    }
    const confirmed = window.confirm("确认取消这个后台任务？");
    if (!confirmed) {
      return;
    }

    setBusyJobId(jobId);
    setMessage(null);
    try {
      const payload = await requestCancelSessionJob(sessionKey, jobId);
      if (payload.session && typeof payload.session === "object") {
        applyAdminRealtimeEvent({
          sequence: 0,
          kind: "session.update",
          scope: "session",
          sessionKey,
          session: payload.session,
          createdAt: new Date().toISOString(),
        });
      }
      const timelinePayload = await requestJson(sessionTimelineApiPath(sessionKey, { limit: TIMELINE_PAGE_SIZE }));
      publishTimelinePayload(sessionKey, timelinePayload as TimelinePayload);
      setMessage("已取消 job");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusyJobId(null);
    }
  }

  if (!jobs.length) return <div className="summary-detail">{expectedCount ? "任务明细加载中" : "没有运行任务"}</div>;
  return (
    <>
      <table className="table" style={{ marginTop: 10 }}>
        <thead>
          <tr>
            <th>状态</th>
            <th>类型</th>
            <th>操作</th>
          </tr>
        </thead>
        <tbody>
          {jobs.slice(0, 5).map((job, index) => {
            const jobId = String(job.id || "");
            const cancellable = jobCancellable(job);
            return (
              <tr key={(job.id || job.kind || "") + ":" + index}>
                <td>
                  <Badge label={job.status || "unknown"} tone={statusTone(job.status)} />
                </td>
                <td>{job.kind || ""}</td>
                <td>
                  {cancellable ? (
                    <button
                      type="button"
                      className="danger"
                      disabled={busyJobId === jobId || !sessionKey || !jobId}
                      onClick={() => {
                        void cancelJob(job);
                      }}
                    >
                      {busyJobId === jobId ? "取消中" : "取消"}
                    </button>
                  ) : (
                    <span className="summary-detail">-</span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {message ? (
        <div className="summary-detail" style={{ marginTop: 8 }}>
          {message}
        </div>
      ) : null}
    </>
  );
}

export function Badge({ label, tone, title }: { readonly label: unknown; readonly tone?: string; readonly title?: string }): React.JSX.Element {
  return (
    <span className={"badge " + (tone || statusTone(label))} title={title}>
      {statusLabel(label)}
    </span>
  );
}

export function sessionMatchesFilter(session: SessionRecord, mode: string): boolean {
  const hasIssue = Number(session.blockedInboundCount || 0) > 0 || sessionSelectionBlocked(session);
  if (mode === "jobs" && !activeBackgroundJobCount(session)) return false;
  if (mode === "issues" && !hasIssue) return false;
  return true;
}

export function resolveSelectedSession(sessions: readonly SessionRecord[], selectedSessionKey: string | null): SessionRecord | null {
  if (!sessions.length) return null;
  return sessions.find((session) => session.key === selectedSessionKey) || sessions[0] || null;
}

export function sessionPrimaryText(session: SessionRecord): string {
  return messagePreview(session.lastUserMessage) || summarizeSessionLead(session);
}

export function sessionFirstText(session: SessionRecord): string {
  return messagePreview(session.firstUserMessage) || "没有用户消息";
}

export function messagePreview(message: Record<string, any> | undefined): string {
  return String(message?.textPreview || message?.text || "").trim();
}

export function summarizeSessionLead(session: SessionRecord): string {
  if (session.lastUserMessage) return messagePreview(session.lastUserMessage) || "用户消息";
  const activeJob = activeBackgroundJobs(session)[0];
  if (activeJob) {
    const running = activeJob;
    return (running.kind || "任务") + "（" + statusLabel(running.status || "?") + "）";
  }
  return "暂无输入记录";
}

export function compareSessionsForMode(mode: string, left: SessionRecord, right: SessionRecord): number {
  if (mode === "all") {
    const activityDelta = sessionActivityMs(right) - sessionActivityMs(left);
    if (activityDelta) return activityDelta;
  }
  const rankDelta = sessionOperationalState(right).rank - sessionOperationalState(left).rank;
  if (rankDelta) return rankDelta;
  const activityDelta = sessionActivityMs(right) - sessionActivityMs(left);
  if (activityDelta) return activityDelta;
  return String(left.key).localeCompare(String(right.key));
}

export async function requestJson(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(path, init);
  const payload = (await response.json().catch(() => ({}))) as Record<string, any>;
  if (!response.ok || payload.ok === false) throw new Error(payload.error || response.statusText || "请求失败");
  return payload;
}
