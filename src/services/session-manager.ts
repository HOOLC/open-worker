import fs from "node:fs/promises";
import path from "node:path";

import type {
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
  PersistedInboundMessageStatus,
  PersistedInboundSource,
  PersistedSlackEvent,
  SlackSessionRecord,
  SlackTurnSignalKind,
} from "../types.js";
import { StateStore } from "../store/state-store.js";
import { ensureDir } from "../utils/fs.js";
import { type ChatSessionCoordinates, createChatSessionKey, createChatWorkspacePath } from "./chat/chat-session-key.js";
import type { ChatConversationKind, ChatPlatform } from "./chat/chat-types.js";

export interface SessionChannelMetadata {
  readonly channelName?: string | undefined;
  readonly channelType?: string | undefined;
}

export interface SessionInitiatorMetadata {
  readonly initiatorUserId?: string | undefined;
  readonly initiatorMessageTs?: string | undefined;
}

export type EnsureSessionMetadata = SessionChannelMetadata & SessionInitiatorMetadata;

export class SessionManager {
  readonly #stateStore: StateStore;
  readonly #sessionsRoot: string;

  constructor(options: { readonly stateStore: StateStore; readonly sessionsRoot: string }) {
    this.#stateStore = options.stateStore;
    this.#sessionsRoot = options.sessionsRoot;
  }

  static createKey(channelId: string, rootThreadTs: string): string {
    return `${channelId}:${rootThreadTs}`;
  }

  static createChatKey(coordinates: ChatSessionCoordinates): string {
    if (coordinates.platform === "slack") {
      return SessionManager.createKey(coordinates.conversationId, coordinates.rootMessageId);
    }

    return createChatSessionKey(coordinates);
  }

