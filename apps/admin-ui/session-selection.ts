import { activeBackgroundJobCount, activeBackgroundJobs, sessionActivityMs, sessionOperationalState, sessionSelectionBlocked } from "./session-row-display";

import { SessionRecord } from "./session-types.js";

import { statusLabel } from "./timeline-display";

export function sessionMatchesFilter(session: SessionRecord, mode: string): boolean {
  const hasIssue = Number(session.blockedInboundCount || 0) > 0 || sessionSelectionBlocked(session);
  if (mode === "jobs" && !activeBackgroundJobCount(session)) return false;
  if (mode === "issues" && !hasIssue) return false;
  return true;
}

export function resolveSelectedSession(sessions: readonly SessionRecord[], selectedSessionKey: string | null): SessionRecord | null {
  if (!sessions.length) return null;
  return sessions.find((session) => session.key === selectedSessionKey) || sessions[0] || null;
}

export function sessionPrimaryText(session: SessionRecord): string {
  return messagePreview(session.lastUserMessage) || summarizeSessionLead(session);
}

export function sessionFirstText(session: SessionRecord): string {
  return messagePreview(session.firstUserMessage) || "没有用户消息";
}

export function messagePreview(message: Record<string, any> | undefined): string {
  return String(message?.textPreview || message?.text || "").trim();
}

export function summarizeSessionLead(session: SessionRecord): string {
  if (session.lastUserMessage) return messagePreview(session.lastUserMessage) || "用户消息";
  const activeJob = activeBackgroundJobs(session)[0];
  if (activeJob) {
    const running = activeJob;
    return (running.kind || "任务") + "（" + statusLabel(running.status || "?") + "）";
  }
  return "暂无输入记录";
}

export function compareSessionsForMode(mode: string, left: SessionRecord, right: SessionRecord): number {
  if (mode === "all") {
    const activityDelta = sessionActivityMs(right) - sessionActivityMs(left);
    if (activityDelta) return activityDelta;
  }
  const rankDelta = sessionOperationalState(right).rank - sessionOperationalState(left).rank;
  if (rankDelta) return rankDelta;
  const activityDelta = sessionActivityMs(right) - sessionActivityMs(left);
  if (activityDelta) return activityDelta;
  return String(left.key).localeCompare(String(right.key));
}
