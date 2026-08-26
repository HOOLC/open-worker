import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { profileDisplayLabel, profileIsSelectable, profileOptionLabel, profileQuotaLabel, profileSessionActionLabel, profileTitle } from "./auth-profile-display";

import { applyAdminRealtimeEvent, getAdminStatusSnapshot, getTimelineSnapshot, publishTimelinePayload, subscribeAdminStatus, subscribeTimeline } from "./admin-status-store";

import { agentTranscriptAvatar, agentTranscriptKind, agentTranscriptSpeaker } from "./agent-transcript-display";

import { requestCancelSessionJob } from "./session-job-actions";

import { stableSessionOrder } from "./session-order";

import { activeBackgroundJobCount, activeBackgroundJobs, buildChannelLabelById, renderSessionMeta, resolveSessionChannelLabel, sessionActivityAt, sessionActivityMs, sessionDeliveryIssueIndicator, sessionOperationalState, sessionSelectionBlocked, shouldShowSessionState } from "./session-row-display";

import type { SessionOperationalState } from "./session-row-display";

import { filterVisibleTimelineEvents, getTimelineEventDisplay, statusLabel, type TimelineEvent } from "./timeline-display";

import { UiState, SessionRecord, TimelinePayload, timelinePayloadSession, mergeSessionRecords, sessionFilters, TIMELINE_PAGE_SIZE, TIMELINE_AUTO_LOAD_THRESHOLD, GitHubBindPage, SessionPermalinkView, SessionRow, SessionDetail, AgentSessionHero, SessionActions } from "./session-view-helpers-1.js";
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
  fmtTokens,
  fmtPercent,
  shortValue,
} from "./session-view-helpers-5.js";

export function GitHubIdentityPanel({ session }: { readonly session: SessionRecord }): React.JSX.Element {
  const sessionKey = String(session.key || "");
  return <GitHubBindingFlow sessionKey={sessionKey} variant="panel" />;
}

