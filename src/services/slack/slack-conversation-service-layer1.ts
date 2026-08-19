import { readFile } from "node:fs/promises";
import path from "node:path";

import { logger } from "../../logger.js";
import { formatSessionPageLinkMessage } from "../chat/session-page-link-message.js";
import type { BackgroundJobEventPayload, PersistedInboundMessage, ResolvedSlackThreadMessage, SlackInputMessage, SlackSessionRecord, SlackThreadMessage, SlackTurnSignalKind } from "../../types.js";
import type { AgentRuntime, AgentRuntimeEvent } from "../agent-runtime/types.js";
import { isAuthProfileUnavailableError, type AuthProfileUnavailableError } from "../agent-runtime/session-auth-profile-runtime.js";
import { isSlackRateLimitError, type SlackUploadedFile } from "./slack-api.js";
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
import { formatSlackHistoryContextForAgent } from "./slack-message-format.js";
import { markdownishToMrkdwn } from "./slack-mrkdwn.js";
import { planCompletedTurnDisposition } from "./slack-turn-disposition.js";

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

import { SlackConversationServiceBase } from "./slack-conversation-service-base.js";
export class SlackConversationServiceLayer1 extends SlackConversationServiceBase {
  setBotUserId(botUserId: string): void {
    this.privateBotUserId = botUserId;
  }

  async start(): Promise<void> {
    this.privateStopped = false;
    this.privateStartupRecoveryPromise = this.privateRunStartupRecovery();
    void this.privateStartupRecoveryPromise;
  }

  async stop(): Promise<void> {
    this.privateStopped = true;
    this.privateStopActiveTurnReconciler();
    this.privateAgentRuntime.off("event", this.privateAgentRuntimeEventHandler);
    for (const runtime of this.privateRuntimeSessions.values()) {
      if (!runtime.autoResumeTimer) {
        continue;
      }
      clearTimeout(runtime.autoResumeTimer);
      runtime.autoResumeTimer = undefined;
    }
    const stopPromises = [...this.privateStatusControllers.values()].map((controller) => controller.stop());
    this.privateStatusControllers.clear();
    await Promise.all(stopPromises);
  }

  async privateRunStartupRecovery(): Promise<void> {
    try {
      await this.privateReconcilePersistedActiveTurns();
      await this.recoverMissedThreadMessages("socket_ready");
      await this.privateRecoverPendingSessionsOnBoot();
      await this.privateRecoverPendingSyntheticMessages();
    } catch (error) {
      logger.error("Failed to finish Slack startup recovery", {
        error: error instanceof Error ? error.message : String(error),
      });
    } finally {
      this.privateStartupRecoveryPromise = undefined;
      if (!this.privateStopped) {
        this.privateStartActiveTurnReconciler();
      }
    }
  }

  isAlreadyHandled(session: SlackSessionRecord, messageTs?: string | undefined): boolean {
    return this.privateInboundStore.isAlreadyHandled(session, messageTs);
  }

  async ensureAgentSession(session: SlackSessionRecord): Promise<SlackSessionRecord> {
    return await this.privateTurnRunner.ensureAgentSession(session);
  }

  async readThreadHistory(options: { readonly channelId: string; readonly rootThreadTs: string; readonly beforeMessageTs?: string | undefined; readonly limit?: number | undefined; readonly channelType?: string | undefined }): Promise<{
    readonly messages: ResolvedSlackThreadMessage[];
    readonly formattedText?: string | undefined;
    readonly hasMore: boolean;
  }> {
    const effectiveLimit = clampHistoryLimit(options.limit, this.privateConfig.slackInitialThreadHistoryCount, this.privateConfig.slackHistoryApiMaxLimit);

    if (effectiveLimit === 0) {
      return {
        messages: [],
        formattedText: undefined,
        hasMore: false,
      };
    }

    const threadMessages = await this.privateSlackApi.listThreadMessages({
      channelId: options.channelId,
      channelType: options.channelType,
      rootThreadTs: options.rootThreadTs,
    });
    const filteredMessages = threadMessages.filter((message) => !this.privateSelfMessageFilter.shouldIgnoreThreadMessage(message)).filter((message) => isBeforeSlackTs(message.messageTs, options.beforeMessageTs));
    const boundedMessages = filteredMessages.slice(-effectiveLimit);
    const resolvedMessages = await Promise.all(
      boundedMessages.map(async (message) => {
        const metadata = parseSlackTextMetadata(message.text);
        return {
          ...message,
          text: metadata.text,
          mentionedUserIds: metadata.mentionedUserIds,
          mentionedUsers: await Promise.all(metadata.mentionedUserIds.map((userId) => this.privateSlackApi.getUserIdentity(userId))).then((users) => users.filter((user): user is NonNullable<typeof user> => user !== null)),
          sender: message.senderKind === "user" ? await this.privateSlackApi.getUserIdentity(message.userId) : null,
        };
      }),
    );

    return {
      messages: resolvedMessages,
      formattedText: formatSlackHistoryContextForAgent(resolvedMessages),
      hasMore: filteredMessages.length > boundedMessages.length,
    };
  }

