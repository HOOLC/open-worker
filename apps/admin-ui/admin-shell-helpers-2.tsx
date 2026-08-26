import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { formatAuthQuotaDisplay, formatWeightedWeeklyQuotaScore, remainingPercent, weightedWeeklyQuotaScore, daysUntilReset } from "./auth-profile-quota";

import { profileAccountLabel, profilePlanLabel, profileTitle } from "./auth-profile-display";

import { connectAdminRealtime, getAdminStatusSnapshot, mergeAdminStatusSnapshot, publishAdminStatus, subscribeAdminStatus } from "./admin-status-store";

import { AdminSessionsView } from "./session-view";

import { statusLabel } from "./timeline-display";

import { AdminStatus, AdminView, Tone, OperationsView, DeployPanel, OperationRecords, ProfilesPanel, GitHubAccountsPanel } from "./admin-shell-helpers-1.js";
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

export function LogsPanel({ logs }: { readonly logs: readonly Record<string, any>[] }): React.JSX.Element {
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">系统日志</div>
      </div>
      <div className="log-list">
        {logs.length ? (
          logs.slice(0, 10).map((entry, index) => (
            <div className={"log-entry " + statusTone(entry.level)} key={`${entry.ts || index}-${entry.message || entry.raw || ""}`}>
              <span>{fmtTime(entry.ts)}</span>
              <span>{entry.message || entry.raw || ""}</span>
            </div>
          ))
        ) : (
          <div className="empty-state">暂无日志</div>
        )}
      </div>
    </section>
  );
}

export function ServicePanel({ service }: { readonly service: Record<string, any> }): React.JSX.Element {
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">运行信息</div>
      </div>
      <div className="panel-body summary-detail" style={{ display: "grid", gap: 6 }}>
        <div>名称：{service.name || "--"}</div>
        <div>模式：{statusLabel(service.mode || "--")}</div>
        <div>端口：{service.port || "--"}</div>
        <div>启动：{fmtDateTime(service.startedAt)}</div>
        <div style={{ wordBreak: "break-all" }}>会话目录：{service.sessionsRoot || "--"}</div>
        <div style={{ wordBreak: "break-all" }}>DATA_ROOT: {service.dataRoot || "--"}</div>
      </div>
    </section>
  );
}

