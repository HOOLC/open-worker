import React, { useEffect, useState } from "react";

import { requestJson } from "./admin-shell-helpers-3.js";
import { errorMessage } from "./admin-shell-helpers-4.js";

export type SlackSetup = {
  readonly configured: boolean;
  readonly appTokenSet: boolean;
  readonly botTokenSet: boolean;
};

export function SlackSettingsPanel({ setup, onChange }: { readonly setup: SlackSetup | null; readonly onChange: (setup: SlackSetup) => void }): React.JSX.Element {
  const [appToken, setAppToken] = useState("");
  const [botToken, setBotToken] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function save(): Promise<void> {
    setBusy(true);
    setMessage(null);
    try {
      const payload = await requestJson("/admin/api/settings", {
        method: "PUT",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          slack: {
            appToken: appToken.trim() || undefined,
            botToken: botToken.trim() || undefined,
          },
        }),
      });
      const next = payload.slack as SlackSetup;
      onChange(next);
      setAppToken("");
      setBotToken("");
      setMessage(next.configured ? "Slack 已保存，gateway 会自动连上。" : "已保存，还差另一个 token。");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <h2>Slack</h2>
        <span className="summary-detail">{setup?.configured ? "已配置" : "粘贴 Socket Mode 的 App Token 和 Bot Token"}</span>
      </div>
      <div className="panel-body" style={{ display: "grid", gap: 8 }}>
        <label>
          App Token
          <input type="password" autoComplete="off" placeholder={setup?.appTokenSet ? "已保存，留空则不改" : "xapp-..."} value={appToken} onChange={(event) => setAppToken(event.target.value)} />
        </label>
        <label>
          Bot Token
          <input type="password" autoComplete="off" placeholder={setup?.botTokenSet ? "已保存，留空则不改" : "xoxb-..."} value={botToken} onChange={(event) => setBotToken(event.target.value)} />
        </label>
        <div>
          <button type="button" disabled={busy || (!appToken.trim() && !botToken.trim())} onClick={() => void save()}>
            保存
          </button>
        </div>
        {message ? <div className="summary-detail">{message}</div> : null}
      </div>
    </section>
  );
}

export function useSlackSetup(): [SlackSetup | null, (setup: SlackSetup) => void] {
  const [setup, setSetup] = useState<SlackSetup | null>(null);
  useEffect(() => {
    let cancelled = false;
    void requestJson("/admin/api/settings")
      .then((payload) => {
        if (!cancelled) setSetup(payload.slack as SlackSetup);
      })
      .catch(() => {
        if (!cancelled) setSetup({ configured: false, appTokenSet: false, botTokenSet: false });
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return [setup, setSetup];
}
