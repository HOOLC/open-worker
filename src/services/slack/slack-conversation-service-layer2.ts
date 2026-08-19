import { randomUUID } from "node:crypto";

import { buildAdminSessionUrl } from "../../admin-session-url.js";
import { logger } from "../../logger.js";
import { SessionManager } from "../session-manager.js";
import type { PersistedInboundMessage, SlackInputMessage, SlackSessionRecord } from "../../types.js";
import type { AgentRuntimeEvent } from "../agent-runtime/types.js";
import { isAuthProfileUnavailableError, type AuthProfileUnavailableError } from "../agent-runtime/session-auth-profile-runtime.js";
import { isAuthProfileProbeFailureReason } from "../session-auth-profile-selector.js";
import { SlackApi, isSlackRateLimitError, type SlackUploadedFile } from "./slack-api.js";
import { SlackAssistantStatusController } from "./slack-assistant-status.js";
import { createSlackInputFromThreadMessage, isSlackMessageEffectivelyEmpty, parseSlackTextMetadata } from "./slack-event-parser.js";
import {
  chunkSlackMessage,
  clampHistoryLimit,
  compareIsoTimestamp,
  createSyntheticMessageTs,
  createSlackFailureFingerprint,
  formatSlackRunFailureMessage,
  isBeforeSlackTs,
  isMissingAgentSessionError,
  isRecoverableAgentTurnFailure,
  parseActiveTurnMismatch,
  isMissingActiveTurnInputError,
  isSlackMessageAfterCursor,
  shouldResetConflictingActiveTurnMismatch,
  shouldForceResetStaleIdleRuntime,
  shouldPostSlackRunFailure,
  shouldNotifySlackFailure,
  shouldAutoRecoverSession,
  isSlackPlatformSession,
} from "./slack-conversation-utils.js";

interface RuntimeSessionState {
  readonly queue: PendingDispatchRequest[];
  processing: boolean;
  generation: number;
  autoResumeTimer?: NodeJS.Timeout | undefined;
  blockedUntilMs?: number | undefined;
  blockedFailureFingerprint?: string | undefined;
  lastFailureNotificationFingerprint?: string | undefined;
  lastFailureNotificationAtMs?: number | undefined;
}

interface PendingDispatchRequest {
  readonly kind: "dispatch_pending";
  readonly recoveryKind?: "missed_thread_messages" | undefined;
}

const AUTO_RESUME_AFTER_FAILURE_MS = 5_000;
const NONRECOVERABLE_DISPATCH_RETRY_COOLDOWN_MS = 5 * 60 * 1_000;
const MISSED_THREAD_RECOVERY_RATE_LIMIT_MIN_BACKOFF_MS = 60_000;
const MISSED_THREAD_RECOVERY_RATE_LIMIT_MAX_BACKOFF_MS = 10 * 60_000;

import { SlackConversationServiceLayer1 } from "./slack-conversation-service-layer1.js";
export class SlackConversationServiceLayer2 extends SlackConversationServiceLayer1 {
  async privateSubmitPersistedMessageIntoActiveTurn(session: SlackSessionRecord, pendingMessage: PersistedInboundMessage, input: SlackInputMessage): Promise<SlackSessionRecord | null> {
    let latestSession = session;

    for (let attempt = 0; attempt < 2; attempt += 1) {
      if (!latestSession.activeTurnId) {
        return null;
      }

      try {
        await this.privateTurnRunner.submitAdditionalInput(latestSession, input);
        await this.privateInboundStore.markMessagesInflight(latestSession, [pendingMessage], latestSession.activeTurnId);
        logger.debug("Delivered persisted Slack message into active agent turn", {
          sessionKey: session.key,
          turnId: latestSession.activeTurnId,
          source: input.source,
          userId: input.userId,
        });
        return latestSession;
      } catch (error) {
        const syncedSession = isMissingActiveTurnInputError(error)
          ? await this.privateSyncActiveTurnFromActiveInputError(latestSession, error, {
              messageTs: pendingMessage.messageTs,
            })
          : latestSession;
        if (syncedSession.activeTurnId && syncedSession.activeTurnId !== latestSession.activeTurnId) {
          latestSession = syncedSession;
          continue;
        }
        if (!syncedSession.activeTurnId && latestSession.activeTurnId) {
          return null;
        }
        throw error;
      }
    }

    return null;
  }

