import fs from "node:fs/promises";
import path from "node:path";

import { getBrokerLogDirectory, getJobLogDirectory, getSessionLogDirectory, logger } from "../logger.js";
import type {
  AdminOperationKind,
  JsonLike,
  PersistedAdminAuditEvent,
  PersistedAdminEvent,
  PersistedAdminOperation,
  PersistedAgentSessionTraceSummary,
  PersistedAgentSessionUsageSummary,
  PersistedAgentTraceEvent,
  PersistedBackgroundJob,
  PersistedAgentTurnUsage,
  PersistedInboundMessage,
  SlackSessionRecord,
  SlackUserIdentity,
} from "../types.js";
import type { GitHubPrBindingRecord, GitHubPrIdentityStatus } from "./github-pr-identity-service.js";
import { authProfileReasonLabel } from "./session-auth-profile-selector.js";
import type { DeployReleaseOptions, RollbackReleaseOptions, ReleaseDeploymentService } from "./deploy/release-deployment-service.js";
import { type SerializedAccountStatus, type SerializedRateLimitsStatus } from "./codex/account-status.js";

const LOG_TAIL_MAX_BYTES_PER_FILE = 256 * 1024;
const ADMIN_RUNTIME_PROBE_TIMEOUT_MS = 4_000;

interface FileInfo {
  readonly exists: boolean;
  readonly path: string;
  readonly size?: number | undefined;
  readonly mtime?: string | undefined;
}

interface SessionSnapshot {
  readonly allSessions: readonly SlackSessionRecord[];
  readonly activeSessions: readonly SlackSessionRecord[];
  readonly inbound: readonly PersistedInboundMessage[];
  readonly openInbound: readonly PersistedInboundMessage[];
  readonly backgroundJobs: readonly PersistedBackgroundJob[];
  readonly inboundBySession: ReadonlyMap<string, readonly PersistedInboundMessage[]>;
  readonly openInboundBySession: ReadonlyMap<string, readonly PersistedInboundMessage[]>;
  readonly jobsBySession: ReadonlyMap<string, readonly PersistedBackgroundJob[]>;
  readonly usageBySession: ReadonlyMap<string, SessionUsageSummary>;
}

interface RuntimeStatus {
  readonly account: SerializedAccountStatus;
  readonly rateLimits: SerializedRateLimitsStatus;
  readonly deployment: unknown;
  readonly authProfiles: unknown;
  readonly githubAuthorMappings: {
    readonly count: number;
    readonly mappings: readonly unknown[];
  };
  readonly githubPrIdentities: {
    readonly count: number;
    readonly bindings: readonly unknown[];
  };
  readonly githubAccounts: {
    readonly count: number;
    readonly defaultPrAccount: GitHubPrIdentityStatus["defaultAccount"];
    readonly accounts: readonly unknown[];
  };
}

interface SlackConversationLookup {
  getConversationInfo(channelId: string): Promise<{
    readonly channelId: string;
    readonly name?: string | undefined;
    readonly channelType?: string | undefined;
  } | null>;
  getUserIdentity?(userId: string): Promise<SlackUserIdentity | null>;
  getPermalink?(options: { readonly channelId: string; readonly messageTs: string }): Promise<string | null>;
}

interface OperationPreflight {
  readonly operation: string;
  readonly safe: boolean;
  readonly requiresAllowActive: boolean;
  readonly activeCount: number;
  readonly openInboundCount: number;
  readonly runningBackgroundJobCount: number;
  readonly impacts: readonly Record<string, JsonLike>[];
}

interface AdminOperationStore {
  readonly listAdminOperations?: ((limit?: number) => PersistedAdminOperation[]) | undefined;
  readonly upsertAdminOperation?: ((record: PersistedAdminOperation) => Promise<void>) | undefined;
  readonly listAdminAuditEvents?: ((options?: { readonly operationId?: string | undefined; readonly limit?: number | undefined }) => PersistedAdminAuditEvent[]) | undefined;
  readonly appendAdminAuditEvent?: ((record: PersistedAdminAuditEvent) => Promise<void>) | undefined;
  readonly listAgentTurnUsage?: ((limit?: number) => PersistedAgentTurnUsage[]) | undefined;
  readonly listAgentSessionUsageSummaries?: (() => PersistedAgentSessionUsageSummary[]) | undefined;
  readonly getAgentSessionUsageSummary?: ((sessionKey: string) => PersistedAgentSessionUsageSummary | undefined) | undefined;
  readonly listAgentTraceEventsPage?:
    | ((
        sessionKey: string,
        options?: {
          readonly limit?: number | undefined;
          readonly beforeSequence?: number | undefined;
        },
      ) => {
        readonly events: PersistedAgentTraceEvent[];
        readonly hasMore: boolean;
        readonly nextBeforeSequence: number | null;
      })
    | undefined;
  readonly getAgentSessionTraceSummary?: ((sessionKey: string) => PersistedAgentSessionTraceSummary | undefined) | undefined;
}

interface AgentUsageTotals {
  readonly totalTurns: number;
  readonly exactTurns: number;
  readonly estimatedTurns: number;
  readonly missingTurns: number;
  readonly inputTokens: number;
  readonly cachedInputTokens: number;
  readonly outputTokens: number;
  readonly reasoningTokens: number;
  readonly totalTokens: number;
}

