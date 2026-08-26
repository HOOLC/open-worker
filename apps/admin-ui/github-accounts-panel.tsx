import { githubAccountDeviceStartApiPath, githubDevicePollApiPath, loadAdminStatus, publishStatusFromPayload, requestJson } from "./admin-api.js";

import { Badge } from "./admin-badge.js";

import { errorMessage, githubAccountOptionLabel, githubBindingLabel, githubBindingTone } from "./admin-formatters.js";

import { publishAdminStatus } from "./admin-status-store";

import { AdminStatus } from "./admin-types.js";

import { normalizeGitHubAccounts } from "./github-account-model.js";

import React, { useEffect, useRef, useState } from "react";

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
    const deviceId = device.id;
    const intervalSeconds = device.intervalSeconds;
    let cancelled = false;
    let timeout: number | undefined;
    async function poll(): Promise<void> {
      try {
        const payload = (await requestJson(githubDevicePollApiPath(String(deviceId)))) as Record<string, any>;
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
          Math.max(1, Number(result.retryAfterSeconds || intervalSeconds || 5)) * 1000,
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