  async replayThreadMessage(options: { readonly channelId: string; readonly rootThreadTs: string; readonly messageTs: string }): Promise<SlackInputMessage | null> {
    const session = this.privateSessions.getSession(options.channelId, options.rootThreadTs);
    if (!session) {
      return null;
    }

    const threadMessages = await this.privateSlackApi.listThreadMessages({
      channelId: options.channelId,
      rootThreadTs: options.rootThreadTs,
    });
    const message = threadMessages.find((entry) => entry.messageTs === options.messageTs);

    if (!message || this.privateSelfMessageFilter.shouldIgnoreThreadMessage(message)) {
      return null;
    }

    if (this.isAlreadyHandled(session, message.messageTs)) {
      return null;
    }

    const input = createSlackInputFromThreadMessage("thread_reply", message);
    if (isSlackMessageEffectivelyEmpty(input.text, input.images, input.slackMessage)) {
      return null;
    }

    await this.acceptInboundMessage(session, input);
    return input;
  }

  async resumePendingSession(sessionKey: string): Promise<number> {
    const session = this.privateFindSessionByKey(sessionKey);
    if (session.authBlockedAt) {
      throw new Error(`Session auth is still blocked: ${sessionKey}`);
    }

    return await this.privateResumePendingDispatch(sessionKey, {
      forceReset: true,
    });
  }