interface SessionUsageSummary {
  readonly sessionKey: string;
  readonly channelId: string;
  readonly rootThreadTs: string;
  readonly turnCount: number;
  readonly exactTurns: number;
  readonly estimatedTurns: number;
  readonly missingTurns: number;
  readonly inputTokens: number;
  readonly cachedInputTokens: number;
  readonly outputTokens: number;
  readonly reasoningTokens: number;
  readonly totalTokens: number;
  readonly updatedAt: string;
  readonly lastTurnAt: string | null;
  readonly model: string | null;
  readonly effort: string | null;
}
type MutableSessionUsageSummary = { -readonly [Key in keyof SessionUsageSummary]: SessionUsageSummary[Key] };

import { AdminServiceLayer15 } from "./admin-service-layer15.js";
export class AdminServiceLayer16 extends AdminServiceLayer15 {
  privateSummarizeSession(
    session: SlackSessionRecord,
    related: {
      readonly inbound: readonly PersistedInboundMessage[];
      readonly openInbound: readonly PersistedInboundMessage[];
      readonly jobs: readonly PersistedBackgroundJob[];
      readonly usage?: SessionUsageSummary | undefined;
      readonly channelLabels?: ReadonlyMap<string, string> | undefined;
    },
  ): Record<string, unknown> {
    const runningBackgroundJobCount = related.jobs.filter((job) => job.status === "running").length;
    const failedBackgroundJobs = related.jobs.filter((job) => job.status === "failed");
    const failedBackgroundJobCount = failedBackgroundJobs.length;
    const openHumanInboundCount = related.openInbound.filter(isHumanInboundMessage).length;
    const openSystemInboundCount = related.openInbound.length - openHumanInboundCount;
    const userMessages = related.inbound.filter(isUserInboundMessage);
    const firstUserMessage = userMessages.at(0);
    const lastUserMessage = userMessages.at(-1);
    const lastActivityAt = sessionLastActivityAt(session, {
      inbound: related.inbound,
      jobs: related.jobs,
      usage: related.usage,
    });
    return {
      key: session.key,
      platform: session.platform ?? "slack",
      conversationId: session.conversationId ?? session.channelId,
      conversationKind: session.conversationKind ?? session.channelType ?? null,
      rootMessageId: session.rootMessageId ?? session.rootThreadTs,
      platformThreadId: session.platformThreadId ?? null,
      channelId: session.channelId,
      channelLabel: channelLabelForSession(session, related.inbound, related.channelLabels),
      channelName: session.channelName ?? null,
      channelType: session.channelType ?? related.inbound.find((message) => message.channelType)?.channelType ?? null,
      rootThreadTs: session.rootThreadTs,
      initiatorUserId: session.initiatorUserId ?? null,
      threadUrl: isSlackSession(session) ? buildSlackThreadUrl(session.channelId, session.rootThreadTs) : null,
      workspacePath: session.workspacePath,
      agentSessionId: session.agentSessionId ?? null,
      updatedAt: session.updatedAt,
      lastActivityAt,
      createdAt: session.createdAt,
      firstUserMessage: firstUserMessage ? this.privateSummarizeInbound(firstUserMessage) : null,
      lastUserMessage: lastUserMessage ? this.privateSummarizeInbound(lastUserMessage) : null,
      activeTurnId: session.activeTurnId ?? null,
      activeTurnStartedAt: session.activeTurnStartedAt ?? null,
      lastTurnSignalKind: session.lastTurnSignalKind ?? null,
      lastTurnSignalReason: session.lastTurnSignalReason ?? null,
      lastTurnSignalAt: session.lastTurnSignalAt ?? null,
      lastSlackReplyAt: session.lastSlackReplyAt ?? null,
      sessionPageLinkPostedAt: session.sessionPageLinkPostedAt ?? null,
      authProfileName: session.authProfileName ?? null,
      authProfileBoundAt: session.authProfileBoundAt ?? null,
      authBlockedAt: session.authBlockedAt ?? null,
      authBlockReason: session.authBlockReason ?? null,
      authBlockReasonLabel: authProfileReasonLabel(session.authBlockReason),
      authBlockedNoticePostedAt: session.authBlockedNoticePostedAt ?? null,
      lastObservedMessageTs: session.lastObservedMessageTs ?? null,
      lastDeliveredMessageTs: session.lastDeliveredMessageTs ?? null,
      openInboundCount: related.openInbound.length,
      openHumanInboundCount,
      openSystemInboundCount,
      openInbound: related.openInbound.slice(0, 5).map((message: PersistedInboundMessage) => this.privateSummarizeInbound(message)),
      backgroundJobCount: related.jobs.length,
      runningBackgroundJobCount,
      failedBackgroundJobCount,
      failedBackgroundJobs: failedBackgroundJobs.slice(0, 3).map((job) => this.privateSummarizeJob(job)),
      backgroundJobs: sortBackgroundJobsForAdminSummary(related.jobs)
        .slice(0, 5)
        .map((job) => this.privateSummarizeJob(job)),
      usage: related.usage ?? emptySessionUsageSummary(session),
    };
  }

