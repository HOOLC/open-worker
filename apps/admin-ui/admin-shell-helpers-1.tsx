import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { formatAuthQuotaDisplay, formatWeightedWeeklyQuotaScore, remainingPercent, weightedWeeklyQuotaScore, daysUntilReset } from "./auth-profile-quota";

import { profileAccountLabel, profileBillingLabel, profilePlanLabel, profileRuntimeLabel, profileTitle } from "./auth-profile-display";

import { connectAdminRealtime, getAdminStatusSnapshot, mergeAdminStatusSnapshot, publishAdminStatus, subscribeAdminStatus } from "./admin-status-store";

import { AdminSessionsView } from "./session-view";

import { statusLabel } from "./timeline-display";
import { SlackSettingsPanel } from "./slack-settings";

import { LogsPanel, ServicePanel, AddProfileDialog, GitHubAccountBindDialog, TopbarProfiles, RiskPanel, RiskCell, DeploymentPanel, ReleaseTargetPanel } from "./admin-shell-helpers-2.js";
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

export type AdminStatus = Record<string, any>;

export type AdminView = "sessions" | "ops";

export type Tone = "good" | "warn" | "danger" | "info" | "purple" | "";

export function OperationsView({ status, slackSetup, onSlackChange }: { readonly status: AdminStatus; readonly slackSetup: import("./slack-settings").SlackSetup | null; readonly onSlackChange: (setup: import("./slack-settings").SlackSetup) => void }): React.JSX.Element {
  const [addProfileOpen, setAddProfileOpen] = useState(false);
  const [githubBindAccount, setGitHubBindAccount] = useState<Record<string, any> | null>(null);
  const [deployStatus, setDeployStatus] = useState<string | null>(null);
  const [profileStatus, setProfileStatus] = useState<string | null>(null);
  const [githubStatus, setGitHubStatus] = useState<string | null>(null);

  return (
    <div className="ops-page">
      <div className="view-grid ops-grid">
        <SlackSettingsPanel setup={slackSetup} onChange={onSlackChange} />
      </div>
      <div className="view-grid ops-grid">
        <DeployPanel status={status} message={deployStatus} setMessage={setDeployStatus} />
        <OperationRecords status={status} />
      </div>

      <div className="view-grid ops-grid">
        <ProfilesPanel status={status} message={profileStatus} setMessage={setProfileStatus} onAdd={() => setAddProfileOpen(true)} />
        <GitHubAccountsPanel status={status} message={githubStatus} setMessage={setGitHubStatus} onBind={setGitHubBindAccount} />
      </div>

      <div className="view-grid ops-grid">
        <LogsPanel logs={status.state?.recentBrokerLogs || []} />
        <ServicePanel service={status.service || {}} />
      </div>

      {addProfileOpen ? <AddProfileDialog onClose={() => setAddProfileOpen(false)} onStatus={setProfileStatus} /> : null}
      {githubBindAccount ? <GitHubAccountBindDialog account={githubBindAccount} onClose={() => setGitHubBindAccount(null)} onStatus={setGitHubStatus} /> : null}
    </div>
  );
}

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