export function AddProfileDialog({ onClose, onStatus }: { readonly onClose: () => void; readonly onStatus: (message: string | null) => void }): React.JSX.Element {
  const [profileId, setProfileId] = useState("");
  const [text, setText] = useState(`{
  "provider": "openai",
  "billing": "usage",
  "auth": { "type": "api_key", "key": "" },
  "models": [
    {
      "id": "",
      "api": "openai-completions",
      "streaming": true,
      "thinking": ["off"],
      "default_thinking": "off",
      "capabilities": { "input": ["text"] },
      "default": true
    }
  ]
}`);
  const [file, setFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  async function saveProfile(): Promise<void> {
    setBusy(true);
    setMessage("正在保存 Profile...");
    try {
      const id = profileId.trim();
      if (!id) throw new Error("必须填写 Profile ID");
      const content = file ? await file.text() : text.trim();
      if (!content) throw new Error("必须提供 Profile JSON");
      const document = JSON.parse(content) as unknown;
      if (!document || typeof document !== "object" || Array.isArray(document)) {
        throw new Error("Profile JSON 必须是对象");
      }
      await requestJson(`/admin/api/profiles/${encodeURIComponent(id)}`, {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(document),
      });
      const overview = await loadAdminOverview();
      publishAdminStatus(mergeStatusOverview(getAdminStatusSnapshot().status, overview));
      onStatus("Profile 已保存");
      onClose();
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog open>
      <div className="modal-content add-profile-modal">
        <div className="modal-heading">
          <div className="panel-title">添加 Profile</div>
          <div className="summary-detail">Profile 定义 provider、认证、模型、思考深度和输入能力。</div>
        </div>
        <label className="summary-detail" style={{ display: "grid", gap: 4 }}>
          <span>Profile ID</span>
          <input aria-label="Profile ID" value={profileId} onChange={(event) => setProfileId(event.target.value)} />
        </label>
        <input type="file" accept="application/json,.json" onChange={(event) => setFile(event.currentTarget.files?.[0] || null)} />
        <textarea aria-label="Profile JSON" value={text} disabled={file !== null} onChange={(event) => setText(event.target.value)} />
        <div className="modal-actions">
          <button className="secondary" type="button" onClick={onClose}>
            取消
          </button>
          <button
            className="primary"
            type="button"
            disabled={busy}
            onClick={() => {
              void saveProfile();
            }}
          >
            保存 Profile
          </button>
        </div>
        {message ? <div className="summary-detail">{message}</div> : null}
      </div>
    </dialog>
  );
}

export function GitHubAccountBindDialog({ account, onClose, onStatus }: { readonly account: Record<string, any>; readonly onClose: () => void; readonly onStatus: (message: string | null) => void }): React.JSX.Element {
  const [device, setDevice] = useState<Record<string, any> | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const startedRef = useRef(false);
  const identity = account.slackIdentity || {};
  const label = identity.realName || identity.displayName || identity.username || account.slackUserId;

  useEffect(() => {
    if (startedRef.current) {
      return;
    }
    startedRef.current = true;
    void startGitHubAccountDeviceAuthorization();
  }, [account.slackUserId]);

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
          onStatus("GitHub 账号已绑定");
          publishAdminStatus(await loadAdminStatus());
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
        if (!cancelled) setMessage(errorMessage(error));
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

  async function startGitHubAccountDeviceAuthorization(): Promise<void> {
    setBusy(true);
    setMessage("正在申请 GitHub 设备码...");
    try {
      const payload = (await requestJson(githubAccountDeviceStartApiPath(String(account.slackUserId)), { method: "POST" })) as Record<string, any>;
      setDevice(payload.device as Record<string, any>);
      setMessage("打开 GitHub 验证页，输入下面的代码完成绑定。");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <dialog open>
      <div className="modal-content">
        <div className="modal-heading">
          <div className="panel-title">绑定 GitHub</div>
          <div className="summary-detail">
            {label} · {account.slackUserId}
          </div>
        </div>
        {device ? (
          <div className="device-code-panel">
            <div className="device-code-label">GitHub 设备码</div>
            <div className="code-block">{String(device.userCode || "")}</div>
            <a className="link-button" href={String(device.verificationUriComplete || device.verificationUri || "https://github.com/login/device")} target="_blank" rel="noreferrer">
              打开 GitHub 验证页
            </a>
          </div>
        ) : null}
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <button className="secondary" type="button" onClick={onClose}>
            取消
          </button>
          {!device && !busy ? (
            <button
              className="primary"
              type="button"
              onClick={() => {
                void startGitHubAccountDeviceAuthorization();
              }}
            >
              重新申请
            </button>
          ) : null}
        </div>
        {message ? <div className="summary-detail">{message}</div> : null}
      </div>
    </dialog>
  );
}

export function TopbarProfiles({ profiles }: { readonly profiles: readonly Record<string, any>[] }): React.JSX.Element {
  const quotaItems = useMemo(() => profileQuotaItems(profiles), [profiles]);
  return (
    <div className="topbar-center">
      {quotaItems.length ? (
        quotaItems.map((item) => (
          <span className={"quota-pill " + quotaTone(item.remaining)} title={item.title} key={item.title}>
            <strong>{item.label}</strong>
          </span>
        ))
      ) : (
        <span className="quota-meta">Profile 额度未知</span>
      )}
    </div>
  );
}

export function RiskPanel({ state }: { readonly state: Record<string, any> }): React.JSX.Element {
  const blocked = Number(state.blockedInboundCount || 0);
  const running = Number(state.runningBackgroundJobCount || 0);
  return (
    <>
      <div className="risk-strip">
        <RiskCell label="投递失败" value={blocked} danger={blocked > 0} />
        <RiskCell label="运行" value={running} />
      </div>
      <div className="risk-copy">{running === 0 ? "当前没有运行中的后台任务。" : "发布和回滚会中断正在运行的后台任务，执行前必须显式确认。"}</div>
    </>
  );
}

export function RiskCell({ label, value, danger = false }: { readonly label: string; readonly value: number; readonly danger?: boolean }): React.JSX.Element {
  return (
    <div className="risk-cell">
      <div className="risk-number" style={danger ? { color: "var(--red)" } : undefined}>
        {value}
      </div>
      <div className="risk-label">{label}</div>
    </div>
  );
}

export function DeploymentPanel({ deployment }: { readonly deployment: any }): React.JSX.Element {
  if (!deployment) {
    return <div className="summary-detail">发布状态不可用</div>;
  }
  if (deployment.ok === false) {
    return <div className="summary-detail danger">发布状态读取失败：{deployment.error || "unknown"}</div>;
  }
  const control = deployment.control || {};
  const runtime = deployment.runtime || {};
  const gateway = deployment.gateway || {};
  return (
    <>
      <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
        <Badge label={control.launchdLoaded ? "Control 已加载" : "Control 未运行"} tone={control.launchdLoaded ? "good" : "danger"} />
        <Badge label={control.healthOk ? "Control HTTP 正常" : "Control HTTP 异常"} tone={control.healthOk ? "good" : "danger"} />
        <Badge label={runtime.launchdLoaded ? "Runtime 已加载" : "Runtime 未运行"} tone={runtime.launchdLoaded ? "good" : "danger"} />
        <Badge label={runtime.healthOk ? "Runtime HTTP 正常" : "Runtime HTTP 异常"} tone={runtime.healthOk ? "good" : "danger"} />
        <Badge label={runtime.readyOk ? "Runtime 就绪" : "Runtime 异常"} tone={runtime.readyOk ? "good" : "danger"} />
        <Badge label={gateway.launchdLoaded ? "Gateway 已加载" : "Gateway 未运行"} tone={gateway.launchdLoaded ? "good" : "warn"} />
        <Badge label={gateway.healthOk ? "Gateway HTTP 正常" : "Gateway HTTP 异常"} tone={gateway.healthOk ? "good" : "warn"} />
      </div>
      <div className="release-current-grid">
        <ReleaseTargetPanel status={deployment} />
      </div>
    </>
  );
}

export function ReleaseTargetPanel({ status }: { readonly status: any }): React.JSX.Element {
  return (
    <div className="release-stack">
      <div className="profile-line">
        <span className="profile-account">zork</span>
        <span className="profile-plan">{status?.packageName || "package"}</span>
      </div>
      <ReleaseRow label="当前版本" release={status?.currentRelease} />
    </div>
  );
}