  privateSummarizeSessionListItem(
    session: SlackSessionRecord,
    related: {
      readonly inbound: readonly PersistedInboundMessage[];
      readonly openInbound: readonly PersistedInboundMessage[];
      readonly jobs: readonly PersistedBackgroundJob[];
      readonly usage?: SessionUsageSummary | undefined;
      readonly channelLabels?: ReadonlyMap<string, string> | undefined;
    },
  ): Record<string, unknown> {
    const runningBackgroundJobCount = related.jobs.filter((job) => job.status === "running").length;
    const failedBackgroundJobCount = related.jobs.filter((job) => job.status === "failed").length;
    const openHumanInboundCount = related.openInbound.filter(isHumanInboundMessage).length;
    const openSystemInboundCount = related.openInbound.length - openHumanInboundCount;
    const userMessages = related.inbound.filter(isUserInboundMessage);
    const firstUserMessage = userMessages.at(0);
    const lastUserMessage = userMessages.at(-1);
    const usage = related.usage ?? emptySessionUsageSummary(session);
    const lastActivityAt = sessionLastActivityAt(session, {
      inbound: related.inbound,
      jobs: related.jobs,
      usage,
    });
    return {
      key: session.key,
      platform: session.platform ?? "slack",
      conversationId: session.conversationId ?? session.channelId,
      conversationKind: session.conversationKind ?? session.channelType ?? null,
      rootMessageId: session.rootMessageId ?? session.rootThreadTs,
      platformThreadId: session.platformThreadId ?? null,
      channelId: session.channelId,
      channelLabel: channelLabelForSession(session, related.inbound, related.channelLabels),
      channelName: session.channelName ?? null,
      channelType: session.channelType ?? related.inbound.find((message) => message.channelType)?.channelType ?? null,
      rootThreadTs: session.rootThreadTs,
      initiatorUserId: session.initiatorUserId ?? null,
      threadUrl: isSlackSession(session) ? buildSlackThreadUrl(session.channelId, session.rootThreadTs) : null,
      updatedAt: session.updatedAt,
      lastActivityAt,
      createdAt: session.createdAt,
      firstUserMessage: firstUserMessage ? this.privateSummarizeInboundListMessage(firstUserMessage) : null,
      lastUserMessage: lastUserMessage ? this.privateSummarizeInboundListMessage(lastUserMessage) : null,
      activeTurnId: session.activeTurnId ?? null,
      activeTurnStartedAt: session.activeTurnStartedAt ?? null,
      lastTurnSignalKind: session.lastTurnSignalKind ?? null,
      lastTurnSignalReason: session.lastTurnSignalReason ?? null,
      lastTurnSignalAt: session.lastTurnSignalAt ?? null,
      lastSlackReplyAt: session.lastSlackReplyAt ?? null,
      authProfileName: session.authProfileName ?? null,
      authBlockedAt: session.authBlockedAt ?? null,
      authBlockReason: session.authBlockReason ?? null,
      authBlockReasonLabel: authProfileReasonLabel(session.authBlockReason),
      openInboundCount: related.openInbound.length,
      openHumanInboundCount,
      openSystemInboundCount,
      backgroundJobCount: related.jobs.length,
      runningBackgroundJobCount,
      failedBackgroundJobCount,
      usage: summarizeListUsage(usage),
    };
  }
}

function isSlackSession(session: SlackSessionRecord): boolean {
  return !session.platform || session.platform === "slack";
}

function summarizeUsageTotals(records: readonly PersistedAgentTurnUsage[]): AgentUsageTotals {
  let totalTurns = 0;
  let exactTurns = 0;
  let estimatedTurns = 0;
  let missingTurns = 0;
  let inputTokens = 0;
  let cachedInputTokens = 0;
  let outputTokens = 0;
  let reasoningTokens = 0;
  let totalTokens = 0;

  for (const record of records) {
    totalTurns += 1;
    if (record.source === "exact") {
      exactTurns += 1;
    } else if (record.source === "estimated") {
      estimatedTurns += 1;
    } else {
      missingTurns += 1;
    }

    inputTokens += record.inputTokens;
    cachedInputTokens += record.cachedInputTokens;
    outputTokens += record.outputTokens;
    reasoningTokens += record.reasoningTokens;
    totalTokens += record.totalTokens;
  }

  return {
    totalTurns,
    exactTurns,
    estimatedTurns,
    missingTurns,
    inputTokens,
    cachedInputTokens,
    outputTokens,
    reasoningTokens,
    totalTokens,
  };
}

function filterUsageWindow(records: readonly PersistedAgentTurnUsage[], windowMs: number): PersistedAgentTurnUsage[] {
  const cutoff = Date.now() - windowMs;
  return records.filter((record) => usageTimestampMs(record) >= cutoff);
}

function summarizeUsageRecord(record: PersistedAgentTurnUsage): Record<string, unknown> {
  return {
    turnId: record.turnId,
    sessionKey: record.sessionKey,
    channelId: record.channelId,
    rootThreadTs: record.rootThreadTs,
    agentSessionId: record.agentSessionId ?? null,
    status: record.status,
    source: record.source,
    model: record.model ?? null,
    effort: record.effort ?? null,
    inputTokens: record.inputTokens,
    cachedInputTokens: record.cachedInputTokens,
    outputTokens: record.outputTokens,
    reasoningTokens: record.reasoningTokens,
    totalTokens: record.totalTokens,
    startedAt: record.startedAt ?? null,
    completedAt: record.completedAt ?? null,
    updatedAt: record.updatedAt,
  };
}

function summarizeUsageBySessionMap(records: readonly PersistedAgentTurnUsage[]): ReadonlyMap<string, SessionUsageSummary> {
  return new Map(summarizeUsageBySession(records).map((entry) => [entry.sessionKey, entry]));
}

function summarizeUsageBySessionMapFromPersisted(records: readonly PersistedAgentSessionUsageSummary[]): ReadonlyMap<string, SessionUsageSummary> {
  return new Map(records.map((record) => [record.sessionKey, sessionUsageSummaryFromPersisted(record)!]));
}