export function GitHubBindingFlow({ sessionKey, variant = "panel", autoStart = false }: { readonly sessionKey: string; readonly variant?: "panel" | "page"; readonly autoStart?: boolean }): React.JSX.Element {
  const [identity, setIdentity] = useState<Record<string, any> | null>(null);
  const [device, setDevice] = useState<Record<string, any> | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const autoStartRef = useRef(false);
  const binding = identity?.binding || {};
  const defaultAccount = identity?.defaultAccount || {};
  const needsBinding = binding.state === "unbound" || binding.state === "revoked";

  async function refreshIdentity(): Promise<Record<string, any> | null> {
    if (!sessionKey) {
      return null;
    }
    const payload = (await requestJson(githubIdentityApiPath(sessionKey))) as Record<string, any>;
    const nextIdentity = payload.identity as Record<string, any>;
    setIdentity(nextIdentity);
    return nextIdentity;
  }

  async function startDeviceAuthorization(): Promise<void> {
    if (!sessionKey || busy) {
      return;
    }
    setBusy(true);
    setMessage(null);
    try {
      const payload = (await requestJson(githubDeviceStartApiPath(sessionKey), {
        method: "POST",
      })) as Record<string, any>;
      setDevice(payload.device as Record<string, any>);
      setMessage("打开 GitHub 设备码页面，输入下面的代码完成绑定。");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : String(error));
    } finally {
      setBusy(false);
    }
  }

  useEffect(() => {
    let cancelled = false;
    setIdentity(null);
    setDevice(null);
    setMessage(null);
    autoStartRef.current = false;
    void refreshIdentity().catch((error: unknown) => {
      if (!cancelled) setMessage(error instanceof Error ? error.message : String(error));
    });
    return () => {
      cancelled = true;
    };
  }, [sessionKey]);

  useEffect(() => {
    if (!autoStart || autoStartRef.current || !identity || !needsBinding) {
      return;
    }
    autoStartRef.current = true;
    void startDeviceAuthorization();
  }, [autoStart, identity, needsBinding]);

  useEffect(() => {
    if (!device?.id) {
      return;
    }
    let cancelled = false;
    let timeout: number | undefined;
    async function poll(): Promise<void> {
      try {
        const payload = (await requestJson(githubDevicePollApiPath(String(device.id)))) as Record<string, any>;
        const result = payload.result as Record<string, any>;
        if (cancelled) return;
        if (result.status === "completed") {
          setDevice(null);
          setMessage("GitHub 账号已绑定。");
          await refreshIdentity();
          return;
        }
        if (result.status === "expired") {
          setMessage("设备码已过期，请重新发起绑定。");
          setDevice(null);
          return;
        }
        if (result.status === "failed") {
          setMessage(String(result.error || "绑定失败"));
          setDevice(null);
          return;
        }
        timeout = window.setTimeout(
          () => {
            void poll();
          },
          Math.max(1, Number(result.retryAfterSeconds || device.intervalSeconds || 5)) * 1000,
        );
      } catch (error) {
        if (!cancelled) setMessage(error instanceof Error ? error.message : String(error));
      }
    }
    timeout = window.setTimeout(() => {
      void poll();
    }, 800);
    return () => {
      cancelled = true;
      if (timeout !== undefined) window.clearTimeout(timeout);
    };
  }, [device?.id]);

  const actionLabel = device ? "重新生成设备码" : variant === "page" ? "开始绑定 GitHub" : "绑定发起人的 GitHub";
  const busyLabel = variant === "page" ? "正在发起绑定" : "正在发起绑定";
  const visibleMessage = message && !device ? message : null;

  return (
    <div className={"github-identity-panel github-binding-flow " + variant}>
      {variant === "page" ? <GitHubBindingIntro identity={identity} /> : null}
      {identity ? (
        <div className="meta-list">
          {binding.state === "bound" ? (
            <MetaLine label="PR 账号" value={String(binding.githubLogin || "--")} detail={binding.githubEmail || binding.githubName ? [binding.githubEmail, binding.githubName].filter(Boolean).join(" · ") : undefined} tone="good" />
          ) : binding.state === "revoked" ? (
            <MetaLine label="PR 账号" value="绑定失效" detail={String(binding.githubLogin || "")} tone="danger" />
          ) : binding.state === "unbound" && defaultAccount.available ? (
            <MetaLine label="PR 默认" value={String(defaultAccount.githubLogin || "--")} detail="发起人未绑定" tone="warn" />
          ) : binding.state === "unbound" ? (
            <MetaLine label="PR 账号" value="未绑定" detail="没有默认账号" tone="danger" />
          ) : (
            <MetaLine label="PR 账号" value="未记录发起人" tone="danger" />
          )}
        </div>
      ) : (
        <div className="summary-detail">GitHub 绑定状态加载中</div>
      )}
      {needsBinding ? (
        <button
          type="button"
          className="link-button github-bind-button"
          disabled={busy || !sessionKey}
          onClick={() => {
            void startDeviceAuthorization();
          }}
        >
          {busy ? busyLabel : actionLabel}
        </button>
      ) : null}
      {device ? (
        <div className="device-code-panel">
          <div className="device-code-label">GitHub 设备码</div>
          <div className="code-block">{String(device.userCode || "")}</div>
          <div className="summary-detail">在 GitHub 打开验证页，输入这组代码后本页会自动更新绑定状态。</div>
          <a className="link-button" href={String(device.verificationUriComplete || device.verificationUri || "https://github.com/login/device")} target="_blank" rel="noreferrer">
            打开 GitHub 验证页
          </a>
        </div>
      ) : null}
      {visibleMessage ? <div className="summary-detail">{visibleMessage}</div> : null}
    </div>
  );
}

export function GitHubBindingIntro({ identity }: { readonly identity: Record<string, any> | null }): React.JSX.Element {
  const binding = identity?.binding || {};
  const defaultAccount = identity?.defaultAccount || {};
  if (!identity) {
    return <div className="github-binding-status">正在读取当前绑定状态。</div>;
  }
  if (binding.state === "bound") {
    return <div className="github-binding-status good">已经绑定 GitHub，后续 PR 会使用这个账号。</div>;
  }
  if (binding.state === "revoked") {
    return <div className="github-binding-status danger">已有绑定不可用，需要重新完成 GitHub 绑定。</div>;
  }
  if (defaultAccount.available) {
    return <div className="github-binding-status warn">当前发起人未绑定。未绑定时会暂时使用默认账号 {String(defaultAccount.githubLogin || "--")} 创建 PR。</div>;
  }
  return <div className="github-binding-status danger">当前发起人未绑定，且没有可用默认 GitHub PR 账号。</div>;
}

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
  const rows = [
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
  ].filter((row): row is { label: string; value: string; detail?: string; title?: string; tone?: string } => Boolean(row));
  if (!rows.length) return <></>;
  return (
    <div className="mini-panel">
      <div className="mini-title">Gateway 记录</div>
      <div className="mini-body">
        <div className="meta-list">
          {rows.map((row) => (
            <MetaLine key={row.label} label={row.label} value={row.value} detail={row.detail} title={row.title} tone={row.tone} />
          ))}
        </div>
      </div>
    </div>
  );
}

export function MetaLine({ label, value, detail, title, tone }: { readonly title?: string; readonly label: string; readonly value: string; readonly detail?: string; readonly tone?: string }): React.JSX.Element {
  return (
    <div className={"meta-line " + classSafeValue(tone, "")}>
      <span>{label}</span>
      <strong title={title}>{value}</strong>
      {detail ? <em title={detail}>{detail}</em> : null}
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
