import { githubDevicePollApiPath, githubDeviceStartApiPath, githubIdentityApiPath, requestJson } from "./session-api.js";

import { MetaLine } from "./session-metadata.js";

import { SessionRecord } from "./session-types.js";

import React, { useEffect, useRef, useState } from "react";

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
          Math.max(1, Number(result.retryAfterSeconds || intervalSeconds || 5)) * 1000,
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