function sessionUsageSummaryFromPersisted(record: PersistedAgentSessionUsageSummary | undefined): SessionUsageSummary | undefined {
  if (!record) {
    return undefined;
  }
  return {
    sessionKey: record.sessionKey,
    channelId: record.channelId,
    rootThreadTs: record.rootThreadTs,
    turnCount: record.turnCount,
    exactTurns: record.exactTurns,
    estimatedTurns: record.estimatedTurns,
    missingTurns: record.missingTurns,
    inputTokens: record.inputTokens,
    cachedInputTokens: record.cachedInputTokens,
    outputTokens: record.outputTokens,
    reasoningTokens: record.reasoningTokens,
    totalTokens: record.totalTokens,
    updatedAt: record.updatedAt,
    lastTurnAt: record.lastTurnAt ?? null,
    model: record.model ?? null,
    effort: record.effort ?? null,
  };
}

function summarizeListUsage(usage: SessionUsageSummary): Record<string, unknown> {
  return {
    turnCount: usage.turnCount,
    totalTokens: usage.totalTokens,
    lastTurnAt: usage.lastTurnAt,
    updatedAt: usage.updatedAt,
  };
}

function summarizeUsageBySession(records: readonly PersistedAgentTurnUsage[], limit?: number): readonly SessionUsageSummary[] {
  const groups = new Map<string, MutableSessionUsageSummary>();

  for (const record of records) {
    const existing = groups.get(record.sessionKey) ?? {
      sessionKey: record.sessionKey,
      channelId: record.channelId,
      rootThreadTs: record.rootThreadTs,
      turnCount: 0,
      exactTurns: 0,
      estimatedTurns: 0,
      missingTurns: 0,
      inputTokens: 0,
      cachedInputTokens: 0,
      outputTokens: 0,
      reasoningTokens: 0,
      totalTokens: 0,
      updatedAt: record.updatedAt,
      lastTurnAt: record.completedAt ?? record.updatedAt,
      model: record.model ?? null,
      effort: record.effort ?? null,
    };
    existing.turnCount += 1;
    existing.exactTurns += record.source === "exact" ? 1 : 0;
    existing.estimatedTurns += record.source === "estimated" ? 1 : 0;
    existing.missingTurns += record.source === "missing" ? 1 : 0;
    existing.inputTokens += record.inputTokens;
    existing.cachedInputTokens += record.cachedInputTokens;
    existing.outputTokens += record.outputTokens;
    existing.reasoningTokens += record.reasoningTokens;
    existing.totalTokens += record.totalTokens;
    if (usageTimestampMs(record) >= Date.parse(existing.lastTurnAt ?? "")) {
      existing.updatedAt = record.updatedAt;
      existing.lastTurnAt = record.completedAt ?? record.updatedAt;
      existing.model = record.model ?? existing.model;
      existing.effort = record.effort ?? existing.effort;
    }
    groups.set(record.sessionKey, existing);
  }

  const sorted = [...groups.values()].sort((left, right) => right.totalTokens - left.totalTokens || right.turnCount - left.turnCount);
  return limit === undefined ? sorted : sorted.slice(0, limit);
}

function emptySessionUsageSummary(session: SlackSessionRecord): SessionUsageSummary {
  return {
    sessionKey: session.key,
    channelId: session.channelId,
    rootThreadTs: session.rootThreadTs,
    turnCount: 0,
    exactTurns: 0,
    estimatedTurns: 0,
    missingTurns: 0,
    inputTokens: 0,
    cachedInputTokens: 0,
    outputTokens: 0,
    reasoningTokens: 0,
    totalTokens: 0,
    updatedAt: session.updatedAt,
    lastTurnAt: null,
    model: null,
    effort: null,
  };
}

function compareUsageRecordsDescending(left: PersistedAgentTurnUsage, right: PersistedAgentTurnUsage): number {
  return usageTimestampMs(right) - usageTimestampMs(left) || right.updatedAt.localeCompare(left.updatedAt);
}

function usageTimestampMs(record: PersistedAgentTurnUsage): number {
  const parsed = Date.parse(record.completedAt ?? record.updatedAt ?? record.createdAt);
  return Number.isFinite(parsed) ? parsed : 0;
}

function compareSessions(
  left: SlackSessionRecord,
  right: SlackSessionRecord,
  related: {
    readonly inboundBySession: ReadonlyMap<string, readonly PersistedInboundMessage[]>;
    readonly jobsBySession: ReadonlyMap<string, readonly PersistedBackgroundJob[]>;
    readonly usageBySession: ReadonlyMap<string, SessionUsageSummary>;
  },
): number {
  const leftActive = left.activeTurnId ? 1 : 0;
  const rightActive = right.activeTurnId ? 1 : 0;
  if (leftActive !== rightActive) {
    return rightActive - leftActive;
  }
  return (
    sessionLastActivityMs(right, {
      inbound: related.inboundBySession.get(right.key) ?? [],
      jobs: related.jobsBySession.get(right.key) ?? [],
      usage: related.usageBySession.get(right.key),
    }) -
      sessionLastActivityMs(left, {
        inbound: related.inboundBySession.get(left.key) ?? [],
        jobs: related.jobsBySession.get(left.key) ?? [],
        usage: related.usageBySession.get(left.key),
      }) || String(left.key).localeCompare(String(right.key))
  );
}

function sessionLastActivityAt(
  session: SlackSessionRecord,
  related: {
    readonly inbound: readonly PersistedInboundMessage[];
    readonly jobs: readonly PersistedBackgroundJob[];
    readonly usage?: SessionUsageSummary | undefined;
  },
): string {
  const candidates = [session.lastTurnSignalAt, session.lastSlackReplyAt, session.activeTurnStartedAt, related.usage?.lastTurnAt, ...related.inbound.flatMap((message) => [message.updatedAt, message.createdAt]), ...related.jobs.flatMap(jobActivityTimestamps)];
  const latestMs = newestTimestamp(candidates);
  return candidates.find((value) => timestampMs(value) === latestMs) ?? session.createdAt ?? session.updatedAt;
}

