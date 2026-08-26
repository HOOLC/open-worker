import React, { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";

import { formatAuthQuotaDisplay, formatWeightedWeeklyQuotaScore, remainingPercent, weightedWeeklyQuotaScore, daysUntilReset } from "./auth-profile-quota";

import { profileAccountLabel, profilePlanLabel, profileTitle } from "./auth-profile-display";

import { connectAdminRealtime, getAdminStatusSnapshot, mergeAdminStatusSnapshot, publishAdminStatus, subscribeAdminStatus } from "./admin-status-store";

import { AdminSessionsView } from "./session-view";
import { useSlackSetup } from "./slack-settings";

import { statusLabel } from "./timeline-display";

import {
  AdminStatus,
  AdminView,
  Tone,
  OperationsView,
  DeployPanel,
  OperationRecords,
  ProfilesPanel,
  GitHubAccountsPanel,
  LogsPanel,
  ServicePanel,
  AddProfileDialog,
  GitHubAccountBindDialog,
  TopbarProfiles,
  RiskPanel,
  RiskCell,
  DeploymentPanel,
  ReleaseTargetPanel,
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
} from "./admin-shell-helpers.js";

export function AdminShell({ serviceName }: { readonly serviceName: string }): React.JSX.Element {
  const snapshot = useSyncExternalStore(subscribeAdminStatus, getAdminStatusSnapshot, getAdminStatusSnapshot);
  const status = (snapshot.status || {}) as AdminStatus;
  const [adminView, setAdminView] = useState<AdminView>(loadAdminView);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [slackSetup, setSlackSetup] = useSlackSetup();

  useEffect(() => {
    if (slackSetup && !slackSetup.configured) {
      setAdminView("ops");
    }
  }, [slackSetup]);

  useEffect(() => {
    let cancelled = false;
    let disconnectRealtime: (() => void) | undefined;
    async function load(): Promise<void> {
      try {
        const nextStatus = await loadAdminSessionsStatus();
        if (!cancelled) {
          publishAdminStatus(nextStatus);
          disconnectRealtime = connectAdminRealtime();
          setLoadError(null);
          void loadAdminOverview()
            .then((overview) => {
              if (!cancelled) publishAdminStatus(mergeStatusOverview(getAdminStatusSnapshot().status, overview));
            })
            .catch((error) => {
              if (!cancelled) setLoadError(errorMessage(error));
            });
          void loadAdminLogs()
            .then((logsStatus) => {
              if (!cancelled) publishAdminStatus(mergeStatusLogs(getAdminStatusSnapshot().status, logsStatus.logs));
            })
            .catch(() => undefined);
        }
      } catch (error) {
        if (!cancelled) setLoadError(error instanceof Error ? error.message : String(error));
      }
    }
    void load();
    return () => {
      cancelled = true;
      disconnectRealtime?.();
    };
  }, []);

  function switchView(nextView: AdminView): void {
    setAdminView(nextView);
    persistAdminView(nextView);
  }

  return (
    <div className="shell" data-service-name={serviceName}>
      <header className="topbar">
        <nav id="admin-nav" className="admin-nav" aria-label="管理台模块">
          <button className={"nav-item" + (adminView === "sessions" ? " active" : "")} type="button" onClick={() => switchView("sessions")}>
            会话
          </button>
          <button className={"nav-item" + (adminView === "ops" ? " active" : "")} type="button" onClick={() => switchView("ops")}>
            操作
          </button>
        </nav>
        <TopbarProfiles profiles={status.profiles?.items || []} />
      </header>

      <div className="admin-content">
        {loadError ? (
          <div className="summary-detail" style={{ color: "var(--red)", padding: "4px 0" }}>
            {loadError}
          </div>
        ) : null}
        {slackSetup && !slackSetup.configured ? (
          <div className="summary-detail" style={{ padding: "8px 0" }}>
            先在「操作」里填 Slack App Token 和 Bot Token，gateway 才会连上。
          </div>
        ) : null}
        <section className={"admin-view" + (adminView === "sessions" ? " active" : "")} data-admin-view="sessions">
          <AdminSessionsView />
        </section>
        <section className={"admin-view" + (adminView === "ops" ? " active" : "")} data-admin-view="ops">
          <OperationsView status={status} slackSetup={slackSetup} onSlackChange={setSlackSetup} />
        </section>
      </div>
    </div>
  );
}
