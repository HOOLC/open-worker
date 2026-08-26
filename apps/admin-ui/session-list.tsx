import { Badge } from "./session-badge.js";

import { classSafeValue, fmtDateTime, fmtRelativeTime } from "./session-formatters.js";

import { renderSessionMeta, sessionActivityAt, sessionOperationalState, shouldShowSessionState } from "./session-row-display";

import { sessionFirstText, sessionPrimaryText } from "./session-selection.js";

import { SessionRecord } from "./session-types.js";

import React from "react";

export function SessionListRow({ session, selected, channelLabelById, onSelect }: { readonly session: SessionRecord; readonly selected: boolean; readonly channelLabelById?: ReadonlyMap<string, string>; readonly onSelect: () => void }): React.JSX.Element {
  const state = sessionOperationalState(session);
  const activityAt = sessionActivityAt(session);
  const primary = sessionPrimaryText(session);
  const first = sessionFirstText(session);
  const stateBadge = shouldShowSessionState(state) ? <Badge label={state.label} tone={state.tone} title={state.detail} /> : null;
  return (
    <button type="button" className={"session-row-button session-card session-priority-" + classSafeValue(state.tone, "idle") + (selected ? " active" : "")} data-session-key={session.key} onClick={onSelect}>
      <div className="session-summary">
        <div className="session-line">
          <div className="session-lead" title={primary}>
            {primary}
          </div>
          {stateBadge}
          <div className="session-time" title={fmtDateTime(activityAt)}>
            {fmtRelativeTime(activityAt)}
          </div>
        </div>
        <div className="session-channel" title={first}>
          {first}
        </div>
        <div className="session-meta-line">
          {renderSessionMeta(session, channelLabelById).map((pill) => (
            <span key={pill.key} className={"session-meta-pill " + classSafeValue(pill.tone, "")} title={pill.title}>
              {pill.label}
            </span>
          ))}
        </div>
      </div>
    </button>
  );
}