function sessionLastActivityMs(
  session: SlackSessionRecord,
  related: {
    readonly inbound: readonly PersistedInboundMessage[];
    readonly jobs: readonly PersistedBackgroundJob[];
    readonly usage?: SessionUsageSummary | undefined;
  },
): number {
  return timestampMs(sessionLastActivityAt(session, related));
}

function jobActivityTimestamps(job: PersistedBackgroundJob): Array<string | null | undefined> {
  return [job.lastEventAt, job.status === "running" ? null : job.updatedAt, job.createdAt];
}

function sortBackgroundJobsForAdminSummary(jobs: readonly PersistedBackgroundJob[]): PersistedBackgroundJob[] {
  return [...jobs].sort((left, right) => {
    const rankDelta = backgroundJobDisplayRank(left) - backgroundJobDisplayRank(right);
    if (rankDelta) return rankDelta;
    return timestampMs(right.lastEventAt ?? right.updatedAt ?? right.createdAt) - timestampMs(left.lastEventAt ?? left.updatedAt ?? left.createdAt);
  });
}

function backgroundJobDisplayRank(job: PersistedBackgroundJob): number {
  if (isAdminCancellableJob(job)) return 0;
  if (job.status === "failed") return 2;
  return 1;
}

function timestampMs(value: unknown): number {
  const parsed = typeof value === "string" ? Date.parse(value) : NaN;
  return Number.isFinite(parsed) ? parsed : 0;
}

function newestTimestamp(values: readonly unknown[]): number {
  return values.reduce<number>((latest, value) => Math.max(latest, timestampMs(value)), 0);
}

function compareTimelineEvents(left: Record<string, JsonLike>, right: Record<string, JsonLike>): number {
  if (left.type === "session_created" && right.type !== "session_created") {
    return -1;
  }
  if (right.type === "session_created" && left.type !== "session_created") {
    return 1;
  }
  const atComparison = String(left.at ?? "").localeCompare(String(right.at ?? ""));
  if (atComparison !== 0) {
    return atComparison;
  }
  const leftSequence = typeof left.sequence === "number" ? left.sequence : 0;
  const rightSequence = typeof right.sequence === "number" ? right.sequence : 0;
  return leftSequence - rightSequence;
}

function selectTimelinePageEvents(options: { readonly agentEvents: readonly Record<string, JsonLike>[]; readonly limit: number; readonly traceHasMore: boolean; readonly traceNextBeforeSequence: number | null }): {
  readonly events: readonly Record<string, JsonLike>[];
  readonly hasMore: boolean;
  readonly nextBeforeSequence: number | null;
} {
  const newest = [...options.agentEvents].sort(compareTimelineEventsNewestFirst).slice(0, options.limit);
  const returnedTraceSequences = newest.map((event) => (typeof event.sequence === "number" ? event.sequence : null)).filter((sequence): sequence is number => typeof sequence === "number");
  const nextBeforeSequence = returnedTraceSequences.length ? Math.min(...returnedTraceSequences) : options.traceNextBeforeSequence;
  return {
    events: newest.slice().sort(compareTimelineEvents),
    hasMore: Boolean(nextBeforeSequence && options.traceHasMore),
    nextBeforeSequence,
  };
}

function compareTimelineEventsNewestFirst(left: Record<string, JsonLike>, right: Record<string, JsonLike>): number {
  const atComparison = timestampMs(right.at) - timestampMs(left.at);
  if (atComparison !== 0) {
    return atComparison;
  }
  const leftSequence = typeof left.sequence === "number" ? left.sequence : 0;
  const rightSequence = typeof right.sequence === "number" ? right.sequence : 0;
  if (rightSequence !== leftSequence) {
    return rightSequence - leftSequence;
  }
  return String(right.id ?? "").localeCompare(String(left.id ?? ""));
}

function comparePersistedTraceEvents(left: PersistedAgentTraceEvent, right: PersistedAgentTraceEvent): number {
  const atComparison = left.at.localeCompare(right.at);
  if (atComparison !== 0) {
    return atComparison;
  }
  return left.sequence - right.sequence || left.id.localeCompare(right.id);
}

function comparePersistedTraceEventsNewestFirst(left: PersistedAgentTraceEvent, right: PersistedAgentTraceEvent): number {
  const atComparison = timestampMs(right.at) - timestampMs(left.at);
  if (atComparison !== 0) {
    return atComparison;
  }
  return right.sequence - left.sequence || right.id.localeCompare(left.id);
}

function summarizeAgentTrace(events: readonly PersistedAgentTraceEvent[], allEvents: readonly PersistedAgentTraceEvent[] = events): Record<string, JsonLike> {
  const categories: Record<string, number> = {};
  const sources: Record<string, number> = {};
  for (const event of events) {
    categories[event.type] = (categories[event.type] ?? 0) + 1;
    sources[event.source] = (sources[event.source] ?? 0) + 1;
  }
  const modelRequestCount = allEvents.filter((event) => event.type === "agent_token_count").length;
  return {
    source: "broker_db",
    eventCount: events.length,
    modelRequestCount,
    categories,
    sources,
  };
}

function traceSummaryFromPersisted(summary: PersistedAgentSessionTraceSummary | undefined, fallbackEvents: readonly PersistedAgentTraceEvent[]): Record<string, JsonLike> {
  if (!summary) {
    return summarizeAgentTrace(fallbackEvents);
  }
  return {
    source: "broker_db",
    eventCount: summary.eventCount,
    modelRequestCount: summary.modelRequestCount,
    categories: summary.categories,
    sources: summary.sources,
  };
}

