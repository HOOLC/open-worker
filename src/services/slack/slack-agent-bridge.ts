import type { AppConfig } from "../../config.js";
import { logger } from "../../logger.js";
import type { BackgroundJobEventPayload, JsonLike, PersistedInboundMessage, ResolvedSlackThreadMessage, SlackSessionRecord, SlackUserIdentity } from "../../types.js";
import type { AgentRuntime } from "../agent-runtime/types.js";
import type { ChatOutboundFile, ChatOutboundMessage, ChatPlatform, ChatUploadedFile } from "../chat/chat-types.js";
import type { FeishuCodexBridge } from "../feishu/feishu-codex-bridge.js";
import type { GitHubPrIdentityService, GitHubPrTokenResolution } from "../github-pr-identity-service.js";
import { SessionManager } from "../session-manager.js";
import type { SessionChannelMetadata } from "../session-manager.js";
import { type ParsedSlackEvent, isSlackMessageEffectivelyEmpty, parseSlackEvent } from "./slack-event-parser.js";
import { SlackApi, slackApiOptions } from "./slack-api.js";
import { SlackCoauthorService } from "./slack-coauthor-service.js";
import { SlackConversationService } from "./slack-conversation-service.js";
import { isSlackPlatformSession } from "./slack-conversation-utils.js";
import { SlackSelfMessageFilter } from "./slack-self-filter.js";
import { SpoolStore } from "../../store/spool-store.js";

export class SlackAgentBridge {
  readonly #config: AppConfig;
  readonly #sessions: SessionManager;
  readonly #agentRuntime: AgentRuntime;
  readonly #slackApi: SlackApi;
  readonly #spool: SpoolStore;
  readonly #spoolOwner = `worker-${process.pid}`;
  readonly #selfMessageFilter = new SlackSelfMessageFilter();
  readonly #coauthors: SlackCoauthorService;
  readonly #githubPrIdentity: GitHubPrIdentityService;
  readonly #conversations: SlackConversationService;
  readonly #feishuBridge?: FeishuCodexBridge | undefined;
  #botUserId = "";
  #botIdentity: SlackUserIdentity | null = null;
  #slackEventDrainPromise: Promise<void> | undefined;
  #slackEventDrainTimer: NodeJS.Timeout | undefined;
  #slackEventRetryTimer: NodeJS.Timeout | undefined;
  #spoolDrainPromise: Promise<void> | undefined;
  #spoolDrainTimer: NodeJS.Timeout | undefined;