export function OperationRecords({ status }: { readonly status: AdminStatus }): React.JSX.Element {
  const operations = Array.isArray(status.operations) ? status.operations : [];
  const events = Array.isArray(status.auditEvents) ? status.auditEvents : [];
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">操作记录</div>
        <span className="badge purple">审计</span>
      </div>
      <div className="panel-body">
        <div className="operation-list">
          {operations.length ? (
            operations.slice(0, 5).map((operation: Record<string, any>) => (
              <div className="operation-row" key={operation.id || `${operation.kind}-${operation.updatedAt}`}>
                <Badge label={operation.status || "unknown"} tone={statusTone(operation.status)} />
                <div className="operation-main">
                  <div className="operation-title">{operationLabel(operation.kind)}</div>
                  <div className="operation-detail">{pickOperationLabel(operation)}</div>
                </div>
                <div className="summary-detail">{fmtTime(operation.updatedAt)}</div>
              </div>
            ))
          ) : (
            <div className="empty-state">暂无管理操作</div>
          )}
        </div>
        <div className="audit-list">
          {events.slice(0, 6).map((event: Record<string, any>) => (
            <div key={event.id || `${event.action}-${event.createdAt}`}>
              {fmtTime(event.createdAt)} · {operationLabel(event.action)} · {statusLabel(event.status)}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

export function ProfilesPanel({ status, message, setMessage, onAdd }: { readonly status: AdminStatus; readonly message: string | null; readonly setMessage: (message: string | null) => void; readonly onAdd: () => void }): React.JSX.Element {
  const profiles = [...(status.profiles?.items || [])].sort((left, right) => String(left.profile_id || "").localeCompare(String(right.profile_id || "")));

  async function deleteProfile(profileId: string): Promise<void> {
    if (!window.confirm(`删除 Profile ${profileId}？`)) return;
    setMessage("正在删除 Profile...");
    try {
      await requestJson(`/admin/api/profiles/${encodeURIComponent(profileId)}`, {
        method: "DELETE",
      });
      const overview = await loadAdminOverview();
      publishAdminStatus(mergeStatusOverview(getAdminStatusSnapshot().status, overview));
      setMessage("Profile 已删除");
    } catch (error) {
      setMessage(errorMessage(error));
    }
  }

  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">Profiles</div>
        <button type="button" onClick={onAdd}>
          添加
        </button>
      </div>
      <div className="panel-body maintenance-grid">
        {profiles.length ? (
          profiles.map((profile: Record<string, any>) => {
            const quota = profileQuotaSummary(profile);
            const plan = profilePlanLabel(profile);
            const issue = profile.account?.error || profile.rateLimits?.error || "";
            const cardTone = profile.account?.ok === false || quota.ok === false ? "danger" : quota.tone;
            return (
              <div className={"profile-card " + cardTone} key={String(profile.profile_id)} title={profileTitle(profile)}>
                <div className="profile-card-head">
                  <div className="profile-identity">
                    <div className="profile-account-row">
                      <span className="profile-account">{profileAccountLabel(profile)}</span>
                      <span className="profile-plan-badge">{profileBillingLabel(profile)}</span>
                      {plan ? <span className="profile-plan-badge">{plan}</span> : null}
                      <Badge label={profile.auth_configured ? "认证已配置" : "缺少认证"} tone={profile.auth_configured ? "good" : "danger"} />
                    </div>
                    <div className="profile-card-subtitle">{profileRuntimeLabel(profile)}</div>
                    <div className="profile-card-subtitle">{(profile.models || []).map((model: Record<string, any>) => `${model.id} · thinking: ${(model.thinking || []).join("/")} · input: ${(model.capabilities?.input || []).join("/")}`).join("；")}</div>
                    {issue ? <div className="profile-card-subtitle">{issue}</div> : null}
                  </div>
                  <button
                    className="profile-delete-button danger"
                    type="button"
                    onClick={() => {
                      void deleteProfile(String(profile.profile_id || ""));
                    }}
                  >
                    删除
                  </button>
                </div>
                <ProfileQuotaMetrics quota={quota} />
              </div>
            );
          })
        ) : (
          <div className="empty-state">暂无 Profile</div>
        )}
      </div>
      {message ? (
        <div className="summary-detail" style={{ padding: "0 8px 8px" }}>
          {message}
        </div>
      ) : null}
    </section>
  );
}

export function GitHubAccountsPanel({ status, message, setMessage, onBind }: { readonly status: AdminStatus; readonly message: string | null; readonly setMessage: (message: string | null) => void; readonly onBind: (account: Record<string, any>) => void }): React.JSX.Element {
  const accounts = normalizeGitHubAccounts(status);
  const boundAccounts = accounts.filter((account) => account.prBinding?.state === "bound");
  const currentDefaultAccount = accounts.find((account) => account.isDefaultPrAccount);
  const defaultPrAccount = status.githubAccounts?.defaultPrAccount;
  const selectableDefaultAccounts = boundAccounts;
  const defaultSelectValue = currentDefaultAccount?.slackUserId || (defaultPrAccount?.available && defaultPrAccount.source === "env" ? "__env_default__" : "");
  const selectableDefaultAccountKeys = selectableDefaultAccounts.map((account) => account.slackUserId).join("\n");
  const [defaultSelection, setDefaultSelection] = useState("");
  useEffect(() => {
    const nextSelection = defaultSelectValue || selectableDefaultAccounts[0]?.slackUserId || "";
    setDefaultSelection((previous) => (previous && (previous === defaultSelectValue || selectableDefaultAccounts.some((account) => account.slackUserId === previous)) ? previous : nextSelection));
  }, [defaultSelectValue, selectableDefaultAccountKeys]);

  async function setDefault(slackUserId: string): Promise<void> {
    if (!slackUserId) {
      setMessage("先选择一个已绑定的 GitHub 账号");
      return;
    }
    setMessage("正在设置默认 PR 账号...");
    try {
      const payload = await requestJson("/admin/api/github-accounts/default-pr", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ slack_user_id: slackUserId }),
      });
      publishStatusFromPayload(payload);
      setMessage("默认 PR 账号已更新");
    } catch (error) {
      setMessage(errorMessage(error));
    }
  }

  const currentDefaultLabel = currentDefaultAccount ? githubAccountOptionLabel(currentDefaultAccount) : defaultPrAccount?.available && defaultPrAccount.source === "env" ? `环境默认账号 ${defaultPrAccount.githubLogin || ""}`.trim() : "未设置";
  const canSwitchDefault = Boolean(defaultSelection) && defaultSelection !== defaultSelectValue && selectableDefaultAccounts.some((account) => account.slackUserId === defaultSelection);

  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">GitHub 账号</div>
      </div>
      <div className="github-default-control">
        <label className="github-default-field">
          <span className="summary-label">默认 PR 账号</span>
          <select aria-label="选择候选 GitHub PR 账号" value={defaultSelection} disabled={selectableDefaultAccounts.length === 0} onChange={(event) => setDefaultSelection(event.target.value)}>
            {defaultSelectValue && !currentDefaultAccount ? <option value={defaultSelectValue}>{currentDefaultLabel}</option> : null}
            {selectableDefaultAccounts.length ? (
              selectableDefaultAccounts.map((account) => (
                <option key={account.slackUserId} value={account.slackUserId}>
                  {githubAccountOptionLabel(account)}
                </option>
              ))
            ) : (
              <option value="">未设置</option>
            )}
          </select>
        </label>
        <div className="github-default-actions">
          <button
            className="secondary"
            type="button"
            disabled={!canSwitchDefault}
            onClick={() => {
              void setDefault(defaultSelection);
            }}
          >
            切换
          </button>
        </div>
        {boundAccounts.length === 0 ? <div className="summary-detail github-default-hint">先绑定任意 Slack 用户的 GitHub OAuth 后，才能设置默认账号。</div> : null}
      </div>
      <div className="panel-body maintenance-grid">
        {accounts.length ? (
          accounts.map((account) => {
            const identity = account.slackIdentity || {};
            const binding = account.prBinding || {};
            const label = identity.realName || identity.displayName || identity.username || account.slackUserId;
            const detail = [account.slackUserId, identity.email].filter(Boolean).join(" · ");
            const githubEmail = binding.githubEmail || "";
            const githubSummary = binding.githubLogin ? `GitHub：${binding.githubLogin}${githubEmail ? ` · ${githubEmail}` : ""}` : "";
            return (
              <div className="profile-row" key={account.slackUserId}>
                <div className="profile-line">
                  <span className="profile-account">{label}</span>
                  <span className="profile-plan">{detail || account.slackUserId}</span>
                  <Badge label={githubBindingLabel(binding)} tone={githubBindingTone(binding)} />
                  {account.isDefaultPrAccount ? <Badge label="默认 PR" tone="purple" /> : null}
                </div>
                {githubSummary ? <div className="summary-detail">{githubSummary}</div> : null}
                <div className="profile-actions">
                  <button className="secondary" type="button" onClick={() => onBind(account)}>
                    {binding.state === "bound" ? "重新绑定 GitHub" : "绑定 GitHub"}
                  </button>
                  {binding.state === "bound" && !account.isDefaultPrAccount ? (
                    <button
                      className="secondary"
                      type="button"
                      onClick={() => {
                        void setDefault(account.slackUserId);
                      }}
                    >
                      设为默认 PR
                    </button>
                  ) : null}
                </div>
              </div>
            );
          })
        ) : (
          <div className="empty-state">暂无 GitHub 账号</div>
        )}
      </div>
      {message ? (
        <div className="summary-detail" style={{ padding: "0 8px 8px" }}>
          {message}
        </div>
      ) : null}
    </section>
  );
}