function isVisibleTimelineTraceEvent(event: PersistedAgentTraceEvent): boolean {
  if (event.type === "agent_token_count") {
    return false;
  }
  if (event.type === "agent_input_delivered" || event.type === "agent_turn_started") {
    return false;
  }
  if (event.type === "agent_turn_completed" && event.status === "completed") {
    return false;
  }
  return true;
}

function visibleTimelineTraceEvents(events: readonly PersistedAgentTraceEvent[]): PersistedAgentTraceEvent[] {
  const completedToolCallKeys = new Set(
    events
      .filter((event) => event.type === "agent_tool_result")
      .map(toolTraceKey)
      .filter(Boolean),
  );
  return events.filter((event) => {
    if (!isVisibleTimelineTraceEvent(event)) {
      return false;
    }
    return !(event.type === "agent_tool_call" && completedToolCallKeys.has(toolTraceKey(event)));
  });
}

function toolTraceKey(event: Pick<PersistedAgentTraceEvent, "turnId" | "callId" | "toolName">): string {
  if (event.callId) {
    return [event.turnId ?? "", event.callId].join("\u001f");
  }
  if (!event.turnId && !event.toolName) {
    return "";
  }
  return [event.turnId ?? "", event.toolName ?? ""].join("\u001f");
}

function agentTraceEventToTimelineEvent(
  event: PersistedAgentTraceEvent,
  options: {
    readonly includeDetail?: boolean | undefined;
  } = {},
): Record<string, JsonLike> {
  return withoutUndefined({
    id: event.id,
    sessionKey: event.sessionKey,
    type: event.type,
    at: event.at,
    sequence: event.sequence,
    title: event.title,
    summary: event.summary,
    detail: options.includeDetail ? event.detail : undefined,
    detailAvailable: Boolean(event.detail),
    status: event.status,
    role: event.role,
    toolName: event.toolName,
    callId: event.callId,
    turnId: event.turnId,
    source: event.source,
    detailTruncated: event.detailTruncated,
    detailOriginalChars: event.detailOriginalChars,
    metadata: event.metadata,
  });
}

function withoutUndefined(values: Record<string, JsonLike | undefined>): Record<string, JsonLike> {
  const result: Record<string, JsonLike> = {};
  for (const [key, value] of Object.entries(values)) {
    if (value !== undefined) {
      result[key] = value;
    }
  }
  return result;
}

async function listJsonlFiles(directoryPath: string): Promise<
  Array<{
    readonly path: string;
    readonly mtimeMs: number;
  }>
> {
  const entries = await fs.readdir(directoryPath, { withFileTypes: true }).catch((error) => {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return [];
    }
    throw error;
  });

  const files = [];
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith(".jsonl")) {
      continue;
    }
    const filePath = path.join(directoryPath, entry.name);
    const stat = await fs.stat(filePath);
    files.push({
      path: filePath,
      mtimeMs: stat.mtimeMs,
    });
  }
  return files;
}

async function readJsonlFileTail(filePath: string, limit: number): Promise<unknown[]> {
  if (limit <= 0) {
    return [];
  }

  const handle = await fs.open(filePath, "r");
  try {
    const stat = await handle.stat();
    if (stat.size === 0) {
      return [];
    }

    const readLength = Math.min(stat.size, LOG_TAIL_MAX_BYTES_PER_FILE);
    const start = stat.size - readLength;
    const buffer = Buffer.alloc(readLength);
    const { bytesRead } = await handle.read(buffer, 0, readLength, start);
    let raw = buffer.subarray(0, bytesRead).toString("utf8");
    if (start > 0) {
      const firstNewline = raw.indexOf("\n");
      raw = firstNewline >= 0 ? raw.slice(firstNewline + 1) : "";
    }

    return raw
      .trim()
      .split("\n")
      .filter(Boolean)
      .slice(-limit)
      .map((line) => {
        try {
          return JSON.parse(line);
        } catch {
          return { raw: line };
        }
      });
  } finally {
    await handle.close();
  }
}

function groupBySession<T extends { readonly sessionKey: string }>(items: readonly T[]): Map<string, T[]> {
  const groups = new Map<string, T[]>();
  for (const item of items) {
    const existing = groups.get(item.sessionKey);
    if (existing) {
      existing.push(item);
      continue;
    }
    groups.set(item.sessionKey, [item]);
  }
  return groups;
}

function isHumanInboundMessage(message: PersistedInboundMessage): boolean {
  return message.source === "app_mention" || message.source === "direct_message" || message.source === "thread_reply";
}

function isUserInboundMessage(message: PersistedInboundMessage): boolean {
  return isHumanInboundMessage(message) && message.senderKind !== "bot" && message.senderKind !== "app" && message.text.trim().length > 0;
}

function channelLabelForSession(session: SlackSessionRecord, inbound: readonly PersistedInboundMessage[], channelLabels?: ReadonlyMap<string, string> | undefined): string {
  return channelHumanLabelForSession(session, inbound) ?? channelLabels?.get(session.channelId) ?? session.channelId;
}

function channelLabelForConversationInfo(info: Awaited<ReturnType<SlackConversationLookup["getConversationInfo"]>>): string | undefined {
  if (!info) {
    return undefined;
  }
  if (info.name) {
    return formatSlackChannelName(info.name);
  }
  if (info.channelType === "im") {
    return "私信";
  }
  if (info.channelType === "mpim") {
    return "群聊";
  }
  return undefined;
}