  constructor(options: { readonly config: AppConfig; readonly sessions: SessionManager; readonly agentRuntime: AgentRuntime; readonly githubPrIdentity: GitHubPrIdentityService; readonly feishuBridge?: FeishuCodexBridge | undefined }) {
    this.#config = options.config;
    this.#sessions = options.sessions;
    this.#agentRuntime = options.agentRuntime;
    this.#slackApi = new SlackApi(slackApiOptions(this.#config));
    this.#spool = new SpoolStore(this.#config.stateDir);
    this.#coauthors = new SlackCoauthorService({
      sessions: this.#sessions,
      slackApi: this.#slackApi,
      githubPrIdentity: options.githubPrIdentity,
    });
    this.#githubPrIdentity = options.githubPrIdentity;
    this.#feishuBridge = options.feishuBridge;
    this.#conversations = new SlackConversationService({
      config: this.#config,
      sessions: this.#sessions,
      agentRuntime: this.#agentRuntime,
      slackApi: this.#slackApi,
      selfMessageFilter: this.#selfMessageFilter,
      coauthors: this.#coauthors,
      githubPrIdentity: this.#githubPrIdentity,
    });
  }

  async start(): Promise<void> {
    await this.#agentRuntime.start();

    const auth = await this.#slackApi.authTest();
    this.#botUserId = auth.userId;
    this.#selfMessageFilter.setIdentity(auth);
    this.#conversations.setBotUserId(auth.userId);

    this.#botIdentity = await this.#slackApi.getUserIdentity(this.#botUserId);
    this.#agentRuntime.setSlackBotIdentity(this.#botIdentity);

    await this.#backfillSessionChannelMetadata("startup");
    await this.#backfillInboundMentionedUsers("startup");
    await this.#conversations.start();
    await this.#startFeishuBridge();
    await this.#drainPersistedSlackEvents("startup");
    await this.#spool.load();
    this.#startSpoolDrainLoop();
    void this.#conversations.recoverMissedThreadMessages("socket_ready");
    logger.info("chat.platform.ready", {
      platform: "slack",
      source: "spool",
    });
  }

  async stop(): Promise<void> {
    this.#clearSlackEventDrainTimer();
    this.#clearSlackEventRetryTimer();
    this.#stopSpoolDrainLoop();
    this.#spool.close();
    await this.#feishuBridge?.stop();
    await this.#conversations.stop();
    await this.#agentRuntime.stop();
  }

  async readThreadHistory(options: { readonly channelId: string; readonly rootThreadTs: string; readonly beforeMessageTs?: string | undefined; readonly limit?: number | undefined; readonly channelType?: string | undefined }): Promise<{
    readonly messages: readonly ResolvedSlackThreadMessage[];
    readonly formattedText?: string | undefined;
    readonly hasMore: boolean;
  }> {
    return await this.#conversations.readThreadHistory(options);
  }

  async replayThreadMessage(options: { readonly channelId: string; readonly rootThreadTs: string; readonly messageTs: string }) {
    return await this.#conversations.replayThreadMessage(options);
  }

  async resumePendingSession(sessionKey: string): Promise<number> {
    return await this.#conversations.resumePendingSession(sessionKey);
  }

  async resetSession(sessionKey: string) {
    return await this.#conversations.resetSession(sessionKey);
  }

  async deleteSession(sessionKey: string) {
    return await this.#conversations.deleteSession(sessionKey);
  }

  async acceptBackgroundJobEvent(options: {
    readonly platform?: ChatPlatform | undefined;
    readonly conversationId?: string | undefined;
    readonly rootMessageId?: string | undefined;
    readonly channelId?: string | undefined;
    readonly rootThreadTs?: string | undefined;
    readonly payload: BackgroundJobEventPayload;
  }): Promise<void> {
    if (options.platform === "feishu") {
      await this.#requireFeishuBridge().acceptBackgroundJobEvent({
        conversationId: options.conversationId ?? options.channelId ?? "",
        rootMessageId: options.rootMessageId ?? options.rootThreadTs ?? "",
        payload: options.payload,
      });
      return;
    }

    const channelId = options.channelId ?? options.conversationId;
    const rootThreadTs = options.rootThreadTs ?? options.rootMessageId;
    if (!channelId || !rootThreadTs) {
      throw new Error("Slack background job event requires channelId and rootThreadTs");
    }

    await this.#conversations.acceptBackgroundJobEvent({
      channelId,
      rootThreadTs,
      payload: options.payload,
    });
  }

  async readChatThreadHistory(options: { readonly platform: ChatPlatform; readonly conversationId: string; readonly rootMessageId: string; readonly beforeMessageId?: string | undefined; readonly beforeCursor?: string | undefined; readonly limit?: number | undefined }) {
    if (options.platform === "feishu") {
      return await this.#requireFeishuBridge().readChatThreadHistory(options);
    }

    const history = await this.readThreadHistory({
      channelId: options.conversationId,
      rootThreadTs: options.rootMessageId,
      beforeMessageTs: options.beforeMessageId ?? options.beforeCursor,
      limit: options.limit,
    });
    return {
      ...history,
      nextCursor: undefined,
    };
  }

  async postSlackMessage(options: { readonly channelId: string; readonly rootThreadTs: string; readonly text: string; readonly kind?: "progress" | "final" | "block" | "wait" | undefined; readonly reason?: string | undefined }): Promise<void> {
    await this.#conversations.postSlackMessage(options);
  }

  async postSlackState(options: { readonly channelId: string; readonly rootThreadTs: string; readonly kind: "wait" | "block" | "final"; readonly reason?: string | undefined }): Promise<void> {
    await this.#conversations.postSlackState(options);
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
  }) {
    return await this.#conversations.postSlackFile(options);
  }

  async postChatMessage(options: {
    readonly platform: ChatPlatform;
    readonly conversationId: string;
    readonly rootMessageId: string;
    readonly text: string;
    readonly format?: ChatOutboundMessage["format"] | undefined;
    readonly kind?: ChatOutboundMessage["kind"] | undefined;
    readonly reason?: string | undefined;
    readonly richText?: ChatOutboundMessage["richText"] | undefined;
    readonly card?: ChatOutboundMessage["card"] | undefined;
  }) {
    if (options.platform === "feishu") {
      return await this.#requireFeishuBridge().postChatMessage(options);
    }

    await this.postSlackMessage({
      channelId: options.conversationId,
      rootThreadTs: options.rootMessageId,
      text: options.text,
      kind: options.kind,
      reason: options.reason,
    });
    logger.info("chat.outbound.posted", {
      platform: "slack",
      sessionKey: `${options.conversationId}:${options.rootMessageId}`,
      conversationId: options.conversationId,
      rootMessageId: options.rootMessageId,
      format: options.format ?? "text",
      durationMs: 0,
    });
    return {
      platform: "slack" as const,
      conversationId: options.conversationId,
      rootMessageId: options.rootMessageId,
    };
  }

  async postChatState(options: { readonly platform: ChatPlatform; readonly conversationId: string; readonly rootMessageId: string; readonly kind: "wait" | "block" | "final"; readonly reason?: string | undefined }): Promise<void> {
    if (options.platform === "feishu") {
      await this.#requireFeishuBridge().postChatState(options);
      return;
    }

    await this.postSlackState({
      channelId: options.conversationId,
      rootThreadTs: options.rootMessageId,
      kind: options.kind,
      reason: options.reason,
    });
  }

  async postChatFile(
    options: {
      readonly platform: ChatPlatform;
      readonly conversationId: string;
      readonly rootMessageId: string;
    } & ChatOutboundFile,
  ): Promise<ChatUploadedFile | unknown> {
    if (options.platform === "feishu") {
      return await this.#requireFeishuBridge().postChatFile(options);
    }

    const startedAt = Date.now();
    try {
      const uploaded = await this.postSlackFile({
        channelId: options.conversationId,
        rootThreadTs: options.rootMessageId,
        filePath: options.filePath,
        contentBase64: options.contentBase64,
        filename: options.filename,
        title: options.title,
        initialComment: options.initialComment,
        altText: options.altText,
        snippetType: options.snippetType,
        contentType: options.contentType,
      });
      logger.info("chat.outbound.posted", {
        platform: "slack",
        sessionKey: `${options.conversationId}:${options.rootMessageId}`,
        conversationId: options.conversationId,
        rootMessageId: options.rootMessageId,
        format: options.contentType?.startsWith("image/") ? "image" : "file",
        durationMs: Date.now() - startedAt,
      });
      return uploaded;
    } catch (error) {
      logger.warn("chat.outbound.failed", {
        platform: "slack",
        sessionKey: `${options.conversationId}:${options.rootMessageId}`,
        conversationId: options.conversationId,
        rootMessageId: options.rootMessageId,
        format: options.contentType?.startsWith("image/") ? "image" : "file",
        durationMs: Date.now() - startedAt,
        error: error instanceof Error ? error.message : String(error),
      });
      throw error;
    }
  }

  async getCommitCoauthorStatus(cwd: string) {
    return await this.#coauthors.getCommitCoauthorStatus(cwd);
  }

  async configureSessionCoauthors(options: { readonly cwd: string; readonly coauthors?: readonly string[] | undefined; readonly userIds?: readonly string[] | undefined; readonly ignoreMissing?: boolean | undefined; readonly mappings?: ReadonlyArray<unknown> | undefined }) {
    return await this.#coauthors.configureSessionCoauthors(options);
  }

  async resolveCommitCoauthors(options: { readonly cwd: string; readonly commitMessage: string; readonly primaryAuthorEmail?: string | undefined }) {
    const slackResult = await this.#coauthors.resolveCommitCoauthors(options);
    if (slackResult.status !== "noop" || !this.#feishuBridge) {
      return slackResult;
    }

    return await this.#feishuBridge.resolveCommitCoauthors(options);
  }

  async resolveGitHubPrToken(options: { readonly cwd: string; readonly command: readonly string[] }): Promise<GitHubPrTokenResolution> {
    const session = this.#sessions.findSessionByWorkspace(options.cwd);
    if (!session) {
      return {
        ok: false,
        mode: "blocked",
        reason: "session_not_found",
        message: `No Slack session is associated with ${options.cwd}.`,
      };
    }

    return await this.#githubPrIdentity.resolveTokenForSession({
      session,
      command: options.command,
    });
  }

  async #startFeishuBridge(): Promise<void> {
    if (!this.#feishuBridge) {
      return;
    }

    try {
      await this.#feishuBridge.start();
      if (this.#config.feishuGroupMessageMode === "at_only") {
        logger.warn("chat.platform.degraded", {
          platform: "feishu",
          source: "long_connection",
          groupMessageMode: this.#config.feishuGroupMessageMode,
          startupRequired: this.#config.feishuStartupRequired,
          degradedReason: "group_message_all_unavailable",
          permission: "im:message.group_msg",
        });
      } else if (!this.#config.feishuAllMessageDeliveryVerified) {
        logger.warn("chat.platform.degraded", {
          platform: "feishu",
          source: "long_connection",
          groupMessageMode: this.#config.feishuGroupMessageMode,
          startupRequired: this.#config.feishuStartupRequired,
          degradedReason: "all_message_delivery_unverified",
          permission: "im:message.group_msg",
        });
      }
    } catch (error) {
      logger.warn("chat.platform.degraded", {
        platform: "feishu",
        source: "long_connection",
        groupMessageMode: this.#config.feishuGroupMessageMode,
        startupRequired: this.#config.feishuStartupRequired,
        degradedReason: "startup_failed",
        errorClass: error instanceof Error ? error.name : "Error",
      });
      if (this.#config.feishuStartupRequired) {
        throw error;
      }
    }
  }

  #requireFeishuBridge(): FeishuCodexBridge {
    if (!this.#feishuBridge) {
      throw new Error("Feishu bridge is not enabled in this runtime");
    }

    return this.#feishuBridge;
  }

  async #acceptEventsApi(payload: { readonly event?: Record<string, any>; readonly event_id?: string }): Promise<void> {
    if (!payload.event || !payload.event_id) {
      return;
    }

    if (this.#sessions.hasProcessedEvent(payload.event_id)) {
      return;
    }

    await this.#sessions.enqueueSlackEvent(payload.event_id, payload as JsonLike);
    this.#scheduleSlackEventDrain("socket_event");
  }

  #scheduleSlackEventDrain(reason: "socket_event" | "retry"): void {
    if (this.#slackEventDrainTimer) {
      return;
    }
    this.#slackEventDrainTimer = setTimeout(() => {
      this.#slackEventDrainTimer = undefined;
      void this.#drainPersistedSlackEvents(reason);
    }, 0);
    this.#slackEventDrainTimer.unref();
  }

  #clearSlackEventDrainTimer(): void {
    if (!this.#slackEventDrainTimer) {
      return;
    }
    clearTimeout(this.#slackEventDrainTimer);
    this.#slackEventDrainTimer = undefined;
  }

  #scheduleSlackEventRetry(): void {
    if (this.#slackEventRetryTimer) {
      return;
    }
    this.#slackEventRetryTimer = setTimeout(() => {
      this.#slackEventRetryTimer = undefined;
      this.#scheduleSlackEventDrain("retry");
    }, 5_000);
    this.#slackEventRetryTimer.unref();
  }

  #clearSlackEventRetryTimer(): void {
    if (!this.#slackEventRetryTimer) {
      return;
    }
    clearTimeout(this.#slackEventRetryTimer);
    this.#slackEventRetryTimer = undefined;
  }

  #startSpoolDrainLoop(): void {
    if (this.#spoolDrainTimer) {
      return;
    }
    this.#spoolDrainTimer = setInterval(() => {
      void this.#drainInboundSpool();
    }, 50);
    this.#spoolDrainTimer.unref();
    void this.#drainInboundSpool();
  }

  #stopSpoolDrainLoop(): void {
    if (this.#spoolDrainTimer) {
      clearInterval(this.#spoolDrainTimer);
      this.#spoolDrainTimer = undefined;
    }
  }

  async #drainInboundSpool(): Promise<void> {
    if (this.#spoolDrainPromise) {
      await this.#spoolDrainPromise;
      return;
    }

    this.#spoolDrainPromise = this.#runInboundSpoolDrain().finally(() => {
      this.#spoolDrainPromise = undefined;
    });
    await this.#spoolDrainPromise;
  }

  async #runInboundSpoolDrain(): Promise<void> {
    const rows = this.#spool.claim({
      direction: "inbound",
      owner: this.#spoolOwner,
      leaseMs: 30_000,
    });
    for (const row of rows) {
      try {
        await this.#ingestSpoolPayload(row.payload);
        this.#spool.ack(row.id);
      } catch (error) {
        logger.error("Failed to ingest inbound spool row", {
          id: row.id,
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }
  }

  async #ingestSpoolPayload(raw: string): Promise<void> {
    const payload = JSON.parse(raw) as {
      readonly event?: Record<string, any>;
      readonly event_id?: string;
    };
    if (payload.event && payload.event_id) {
      await this.#acceptEventsApi(payload);
      return;
    }

    await this.#handleInteractive(payload);
  }

  async #drainPersistedSlackEvents(reason: "startup" | "socket_event" | "retry"): Promise<void> {
    if (this.#slackEventDrainPromise) {
      await this.#slackEventDrainPromise;
      return;
    }

    this.#slackEventDrainPromise = this.#runSlackEventDrain(reason)
      .catch((error) => {
        logger.error("Failed to drain persisted Slack event queue", {
          reason,
          error: error instanceof Error ? error.message : String(error),
        });
        this.#scheduleSlackEventRetry();
      })
      .finally(() => {
        this.#slackEventDrainPromise = undefined;
      });

    await this.#slackEventDrainPromise;
  }

  async #runSlackEventDrain(reason: "startup" | "socket_event" | "retry"): Promise<void> {
    let failedCount = 0;
    let processedCount = 0;

    while (true) {
      const pendingEvents = this.#sessions.listPendingSlackEvents();
      if (pendingEvents.length === 0) {
        break;
      }

      let batchFailedCount = 0;
      for (const queuedEvent of pendingEvents) {
        const payload = queuedEvent.payload as {
          readonly event?: Record<string, any>;
          readonly event_id?: string;
        };

        if (!payload.event || payload.event_id !== queuedEvent.eventId) {
          await this.#sessions.markSlackEventProcessed(queuedEvent.eventId);
          continue;
        }

        if (this.#sessions.hasProcessedEvent(queuedEvent.eventId)) {
          await this.#sessions.markSlackEventProcessed(queuedEvent.eventId);
          continue;
        }

        try {
          await this.#routeSlackEvent(payload.event);
          await this.#sessions.markSlackEventProcessed(queuedEvent.eventId);
          processedCount += 1;
        } catch (error) {
          failedCount += 1;
          batchFailedCount += 1;
          logger.error("Failed to process persisted Slack event", {
            reason,
            eventId: queuedEvent.eventId,
            error: error instanceof Error ? error.message : String(error),
          });
        }
      }

      if (batchFailedCount > 0) {
        break;
      }
    }

    if (processedCount > 0 || failedCount > 0) {
      logger.info("Drained persisted Slack event queue", {
        reason,
        processedCount,
        failedCount,
      });
    }

    if (failedCount > 0) {
      this.#scheduleSlackEventRetry();
    }
  }

  async #handleInteractive(payload: Record<string, unknown>): Promise<void> {
    try {
      await this.#coauthors.handleInteractivePayload(payload);
    } catch (error) {
      logger.error("Failed to process Slack interactive payload", {
        error: error instanceof Error ? error.message : String(error),
      });
    }
  }

  async #routeSlackEvent(event: Record<string, any>): Promise<void> {
    if (this.#selfMessageFilter.shouldIgnoreEvent(event)) {
      return;
    }

    const parsed = parseSlackEvent(event, this.#botUserId);
    if (!parsed) {
      return;
    }

    logger.info("chat.message.accepted", {
      platform: "slack",
      sessionKey: `${parsed.channelId}:${parsed.rootThreadTs}`,
      conversationId: parsed.channelId,
      rootMessageId: parsed.rootThreadTs,
      messageId: parsed.messageTs,
      route: parsed.route,
    });

    const channelMetadata = await this.#resolveChannelMetadata(parsed);

    switch (parsed.route) {
      case "app_mention":
        await this.#handleInteractiveSessionEvent(parsed, {
          createSession: true,
          preloadHistory: parsed.rootThreadTs !== parsed.messageTs,
          channelMetadata,
        });
        return;
      case "direct_message":
        if (parsed.controlText === "-stop" && (parsed.input.images?.length ?? 0) === 0) {
          const existing = await this.#getSessionWithChannelMetadata(parsed, channelMetadata);
          if (existing) {
            await this.#handleStop(existing);
          }
          return;
        }

        await this.#handleInteractiveSessionEvent(parsed, {
          createSession: true,
          preloadHistory: false,
          channelMetadata,
        });
        return;
      case "thread_reply": {
        const session = await this.#getSessionWithChannelMetadata(parsed, channelMetadata);
        if (!session) {
          return;
        }

        if (this.#conversations.isAlreadyHandled(session, parsed.messageTs)) {
          return;
        }

        if (parsed.controlText === "-stop" && (parsed.input.images?.length ?? 0) === 0) {
          await this.#handleStop(session);
          return;
        }

        if (isSlackMessageEffectivelyEmpty(parsed.input.text, parsed.input.images, parsed.input.slackMessage)) {
          return;
        }

        await this.#conversations.acceptInboundMessage(session, parsed.input);
        return;
      }
      default:
        return;
    }
  }

  async #getSessionWithChannelMetadata(parsed: ParsedSlackEvent, metadata: SessionChannelMetadata): Promise<SlackSessionRecord | undefined> {
    const session = this.#sessions.getSession(parsed.channelId, parsed.rootThreadTs);
    if (!session) {
      return undefined;
    }

    return await this.#sessions.setChannelMetadata(parsed.channelId, parsed.rootThreadTs, metadata);
  }

  async #resolveChannelMetadata(parsed: ParsedSlackEvent): Promise<SessionChannelMetadata> {
    const fallback: SessionChannelMetadata = {
      channelType: parsed.channelType,
    };
    const info = await this.#slackApi.getConversationInfo(parsed.channelId);
    if (!info) {
      return fallback;
    }

    return {
      channelName: info.name,
      channelType: parsed.channelType ?? info.channelType,
    };
  }

  async #backfillSessionChannelMetadata(reason: string): Promise<void> {
    const sessionsByChannel = new Map<string, SlackSessionRecord[]>();
    for (const session of this.#sessions.listSessions()) {
      if (!isSlackPlatformSession(session)) {
        continue;
      }

      if (session.channelName && session.channelType) {
        continue;
      }

      const sessions = sessionsByChannel.get(session.channelId) ?? [];
      sessions.push(session);
      sessionsByChannel.set(session.channelId, sessions);
    }

    if (!sessionsByChannel.size) {
      return;
    }

    let updatedCount = 0;
    for (const [channelId, sessions] of sessionsByChannel.entries()) {
      const info = await this.#slackApi.getConversationInfo(channelId);
      if (!info) {
        continue;
      }

      for (const session of sessions) {
        await this.#sessions.setChannelMetadata(session.channelId, session.rootThreadTs, {
          channelName: info.name,
          channelType: info.channelType,
        });
        updatedCount += 1;
      }
    }

    if (updatedCount) {
      logger.info("Backfilled Slack session channel metadata", {
        reason,
        updatedCount,
        channelCount: sessionsByChannel.size,
      });
    }
  }

  async #backfillInboundMentionedUsers(reason: string): Promise<void> {
    const candidates = this.#sessions.listInboundMessages({
      source: ["app_mention", "direct_message", "thread_reply"],
      needsMentionUserBackfill: true,
    });

    if (!candidates.length) {
      return;
    }

    let updatedCount = 0;
    for (const message of candidates) {
      const mentionedUsers = await this.#resolveMentionedUsers(message);
      if (!mentionedUsers.length) {
        continue;
      }

      await this.#sessions.upsertInboundMessage({
        ...message,
        mentionedUsers,
        updatedAt: new Date().toISOString(),
      });
      updatedCount += 1;
    }

    if (updatedCount) {
      logger.info("Backfilled Slack inbound mention identities", {
        reason,
        updatedCount,
      });
    }
  }

  async #resolveMentionedUsers(message: PersistedInboundMessage): Promise<readonly SlackUserIdentity[]> {
    const mentionedUserIds = message.mentionedUserIds ?? [];
    if (!mentionedUserIds.length) {
      return [];
    }

    const knownUsers = new Map((message.mentionedUsers ?? []).map((user) => [user.userId, user]));

    for (const userId of mentionedUserIds) {
      if (knownUsers.has(userId)) {
        continue;
      }

      const identity = await this.#slackApi.getUserIdentity(userId);
      if (identity) {
        knownUsers.set(userId, identity);
      }
    }

    return mentionedUserIds.map((userId) => knownUsers.get(userId)).filter((user): user is SlackUserIdentity => Boolean(user));
  }

  async #handleInteractiveSessionEvent(
    parsed: ParsedSlackEvent,
    options: {
      readonly createSession: boolean;
      readonly preloadHistory: boolean;
      readonly channelMetadata: SessionChannelMetadata;
    },
  ): Promise<void> {
    const existing = this.#sessions.getSession(parsed.channelId, parsed.rootThreadTs);
    let session = options.createSession
      ? await this.#sessions.ensureSession(parsed.channelId, parsed.rootThreadTs, {
          ...options.channelMetadata,
          ...(parsed.input.senderKind === "user"
            ? {
                initiatorUserId: parsed.input.userId,
                initiatorMessageTs: parsed.messageTs,
              }
            : {}),
        })
      : existing;

    if (!session) {
      return;
    }

    if (!options.createSession) {
      session = await this.#sessions.setChannelMetadata(parsed.channelId, parsed.rootThreadTs, options.channelMetadata);
    }

    if (this.#conversations.isAlreadyHandled(session, parsed.messageTs)) {
      return;
    }

    session = await this.#conversations.ensureAgentSession(session);

    if (isSlackMessageEffectivelyEmpty(parsed.input.text, parsed.input.images, parsed.input.slackMessage)) {
      return;
    }

    const history =
      !existing && options.preloadHistory && parsed.messageTs
        ? await this.#conversations.readThreadHistory({
            channelId: parsed.channelId,
            channelType: parsed.channelType,
            rootThreadTs: parsed.rootThreadTs,
            beforeMessageTs: parsed.messageTs,
            limit: this.#config.slackInitialThreadHistoryCount,
          })
        : undefined;

    await this.#conversations.acceptInboundMessage(session, {
      ...parsed.input,
      contextText: history?.formattedText,
    });
  }

  async #handleStop(session: SlackSessionRecord): Promise<void> {
    const stopped = await this.#conversations.stopActiveTurn(session);
    await this.#conversations.postSlackMessage({
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      text: stopped ? "Stopped the current run." : "No active run to stop.",
    });
  }
}
