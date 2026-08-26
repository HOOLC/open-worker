import { applyAdminRealtimeEvent, getAdminStatusSnapshot, getTimelineSnapshot, publishAdminStatus, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { agentTranscriptAvatar, agentTranscriptKind, agentTranscriptSpeaker } from "./agent-transcript-display";

import { profileTitle } from "./auth-profile-display";

import { AUTOMATIC_PROFILE_ID, defaultThinking, modelOptions, profileOptions, thinkingOptions } from "./profile-selection";

import { requestJson, sessionTimelineApiPath, sessionTimelineEventApiPath } from "./session-api.js";

import { Badge } from "./session-badge.js";

import { fmtDateTime, fmtTime, jobCancellable, statusTone, timelineEventIdentity, timelineEventKey, timestampMs, toolTimelineStatusLabel } from "./session-formatters.js";

import { requestCancelSessionJob } from "./session-job-actions";

import { sessionSelectionBlocked } from "./session-row-display";

import { SessionRecord, TIMELINE_AUTO_LOAD_THRESHOLD, TIMELINE_PAGE_SIZE, TimelinePayload } from "./session-types.js";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, statusLabel, TimelineEvent } from "./timeline-display";

import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

export function SessionTimeline({ session }: { readonly session: SessionRecord }): React.JSX.Element {
  const sessionKey = String(session.key || "");
  const timelineSnapshot = useSyncExternalStore(
    (listener) => subscribeTimeline(sessionKey, listener),
    () => getTimelineSnapshot(sessionKey),
    () => getTimelineSnapshot(sessionKey),
  );
  const payload = timelineSnapshot.payload as TimelinePayload | null;
  const [error, setError] = useState<string | null>(null);
  const [olderBusy, setOlderBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setError(null);
    void requestJson(sessionTimelineApiPath(sessionKey, { limit: TIMELINE_PAGE_SIZE }))
      .then((nextPayload) => {
        if (cancelled) return;
        publishTimelinePayload(sessionKey, nextPayload as TimelinePayload);
      })
      .catch((nextError: unknown) => {
        if (!cancelled) setError(nextError instanceof Error ? nextError.message : String(nextError));
      });
    return () => {
      cancelled = true;
    };
  }, [sessionKey]);

  const loadOlder = useCallback(async (): Promise<void> => {
    if (!payload || Array.isArray(payload) || olderBusy || !payload.page?.hasMore || !payload.page.nextBeforeSequence) {
      return;
    }
    setOlderBusy(true);
    setError(null);
    try {
      const olderPayload = (await requestJson(
        sessionTimelineApiPath(sessionKey, {
          limit: TIMELINE_PAGE_SIZE,
          beforeSequence: payload.page.nextBeforeSequence,
        }),
      )) as TimelinePayload;
      publishTimelinePayload(sessionKey, mergeTimelinePayloads(getTimelineSnapshot(sessionKey).payload as TimelinePayload | null, olderPayload));
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setOlderBusy(false);
    }
  }, [olderBusy, payload, sessionKey]);

  useEffect(() => {
    if (!payload || Array.isArray(payload) || olderBusy || !payload.page?.hasMore) {
      return;
    }
    const visibleCount = filterVisibleTimelineEvents(payload.events || []).length;
    if (visibleCount >= TIMELINE_PAGE_SIZE) {
      return;
    }
    void loadOlder();
  }, [loadOlder, olderBusy, payload]);

  if (error) return <div className="summary-detail">{error}</div>;
  if (!payload) return <Timeline events={[{ at: session.createdAt, type: "session", title: "已创建" }]} />;
  const page = !Array.isArray(payload) ? payload.page : null;
  return (
    <div className="timeline-shell">
      <TimelinePayloadView payload={payload} hasMore={Boolean(page?.hasMore)} olderBusy={olderBusy} onLoadOlder={loadOlder} />
    </div>
  );
}