  async load(): Promise<void> {
    await this.#stateStore.load();
    await Promise.all(
      this.#stateStore.listSessions().map(async (session) => {
        await ensureDir(session.workspacePath);
      }),
    );
  }

  getSession(channelId: string, rootThreadTs: string): SlackSessionRecord | undefined {
    return this.#stateStore.getSession(SessionManager.createKey(channelId, rootThreadTs));
  }

  getChatSession(coordinates: ChatSessionCoordinates): SlackSessionRecord | undefined {
    return this.#stateStore.getSession(SessionManager.createChatKey(coordinates));
  }

  listSessions(): SlackSessionRecord[] {
    return this.#stateStore.listSessions();
  }

  getSessionByKey(key: string): SlackSessionRecord | undefined {
    return this.#stateStore.getSession(key);
  }

  async deleteSessionByKey(key: string): Promise<boolean> {
    const session = this.#stateStore.getSession(key);
    if (session) {
      const sessionRoot = this.#resolveSessionRoot(session.workspacePath);
      if (sessionRoot) {
        await fs.rm(sessionRoot, { force: true, recursive: true });
      }
    }
    return await this.#stateStore.deleteSession(key);
  }

  hasProcessedEvent(eventId: string): boolean {
    return this.#stateStore.hasProcessedEvent(eventId);
  }

  async markProcessedEvent(eventId: string): Promise<void> {
    await this.#stateStore.markProcessedEvent(eventId);
  }

  async enqueueSlackEvent(eventId: string, payload: JsonLike): Promise<void> {
    await this.#stateStore.enqueueSlackEvent(eventId, payload);
  }

  listPendingSlackEvents(limit?: number): PersistedSlackEvent[] {
    return this.#stateStore.listPendingSlackEvents(limit);
  }

  async markSlackEventProcessed(eventId: string): Promise<void> {
    await this.#stateStore.markSlackEventProcessed(eventId);
  }

  async ensureSession(channelId: string, rootThreadTs: string, metadata?: EnsureSessionMetadata | undefined): Promise<SlackSessionRecord> {
    const existing = this.getSession(channelId, rootThreadTs);
    if (existing) {
      await ensureDir(existing.workspacePath);
      return await this.#applyChannelMetadata(existing, metadata);
    }

    const workspacePath = this.#createWorkspacePath(channelId, rootThreadTs);
    await ensureDir(workspacePath);

    const record: SlackSessionRecord = {
      key: SessionManager.createKey(channelId, rootThreadTs),
      channelId,
      ...normalizeChannelMetadata(metadata),
      ...normalizeSessionInitiator(metadata),
      rootThreadTs,
      workspacePath,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    await this.#stateStore.upsertSession(record);
    return record;
  }

  async ensureChatSession(
    coordinates: ChatSessionCoordinates,
    options?: {
      readonly conversationKind?: ChatConversationKind | undefined;
      readonly platformThreadId?: string | undefined;
      readonly initiatorUserId?: string | undefined;
      readonly initiatorMessageTs?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    const existing = this.getChatSession(coordinates);
    if (existing) {
      await ensureDir(existing.workspacePath);
      return existing;
    }

    const workspacePath = coordinates.platform === "slack" ? this.#createWorkspacePath(coordinates.conversationId, coordinates.rootMessageId) : createChatWorkspacePath(this.#sessionsRoot, coordinates);
    await ensureDir(workspacePath);

    const now = new Date().toISOString();
    const record: SlackSessionRecord = {
      key: SessionManager.createChatKey(coordinates),
      platform: coordinates.platform,
      conversationId: coordinates.conversationId,
      conversationKind: options?.conversationKind,
      rootMessageId: coordinates.rootMessageId,
      platformThreadId: options?.platformThreadId,
      channelId: coordinates.conversationId,
      ...normalizeSessionInitiator(options),
      rootThreadTs: coordinates.rootMessageId,
      workspacePath,
      createdAt: now,
      updatedAt: now,
    };

    await this.#stateStore.upsertSession(record);
    return record;
  }

  async updateSession(record: SlackSessionRecord): Promise<void> {
    await this.#stateStore.patchSession(record.key, {
      ...record,
      updatedAt: new Date().toISOString(),
    });
  }

  async setChannelMetadata(channelId: string, rootThreadTs: string, metadata: SessionChannelMetadata): Promise<SlackSessionRecord> {
    const session = this.#requireSession(channelId, rootThreadTs);
    return await this.#applyChannelMetadata(session, metadata);
  }

  findSessionByWorkspace(cwd: string): SlackSessionRecord | undefined {
    const targetPath = path.resolve(cwd);
    const candidates = this.listSessions()
      .filter((session) => isSubpathOf(session.workspacePath, targetPath))
      .sort((left, right) => right.workspacePath.length - left.workspacePath.length);

    return candidates[0];
  }

  findSessionByAgentActivity(options: { readonly agentSessionId?: string | undefined; readonly turnId?: string | undefined }): SlackSessionRecord | undefined {
    const sessionKey = this.#stateStore.getSessionKeyForAgentActivity(options);
    return sessionKey ? this.getSessionByKey(sessionKey) : undefined;
  }

  async setAgentSessionId(channelId: string, rootThreadTs: string, agentSessionId: string | undefined): Promise<SlackSessionRecord> {
    const session = await this.#patchSession(channelId, rootThreadTs, {
      agentSessionId,
    });
    if (agentSessionId) {
      await this.#stateStore.bindAgentSession({
        sessionKey: session.key,
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        agentSessionId,
      });
    }
    return session;
  }

  async setChatCodexThreadId(coordinates: ChatSessionCoordinates, codexThreadId: string | undefined): Promise<SlackSessionRecord> {
    const session = await this.#patchChatSession(coordinates, {
      agentSessionId: codexThreadId,
      codexThreadId,
    });
    if (codexThreadId) {
      await this.#stateStore.bindAgentSession({
        sessionKey: session.key,
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        agentSessionId: codexThreadId,
      });
    }
    return session;
  }

  async setActiveTurnId(channelId: string, rootThreadTs: string, activeTurnId: string | undefined): Promise<SlackSessionRecord> {
    const now = new Date().toISOString();
    const session = await this.#patchSession(channelId, rootThreadTs, {
      activeTurnId,
      activeTurnStartedAt: activeTurnId ? now : undefined,
    });
    if (activeTurnId) {
      await this.#stateStore.bindAgentTurn({
        sessionKey: session.key,
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        agentSessionId: session.agentSessionId,
        turnId: activeTurnId,
        at: now,
      });
    }
    return session;
  }

  async setChatActiveTurnId(coordinates: ChatSessionCoordinates, activeTurnId: string | undefined): Promise<SlackSessionRecord> {
    const now = new Date().toISOString();
    const session = await this.#patchChatSession(coordinates, {
      activeTurnId,
      activeTurnStartedAt: activeTurnId ? now : undefined,
    });
    if (activeTurnId) {
      await this.#stateStore.bindAgentTurn({
        sessionKey: session.key,
        channelId: session.channelId,
        rootThreadTs: session.rootThreadTs,
        agentSessionId: session.agentSessionId ?? session.codexThreadId,
        turnId: activeTurnId,
        at: now,
      });
    }
    return session;
  }

  async clearActiveTurnIdIfMatches(channelId: string, rootThreadTs: string, expectedTurnId: string): Promise<SlackSessionRecord> {
    const session = this.#requireSession(channelId, rootThreadTs);
    if (session.activeTurnId !== expectedTurnId) {
      return session;
    }

    return await this.#patchSession(channelId, rootThreadTs, {
      activeTurnId: undefined,
      activeTurnStartedAt: undefined,
    });
  }

  async resetSessionRuntimeState(sessionKey: string): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      agentSessionId: undefined,
      activeTurnId: undefined,
      activeTurnStartedAt: undefined,
      lastTurnSignalTurnId: undefined,
      lastTurnSignalKind: undefined,
      lastTurnSignalReason: undefined,
      lastTurnSignalAt: undefined,
      updatedAt: new Date().toISOString(),
    });
  }

  async setLastObservedMessageTs(channelId: string, rootThreadTs: string, lastObservedMessageTs: string | undefined): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      lastObservedMessageTs,
    });
  }

  async setChatLastObservedMessageTs(coordinates: ChatSessionCoordinates, lastObservedMessageTs: string | undefined): Promise<SlackSessionRecord> {
    return await this.#patchChatSession(coordinates, {
      lastObservedMessageTs,
    });
  }

  async setLastDeliveredMessageTs(channelId: string, rootThreadTs: string, lastDeliveredMessageTs: string | undefined): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      lastDeliveredMessageTs,
    });
  }

  async setLastSlackReplyAt(channelId: string, rootThreadTs: string, lastSlackReplyAt: string | undefined): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      lastSlackReplyAt,
    });
  }

  async setSessionPageLinkPostedAt(channelId: string, rootThreadTs: string, sessionPageLinkPostedAt: string): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      sessionPageLinkPostedAt,
    });
  }

  async setChatSessionPageLinkPostedAt(coordinates: ChatSessionCoordinates, sessionPageLinkPostedAt: string): Promise<SlackSessionRecord> {
    return await this.#patchChatSession(coordinates, {
      sessionPageLinkPostedAt,
    });
  }

  async setSessionAuthProfile(
    sessionKey: string,
    profileName: string,
    options?: {
      readonly boundAt?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      authProfileName: profileName,
      authProfileBoundAt: options?.boundAt ?? new Date().toISOString(),
    });
  }

  async markSessionAuthBlocked(
    sessionKey: string,
    options: {
      readonly reason: string;
      readonly blockedAt?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      authBlockedAt: options.blockedAt ?? new Date().toISOString(),
      authBlockReason: options.reason,
    });
  }

  async setSessionAuthBlockedNoticePostedAt(sessionKey: string, postedAt: string): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      authBlockedNoticePostedAt: postedAt,
    });
  }

  async clearSessionAuthBlock(sessionKey: string): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      authBlockedAt: undefined,
      authBlockReason: undefined,
      authBlockedNoticePostedAt: undefined,
    });
  }

  async switchSessionAuthProfileAndClearBlock(
    sessionKey: string,
    profileName: string,
    options?: {
      readonly boundAt?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    return await this.#stateStore.patchSession(sessionKey, {
      authProfileName: profileName,
      authProfileBoundAt: options?.boundAt ?? new Date().toISOString(),
      authBlockedAt: undefined,
      authBlockReason: undefined,
      authBlockedNoticePostedAt: undefined,
      agentSessionId: undefined,
      activeTurnId: undefined,
      activeTurnStartedAt: undefined,
    });
  }

  async recordTurnSignal(
    channelId: string,
    rootThreadTs: string,
    signal: {
      readonly turnId?: string | undefined;
      readonly kind: SlackTurnSignalKind;
      readonly reason?: string | undefined;
      readonly occurredAt?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      lastTurnSignalTurnId: signal.turnId,
      lastTurnSignalKind: signal.kind,
      lastTurnSignalReason: signal.reason?.trim() || undefined,
      lastTurnSignalAt: signal.occurredAt ?? new Date().toISOString(),
    });
  }

  async recordChatTurnSignal(
    coordinates: ChatSessionCoordinates,
    signal: {
      readonly turnId?: string | undefined;
      readonly kind: SlackTurnSignalKind;
      readonly reason?: string | undefined;
      readonly occurredAt?: string | undefined;
    },
  ): Promise<SlackSessionRecord> {
    return await this.#patchChatSession(coordinates, {
      lastTurnSignalTurnId: signal.turnId,
      lastTurnSignalKind: signal.kind,
      lastTurnSignalReason: signal.reason?.trim() || undefined,
      lastTurnSignalAt: signal.occurredAt ?? new Date().toISOString(),
    });
  }

  async addCoAuthorCandidates(channelId: string, rootThreadTs: string, userIds: readonly string[]): Promise<SlackSessionRecord> {
    const session = this.#requireSession(channelId, rootThreadTs);
    const additions = userIds
      .map((userId) => userId.trim())
      .filter(Boolean)
      .filter((userId, index, array) => array.indexOf(userId) === index)
      .filter((userId) => !(session.coAuthorCandidateUserIds ?? []).includes(userId));

    if (additions.length === 0) {
      return session;
    }

    return await this.#patchSession(channelId, rootThreadTs, {
      coAuthorCandidateUserIds: [...(session.coAuthorCandidateUserIds ?? []), ...additions],
      coAuthorCandidateRevision: (session.coAuthorCandidateRevision ?? 0) + 1,
      coAuthorIgnoreMissingRevision: undefined,
      coAuthorPromptRevision: undefined,
      coAuthorPromptedAt: undefined,
    });
  }

  async addChatCoAuthorCandidates(coordinates: ChatSessionCoordinates, userIds: readonly string[]): Promise<SlackSessionRecord> {
    const session = this.#requireChatSession(coordinates);
    const additions = userIds
      .map((userId) => userId.trim())
      .filter(Boolean)
      .filter((userId, index, array) => array.indexOf(userId) === index)
      .filter((userId) => !(session.coAuthorCandidateUserIds ?? []).includes(userId));

    if (additions.length === 0) {
      return session;
    }

    return await this.#patchChatSession(coordinates, {
      coAuthorCandidateUserIds: [...(session.coAuthorCandidateUserIds ?? []), ...additions],
      coAuthorCandidateRevision: (session.coAuthorCandidateRevision ?? 0) + 1,
      coAuthorIgnoreMissingRevision: undefined,
      coAuthorPromptRevision: undefined,
      coAuthorPromptedAt: undefined,
    });
  }

  async confirmCoAuthors(
    channelId: string,
    rootThreadTs: string,
    options: {
      readonly userIds: readonly string[];
      readonly candidateRevision: number;
      readonly ignoreMissing?: boolean | undefined;
    },
  ): Promise<SlackSessionRecord> {
    const session = this.#requireSession(channelId, rootThreadTs);
    const confirmedUserIds = (session.coAuthorCandidateUserIds ?? []).filter((userId) => options.userIds.includes(userId));

    return await this.#patchSession(channelId, rootThreadTs, {
      coAuthorConfirmedUserIds: confirmedUserIds,
      coAuthorConfirmedRevision: options.candidateRevision,
      coAuthorIgnoreMissingRevision: options.ignoreMissing ? options.candidateRevision : undefined,
      coAuthorPromptRevision: options.candidateRevision,
      coAuthorPromptedAt: undefined,
    });
  }

  async confirmChatCoAuthors(
    coordinates: ChatSessionCoordinates,
    options: {
      readonly userIds: readonly string[];
      readonly candidateRevision: number;
      readonly ignoreMissing?: boolean | undefined;
    },
  ): Promise<SlackSessionRecord> {
    const session = this.#requireChatSession(coordinates);
    const confirmedUserIds = (session.coAuthorCandidateUserIds ?? []).filter((userId) => options.userIds.includes(userId));

    return await this.#patchChatSession(coordinates, {
      coAuthorConfirmedUserIds: confirmedUserIds,
      coAuthorConfirmedRevision: options.candidateRevision,
      coAuthorIgnoreMissingRevision: options.ignoreMissing ? options.candidateRevision : undefined,
      coAuthorPromptRevision: options.candidateRevision,
      coAuthorPromptedAt: undefined,
    });
  }

  async allowMissingCoAuthors(channelId: string, rootThreadTs: string, candidateRevision: number): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      coAuthorIgnoreMissingRevision: candidateRevision,
      coAuthorPromptRevision: candidateRevision,
      coAuthorPromptedAt: undefined,
    });
  }

  async markCoAuthorPrompted(channelId: string, rootThreadTs: string, promptRevision: number): Promise<SlackSessionRecord> {
    return await this.#patchSession(channelId, rootThreadTs, {
      coAuthorPromptRevision: promptRevision,
      coAuthorPromptedAt: new Date().toISOString(),
    });
  }

  async markChatCoAuthorPrompted(coordinates: ChatSessionCoordinates, promptRevision: number): Promise<SlackSessionRecord> {
    return await this.#patchChatSession(coordinates, {
      coAuthorPromptRevision: promptRevision,
      coAuthorPromptedAt: new Date().toISOString(),
    });
  }

  getInboundMessage(channelId: string, rootThreadTs: string, messageTs: string): PersistedInboundMessage | undefined {
    return this.#stateStore.getInboundMessage(SessionManager.createKey(channelId, rootThreadTs), messageTs);
  }

  listInboundMessages(options?: {
    readonly channelId?: string | undefined;
    readonly rootThreadTs?: string | undefined;
    readonly status?: PersistedInboundMessageStatus | readonly PersistedInboundMessageStatus[] | undefined;
    readonly batchId?: string | undefined;
    readonly source?: PersistedInboundSource | readonly PersistedInboundSource[] | undefined;
    readonly needsMentionUserBackfill?: boolean | undefined;
  }): PersistedInboundMessage[] {
    return this.#stateStore.listInboundMessages({
      sessionKey: options?.channelId && options?.rootThreadTs ? SessionManager.createKey(options.channelId, options.rootThreadTs) : undefined,
      status: options?.status,
      batchId: options?.batchId,
      source: options?.source,
      needsMentionUserBackfill: options?.needsMentionUserBackfill,
    });
  }

  getLatestInboundMessageTs(channelId: string, rootThreadTs: string): string | undefined {
    return this.#stateStore.getLatestInboundMessageTs(SessionManager.createKey(channelId, rootThreadTs));
  }

  getLatestSlackInboundMessageTs(channelId: string, rootThreadTs: string): string | undefined {
    return this.#stateStore.getLatestInboundMessageTs(SessionManager.createKey(channelId, rootThreadTs), {
      source: ["app_mention", "direct_message", "thread_reply"],
    });
  }

  async upsertInboundMessage(record: PersistedInboundMessage): Promise<void> {
    await this.#stateStore.upsertInboundMessage(record);
  }

  async updateInboundMessagesForBatch(
    channelId: string,
    rootThreadTs: string,
    messageTsList: readonly string[],
    patch: {
      readonly status?: PersistedInboundMessageStatus | undefined;
      readonly batchId?: string | undefined;
    },
  ): Promise<PersistedInboundMessage[]> {
    return await this.#stateStore.updateInboundMessagesForBatch(SessionManager.createKey(channelId, rootThreadTs), messageTsList, patch);
  }

  async resetInflightMessages(channelId: string, rootThreadTs: string, batchId?: string | undefined): Promise<PersistedInboundMessage[]> {
    return await this.#stateStore.resetInflightMessages(SessionManager.createKey(channelId, rootThreadTs), batchId);
  }

  listBackgroundJobs(options?: { readonly platform?: ChatPlatform | undefined; readonly conversationId?: string | undefined; readonly rootMessageId?: string | undefined; readonly channelId?: string | undefined; readonly rootThreadTs?: string | undefined; readonly id?: string | undefined }): PersistedBackgroundJob[] {
    const sessionKey =
      options?.platform && options.conversationId && options.rootMessageId
        ? SessionManager.createChatKey({
            platform: options.platform,
            conversationId: options.conversationId,
            rootMessageId: options.rootMessageId,
          })
        : undefined;
    return this.#stateStore.listBackgroundJobs({
      sessionKey: sessionKey ?? (options?.channelId && options?.rootThreadTs ? SessionManager.createKey(options.channelId, options.rootThreadTs) : undefined),
      id: options?.id,
    });
  }

  getBackgroundJob(id: string): PersistedBackgroundJob | undefined {
    return this.#stateStore.getBackgroundJob(id);
  }

  async upsertBackgroundJob(record: PersistedBackgroundJob): Promise<void> {
    await this.#stateStore.upsertBackgroundJob(record);
  }

  listAdminOperations(limit?: number): PersistedAdminOperation[] {
    return this.#stateStore.listAdminOperations(limit);
  }

  getAdminOperation(id: string): PersistedAdminOperation | undefined {
    return this.#stateStore.getAdminOperation(id);
  }

  async upsertAdminOperation(record: PersistedAdminOperation): Promise<void> {
    await this.#stateStore.upsertAdminOperation(record);
  }

  listAdminAuditEvents(options?: { readonly operationId?: string | undefined; readonly limit?: number | undefined }): PersistedAdminAuditEvent[] {
    return this.#stateStore.listAdminAuditEvents(options);
  }

  async appendAdminAuditEvent(record: PersistedAdminAuditEvent): Promise<void> {
    await this.#stateStore.appendAdminAuditEvent(record);
  }

  listAgentTurnUsage(limit?: number): PersistedAgentTurnUsage[] {
    return this.#stateStore.listAgentTurnUsage(limit);
  }

  listAgentSessionUsageSummaries(): PersistedAgentSessionUsageSummary[] {
    return this.#stateStore.listAgentSessionUsageSummaries();
  }

  getAgentSessionUsageSummary(sessionKey: string): PersistedAgentSessionUsageSummary | undefined {
    return this.#stateStore.getAgentSessionUsageSummary(sessionKey);
  }

  async upsertAgentTurnUsage(record: PersistedAgentTurnUsage): Promise<void> {
    await this.#stateStore.upsertAgentTurnUsage(record);
  }

  listAgentTraceEvents(sessionKey: string, limit?: number): PersistedAgentTraceEvent[] {
    return this.#stateStore.listAgentTraceEvents(sessionKey, limit);
  }

  getAgentTraceEvent(sessionKey: string, id: string): PersistedAgentTraceEvent | undefined {
    return this.#stateStore.getAgentTraceEvent(sessionKey, id);
  }

  listAgentTraceEventsPage(
    sessionKey: string,
    options?: {
      readonly limit?: number | undefined;
      readonly beforeSequence?: number | undefined;
    },
  ): {
    readonly events: PersistedAgentTraceEvent[];
    readonly hasMore: boolean;
    readonly nextBeforeSequence: number | null;
  } {
    return this.#stateStore.listAgentTraceEventsPage(sessionKey, options);
  }

  getAgentSessionTraceSummary(sessionKey: string): PersistedAgentSessionTraceSummary | undefined {
    return this.#stateStore.getAgentSessionTraceSummary(sessionKey);
  }

  rebuildTraceSummaries(): void {
    this.#stateStore.rebuildTraceSummaries();
  }

  async upsertAgentTraceEvent(record: PersistedAgentTraceEvent): Promise<void> {
    await this.#stateStore.upsertAgentTraceEvent(record);
  }

  listAdminEvents(options?: { readonly afterSequence?: number | undefined; readonly sessionKey?: string | undefined; readonly limit?: number | undefined }): PersistedAdminEvent[] {
    return this.#stateStore.listAdminEvents(options);
  }

  getLatestAdminEventSequence(): number {
    return this.#stateStore.getLatestAdminEventSequence();
  }

  #requireSession(channelId: string, rootThreadTs: string): SlackSessionRecord {
    const session = this.getSession(channelId, rootThreadTs);
    if (!session) {
      throw new Error(`Unknown session: ${channelId}:${rootThreadTs}`);
    }

    return session;
  }

  #requireChatSession(coordinates: ChatSessionCoordinates): SlackSessionRecord {
    const session = this.getChatSession(coordinates);
    if (!session) {
      throw new Error(`Unknown chat session: ${coordinates.platform}:${coordinates.conversationId}:${coordinates.rootMessageId}`);
    }

    return session;
  }

  async #patchChatSession(coordinates: ChatSessionCoordinates, patch: Partial<SlackSessionRecord>): Promise<SlackSessionRecord> {
    const session = this.#requireChatSession(coordinates);
    return await this.#stateStore.patchSession(session.key, {
      ...patch,
      updatedAt: new Date().toISOString(),
    });
  }

  #createWorkspacePath(channelId: string, rootThreadTs: string): string {
    return path.join(this.#sessionsRoot, `${channelId}-${rootThreadTs}`.replace(/[^a-zA-Z0-9]+/g, "-").replace(/^-+|-+$/g, ""), "workspace");
  }

  #resolveSessionRoot(workspacePath: string): string | undefined {
    const resolvedSessionsRoot = path.resolve(this.#sessionsRoot);
    const resolvedWorkspacePath = path.resolve(workspacePath);
    const sessionRoot = path.basename(resolvedWorkspacePath) === "workspace" ? path.dirname(resolvedWorkspacePath) : resolvedWorkspacePath;

    return isSubpathOf(resolvedSessionsRoot, sessionRoot) ? sessionRoot : undefined;
  }

  async #applyChannelMetadata(session: SlackSessionRecord, metadata?: SessionChannelMetadata | undefined): Promise<SlackSessionRecord> {
    const normalized = normalizeChannelMetadata(metadata);
    if (!normalized.channelName && !normalized.channelType) {
      return session;
    }

    const patch: Record<string, string> = {};
    if (normalized.channelName && normalized.channelName !== session.channelName) {
      patch.channelName = normalized.channelName;
    }
    if (normalized.channelType && normalized.channelType !== session.channelType) {
      patch.channelType = normalized.channelType;
    }
    if (!Object.keys(patch).length) {
      return session;
    }

    return await this.#stateStore.patchSession(session.key, {
      ...patch,
      updatedAt: new Date().toISOString(),
    });
  }

  async #patchSession(channelId: string, rootThreadTs: string, patch: Partial<SlackSessionRecord>): Promise<SlackSessionRecord> {
    const session = this.#requireSession(channelId, rootThreadTs);
    return await this.#stateStore.patchSession(session.key, {
      ...patch,
      updatedAt: new Date().toISOString(),
    });
  }
}

function normalizeChannelMetadata(metadata?: SessionChannelMetadata | undefined): SessionChannelMetadata {
  return {
    channelName: normalizeNonEmptyString(metadata?.channelName),
    channelType: normalizeNonEmptyString(metadata?.channelType),
  };
}

function normalizeSessionInitiator(metadata?: SessionInitiatorMetadata | undefined): Pick<SlackSessionRecord, "initiatorUserId" | "initiatorMessageTs" | "initiatorCapturedAt"> {
  const initiatorUserId = normalizeNonEmptyString(metadata?.initiatorUserId);
  const initiatorMessageTs = normalizeNonEmptyString(metadata?.initiatorMessageTs);
  if (!initiatorUserId) {
    return {};
  }

  return {
    initiatorUserId,
    initiatorMessageTs,
    initiatorCapturedAt: new Date().toISOString(),
  };
}

function normalizeNonEmptyString(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}

function isSubpathOf(rootPath: string, targetPath: string): boolean {
  const relative = path.relative(path.resolve(rootPath), targetPath);
  return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}