function uniqueChannelIds(sessions: readonly SlackSessionRecord[]): string[] {
  return [...new Set(sessions.map((session: SlackSessionRecord) => session.channelId).filter((channelId) => channelId))];
}

function looksLikeSlackConversationId(channelId: string): boolean {
  return /^[CDG][A-Z0-9]+$/.test(channelId);
}

function looksLikeSlackUserId(userId: string): boolean {
  return /^U[A-Z0-9]+$/.test(userId);
}

function buildChannelLabelLookup(sessions: readonly SlackSessionRecord[], inboundBySession: ReadonlyMap<string, readonly PersistedInboundMessage[]>): Map<string, string> {
  const labels = new Map<string, string>();
  for (const session of sessions) {
    const label = channelHumanLabelForSession(session, inboundBySession.get(session.key) ?? []);
    if (label) {
      labels.set(session.channelId, label);
    }
  }
  return labels;
}

function buildGitHubAccounts(options: {
  readonly bindings: readonly GitHubPrBindingRecord[];
  readonly defaultPrAccount: GitHubPrIdentityStatus["defaultAccount"];
  readonly sessions?: readonly SlackSessionRecord[] | undefined;
  readonly inbound?: readonly PersistedInboundMessage[] | undefined;
  readonly slackIdentities?: ReadonlyMap<string, SlackUserIdentity> | undefined;
}): RuntimeStatus["githubAccounts"] {
  const rows = new Map<
    string,
    {
      slackUserId: string;
      slackIdentity?: SlackUserIdentity | undefined;
      binding?: GitHubPrBindingRecord | undefined;
    }
  >();
  const slackIdentities = collectSlackIdentities(options.sessions ?? [], options.inbound ?? []);
  for (const [userId, identity] of options.slackIdentities ?? []) {
    slackIdentities.set(userId, identity);
  }

  for (const session of options.sessions ?? []) {
    const slackUserId = normalizeNonEmptyString(session.initiatorUserId);
    if (!slackUserId) {
      continue;
    }
    rows.set(slackUserId, {
      ...(rows.get(slackUserId) ?? { slackUserId }),
      slackUserId,
      slackIdentity: slackIdentities.get(slackUserId) ?? fallbackSlackIdentity(slackUserId),
    });
  }

  for (const message of options.inbound ?? []) {
    if (!isUserInboundMessage(message) || isSyntheticSlackUserId(message.userId)) {
      continue;
    }
    const slackUserId = message.userId;
    rows.set(slackUserId, {
      ...(rows.get(slackUserId) ?? { slackUserId }),
      slackUserId,
      slackIdentity: rows.get(slackUserId)?.slackIdentity ?? slackIdentities.get(slackUserId) ?? fallbackSlackIdentity(slackUserId),
    });
  }

  for (const binding of options.bindings) {
    rows.set(binding.slackUserId, {
      ...(rows.get(binding.slackUserId) ?? { slackUserId: binding.slackUserId }),
      slackUserId: binding.slackUserId,
      slackIdentity: rows.get(binding.slackUserId)?.slackIdentity ?? slackIdentities.get(binding.slackUserId) ?? fallbackSlackIdentity(binding.slackUserId),
      binding,
    });
  }

  const defaultSlackUserId = options.defaultPrAccount.available && options.defaultPrAccount.source === "bound" ? options.defaultPrAccount.slackUserId : undefined;
  const accounts = [...rows.values()]
    .map((row) => ({
      slackUserId: row.slackUserId,
      slackIdentity: row.slackIdentity ?? {
        userId: row.slackUserId,
        mention: `<@${row.slackUserId}>`,
      },
      isDefaultPrAccount: row.slackUserId === defaultSlackUserId,
      prBinding: row.binding
        ? {
            state: row.binding.revokedAt ? "revoked" : "bound",
            githubLogin: row.binding.githubLogin,
            githubUserId: row.binding.githubUserId,
            githubEmail: row.binding.githubEmail ?? null,
            githubName: row.binding.githubName ?? null,
            scopes: row.binding.scopes,
            createdAt: row.binding.createdAt,
            updatedAt: row.binding.updatedAt,
            lastValidatedAt: row.binding.lastValidatedAt ?? null,
            revokedAt: row.binding.revokedAt ?? null,
          }
        : {
            state: "unbound",
          },
    }))
    .sort((left, right) => {
      if (left.isDefaultPrAccount !== right.isDefaultPrAccount) {
        return left.isDefaultPrAccount ? -1 : 1;
      }
      const leftBound = left.prBinding.state === "bound";
      const rightBound = right.prBinding.state === "bound";
      if (leftBound !== rightBound) {
        return leftBound ? -1 : 1;
      }
      return left.slackUserId.localeCompare(right.slackUserId);
    });

  return {
    count: accounts.length,
    defaultPrAccount: options.defaultPrAccount,
    accounts,
  };
}

function collectKnownGitHubAccountSlackUserIds(options: { readonly bindings: readonly GitHubPrBindingRecord[]; readonly sessions: readonly SlackSessionRecord[]; readonly inbound?: readonly PersistedInboundMessage[] | undefined }): Set<string> {
  const ids = new Set<string>();
  for (const binding of options.bindings) ids.add(binding.slackUserId);
  for (const session of options.sessions) {
    const initiatorUserId = normalizeNonEmptyString(session.initiatorUserId);
    if (initiatorUserId) ids.add(initiatorUserId);
  }
  for (const message of options.inbound ?? []) {
    if (isUserInboundMessage(message) && !isSyntheticSlackUserId(message.userId)) {
      ids.add(message.userId);
    }
  }
  return ids;
}