  async privateSubmitPersistedBatchIntoActiveTurn(session: SlackSessionRecord, pendingMessages: readonly PersistedInboundMessage[], input: SlackInputMessage): Promise<SlackSessionRecord | null> {
    let latestSession = session;

    for (let attempt = 0; attempt < 2; attempt += 1) {
      if (!latestSession.activeTurnId) {
        return null;
      }

      try {
        await this.privateTurnRunner.submitAdditionalInput(latestSession, input);
        await this.privateInboundStore.markMessagesInflight(latestSession, pendingMessages, latestSession.activeTurnId);
        return latestSession;
      } catch (error) {
        const syncedSession = isMissingActiveTurnInputError(error) ? await this.privateSyncActiveTurnFromActiveInputError(latestSession, error) : latestSession;
        if (syncedSession.activeTurnId && syncedSession.activeTurnId !== latestSession.activeTurnId) {
          latestSession = syncedSession;
          continue;
        }
        if (!syncedSession.activeTurnId && latestSession.activeTurnId) {
          return null;
        }
        throw error;
      }
    }

    return null;
  }

  async privateSyncActiveTurnFromActiveInputError(
    session: SlackSessionRecord,
    error: unknown,
    options?: {
      readonly messageTs?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    const mismatch = parseActiveTurnMismatch(error);
    if (mismatch && mismatch.actualTurnId !== session.activeTurnId) {
      const inflightBatchIds = this.privateSessions
        .listInboundMessages({
          channelId: session.channelId,
          rootThreadTs: session.rootThreadTs,
          status: "inflight",
        })
        .map((message) => message.batchId);

      if (shouldResetConflictingActiveTurnMismatch(inflightBatchIds, mismatch.actualTurnId)) {
        logger.warn("Detected conflicting inflight Slack batches during active-turn resync; resetting broker runtime state", {
          sessionKey: session.key,
          previousTurnId: session.activeTurnId,
          actualTurnId: mismatch.actualTurnId,
          inflightBatchIds: [...new Set(inflightBatchIds.filter((batchId): batchId is string => Boolean(batchId)))],
          messageTs: options?.messageTs ?? null,
        });
        await this.privateSessions.resetInflightMessages(session.channelId, session.rootThreadTs);
        const latestSession = await this.privateSessions.setActiveTurnId(session.channelId, session.rootThreadTs, undefined);
        this.privateResetRuntimeProcessing(session.key);
        return latestSession;
      }

      logger.warn("Synchronizing broker active turn id to agent-runtime-reported active turn", {
        sessionKey: session.key,
        previousTurnId: session.activeTurnId,
        actualTurnId: mismatch.actualTurnId,
        messageTs: options?.messageTs ?? null,
      });
      return await this.privateSessions.setActiveTurnId(session.channelId, session.rootThreadTs, mismatch.actualTurnId);
    }

    logger.warn("Detected stale active agent turn; resetting broker runtime state", {
      sessionKey: session.key,
      turnId: session.activeTurnId,
      messageTs: options?.messageTs ?? null,
    });
    await this.privateSessions.resetInflightMessages(session.channelId, session.rootThreadTs, session.activeTurnId);
    const latestSession = await this.privateSessions.setActiveTurnId(session.channelId, session.rootThreadTs, undefined);
    this.privateResetRuntimeProcessing(session.key);
    return latestSession;
  }

  async privateDrainQueue(sessionKey: string): Promise<void> {
    const runtime = this.privateGetRuntimeSession(sessionKey);
    const generation = runtime.generation;
    runtime.processing = true;

    while (runtime.queue.length > 0) {
      if (runtime.generation !== generation) {
        return;
      }

      const next = runtime.queue.shift();
      if (!next) {
        continue;
      }

      let session = this.privateFindSessionByKey(sessionKey);

      try {
        if (session.activeTurnId) {
          runtime.queue.unshift(next);
          break;
        }

        this.privateSetAssistantThinking(session);
        session = await this.privateTurnRunner.ensureAgentSession(session);
        const pendingMessages = this.privateInboundStore.listPendingMessages(session);

        if (pendingMessages.length === 0) {
          continue;
        }

        session = await this.privatePostSessionPageLinkIfNeeded(session);
        const dispatchMessages = next.recoveryKind ? pendingMessages : [pendingMessages[0]!];
        const slackInput = next.recoveryKind ? await this.privateInboundStore.createRecoveredBatchInput(session, dispatchMessages, next.recoveryKind) : this.privateInboundStore.createSlackInputFromPersistedMessage(dispatchMessages[0]!);

        if (!slackInput) {
          continue;
        }

        const input = await this.privateTurnRunner.buildTurnInput(slackInput);
        const turnOutcome = await this.privateTurnRunner.submitInputWithRecovery({
          session,
          sessionKey,
          senderUserId: slackInput.userId,
          input,
          messageTsList: dispatchMessages.map((message) => message.messageTs),
        });

        if (runtime.generation !== generation) {
          return;
        }

        session = turnOutcome.session;
        const result = turnOutcome.result;
        logger.debug("agent turn finished without broker-managed Slack reply forwarding", {
          sessionKey,
          turnId: result.turnId,
          aborted: result.aborted,
          hadFinalMessage: Boolean(result.finalMessage),
        });
        session = await this.privateHandleCompletedTurnDisposition(session, result.turnId, dispatchMessages, {
          aborted: result.aborted,
        });
        this.privateMaybeClearAssistantStatusIfIdle(session);
      } catch (error) {
        if (runtime.generation !== generation) {
          return;
        }

        if (isAuthProfileUnavailableError(error)) {
          await this.privateHandleAuthProfileUnavailable(session, error, runtime);
          break;
        }

        logger.error("Slack conversation turn dispatch failed", {
          sessionKey,
          channelId: session.channelId,
          rootThreadTs: session.rootThreadTs,
          error: error instanceof Error ? error.message : String(error),
        });
        const nowMs = Date.now();
        if (
          shouldNotifySlackFailure({
            previousFingerprint: runtime.lastFailureNotificationFingerprint,
            previousNotifiedAtMs: runtime.lastFailureNotificationAtMs,
            error,
            nowMs,
          })
        ) {
          if (shouldPostSlackRunFailure(error)) {
            await this.privatePostBotThreadMessage(session.channelId, session.rootThreadTs, formatSlackRunFailureMessage(error));
            runtime.lastFailureNotificationFingerprint = createSlackFailureFingerprint(error);
            runtime.lastFailureNotificationAtMs = nowMs;
          } else {
            logger.info("Suppressing recoverable Slack reconnect notification", {
              sessionKey,
              error: error instanceof Error ? error.message : String(error),
            });
          }
        } else {
          logger.warn("Suppressing duplicate Slack run failure notification", {
            sessionKey,
            error: error instanceof Error ? error.message : String(error),
          });
        }
        await this.privateSessions.setActiveTurnId(session.channelId, session.rootThreadTs, undefined);
        if (isRecoverableAgentTurnFailure(error) || isMissingActiveTurnInputError(error) || isMissingAgentSessionError(error)) {
          this.privateScheduleAutoResume(session.key);
        } else {
          runtime.blockedUntilMs = nowMs + NONRECOVERABLE_DISPATCH_RETRY_COOLDOWN_MS;
          runtime.blockedFailureFingerprint = createSlackFailureFingerprint(error);
          logger.warn("Pausing automatic retries for a session after non-recoverable dispatch failure", {
            sessionKey,
            blockedUntilMs: runtime.blockedUntilMs,
            error: error instanceof Error ? error.message : String(error),
          });
        }
        this.privateClearAssistantStatus(session.channelId, session.rootThreadTs);
        break;
      }
    }

    if (runtime.generation === generation) {
      runtime.processing = false;
    }
  }

  privateGetRuntimeSession(sessionKey: string): RuntimeSessionState {
    let runtime = this.privateRuntimeSessions.get(sessionKey);

    if (!runtime) {
      runtime = {
        queue: [],
        processing: false,
        generation: 0,
      };
      this.privateRuntimeSessions.set(sessionKey, runtime);
    }

    return runtime;
  }

  privateResetRuntimeForManualSessionReset(sessionKey: string): void {
    const runtime = this.privateGetRuntimeSession(sessionKey);
    if (runtime.autoResumeTimer) {
      clearTimeout(runtime.autoResumeTimer);
      runtime.autoResumeTimer = undefined;
    }
    runtime.queue.length = 0;
    runtime.processing = false;
    runtime.generation += 1;
    runtime.blockedUntilMs = undefined;
    runtime.blockedFailureFingerprint = undefined;
    runtime.lastFailureNotificationFingerprint = undefined;
    runtime.lastFailureNotificationAtMs = undefined;
  }

  privateFindSessionByKey(sessionKey: string): SlackSessionRecord {
    const session = this.privateSessions.listSessions().find((entry) => entry.key === sessionKey);
    if (!session) {
      throw new Error(`Unknown session runtime key: ${sessionKey}`);
    }

    return session;
  }

  privateResetRuntimeProcessing(sessionKey: string): void {
    const runtime = this.privateGetRuntimeSession(sessionKey);
    runtime.generation += 1;
    runtime.processing = false;
  }

  privateClearDispatchFailureBlock(sessionKey: string): void {
    const runtime = this.privateGetRuntimeSession(sessionKey);
    runtime.blockedUntilMs = undefined;
    runtime.blockedFailureFingerprint = undefined;
  }

  async privateAppendSessionResetTrace(
    session: SlackSessionRecord,
    detail: {
      readonly previousAgentSessionId: string | null;
      readonly previousActiveTurnId: string | null;
      readonly clearedInboundCount: number;
      readonly resetMessageTs: string;
      readonly historyMessageCount: number;
    },
  ): Promise<void> {
    const now = new Date().toISOString();
    const sequence = this.privateSessions.listAgentTraceEvents(session.key, 10_000).length + 1;
    await this.privateSessions.upsertAgentTraceEvent({
      id: randomUUID(),
      sessionKey: session.key,
      source: "broker",
      type: "agent_session_reset",
      at: now,
      sequence,
      title: "Session 已重置",
      summary: "已清空 agent history 并重新唤起 bot",
      detail: JSON.stringify(detail, null, 2),
      status: "completed",
      role: "system",
      metadata: detail,
      createdAt: now,
      updatedAt: now,
    });
  }

  privateScheduleAutoResume(sessionKey: string): void {
    const runtime = this.privateGetRuntimeSession(sessionKey);
    if (runtime.autoResumeTimer) {
      return;
    }

    runtime.autoResumeTimer = setTimeout(() => {
      runtime.autoResumeTimer = undefined;
      void this.privateResumePendingDispatch(sessionKey, {
        forceReset: true,
      }).catch((error) => {
        logger.warn("Failed to auto-resume pending Slack dispatch after recoverable turn failure", {
          sessionKey,
          error: error instanceof Error ? error.message : String(error),
        });
      });
    }, AUTO_RESUME_AFTER_FAILURE_MS);

    logger.warn("Scheduled automatic retry for pending Slack dispatch after recoverable turn failure", {
      sessionKey,
      delayMs: AUTO_RESUME_AFTER_FAILURE_MS,
    });
  }

  async privateResumePendingDispatch(
    sessionKey: string,
    options?: {
      readonly forceReset?: boolean | undefined;
    },
  ): Promise<number> {
    const session = this.privateSessions.listSessions().find((entry) => entry.key === sessionKey);
    if (!session) {
      return 0;
    }

    const pendingMessages = this.privateInboundStore.listPendingMessages(session);
    if (pendingMessages.length === 0) {
      return 0;
    }

    if (!session.activeTurnId && options?.forceReset) {
      logger.warn("Force-resetting broker runtime state before resuming pending Slack dispatch", {
        sessionKey,
        pendingCount: pendingMessages.length,
      });
      this.privateResetRuntimeProcessing(sessionKey);
    }

    this.privateSetAssistantThinking(session);
    this.privateEnqueueDispatch(session, {
      kind: "dispatch_pending",
    });

    return pendingMessages.length;
  }

  async privateRecoverPendingSessionsOnBoot(): Promise<void> {
    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .filter((session) => !session.activeTurnId)
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    let resumedSessionCount = 0;
    let resumedMessageCount = 0;
    let orphanedInflightDoneCount = 0;
    let orphanedInflightResetCount = 0;

    for (const session of sessions) {
      const latestSession = this.privateFindSessionByKey(session.key);
      if (latestSession.activeTurnId) {
        continue;
      }

      const reconciled = await this.privateInboundStore.reconcileOrphanedInflightMessages(latestSession);
      orphanedInflightDoneCount += reconciled.markedDoneCount;
      orphanedInflightResetCount += reconciled.resetToPendingCount;

      const runtime = this.privateGetRuntimeSession(session.key);
      if (runtime.processing || runtime.queue.some((entry) => entry.kind === "dispatch_pending")) {
        continue;
      }

      const refreshedSession = this.privateFindSessionByKey(session.key);
      if (refreshedSession.activeTurnId) {
        continue;
      }
      const resumedCount = await this.privateResumePendingDispatch(refreshedSession.key);

      if (resumedCount === 0) {
        continue;
      }

      resumedSessionCount += 1;
      resumedMessageCount += resumedCount;
    }

    if (resumedSessionCount > 0) {
      logger.warn("Recovered pending Slack dispatch backlog during broker startup", {
        resumedSessionCount,
        resumedMessageCount,
        orphanedInflightDoneCount,
        orphanedInflightResetCount,
      });
    } else if (orphanedInflightDoneCount > 0 || orphanedInflightResetCount > 0) {
      logger.warn("Reconciled orphaned inflight Slack messages during broker startup", {
        orphanedInflightDoneCount,
        orphanedInflightResetCount,
      });
    }
  }

  async privateRecoverDormantPendingSessions(): Promise<void> {
    const nowMs = Date.now();
    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .filter((session) => !session.activeTurnId)
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    for (const session of sessions) {
      const runtime = this.privateGetRuntimeSession(session.key);
      const openMessages = this.privateSessions.listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        status: ["pending", "inflight"],
      });

      const latestOpenMessageUpdatedAt = openMessages
        .map((message) => message.updatedAt)
        .filter(Boolean)
        .sort(compareIsoTimestamp)
        .at(-1);

      if (
        shouldForceResetStaleIdleRuntime({
          activeTurnId: session.activeTurnId,
          runtimeProcessing: runtime.processing,
          latestOpenMessageUpdatedAt,
          nowMs,
          staleAfterMs: this.privateConfig.slackActiveTurnReconcileIntervalMs,
        })
      ) {
        logger.warn("Force-resetting stale idle Slack runtime state", {
          sessionKey: session.key,
          latestOpenMessageUpdatedAt,
          openMessageCount: openMessages.length,
        });
        this.privateResetRuntimeProcessing(session.key);
      }

      if (runtime.processing) {
        continue;
      }
      const authBlocked = Boolean(session.authBlockedAt);

      const reconciled = await this.privateInboundStore.reconcileOrphanedInflightMessages(session);
      if (reconciled.markedDoneCount > 0 || reconciled.resetToPendingCount > 0) {
        logger.warn("Reconciled orphaned inflight Slack messages for idle session", {
          sessionKey: session.key,
          markedDoneCount: reconciled.markedDoneCount,
          resetToPendingCount: reconciled.resetToPendingCount,
        });
      }

      if (!authBlocked && runtime.blockedUntilMs && runtime.blockedUntilMs > nowMs) {
        continue;
      }

      await this.privateResumePendingDispatch(session.key);
    }
  }

  async privateRecoverPendingSyntheticMessages(): Promise<void> {
    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    for (const session of sessions) {
      const pendingSyntheticMessages = this.privateSessions.listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        status: "pending",
        source: "background_job_event",
      });

      for (const message of pendingSyntheticMessages) {
        await this.privateDispatchPersistedMessage(session, message.messageTs);
      }
    }
  }

  privateShouldRunPeriodicMissedThreadRecovery(): boolean {
    const intervalMs = Math.max(this.privateConfig.slackMissedThreadRecoveryIntervalMs, this.privateConfig.slackActiveTurnReconcileIntervalMs);
    return Date.now() - this.privateLastMissedThreadRecoveryAtMs >= intervalMs;
  }

  privateDeferMissedThreadRecoveryAfterRateLimit(retryAfterMs: number | undefined): number {
    const nextExponentialBackoffMs = this.privateMissedThreadRecoveryRateLimitBackoffMs > 0 ? Math.min(this.privateMissedThreadRecoveryRateLimitBackoffMs * 2, MISSED_THREAD_RECOVERY_RATE_LIMIT_MAX_BACKOFF_MS) : MISSED_THREAD_RECOVERY_RATE_LIMIT_MIN_BACKOFF_MS;
    const requestedBackoffMs = retryAfterMs ?? 0;
    const nextBackoffMs = Math.min(Math.max(nextExponentialBackoffMs, requestedBackoffMs, MISSED_THREAD_RECOVERY_RATE_LIMIT_MIN_BACKOFF_MS), MISSED_THREAD_RECOVERY_RATE_LIMIT_MAX_BACKOFF_MS);
    this.privateMissedThreadRecoveryRateLimitBackoffMs = nextBackoffMs;
    this.privateMissedThreadRecoveryRateLimitUntilMs = Date.now() + nextBackoffMs;
    return nextBackoffMs;
  }

  privateSetAssistantThinking(session: SlackSessionRecord): void {
    this.privateGetStatusController(session.channelId, session.rootThreadTs).setThinking();
  }

  privateClearAssistantStatus(channelId: string, rootThreadTs: string): void {
    const sessionKey = SessionManager.createKey(channelId, rootThreadTs);
    this.privateStatusControllers.get(sessionKey)?.clear();
  }

  privateMaybeClearAssistantStatusIfIdle(session: SlackSessionRecord): void {
    if (session.activeTurnId) {
      return;
    }

    const hasPendingOrInflightMessages =
      this.privateSessions.listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        status: ["pending", "inflight"],
      }).length > 0;

    if (hasPendingOrInflightMessages) {
      return;
    }

    this.privateClearAssistantStatus(session.channelId, session.rootThreadTs);
  }

  privateGetStatusController(channelId: string, rootThreadTs: string): SlackAssistantStatusController {
    const sessionKey = SessionManager.createKey(channelId, rootThreadTs);
    let controller = this.privateStatusControllers.get(sessionKey);

    if (!controller) {
      controller = new SlackAssistantStatusController({
        slackApi: this.privateSlackApi,
        channelId,
        threadTs: rootThreadTs,
      });
      this.privateStatusControllers.set(sessionKey, controller);
    }

    return controller;
  }

  privateHandleAgentRuntimeEvent(event: AgentRuntimeEvent): void {
    void this.privateTraceRecorder.record(event).catch((error: unknown) => {
      logger.warn("Failed to persist agent runtime trace event", {
        type: event.type,
        error: error instanceof Error ? error.message : String(error),
      });
    });

    const sessionKey = "brokerSessionKey" in event ? event.brokerSessionKey : undefined;
    const session = sessionKey ? this.privateSessions.getSessionByKey(sessionKey) : undefined;
    if (!session) {
      return;
    }

    const controller = this.privateGetStatusController(session.channelId, session.rootThreadTs);
    switch (event.type) {
      case "agent.tool.started":
        controller.handleToolStart({
          name: event.name,
          callId: event.callId,
          turnId: event.turnId,
        });
        return;
      case "agent.tool.completed":
        controller.handleToolEnd({
          name: event.name,
          callId: event.callId,
          turnId: event.turnId,
          status: event.status,
        });
        return;
      case "agent.message.delta":
      case "agent.message.completed":
      case "agent.turn.completed":
      case "agent.error":
        controller.clear();
        return;
      default:
        return;
    }
  }

  async privateHandleAuthProfileUnavailable(session: SlackSessionRecord, error: AuthProfileUnavailableError, runtime: RuntimeSessionState): Promise<void> {
    if (isAuthProfileProbeFailureReason(error.reason)) {
      await this.privateHandleAuthProfileProbeFailure(session, error, runtime);
      return;
    }

    logger.warn("Pausing Slack session until a human switches auth profile", {
      sessionKey: session.key,
      profileName: error.profileName ?? null,
      reason: error.reason,
    });

    runtime.blockedUntilMs = Number.MAX_SAFE_INTEGER;
    runtime.blockedFailureFingerprint = `auth:${error.profileName ?? "none"}:${error.reason}`;
    const alreadyBlocked = Boolean(session.authBlockedAt);
    const blockedAt = new Date().toISOString();
    let blockedSession = await this.privateSessions.markSessionAuthBlocked(session.key, {
      reason: error.reason,
      blockedAt,
    });
    blockedSession = await this.privateSessions.setActiveTurnId(blockedSession.channelId, blockedSession.rootThreadTs, undefined);
    if (!alreadyBlocked) {
      await this.privateRecordAuthBlockedTrace(blockedSession, error, blockedAt);
    }

    if (!blockedSession.authBlockedNoticePostedAt) {
      const url = buildAdminSessionUrl(this.privateConfig.adminBaseUrl, blockedSession.key);
      const postedAt = new Date().toISOString();
      await this.privatePostBotThreadMessage(blockedSession.channelId, blockedSession.rootThreadTs, [`当前会话绑定的账号额度不可用：${error.userMessage}`, `<${url}|打开 Session 页面手动切换账号并继续处理>`].join("\n"), {
        alreadyFormatted: true,
        turnSignal: {
          kind: "block",
          reason: error.reason,
        },
      });
      await this.privateSessions.setSessionAuthBlockedNoticePostedAt(blockedSession.key, postedAt);
    }

    this.privateClearAssistantStatus(blockedSession.channelId, blockedSession.rootThreadTs);
  }

  async privateHandleAuthProfileProbeFailure(session: SlackSessionRecord, error: AuthProfileUnavailableError, runtime: RuntimeSessionState): Promise<void> {
    logger.warn("Deferring Slack session until auth profile status can be read", {
      sessionKey: session.key,
      profileName: error.profileName ?? null,
      reason: error.reason,
    });

    runtime.blockedUntilMs = undefined;
    runtime.blockedFailureFingerprint = `auth-probe:${error.profileName ?? "none"}:${error.reason}`;
    const latestSession = session.authBlockedAt && isAuthProfileProbeFailureReason(session.authBlockReason) ? await this.privateSessions.clearSessionAuthBlock(session.key) : session;
    await this.privateSessions.setActiveTurnId(latestSession.channelId, latestSession.rootThreadTs, undefined);
    this.privateScheduleAutoResume(latestSession.key);
    this.privateClearAssistantStatus(latestSession.channelId, latestSession.rootThreadTs);
  }

  async privateRecordAuthBlockedTrace(session: SlackSessionRecord, error: AuthProfileUnavailableError, at: string): Promise<void> {
    const existingCount = this.privateSessions.listAgentTraceEvents(session.key, 10_000).length;
    await this.privateSessions.upsertAgentTraceEvent({
      id: randomUUID(),
      sessionKey: session.key,
      source: "broker",
      type: "agent_runtime_error",
      at,
      sequence: existingCount + 1,
      title: "Auth 不可用",
      summary: `等待人工切换账号：${error.userMessage}`,
      detail: JSON.stringify(
        {
          profileName: error.profileName ?? null,
          reason: error.reason,
        },
        null,
        2,
      ),
      status: "blocked",
      metadata: {
        profileName: error.profileName ?? null,
        reason: error.reason,
      },
      createdAt: at,
      updatedAt: at,
    });
  }
}