export function SessionSelectionPanel({ session }: { readonly session: SessionRecord }): React.JSX.Element {
  const snapshot = useSyncExternalStore(subscribeAdminStatus, getAdminStatusSnapshot, getAdminStatusSnapshot);
  const profiles = (((snapshot.status || {}) as Record<string, any>).profiles?.items || []) as Record<string, any>[];
  const models = useMemo(() => modelOptions(profiles), [profiles]);
  const capabilitySignature = JSON.stringify(
    profiles.map((profile) => ({
      profile_id: profile.profile_id,
      auth_configured: profile.auth_configured,
      models: profile.models,
    })),
  );
  const initialModel = models.includes(String(session.model || "")) ? String(session.model) : models[0] || "";
  const initialThinkingOptions = thinkingOptions(profiles, initialModel);
  const initialThinking = initialThinkingOptions.includes(String(session.thinking || "")) ? String(session.thinking) : defaultThinking(profiles, initialModel);
  const initialProfiles = profileOptions(profiles, initialModel, initialThinking);
  const initialProfile = initialProfiles.some((profile) => profile.profile_id === session.profileId) ? String(session.profileId) : AUTOMATIC_PROFILE_ID;
  const [model, setModel] = useState(initialModel);
  const [thinking, setThinking] = useState(initialThinking);
  const [profileId, setProfileId] = useState(initialProfile);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const thinkings = useMemo(() => thinkingOptions(profiles, model), [capabilitySignature, model]);
  const compatibleProfiles = profileOptions(profiles, model, thinking);
  const blocked = sessionSelectionBlocked(session);

  useEffect(() => {
    const nextModels = modelOptions(profiles);
    const nextModel = nextModels.includes(String(session.model || "")) ? String(session.model) : nextModels[0] || "";
    const nextThinkings = thinkingOptions(profiles, nextModel);
    const nextThinking = nextThinkings.includes(String(session.thinking || "")) ? String(session.thinking) : defaultThinking(profiles, nextModel);
    const nextProfiles = profileOptions(profiles, nextModel, nextThinking);
    setModel(nextModel);
    setThinking(nextThinking);
    setProfileId(nextProfiles.some((profile) => profile.profile_id === session.profileId) ? String(session.profileId) : AUTOMATIC_PROFILE_ID);
  }, [session.key, session.model, session.thinking, session.profileId, capabilitySignature]);

  const chooseModel = (nextModel: string): void => {
    const nextThinkings = thinkingOptions(profiles, nextModel);
    const nextThinking = nextThinkings.includes(thinking) ? thinking : defaultThinking(profiles, nextModel);
    const nextProfiles = profileOptions(profiles, nextModel, nextThinking);
    setModel(nextModel);
    setThinking(nextThinking);
    if (profileId !== AUTOMATIC_PROFILE_ID && !nextProfiles.some((profile) => profile.profile_id === profileId)) {
      setProfileId(AUTOMATIC_PROFILE_ID);
    }
  };

  const chooseThinking = (nextThinking: string): void => {
    const nextProfiles = profileOptions(profiles, model, nextThinking);
    setThinking(nextThinking);
    if (profileId !== AUTOMATIC_PROFILE_ID && !nextProfiles.some((profile) => profile.profile_id === profileId)) {
      setProfileId(AUTOMATIC_PROFILE_ID);
    }
  };

  const applySelection = async (): Promise<void> => {
    if (!model || !thinking || compatibleProfiles.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      await requestJson(`/admin/api/sessions/${encodeURIComponent(String(session.key || ""))}/selection`, {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ profile_id: profileId, model, thinking }),
      });
      publishAdminStatus(await requestJson("/admin/api/overview"));
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="auth-profile-panel">
      <div className="session-selection-fields">
        <label className="session-selection-field">
          <span>模型</span>
          <select value={model} disabled={busy || models.length === 0} onChange={(event) => chooseModel(event.target.value)}>
            {models.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>
        <label className="session-selection-field">
          <span>思考深度</span>
          <select value={thinking} disabled={busy || thinkings.length === 0} onChange={(event) => chooseThinking(event.target.value)}>
            {thinkings.map((option) => (
              <option key={option} value={option}>
                {option}
              </option>
            ))}
          </select>
        </label>
        <label className="session-selection-field">
          <span>Profile</span>
          <select value={profileId} disabled={busy || compatibleProfiles.length === 0} onChange={(event) => setProfileId(event.target.value)}>
            <option value={AUTOMATIC_PROFILE_ID}>自动</option>
            {compatibleProfiles.map((profile) => (
              <option key={String(profile.profile_id)} value={String(profile.profile_id)}>
                {profileTitle(profile)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <button className="link-button session-selection-apply" type="button" disabled={busy || !model || !thinking || compatibleProfiles.length === 0} onClick={() => void applySelection()}>
        {busy ? "应用中…" : "应用"}
      </button>
      {blocked ? <div className="summary-detail">{String(session.selectionBlockReason || "没有可用的 Profile/模型组合")}</div> : null}
      {error ? <div className="summary-detail danger">{error}</div> : null}
    </div>
  );
}

export function TimelinePayloadView({ payload, hasMore = false, olderBusy = false, onLoadOlder }: { readonly payload: TimelinePayload; readonly hasMore?: boolean; readonly olderBusy?: boolean; readonly onLoadOlder?: (() => Promise<void>) | undefined }): React.JSX.Element {
  const events = filterVisibleTimelineEvents(Array.isArray(payload) ? payload : payload.events || []);
  if (!events.length) return <div className="summary-detail">暂无时间线事件</div>;
  return <Timeline events={events} hasMore={hasMore} olderBusy={olderBusy} onLoadOlder={onLoadOlder} />;
}

export function mergeTimelinePayloads(current: TimelinePayload | null, older: TimelinePayload): TimelinePayload {
  if (Array.isArray(current) || Array.isArray(older)) {
    return mergeTimelineEvents(Array.isArray(older) ? older : older.events || [], Array.isArray(current) ? current : current?.events || []);
  }
  const olderEvents = older.events || [];
  const currentEvents = current?.events || [];
  return {
    ...current,
    ...older,
    session: current?.session || older.session,
    trace: older.trace || current?.trace,
    events: mergeTimelineEvents(olderEvents, currentEvents),
  };
}

export function mergeTimelineEvents(left: readonly TimelineEvent[], right: readonly TimelineEvent[]): TimelineEvent[] {
  const seen = new Set<string>();
  const merged: TimelineEvent[] = [];
  for (const event of [...left, ...right]) {
    const key = timelineEventIdentity(event);
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    merged.push(event);
  }
  return merged.sort((first, second) => timestampMs(first.at) - timestampMs(second.at) || Number(first.sequence || 0) - Number(second.sequence || 0) || String(first.id || "").localeCompare(String(second.id || "")));
}

export function Timeline({ events, hasMore = false, olderBusy = false, onLoadOlder }: { readonly events: readonly TimelineEvent[]; readonly hasMore?: boolean; readonly olderBusy?: boolean; readonly onLoadOlder?: (() => Promise<void>) | undefined }): React.JSX.Element {
  const containerRef = useRef<HTMLDivElement>(null);
  const shouldFollowRef = useRef(true);
  const olderLoadInFlightRef = useRef(false);
  const firstEventKey = events.length ? timelineEventIdentity(events[0]) : "";
  const pendingPrependAnchorRef = useRef<{
    readonly scrollHeight: number;
    readonly scrollTop: number;
    readonly firstEventKey: string;
  } | null>(null);

  const loadOlderWithAnchor = useCallback(() => {
    if (!hasMore || olderBusy || olderLoadInFlightRef.current || !onLoadOlder) {
      return;
    }
    const container = containerRef.current;
    if (container) {
      pendingPrependAnchorRef.current = {
        scrollHeight: container.scrollHeight,
        scrollTop: container.scrollTop,
        firstEventKey,
      };
    }
    shouldFollowRef.current = false;
    olderLoadInFlightRef.current = true;
    void onLoadOlder().finally(() => {
      olderLoadInFlightRef.current = false;
    });
  }, [firstEventKey, hasMore, olderBusy, onLoadOlder]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const anchor = pendingPrependAnchorRef.current;
    if (anchor && firstEventKey && firstEventKey !== anchor.firstEventKey) {
      const insertedHeight = container.scrollHeight - anchor.scrollHeight;
      container.scrollTop = anchor.scrollTop + insertedHeight;
      pendingPrependAnchorRef.current = null;
      updateFollowState();
      return;
    }
    if (!shouldFollowRef.current) {
      updateFollowState();
      return;
    }
    container.scrollTop = container.scrollHeight;
    updateFollowState();
  }, [events.length, firstEventKey]);

  useEffect(() => {
    const anchor = pendingPrependAnchorRef.current;
    if (!olderBusy && anchor && firstEventKey === anchor.firstEventKey) {
      pendingPrependAnchorRef.current = null;
    }
  }, [firstEventKey, olderBusy]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container || !hasMore || olderBusy || !onLoadOlder) {
      return;
    }
    if (container.scrollHeight <= container.clientHeight + TIMELINE_AUTO_LOAD_THRESHOLD) {
      loadOlderWithAnchor();
    }
  }, [events.length, hasMore, loadOlderWithAnchor, olderBusy, onLoadOlder]);

  function updateFollowState(): void {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    if (container.scrollTop <= TIMELINE_AUTO_LOAD_THRESHOLD && hasMore && !olderBusy && onLoadOlder) {
      loadOlderWithAnchor();
    }
    shouldFollowRef.current = container.scrollHeight - container.scrollTop - container.clientHeight < 24;
  }

  return (
    <div className="timeline" ref={containerRef} onScroll={updateFollowState} onMouseEnter={updateFollowState}>
      {hasMore ? (
        <button type="button" className="timeline-load-older" disabled={olderBusy} onClick={loadOlderWithAnchor}>
          {olderBusy ? "正在加载" : "加载更早活动"}
        </button>
      ) : null}
      <div className="agent-transcript">
        {events.map((event, index) => (
          <TimelineRow key={timelineEventKey(event, index)} event={event} />
        ))}
      </div>
    </div>
  );
}

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