function collectSlackIdentities(sessions: readonly SlackSessionRecord[], inbound: readonly PersistedInboundMessage[]): Map<string, SlackUserIdentity> {
  const identities = new Map<string, SlackUserIdentity>();
  for (const message of inbound) {
    for (const user of message.mentionedUsers ?? []) {
      if (user.userId) identities.set(user.userId, user);
    }
    const userId = normalizeNonEmptyString(message.userId);
    if (userId && !userId.startsWith("username:")) {
      identities.set(userId, {
        ...fallbackSlackIdentity(userId),
        ...(normalizeNonEmptyString(message.senderUsername) ? { username: normalizeNonEmptyString(message.senderUsername) } : {}),
        ...slackIdentityFromMessagePayload(userId, message.slackMessage),
      });
    }
  }
  for (const session of sessions) {
    const initiatorUserId = normalizeNonEmptyString(session.initiatorUserId);
    if (initiatorUserId && !identities.has(initiatorUserId)) {
      identities.set(initiatorUserId, fallbackSlackIdentity(initiatorUserId));
    }
  }
  return identities;
}

function slackIdentityFromMessagePayload(userId: string, payload: JsonLike | undefined): Partial<SlackUserIdentity> {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    return {};
  }
  const profile = "user_profile" in payload && payload.user_profile && typeof payload.user_profile === "object" && !Array.isArray(payload.user_profile) ? (payload.user_profile as Record<string, unknown>) : undefined;
  return {
    userId,
    mention: `<@${userId}>`,
    ...(normalizeNonEmptyString(profile?.display_name) ? { displayName: normalizeNonEmptyString(profile?.display_name) } : {}),
    ...(normalizeNonEmptyString(profile?.real_name) ? { realName: normalizeNonEmptyString(profile?.real_name) } : {}),
    ...(normalizeNonEmptyString(profile?.email) ? { email: normalizeNonEmptyString(profile?.email) } : {}),
  };
}

function inboundMessageSlackIdentity(message: PersistedInboundMessage): SlackUserIdentity {
  return {
    ...fallbackSlackIdentity(message.userId),
    ...(normalizeNonEmptyString(message.senderUsername) ? { username: normalizeNonEmptyString(message.senderUsername) } : {}),
    ...slackIdentityFromMessagePayload(message.userId, message.slackMessage),
  };
}

function fallbackSlackIdentity(userId: string): SlackUserIdentity {
  return {
    userId,
    mention: `<@${userId}>`,
  };
}

function isSyntheticSlackUserId(userId: string): boolean {
  return userId.startsWith("username:");
}

function channelHumanLabelForSession(session: SlackSessionRecord, inbound: readonly PersistedInboundMessage[]): string | undefined {
  if (session.channelName) {
    return formatSlackChannelName(session.channelName);
  }

  const channelName = inbound.map((message: PersistedInboundMessage) => readStringField(message.slackMessage, "channel_name")).find((value) => value);
  if (channelName) {
    return formatSlackChannelName(channelName);
  }

  const channelType = session.channelType ?? inbound.find((message) => message.channelType)?.channelType;
  if (channelType === "im") {
    return "私信";
  }
  if (channelType === "mpim") {
    return "群聊";
  }
  return undefined;
}

function formatSlackChannelName(channelName: string): string {
  return channelName.startsWith("#") ? channelName : `#${channelName}`;
}

function buildSlackThreadUrl(channelId: string, rootThreadTs: string): string {
  const params = new URLSearchParams({
    channel: channelId,
    message_ts: rootThreadTs,
  });
  return `https://slack.com/app_redirect?${params.toString()}`;
}

function readStringField(value: JsonLike | undefined, key: string): string | undefined {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return undefined;
  }
  const candidate = value[key];
  return typeof candidate === "string" && candidate.trim() ? candidate.trim() : undefined;
}

function readNestedBoolean(value: unknown, path: readonly string[]): boolean {
  let current = value;
  for (const key of path) {
    if (!current || typeof current !== "object" || Array.isArray(current)) {
      return false;
    }
    current = (current as Record<string, unknown>)[key];
  }
  return current === true;
}

function assertSubpathOf(rootPath: string, targetPath: string): void {
  const resolvedRoot = path.resolve(rootPath);
  const resolvedTarget = path.resolve(targetPath);
  const relative = path.relative(resolvedRoot, resolvedTarget);
  if (relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative))) {
    return;
  }
  throw new Error(`Refusing to delete artifact outside managed root: ${resolvedTarget}`);
}

function isAdminCancellableJob(job: PersistedBackgroundJob): boolean {
  return job.status === "registered" || job.status === "running";
}

function normalizeNonEmptyString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function clampPositiveInteger(value: unknown, min: number, max: number): number {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return min;
  }
  return Math.max(min, Math.min(max, Math.floor(number)));
}

function withAdminProbeTimeout<T>(promise: Promise<T>, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return new Promise<T>((resolve, reject) => {
    timer = setTimeout(() => {
      reject(new Error(`${label} timed out after ${ADMIN_RUNTIME_PROBE_TIMEOUT_MS}ms`));
    }, ADMIN_RUNTIME_PROBE_TIMEOUT_MS);
    promise.then(resolve, reject).finally(() => {
      if (timer) {
        clearTimeout(timer);
      }
    });
  });
}

function serializeProbeError(error: unknown): Record<string, unknown> {
  return {
    ok: false,
    error: error instanceof Error ? error.message : String(error),
  };
}
