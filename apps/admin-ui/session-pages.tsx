import { getAdminStatusSnapshot, getTimelineSnapshot, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { adminSessionPath, requestJson, sessionTimelineApiPath } from "./session-api.js";

import { SessionDetail } from "./session-detail.js";

import { GitHubBindingFlow } from "./session-github-binding.js";

import { mergeSessionRecords, SessionRecord, TIMELINE_PAGE_SIZE, TimelinePayload, timelinePayloadSession } from "./session-types.js";

import React, { useEffect, useState, useSyncExternalStore } from "react";

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
