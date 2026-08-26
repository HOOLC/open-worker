import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { profileDisplayLabel, profileIsSelectable, profileOptionLabel, profileQuotaLabel, profileSessionActionLabel, profileTitle } from "./auth-profile-display";

import { applyAdminRealtimeEvent, getAdminStatusSnapshot, getTimelineSnapshot, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { agentTranscriptAvatar, agentTranscriptKind, agentTranscriptSpeaker } from "./agent-transcript-display";

import { requestCancelSessionJob } from "./session-job-actions";

import { stableSessionOrder } from "./session-order";

import { activeBackgroundJobCount, activeBackgroundJobs, buildChannelLabelById, renderSessionMeta, resolveSessionChannelLabel, sessionActivityAt, sessionActivityMs, sessionOperationalState, shouldShowSessionState } from "./session-row-display";

import type { SessionOperationalState } from "./session-row-display";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, statusLabel, type TimelineEvent } from "./timeline-display";

import {
  UiState,
  SessionRecord,
  TimelinePayload,
  timelinePayloadSession,
  mergeSessionRecords,
  sessionFilters,
  TIMELINE_PAGE_SIZE,
  TIMELINE_AUTO_LOAD_THRESHOLD,
  GitHubBindPage,
  SessionPermalinkView,
  SessionRow,
  SessionDetail,
  AgentSessionHero,
  SessionActions,
  GitHubIdentityPanel,
  GitHubBindingFlow,
  GitHubBindingIntro,
  SessionResetButton,
  SessionRuntimePanel,
  MetaLine,
  SessionDebugPanel,
  SessionTraceStats,
  SessionTimeline,
  SessionSelectionPanel,
  TimelinePayloadView,
  mergeTimelinePayloads,
  mergeTimelineEvents,
  TraceSummary,
  Timeline,
  TimelineRow,
  JobsTable,
  Badge,
  sessionMatchesFilter,
  resolveSelectedSession,
  sessionPrimaryText,
  sessionFirstText,
  messagePreview,
  summarizeSessionLead,
  compareSessionsForMode,
  requestJson,
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
} from "./session-view-helpers.js";

export function AdminSessionsView(): React.JSX.Element {
  const githubBindSessionKey = readGitHubBindSessionKey();
  if (githubBindSessionKey) {
    return <GitHubBindPage sessionKey={githubBindSessionKey} />;
  }

  const permalinkSessionKey = readPermalinkSessionKey();
  if (permalinkSessionKey) {
    return <SessionPermalinkView sessionKey={permalinkSessionKey} />;
  }

  const snapshot = useSyncExternalStore(subscribeAdminStatus, getAdminStatusSnapshot, getAdminStatusSnapshot);
  const status = (snapshot.status || {}) as Record<string, any>;
  const sessions = (status.state?.sessions || []) as SessionRecord[];
  const state = status.state || {};
  const channelLabelById = useMemo(() => buildChannelLabelById(sessions), [sessions]);
  const [uiState, setUiState] = useState(loadUiState);
  const mode = uiState.sessionFilter;
  const orderRef = useRef<{ viewKey: string; keys: readonly string[] }>({ viewKey: "", keys: [] });

  const filtered = useMemo(() => {
    return sessions.filter((session) => sessionMatchesFilter(session, mode)).sort((left, right) => compareSessionsForMode(mode, left, right));
  }, [mode, sessions]);

  const filteredKeys = filtered.map((session) => String(session.key)).join("\u001f");
  const viewKey = mode;
  const filteredByKey = new Map(filtered.map((session) => [String(session.key), session]));

  orderRef.current = stableSessionOrder(
    orderRef.current,
    viewKey,
    filtered.map((session) => String(session.key)),
  );

  const orderedSessions = orderRef.current.keys.map((key) => filteredByKey.get(key)).filter((session): session is SessionRecord => Boolean(session));

  const selectedSession = resolveSelectedSession(orderedSessions, uiState.selectedSessionKey);

  useEffect(() => {
    if (selectedSession?.key && selectedSession.key !== uiState.selectedSessionKey) {
      updateSessionUiState({ selectedSessionKey: selectedSession.key });
    }
  }, [filteredKeys, selectedSession?.key, uiState.selectedSessionKey]);

  function updateSessionUiState(patch: Partial<UiState>): void {
    setUiState((previous) => {
      const next = normalizeUiState({ ...loadUiState(), ...previous, ...patch });
      persistUiState(next);
      return next;
    });
  }

  return (
    <div className="session-master-detail">
      <section className="panel session-index-panel">
        <div className="panel-head">
          <div className="panel-title">Agent 会话</div>
          <span className="summary-detail">
            {orderedSessions.length} / {sessions.length} · 投递失败 <span id="session-blocked-count">{state.blockedInboundCount || 0}</span>
          </span>
        </div>
        <div className="toolbar session-filter-bar">
          <span className="session-filter-label">视图</span>
          <select id="session-filter" value={mode} onChange={(event) => updateSessionUiState({ sessionFilter: event.target.value })}>
            <option value="all">全部</option>
            <option value="jobs">有运行任务</option>
            <option value="issues">有问题</option>
          </select>
        </div>
        <div id="sessions-panel" className="session-list">
          {orderedSessions.length ? (
            orderedSessions.map((session) => <SessionRow key={session.key} session={session} selected={selectedSession?.key === session.key} channelLabelById={channelLabelById} onSelect={() => updateSessionUiState({ selectedSessionKey: session.key })} />)
          ) : (
            <div className="empty-state">没有符合当前筛选的会话</div>
          )}
        </div>
      </section>

      <section className="panel session-detail-panel">
        <div id="session-detail-panel" className="panel-body">
          {selectedSession ? <SessionDetail key={selectedSession.key} session={selectedSession} /> : <div className="empty-state">没有可检查的 session</div>}
        </div>
      </section>
    </div>
  );
}