  async resetSession(sessionKey: string): Promise<{
    readonly clearedInboundCount: number;
    readonly resetMessageTs: string;
    readonly resumedCount: number;
    readonly interruptedActiveTurn: boolean;
    readonly previousAgentSessionId: string | null;
    readonly previousActiveTurnId: string | null;
    readonly historyMessageCount: number;
    readonly authBlocked: boolean;
  }> {
    const session = this.privateFindSessionByKey(sessionKey);
    const history = await this.readThreadHistory({
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      channelType: session.channelType,
      limit: this.privateConfig.slackHistoryApiMaxLimit,
    });
    const previousAgentSessionId = session.agentSessionId ?? null;
    const previousActiveTurnId = session.activeTurnId ?? null;
    let interruptedActiveTurn = false;

    this.privateResetRuntimeForManualSessionReset(session.key);

    if (session.activeTurnId && session.agentSessionId) {
      try {
        await this.privateTurnRunner.interrupt(session);
        interruptedActiveTurn = true;
      } catch (error) {
        logger.warn("Failed to interrupt active turn during manual session reset", {
          sessionKey: session.key,
          agentSessionId: session.agentSessionId,
          turnId: session.activeTurnId,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }

    const openMessages = this.privateSessions.listInboundMessages({
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      status: ["pending", "inflight"],
    });
    if (openMessages.length > 0) {
      await this.privateSessions.updateInboundMessagesForBatch(
        session.channelId,
        session.rootThreadTs,
        openMessages.map((message) => message.messageTs),
        {
          status: "done",
          batchId: undefined,
        },
      );
    }

    let resetSession = await this.privateSessions.resetSessionRuntimeState(session.key);
    this.privateClearAssistantStatus(resetSession.channelId, resetSession.rootThreadTs);

    const resetMessageTs = createSyntheticMessageTs();
    await this.privateInboundStore.recordInboundMessage(resetSession, {
      source: "admin_session_reset",
      channelId: resetSession.channelId,
      channelType: resetSession.channelType,
      rootThreadTs: resetSession.rootThreadTs,
      messageTs: resetMessageTs,
      userId: this.privateBotUserId || "BROKER_ADMIN",
      senderKind: "app",
      text: "管理员已重置这个 session，丢弃旧 agent history 并从当前 Slack thread 重新开始。",
      contextText: history.formattedText,
    });
    await this.privateAppendSessionResetTrace(resetSession, {
      previousAgentSessionId,
      previousActiveTurnId,
      clearedInboundCount: openMessages.length,
      resetMessageTs,
      historyMessageCount: history.messages.length,
    });

    resetSession = this.privateFindSessionByKey(resetSession.key);
    const authBlocked = Boolean(resetSession.authBlockedAt);
    const resumedCount = authBlocked
      ? 0
      : await this.privateResumePendingDispatch(resetSession.key, {
          forceReset: true,
        });

    return {
      clearedInboundCount: openMessages.length,
      resetMessageTs,
      resumedCount,
      interruptedActiveTurn,
      previousAgentSessionId,
      previousActiveTurnId,
      historyMessageCount: history.messages.length,
      authBlocked,
    };
  }

  async deleteSession(sessionKey: string): Promise<{
    readonly deleted: boolean;
    readonly interruptedActiveTurn: boolean;
    readonly previousAgentSessionId: string | null;
    readonly previousActiveTurnId: string | null;
    readonly clearedInboundCount: number;
    readonly interruptError?: string | undefined;
  }> {
    const session = this.privateFindSessionByKey(sessionKey);
    const previousAgentSessionId = session.agentSessionId ?? null;
    const previousActiveTurnId = session.activeTurnId ?? null;
    let interruptedActiveTurn = false;
    let interruptError: string | undefined;

    this.privateResetRuntimeForManualSessionReset(session.key);

    if (session.activeTurnId && session.agentSessionId) {
      try {
        await this.privateTurnRunner.interrupt(session);
        interruptedActiveTurn = true;
      } catch (error) {
        interruptError = error instanceof Error ? error.message : String(error);
        logger.warn("Failed to interrupt active turn during session delete", {
          sessionKey: session.key,
          agentSessionId: session.agentSessionId,
          turnId: session.activeTurnId,
          error: interruptError,
        });
      }
    }

    const openMessages = this.privateSessions.listInboundMessages({
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      status: ["pending", "inflight"],
    });
    if (openMessages.length > 0) {
      await this.privateSessions.updateInboundMessagesForBatch(
        session.channelId,
        session.rootThreadTs,
        openMessages.map((message) => message.messageTs),
        {
          status: "done",
          batchId: undefined,
        },
      );
    }

    this.privateClearAssistantStatus(session.channelId, session.rootThreadTs);
    await this.privateStatusControllers.get(session.key)?.stop();
    this.privateStatusControllers.delete(session.key);
    this.privateRuntimeSessions.delete(session.key);

    const deleted = await this.privateSessions.deleteSessionByKey(session.key);
    return {
      deleted,
      interruptedActiveTurn,
      previousAgentSessionId,
      previousActiveTurnId,
      clearedInboundCount: openMessages.length,
      interruptError,
    };
  }

  async acceptBackgroundJobEvent(options: { readonly channelId: string; readonly rootThreadTs: string; readonly payload: BackgroundJobEventPayload }): Promise<void> {
    const session = this.privateSessions.getSession(options.channelId, options.rootThreadTs);
    if (!session) {
      throw new Error(`Unknown session for background job event: ${options.channelId}:${options.rootThreadTs}`);
    }

    if (session.lastTurnSignalKind === "final" && session.lastTurnSignalAt && options.payload.jobId) {
      const job = this.privateSessions.getBackgroundJob(options.payload.jobId);
      if (job && compareIsoTimestamp(job.createdAt, session.lastTurnSignalAt) <= 0) {
        logger.info("Ignoring stale background job event after session was finalized", {
          sessionKey: session.key,
          jobId: job.id,
          eventKind: options.payload.eventKind,
          summary: options.payload.summary,
        });
        return;
      }
    }

    await this.acceptInboundMessage(session, {
      source: "background_job_event",
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      messageTs: `${Date.now()}.${Math.floor(Math.random() * 1_000_000)
        .toString()
        .padStart(6, "0")}`,
      userId: this.privateBotUserId || "BACKGROUND_JOB",
      text: options.payload.summary,
      backgroundJob: options.payload,
    });
  }

  async acceptUnexpectedTurnStop(options: { readonly session: SlackSessionRecord; readonly previousTurnId: string; readonly reason: string }): Promise<void> {
    await this.acceptInboundMessage(options.session, {
      source: "unexpected_turn_stop",
      channelId: options.session.channelId,
      rootThreadTs: options.session.rootThreadTs,
      messageTs: `${Date.now()}.${Math.floor(Math.random() * 1_000_000)
        .toString()
        .padStart(6, "0")}`,
      userId: this.privateBotUserId || "BROKER",
      text: options.reason,
      unexpectedTurnStop: {
        turnId: options.previousTurnId,
        reason: options.reason,
      },
    });
  }

  async recoverMissedThreadMessages(reason: "socket_ready" | "periodic"): Promise<void> {
    if (this.privateCatchUpPromise) {
      await this.privateCatchUpPromise;
      return;
    }

    this.privateCatchUpPromise = this.privateRunMissedThreadRecovery(reason)
      .catch((error) => {
        logger.error("Failed to recover missed Slack thread messages", {
          reason,
          error: error instanceof Error ? error.message : String(error),
        });
      })
      .finally(() => {
        this.privateCatchUpPromise = undefined;
      });

    await this.privateCatchUpPromise;
  }

  async postSlackMessage(options: { readonly channelId: string; readonly rootThreadTs: string; readonly text: string; readonly kind?: SlackTurnSignalKind | undefined; readonly reason?: string | undefined }): Promise<void> {
    const formattedText = markdownishToMrkdwn(options.text);
    const chunks = chunkSlackMessage(formattedText);
    for (const [index, chunk] of chunks.entries()) {
      await this.privatePostBotThreadMessage(options.channelId, options.rootThreadTs, chunk, {
        alreadyFormatted: true,
        turnSignal:
          index === 0 && options.kind
            ? {
                kind: options.kind,
                reason: options.reason,
              }
            : undefined,
      });
    }
  }

  async postSlackState(options: { readonly channelId: string; readonly rootThreadTs: string; readonly kind: "wait" | "block" | "final"; readonly reason?: string | undefined }): Promise<void> {
    const session = this.privateSessions.getSession(options.channelId, options.rootThreadTs);
    if (!session) {
      throw new Error(`Unknown session for Slack state update: ${options.channelId}:${options.rootThreadTs}`);
    }

    await this.privateRecordStopSignal(session, {
      kind: options.kind,
      reason: options.reason,
      occurredAt: new Date().toISOString(),
    });
  }

  async postSlackFile(options: {
    readonly channelId: string;
    readonly rootThreadTs: string;
    readonly filePath?: string | undefined;
    readonly contentBase64?: string | undefined;
    readonly filename?: string | undefined;
    readonly title?: string | undefined;
    readonly initialComment?: string | undefined;
    readonly altText?: string | undefined;
    readonly snippetType?: string | undefined;
    readonly contentType?: string | undefined;
  }): Promise<SlackUploadedFile> {
    const hasFilePath = Boolean(options.filePath?.trim());
    const hasInlineContent = Boolean(options.contentBase64?.trim());

    if (hasFilePath === hasInlineContent) {
      throw new Error("Provide exactly one of file_path or content_base64");
    }

    let filename = options.filename?.trim() || undefined;
    let bytes: Uint8Array;

    if (hasFilePath) {
      const filePath = options.filePath!.trim();
      bytes = await readFile(filePath);
      filename ??= path.basename(filePath);
    } else {
      const decoded = Buffer.from(options.contentBase64!.trim(), "base64");
      if (decoded.byteLength === 0) {
        throw new Error("Decoded content_base64 was empty");
      }
      if (!filename) {
        throw new Error("filename is required when using content_base64");
      }
      bytes = decoded;
    }

    if (!filename) {
      throw new Error("Unable to determine filename for Slack upload");
    }

    this.privateClearAssistantStatus(options.channelId, options.rootThreadTs);
    const uploaded = await this.privateSlackApi.uploadThreadFile({
      channelId: options.channelId,
      threadTs: options.rootThreadTs,
      filename,
      bytes,
      title: options.title?.trim() || undefined,
      initialComment: options.initialComment ? markdownishToMrkdwn(options.initialComment.trim()) : undefined,
      altText: options.altText?.trim() || undefined,
      snippetType: options.snippetType?.trim() || undefined,
      contentType: options.contentType?.trim() || undefined,
    });
    await this.privateSessions.setLastSlackReplyAt(options.channelId, options.rootThreadTs, new Date().toISOString());
    return uploaded;
  }

  async privateHandleCompletedTurnDisposition(
    session: SlackSessionRecord,
    turnId: string,
    dispatchMessages: readonly PersistedInboundMessage[],
    options: {
      readonly aborted: boolean;
    },
  ): Promise<SlackSessionRecord> {
    if (options.aborted) {
      return session;
    }

    const latestSession = this.privateFindSessionByKey(session.key);
    const disposition = planCompletedTurnDisposition({
      latestSession,
      turnId,
      dispatchMessages,
      aborted: false,
      hasRunningBackgroundJob: this.privateHasRunningBackgroundJob(latestSession),
      hasPendingUnexpectedStopNudge: this.privateHasPendingUnexpectedStopNudge(latestSession, turnId),
    });
    if (disposition.kind === "none") {
      return latestSession;
    }

    await this.acceptUnexpectedTurnStop({
      session: latestSession,
      previousTurnId: turnId,
      reason: disposition.reason,
    });

    return this.privateFindSessionByKey(latestSession.key);
  }

  privateHasRunningBackgroundJob(session: SlackSessionRecord): boolean {
    return this.privateSessions
      .listBackgroundJobs({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
      })
      .some((job) => job.status === "registered" || job.status === "running");
  }

  privateHasPendingUnexpectedStopNudge(session: SlackSessionRecord, turnId: string): boolean {
    return this.privateSessions
      .listInboundMessages({
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        source: "unexpected_turn_stop",
        status: ["pending", "inflight", "done"],
      })
      .some((message) => message.unexpectedTurnStop?.turnId === turnId);
  }

  privateResolveTurnIdForSignal(session: SlackSessionRecord): string | undefined {
    if (session.activeTurnId) {
      return session.activeTurnId;
    }

    const inflightBatchIds = new Set(
      this.privateSessions
        .listInboundMessages({
          channelId: session.channelId,
          rootThreadTs: session.rootThreadTs,
          status: "inflight",
        })
        .map((message) => message.batchId)
        .filter((batchId): batchId is string => Boolean(batchId)),
    );

    if (inflightBatchIds.size === 1) {
      return [...inflightBatchIds][0];
    }

    return undefined;
  }

  async stopActiveTurn(session: SlackSessionRecord): Promise<boolean> {
    const runtime = this.privateGetRuntimeSession(session.key);
    runtime.queue.length = 0;

    if (!session.activeTurnId || !session.agentSessionId) {
      return false;
    }

    await this.privateTurnRunner.ensureAgentSession(session);
    await this.privateTurnRunner.interrupt(session);
    await this.privateInboundStore.markTurnBatchDone(session, session.activeTurnId);
    await this.privateSessions.setActiveTurnId(session.channelId, session.rootThreadTs, undefined);
    this.privateClearAssistantStatus(session.channelId, session.rootThreadTs);
    return true;
  }

  async acceptInboundMessage(session: SlackSessionRecord, item: SlackInputMessage): Promise<void> {
    if (!item.messageTs) {
      logger.warn("Skipping Slack inbound message without message ts", {
        sessionKey: session.key,
        source: item.source,
        userId: item.userId,
      });
      return;
    }

    this.privateClearDispatchFailureBlock(session.key);
    const coauthoredSession = await this.privateCoauthors.noteIncomingSlackInput(session, item);
    const recordedSession = await this.privateInboundStore.recordInboundMessage(coauthoredSession, item);
    this.privateSetAssistantThinking(recordedSession);
    await this.privateDispatchPersistedMessage(recordedSession, item.messageTs);
  }

  async privateReconcilePersistedActiveTurns(): Promise<void> {
    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .filter((session) => session.activeTurnId)
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    if (sessions.length === 0) {
      return;
    }

    logger.info("Reconciling persisted active Slack sessions", {
      candidateSessionCount: sessions.length,
    });

    let clearedCount = 0;
    let retainedCount = 0;

    for (const session of sessions) {
      const outcome = await this.privateReconcileSingleActiveTurn(session, {
        treatMissingAsStale: true,
        resumePending: false,
      });
      if (outcome === "retained") {
        retainedCount += 1;
      } else {
        clearedCount += 1;
      }
    }

    logger.info("Finished persisted active session reconciliation", {
      clearedCount,
      retainedCount,
    });
  }

  privateStartActiveTurnReconciler(): void {
    this.privateStopActiveTurnReconciler();
    this.privateActiveTurnReconcileTimer = setInterval(() => {
      this.privateRunLiveActiveTurnReconcileOnce();
    }, this.privateConfig.slackActiveTurnReconcileIntervalMs);
  }

  privateStopActiveTurnReconciler(): void {
    if (!this.privateActiveTurnReconcileTimer) {
      return;
    }

    clearInterval(this.privateActiveTurnReconcileTimer);
    this.privateActiveTurnReconcileTimer = undefined;
  }

  privateRunLiveActiveTurnReconcileOnce(): void {
    if (this.privateActiveTurnReconcilePromise) {
      logger.debug("Skipping duplicate active-turn reconcile tick while previous pass is still running");
      return;
    }

    this.privateActiveTurnReconcilePromise = this.privateReconcileLiveActiveTurns()
      .catch((error: unknown) => {
        logger.warn("Failed to finish live active-turn reconciliation pass", {
          error: error instanceof Error ? error.message : String(error),
        });
      })
      .finally(() => {
        this.privateActiveTurnReconcilePromise = undefined;
      });
  }

  async privateReconcileLiveActiveTurns(): Promise<void> {
    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .filter((session) => session.activeTurnId)
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    for (const session of sessions) {
      try {
        await this.privateReconcileSingleActiveTurn(session);
      } catch (error) {
        logger.warn("Failed to reconcile live agent turn state", {
          sessionKey: session.key,
          turnId: session.activeTurnId ?? null,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }

    await this.privateRecoverDormantPendingSessions();

    if (this.privateShouldRunPeriodicMissedThreadRecovery()) {
      await this.recoverMissedThreadMessages("periodic");
    }
  }

  async privateReconcileSingleActiveTurn(
    session: SlackSessionRecord,
    options?: {
      readonly treatMissingAsStale?: boolean | undefined;
      readonly resumePending?: boolean | undefined;
    },
  ): Promise<"cleared" | "retained"> {
    const outcome = await this.privateTurnReconciler.reconcileSingleActiveTurn(session, options);

    if (outcome === "cleared") {
      this.privateResetRuntimeProcessing(session.key);
      const latestSession = this.privateFindSessionByKey(session.key);
      this.privateClearAssistantStatus(latestSession.channelId, latestSession.rootThreadTs);
      if (options?.resumePending ?? true) {
        await this.privateResumePendingDispatch(session.key);
      }
    }

    return outcome;
  }

  async privateRunMissedThreadRecovery(reason: "socket_ready" | "periodic"): Promise<void> {
    const now = Date.now();
    this.privateLastMissedThreadRecoveryAtMs = now;
    if (now < this.privateMissedThreadRecoveryRateLimitUntilMs) {
      logger.info("Skipping Slack missed-message recovery during rate-limit backoff", {
        reason,
        retryInMs: this.privateMissedThreadRecoveryRateLimitUntilMs - now,
      });
      return;
    }

    const sessions = this.privateSessions
      .listSessions()
      .filter(isSlackPlatformSession)
      .filter((session) => shouldAutoRecoverSession(session, now))
      .sort((left, right) => compareIsoTimestamp(right.updatedAt, left.updatedAt));

    if (sessions.length === 0) {
      return;
    }

    logger.info("Checking Slack threads for missed messages", {
      reason,
      candidateSessionCount: sessions.length,
    });

    let recoveredBatchCount = 0;
    let recoveredMessageCount = 0;
    let skippedByRateLimit = false;

    for (let session of sessions) {
      let messages: SlackThreadMessage[];
      try {
        messages = await this.privateSlackApi.listThreadMessages({
          channelId: session.channelId,
          rootThreadTs: session.rootThreadTs,
        });
      } catch (error) {
        if (isSlackRateLimitError(error)) {
          const retryInMs = this.privateDeferMissedThreadRecoveryAfterRateLimit(error.retryAfterMs);
          skippedByRateLimit = true;
          logger.warn("Paused Slack missed-message recovery after Slack rate limit", {
            reason,
            sessionKey: session.key,
            retryInMs,
            retryAfterMs: error.retryAfterMs ?? null,
          });
          break;
        }

        logger.warn("Failed to check Slack thread for missed messages", {
          reason,
          sessionKey: session.key,
          error: error instanceof Error ? error.message : String(error),
        });
        continue;
      }
      const latestPersistedMessageTs = this.privateSessions.getLatestSlackInboundMessageTs(session.channelId, session.rootThreadTs) ?? session.lastObservedMessageTs;
      const missedMessages = messages.filter((message) => !this.privateSelfMessageFilter.shouldIgnoreThreadMessage(message)).filter((message) => isSlackMessageAfterCursor(message.messageTs, latestPersistedMessageTs));

      if (missedMessages.length > 0) {
        logger.warn("Recovering missed Slack thread messages", {
          reason,
          sessionKey: session.key,
          missedCount: missedMessages.length,
          fromTs: latestPersistedMessageTs,
          toTs: missedMessages.at(-1)?.messageTs ?? null,
        });
      }

      for (const message of missedMessages) {
        const input = createSlackInputFromThreadMessage("thread_reply", message);
        if (isSlackMessageEffectivelyEmpty(input.text, input.images, input.slackMessage)) {
          continue;
        }

        session = await this.privateInboundStore.recordInboundMessage(session, input);
      }

      const recovered = await this.privateDispatchPendingRecoveryBatch(session, "missed_thread_messages");
      if (recovered > 0) {
        recoveredBatchCount += 1;
        recoveredMessageCount += recovered;
      }
    }

    if (!skippedByRateLimit) {
      this.privateMissedThreadRecoveryRateLimitBackoffMs = 0;
      this.privateMissedThreadRecoveryRateLimitUntilMs = 0;
    }

    logger.info("Finished Slack missed-message recovery", {
      reason,
      recoveredBatchCount,
      recoveredMessageCount,
      skippedByRateLimit,
    });
  }

  async privatePostBotThreadMessage(
    channelId: string,
    rootThreadTs: string,
    text: string,
    options?: {
      readonly alreadyFormatted?: boolean | undefined;
      readonly turnSignal?:
        | {
            readonly kind: SlackTurnSignalKind;
            readonly reason?: string | undefined;
          }
        | undefined;
    },
  ): Promise<string | undefined> {
    this.privateClearAssistantStatus(channelId, rootThreadTs);
    const formattedText = options?.alreadyFormatted ? text : markdownishToMrkdwn(text);
    const ts = await this.privateSlackApi.postThreadMessage(channelId, rootThreadTs, formattedText);
    if (ts) {
      this.privateSelfMessageFilter.rememberPostedMessageTs(ts);
      const occurredAt = new Date().toISOString();
      const session = await this.privateSessions.setLastSlackReplyAt(channelId, rootThreadTs, occurredAt);
      if (options?.turnSignal?.kind) {
        await this.privateRecordStopSignal(session, {
          kind: options.turnSignal.kind,
          reason: options.turnSignal.reason,
          occurredAt,
        });
      }
    }
    return ts;
  }

  async privatePostSessionPageLinkIfNeeded(session: SlackSessionRecord): Promise<SlackSessionRecord> {
    const latestSession = this.privateFindSessionByKey(session.key);
    if (latestSession.sessionPageLinkPostedAt) {
      return latestSession;
    }

    const inflight = this.privateSessionPageLinkPosts.get(session.key);
    if (inflight) {
      return await inflight;
    }

    const posting = this.privatePostSessionPageLinkOnce(latestSession);
    this.privateSessionPageLinkPosts.set(session.key, posting);
    try {
      return await posting;
    } finally {
      if (this.privateSessionPageLinkPosts.get(session.key) === posting) {
        this.privateSessionPageLinkPosts.delete(session.key);
      }
    }
  }

  async privatePostSessionPageLinkOnce(session: SlackSessionRecord): Promise<SlackSessionRecord> {
    const latestSession = this.privateFindSessionByKey(session.key);
    if (latestSession.sessionPageLinkPostedAt) {
      return latestSession;
    }

    const text = formatSessionPageLinkMessage({
      adminBaseUrl: this.privateConfig.adminBaseUrl,
      session: latestSession,
      githubPrIdentity: this.privateGithubPrIdentity,
      style: "slack_mrkdwn",
    });
    try {
      await this.privatePostBotThreadMessage(latestSession.channelId, latestSession.rootThreadTs, text, { alreadyFormatted: true });
      return await this.privateSessions.setSessionPageLinkPostedAt(latestSession.channelId, latestSession.rootThreadTs, new Date().toISOString());
    } catch (error) {
      logger.warn("Failed to post admin session link into Slack thread", {
        sessionKey: session.key,
        error: error instanceof Error ? error.message : String(error),
      });
      return session;
    }
  }

  async privateRecordStopSignal(
    session: SlackSessionRecord,
    signal: {
      readonly kind: SlackTurnSignalKind;
      readonly reason?: string | undefined;
      readonly occurredAt: string;
    },
  ): Promise<SlackSessionRecord> {
    const turnId = this.privateResolveTurnIdForSignal(session);
    let latestSession = await this.privateSessions.recordTurnSignal(session.channelId, session.rootThreadTs, {
      turnId,
      kind: signal.kind,
      reason: signal.reason,
      occurredAt: signal.occurredAt,
    });

    if (turnId) {
      latestSession = await this.privateInboundStore.markTurnBatchDone(latestSession, turnId);
    }

    this.privateClearAssistantStatus(session.channelId, session.rootThreadTs);
    return latestSession;
  }

  async privateDispatchPersistedMessage(session: SlackSessionRecord, messageTs: string): Promise<void> {
    let latestSession = this.privateFindSessionByKey(session.key);
    const pendingMessage = this.privateSessions.getInboundMessage(latestSession.channelId, latestSession.rootThreadTs, messageTs);

    if (!pendingMessage || pendingMessage.status !== "pending") {
      return;
    }

    latestSession = await this.privatePostSessionPageLinkIfNeeded(latestSession);
    this.privateSetAssistantThinking(latestSession);
    if (latestSession.activeTurnId) {
      try {
        const input = this.privateInboundStore.createSlackInputFromPersistedMessage(pendingMessage);
        const submittedSession = await this.privateSubmitPersistedMessageIntoActiveTurn(latestSession, pendingMessage, input);
        if (submittedSession) {
          latestSession = submittedSession;
          return;
        }
      } catch (error) {
        logger.warn("Failed to deliver persisted Slack message into active agent turn", {
          sessionKey: session.key,
          turnId: latestSession.activeTurnId,
          messageTs,
          error: error instanceof Error ? error.message : String(error),
        });

        if (isMissingActiveTurnInputError(error)) {
          latestSession = await this.privateSyncActiveTurnFromActiveInputError(latestSession, error, {
            messageTs,
          });
        }
      }
    }

    this.privateEnqueueDispatch(latestSession, {
      kind: "dispatch_pending",
    });
  }

  async privateDispatchPendingRecoveryBatch(session: SlackSessionRecord, recoveryKind: "missed_thread_messages"): Promise<number> {
    let latestSession = this.privateFindSessionByKey(session.key);
    const pendingMessages = this.privateInboundStore.listPendingMessages(latestSession, {
      source: ["app_mention", "direct_message", "thread_reply"],
    });

    if (pendingMessages.length === 0) {
      return 0;
    }

    this.privateSetAssistantThinking(latestSession);
    if (latestSession.activeTurnId) {
      try {
        const input = await this.privateInboundStore.createRecoveredBatchInput(latestSession, pendingMessages, recoveryKind);
        if (!input) {
          return 0;
        }

        const submittedSession = await this.privateSubmitPersistedBatchIntoActiveTurn(latestSession, pendingMessages, input);
        if (submittedSession) {
          latestSession = submittedSession;
          return pendingMessages.length;
        }
      } catch (error) {
        logger.warn("Failed to deliver recovered Slack backlog into active agent turn", {
          sessionKey: session.key,
          turnId: latestSession.activeTurnId,
          recoveryKind,
          error: error instanceof Error ? error.message : String(error),
        });

        if (isMissingActiveTurnInputError(error)) {
          latestSession = await this.privateSyncActiveTurnFromActiveInputError(latestSession, error);
        }
      }
    }

    this.privateEnqueueDispatch(latestSession, {
      kind: "dispatch_pending",
      recoveryKind,
    });
    return pendingMessages.length;
  }

  privateEnqueueDispatch(session: SlackSessionRecord, request: PendingDispatchRequest): void {
    const runtime = this.privateGetRuntimeSession(session.key);
    runtime.blockedUntilMs = undefined;
    runtime.blockedFailureFingerprint = undefined;
    const existing = runtime.queue.find((entry: PendingDispatchRequest) => entry.kind === "dispatch_pending");

    if (existing) {
      if (!existing.recoveryKind && request.recoveryKind) {
        runtime.queue.splice(runtime.queue.indexOf(existing), 1, request);
      }
    } else {
      runtime.queue.push(request);
    }

    logger.debug("Queued pending Slack dispatch", {
      sessionKey: session.key,
      recoveryKind: request.recoveryKind ?? null,
      queueLength: runtime.queue.length,
    });

    if (!runtime.processing) {
      void this.privateDrainQueue(session.key);
    }
  }
}
