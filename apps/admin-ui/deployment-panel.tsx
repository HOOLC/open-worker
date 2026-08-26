import { confirmInterruptRisk, publishStatusFromPayload, requestJson } from "./admin-api.js";

import { Badge } from "./admin-badge.js";

import { errorMessage, fmtDateTime, shortRevision } from "./admin-formatters.js";

import { AdminStatus } from "./admin-types.js";

import React, { useEffect, useMemo, useState } from "react";

export function DeployPanel({ status, message, setMessage }: { readonly status: AdminStatus; readonly message: string | null; readonly setMessage: (message: string | null) => void }): React.JSX.Element {
  const [busy, setBusy] = useState<"deploy" | null>(null);
  const deployTargetOptions = useMemo(() => buildDeployTargetOptions(status.deployment), [status.deployment]);
  const deployTargetValues = deployTargetOptions.map((option) => option.value).join("\n");
  const [selectedDeployVersion, setSelectedDeployVersion] = useState("");
  useEffect(() => {
    setSelectedDeployVersion((previous) => (previous && deployTargetOptions.some((option) => option.value === previous) ? previous : deployTargetOptions[0]?.value || ""));
  }, [deployTargetValues]);

  async function runDeploy(): Promise<void> {
    if (!selectedDeployVersion) {
      setMessage("没有可发布的 package 版本");
      return;
    }
    setBusy("deploy");
    setMessage("正在部署版本...");
    try {
      const allowActive = await confirmInterruptRisk("deploy", "发布");
      if (allowActive == null) {
        setMessage(null);
        return;
      }
      const payload = await requestJson("/admin/api/deploy", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          version: selectedDeployVersion,
          allow_active: allowActive,
        }),
      });
      publishStatusFromPayload(payload);
      setMessage(`已部署 ${selectedDeployVersion} · 操作 ${payload.operation?.id || ""}`);
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(null);
    }
  }

  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">发布</div>
      </div>
      <div className="panel-body">
        <div className="deploy-actions">
          <label className="deploy-target-field">
            <span className="summary-label">Package 版本</span>
            <select id="deploy-package-version-select" aria-label="Package 版本" value={selectedDeployVersion} disabled={deployTargetOptions.length === 0 || busy !== null} onChange={(event) => setSelectedDeployVersion(event.target.value)}>
              {deployTargetOptions.length ? (
                deployTargetOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))
              ) : (
                <option value="">没有可发布的 package 版本</option>
              )}
            </select>
          </label>
          <button
            className="primary"
            type="button"
            disabled={busy !== null || !selectedDeployVersion}
            onClick={() => {
              void runDeploy();
            }}
          >
            部署版本
          </button>
        </div>
        <RiskPanel state={status.state || {}} />
        <DeploymentPanel deployment={status.deployment} />
        {message ? (
          <div className={"summary-detail " + (message.includes("失败") || message.includes("必须") ? "danger" : "")} style={{ marginTop: 6 }}>
            {message}
          </div>
        ) : null}
      </div>
    </section>
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

export function ReleaseRow({ label, release }: { readonly label: string; readonly release: any }): React.JSX.Element {
  if (!release?.targetPath) {
    return <div className="summary-detail">{label}：无</div>;
  }
  const metadata = release.metadata || {};
  const heading = metadata.packageVersion || metadata.shortRevision || metadata.revision || String(release.targetPath).split("/").pop() || "release";
  const detailTime = metadata.installedAt || metadata.builtAt;
  return (
    <div className="release-row">
      <div className="profile-line">
        <span className="profile-account">
          {label}：{heading}
        </span>
        <span className="profile-plan">{metadata.packageName || metadata.branch || "package"}</span>
      </div>
      <div className="summary-detail">{detailTime ? fmtDateTime(detailTime) : release.targetPath}</div>
    </div>
  );
}

export type DeployTargetOption = {
  readonly value: string;
  readonly label: string;
};

export function buildDeployTargetOptions(deployment: any): readonly DeployTargetOption[] {
  const versions: readonly Record<string, any>[] = Array.isArray(deployment?.recentPackageVersions) ? deployment.recentPackageVersions : [];
  return versions
    .map((entry: Record<string, any>) => {
      const version = String(entry.version || "").trim();
      if (!version) return null;
      const spec = String(entry.packageSpec || "").trim();
      return {
        value: version,
        label: spec || version,
      };
    })
    .filter((option): option is DeployTargetOption => Boolean(option));
}
