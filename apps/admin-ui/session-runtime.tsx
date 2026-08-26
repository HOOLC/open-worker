import { getTimelineSnapshot, publishTimelinePayload, subscribeTimeline } from "./admin-status-store";

import { requestJson, sessionTimelineApiPath } from "./session-api.js";

import { fmtDateTime, fmtRelativeTime, shortValue } from "./session-formatters.js";

import { MetaLine, type SessionMetaItem } from "./session-metadata.js";

import { sessionDeliveryIssueIndicator, SessionOperationalState, shouldShowSessionState } from "./session-row-display";

import { TraceSummary } from "./session-trace-summary.js";

import { SessionRecord, TIMELINE_PAGE_SIZE, TimelinePayload } from "./session-types.js";

import React, { useState, useSyncExternalStore } from "react";

export function SessionResetButton({ session }: { readonly session: SessionRecord }): React.JSX.Element {
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const sessionKey = String(session.key || "");

  async function resetSession(): Promise<void> {
    if (!sessionKey) {
      return;
    }
    const confirmed = window.confirm(["确认重置这个 Session？", "会删除旧 Agent session，并把当前 Slack thread 上下文作为一条新消息写入新 session 的 mailbox。"].join("\n"));
    if (!confirmed) {
      return;
    }

    setBusy(true);
    setMessage(null);
    try {
      await requestJson("/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/reset", {
        method: "POST",
      });
      const timelinePayload = await requestJson(sessionTimelineApiPath(sessionKey, { limit: TIMELINE_PAGE_SIZE }));
      publishTimelinePayload(sessionKey, timelinePayload as TimelinePayload);
      setMessage("已重置，正在重新唤起 bot");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="session-reset-action">
      <button
        type="button"
        className="danger"
        disabled={busy || !sessionKey}
        onClick={() => {
          void resetSession();
        }}
      >
        {busy ? "正在重置" : "重置 Session"}
      </button>
      {message ? <div className="summary-detail">{message}</div> : null}
    </div>
  );
}

export function SessionRuntimePanel({ session, state, blockedInbound, totalJobs, runningJobs }: { readonly session: SessionRecord; readonly state: SessionOperationalState; readonly blockedInbound: number; readonly totalJobs: number; readonly runningJobs: number }): React.JSX.Element {
  const deliveryIssue = sessionDeliveryIssueIndicator({
    ...session,
    blockedInboundCount: blockedInbound,
  });
  const rows: Array<SessionMetaItem | null> = [
    shouldShowSessionState(state)
      ? {
          label: "Gateway 状态",
          value: state.label,
          detail: state.detail,
          tone: state.tone,
        }
      : null,
    deliveryIssue
      ? {
          label: "Mailbox 投递失败",
          value: deliveryIssue.value,
          detail: deliveryIssue.detail,
          title: deliveryIssue.title,
          tone: deliveryIssue.tone,
        }
      : null,
    runningJobs > 0
      ? {
          label: "运行任务",
          value: String(runningJobs),
          detail: totalJobs > runningJobs ? "历史共 " + totalJobs : undefined,
          tone: "good",
        }
      : null,
  ];
  const visibleRows = rows.filter((row): row is SessionMetaItem => row !== null);
  if (!visibleRows.length) return <></>;
  return (
    <div className="mini-panel">
      <div className="mini-title">Gateway 记录</div>
      <div className="mini-body">
        <div className="meta-list">
          {visibleRows.map((row) => (
            <MetaLine key={row.label} label={row.label} value={row.value} detail={row.detail} title={row.title} tone={row.tone} />
          ))}
        </div>
      </div>
    </div>
  );
}

export function SessionDebugPanel({ session, channelLabel, channelTitle, activityAt }: { readonly session: SessionRecord; readonly channelLabel: string; readonly channelTitle: string; readonly activityAt: unknown }): React.JSX.Element {
  return (
    <details className="side-disclosure">
      <summary>展开调试信息</summary>
      <div className="meta-list">
        <MetaLine label="频道" value={channelLabel} title={channelTitle} />
        <MetaLine label="最近活动" value={fmtRelativeTime(activityAt)} detail={fmtDateTime(activityAt)} />
        <MetaLine label="Root TS" value={String(session.rootThreadTs || "--")} />
        <MetaLine label="Canonical ID" value={shortValue(session.id || "--", 28)} title={String(session.id || "")} />
        <MetaLine label="Session" value={shortValue(session.key || "--", 28)} title={String(session.key || "")} />
        {session.profileId ? <MetaLine label="Profile" value={shortValue(session.profileId, 28)} title={String(session.profileId)} /> : null}
        {session.model ? <MetaLine label="Model" value={shortValue(session.model, 28)} title={String(session.model)} /> : null}
        {session.thinking ? <MetaLine label="Thinking" value={String(session.thinking)} /> : null}
      </div>
    </details>
  );
}

export function SessionTraceStats({ sessionKey }: { readonly sessionKey: string }): React.JSX.Element {
  const timelineSnapshot = useSyncExternalStore(
    (listener) => subscribeTimeline(sessionKey, listener),
    () => getTimelineSnapshot(sessionKey),
    () => getTimelineSnapshot(sessionKey),
  );
  const payload = timelineSnapshot.payload as TimelinePayload | null;
  const trace = payload && !Array.isArray(payload) ? payload.trace : null;
  if (!trace) return <div className="summary-detail">活动构成加载中</div>;
  return <TraceSummary trace={trace} />;
}
