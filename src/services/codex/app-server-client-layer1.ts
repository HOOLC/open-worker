import fs from "node:fs/promises";

import WebSocket from "ws";

import { logger } from "../../logger.js";
import type { AgentTurnTokenUsage, GeneratedImageArtifact, JsonLike, SlackUserIdentity } from "../../types.js";
import { buildSlackThreadBaseInstructions } from "./slack-thread-base-instructions.js";
import type { ThreadCoordinates } from "./thread-coordinates.js";

export type { ThreadCoordinates } from "./thread-coordinates.js";

type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

interface RawRateLimitWindow {
  readonly usedPercent?: number;
  readonly windowDurationMins?: number | null;
  readonly resetsAt?: number | null;
}

interface RawCreditsSnapshot {
  readonly hasCredits?: boolean;
  readonly unlimited?: boolean;
  readonly balance?: string | null;
}

interface RawRateLimitSnapshot {
  readonly limitId?: string | null;
  readonly limitName?: string | null;
  readonly primary?: RawRateLimitWindow | null;
  readonly secondary?: RawRateLimitWindow | null;
  readonly credits?: RawCreditsSnapshot | null;
  readonly planType?: string | null;
}

interface PendingRequest {
  readonly resolve: (value: any) => void;
  readonly reject: (error: Error) => void;
}

interface ActiveTurn {
  readonly threadId: string;
  readonly turnId: string;
  text: string;
  generatedImages: GeneratedImageArtifact[];
  usage?: AgentTurnTokenUsage | undefined;
  lastTokenCountCumulativeTokens?: number | undefined;
  resolve: (result: CodexTurnResult) => void;
  reject: (error: Error) => void;
}

interface BufferedTurnEvents {
  text: string;
  terminalState: "completed" | "aborted" | null;
  generatedImages: GeneratedImageArtifact[];
  usage?: AgentTurnTokenUsage | undefined;
  lastTokenCountCumulativeTokens?: number | undefined;
}

interface CodexTokenCountUsageEvent {
  readonly usage: AgentTurnTokenUsage;
  readonly cumulativeTotalTokens?: number | undefined;
}

interface ThreadRuntimeDefaults {
  readonly model?: string | undefined;
  readonly effort?: string | undefined;
}

interface PromptSessionCoordinates {
  readonly platform?: "slack" | "feishu" | undefined;
  readonly conversationId?: string | undefined;
  readonly conversationKind?: string | undefined;
  readonly rootMessageId?: string | undefined;
  readonly platformThreadId?: string | undefined;
  readonly sessionKey?: string | undefined;
  readonly channelId: string;
  readonly rootThreadTs: string;
  readonly workspacePath: string;
}

export interface StartedTurn {
  readonly turnId: string;
  readonly completion: Promise<CodexTurnResult>;
}

export interface CodexTurnResult {
  readonly threadId: string;
  readonly turnId: string;
  readonly finalMessage: string;
  readonly aborted: boolean;
  readonly generatedImages?: readonly GeneratedImageArtifact[] | undefined;
  readonly usage?: AgentTurnTokenUsage | undefined;
}

export interface CodexTextInputItem {
  readonly type: "text";
  readonly text: string;
  readonly text_elements: readonly [];
}

export interface CodexImageInputItem {
  readonly type: "image";
  readonly url: string;
}

export type CodexInputItem = CodexTextInputItem | CodexImageInputItem;

export interface SteerTurnOptions {
  readonly threadId: string;
  readonly turnId: string;
  readonly input: readonly CodexInputItem[];
}

export interface ReadTurnResult {
  readonly status: "completed" | "failed" | "interrupted" | "inProgress" | "unknown";
  readonly finalMessage: string;
  readonly errorMessage?: string | undefined;
  readonly generatedImages: readonly GeneratedImageArtifact[];
  readonly usage?: AgentTurnTokenUsage | undefined;
}

export interface ReadTurnResultOptions {
  readonly syncActiveTurn?: boolean | undefined;
  readonly treatMissingAsStale?: boolean | undefined;
}

export interface AppServerAccountSummary {
  readonly account?: JsonValue | undefined;
  readonly quota?: JsonValue | undefined;
  readonly usage?: JsonValue | undefined;
  readonly requiresOpenaiAuth?: boolean | undefined;
}

export interface AppServerRateLimitWindow {
  readonly usedPercent: number;
  readonly windowDurationMins: number | null;
  readonly resetsAt: number | null;
}

export interface AppServerCreditsSnapshot {
  readonly hasCredits: boolean;
  readonly unlimited: boolean;
  readonly balance: string | null;
}

export type AppServerPlanType = "free" | "go" | "plus" | "pro" | "team" | "business" | "enterprise" | "edu" | "unknown" | string;

export interface AppServerRateLimitSnapshot {
  readonly limitId: string | null;
  readonly limitName: string | null;
  readonly primary: AppServerRateLimitWindow | null;
  readonly secondary: AppServerRateLimitWindow | null;
  readonly credits: AppServerCreditsSnapshot | null;
  readonly planType: AppServerPlanType | null;
}

export interface AppServerRateLimitsResponse {
  readonly rateLimits: AppServerRateLimitSnapshot;
  readonly rateLimitsByLimitId: Record<string, AppServerRateLimitSnapshot> | null;
}

import { AppServerClientBase } from "./app-server-client-base.js";
export class AppServerClientLayer1 extends AppServerClientBase {
  async connect(): Promise<void> {
    this.privateDisconnectHandled = false;
    this.privateSocket = new WebSocket(this.options.url);

    await new Promise<void>((resolve, reject) => {
      this.privateSocket?.once("open", () => resolve());
      this.privateSocket?.once("error", reject);
    });

    this.privateSocket.on("message", (data) => {
      this.privateHandleMessage(data.toString());
    });
    this.privateSocket.on("pong", () => {
      this.privateAwaitingPong = false;
      logger.debug("Codex app-server websocket heartbeat acknowledged");
    });
    this.privateSocket.on("error", (error) => {
      logger.warn("Codex app-server websocket error", {
        error: error instanceof Error ? error.message : String(error),
      });
      this.privateHandleDisconnect(error instanceof Error ? error : new Error(String(error)));
    });
    this.privateSocket.on("close", () => {
      this.privateHandleDisconnect(new Error("Codex app-server websocket closed"));
    });

    await this.request("initialize", {
      clientInfo: {
        name: this.options.serviceName,
        version: "0.1.0",
      },
      capabilities: {
        experimentalApi: true,
      },
    });

    this.privateConnected = true;
    this.privateStartHeartbeat(this.privateSocket, this.options.heartbeatIntervalMs);
  }

  isConnected(): boolean {
    return this.privateConnected;
  }

  setSlackBotIdentity(identity: SlackUserIdentity | null): void {
    this.privateSlackBotIdentity = identity;
  }

  readThreadCoordinates(threadId: string): ThreadCoordinates | undefined {
    return this.privateThreadCoordinates.get(threadId);
  }

  async close(): Promise<void> {
    if (!this.privateSocket) {
      this.privateHandleDisconnect(new Error("Codex app-server websocket closed"));
      return;
    }

    if (this.privateSocket.readyState === WebSocket.CLOSED) {
      this.privateHandleDisconnect(new Error("Codex app-server websocket closed"));
      return;
    }

    await new Promise<void>((resolve) => {
      this.privateSocket?.once("close", () => resolve());
      this.privateSocket?.close();
    });
  }

  async ensureAuthenticated(): Promise<void> {
    logger.debug("Checking Codex authentication");
    const response = (await this.request("account/read", { refreshToken: false })) as {
      account: { type: string } | null;
      requiresOpenaiAuth: boolean;
    };

    if (response.account) {
      return;
    }

    if (!this.options.openAiApiKey) {
      throw new Error("Codex app-server is not authenticated. Mount auth.json into CODEX_HOME or provide OPENAI_API_KEY.");
    }

    await this.request("account/login/start", {
      type: "apiKey",
      apiKey: this.options.openAiApiKey,
    });
  }

  async readAccountSummary(refreshToken = false): Promise<AppServerAccountSummary> {
    const response = (await this.request("account/read", { refreshToken })) as {
      account?: JsonValue;
      quota?: JsonValue;
      usage?: JsonValue;
      requiresOpenaiAuth?: boolean;
    };

    return {
      account: response.account,
      quota: response.quota,
      usage: response.usage,
      requiresOpenaiAuth: response.requiresOpenaiAuth,
    };
  }

  async readAccountRateLimits(): Promise<AppServerRateLimitsResponse> {
    const response = (await this.request("account/rateLimits/read")) as {
      rateLimits?: RawRateLimitSnapshot;
      rateLimitsByLimitId?: Record<string, RawRateLimitSnapshot> | null;
    };

    if (!response.rateLimits) {
      throw new Error("Codex app-server did not return rate limits");
    }

    return {
      rateLimits: normalizeRateLimitSnapshot(response.rateLimits),
      rateLimitsByLimitId: normalizeRateLimitSnapshotMap(response.rateLimitsByLimitId),
    };
  }

  async ensureThread(session: PromptSessionCoordinates & { readonly agentSessionId?: string | undefined }): Promise<string> {
    if (session.agentSessionId) {
      logger.debug("Resuming Codex thread", {
        threadId: session.agentSessionId,
        cwd: session.workspacePath,
      });
      const result = (await this.request("thread/resume", {
        threadId: session.agentSessionId,
        cwd: session.workspacePath,
        approvalPolicy: "never",
        sandbox: "danger-full-access",
        model: null,
        modelProvider: null,
        config: null,
        baseInstructions: null,
        developerInstructions: null,
        personality: null,
        persistExtendedHistory: true,
      })) as {
        thread: { id: string };
        model?: unknown;
        reasoningEffort?: unknown;
        reasoning_effort?: unknown;
        effort?: unknown;
      };

      this.privateRememberThreadRuntimeDefaults(result.thread.id, result);
      this.privateRememberThreadCoordinates(result.thread.id, session);
      return result.thread.id;
    }

    const baseInstructions = await this.privateBuildBaseInstructions(session);
    const threadStartParams: Record<string, JsonLike> = {
      cwd: session.workspacePath,
      approvalPolicy: "never",
      sandbox: "danger-full-access",
      model: null,
      modelProvider: null,
      config: null,
      serviceName: this.options.serviceName,
      baseInstructions,
      developerInstructions: null,
      personality: null,
      ephemeral: false,
      experimentalRawEvents: true,
      persistExtendedHistory: true,
    };

    const result = (await this.request("thread/start", threadStartParams)) as {
      thread: { id: string };
      model?: unknown;
      reasoningEffort?: unknown;
      reasoning_effort?: unknown;
      effort?: unknown;
    };
    this.privateRememberThreadRuntimeDefaults(result.thread.id, result);
    this.privateRememberThreadCoordinates(result.thread.id, session);
    this.emit("notification", "broker/system_prompt", {
      threadId: result.thread.id,
      cwd: session.workspacePath,
      baseInstructions,
    });
    logger.debug("Started Codex thread", {
      threadId: result.thread.id,
      cwd: session.workspacePath,
    });

    return result.thread.id;
  }

  async startTurn(threadId: string, cwd: string, input: readonly CodexInputItem[]): Promise<StartedTurn> {
    logger.debug("Starting Codex turn", {
      threadId,
      cwd,
      inputItemCount: input.length,
    });
    const requestInput = [...input];
    const result = (await this.request("turn/start", {
      threadId,
      input: requestInput,
      cwd,
      approvalPolicy: "never",
      sandboxPolicy: {
        type: "dangerFullAccess",
      },
      collaborationMode: null,
      outputSchema: null,
      model: null,
      effort: null,
      summary: "auto",
      personality: null,
    } as unknown as JsonValue)) as {
      turn: { id: string };
    };

    const completion = new Promise<CodexTurnResult>((resolve, reject) => {
      this.privateActiveTurns.set(result.turn.id, {
        threadId,
        turnId: result.turn.id,
        text: "",
        generatedImages: [],
        resolve,
        reject,
      });
    });
    this.privateApplyPendingAnonymousTurnUsage(result.turn.id);
    this.privateApplyBufferedTurnEvents(result.turn.id);
    // A websocket disconnect can reject the turn before the caller gets to `await completion`.
    // Keep a no-op rejection handler attached so Node does not treat that window as unhandled.
    void completion.catch(() => {});

    return {
      turnId: result.turn.id,
      completion,
    };
  }

  async steerTurn(options: SteerTurnOptions): Promise<void> {
    const maxAttempts = 8;
    const requestInput = [...options.input];

    for (let attempt = 1; attempt <= maxAttempts; attempt += 1) {
      try {
        await this.request("turn/steer", {
          threadId: options.threadId,
          expectedTurnId: options.turnId,
          input: requestInput,
        } as unknown as JsonValue);
        return;
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        const shouldRetry = /no active turn to steer/i.test(message) || /expectedTurnId/i.test(message);

        if (!shouldRetry || attempt === maxAttempts) {
          throw error;
        }

        await new Promise((resolve) => setTimeout(resolve, 150));
      }
    }
  }

  async interruptTurn(threadId: string, turnId: string): Promise<void> {
    await this.request("turn/interrupt", {
      threadId,
      turnId,
    });
  }

  privateRememberThreadCoordinates(threadId: string, session: PromptSessionCoordinates): void {
    // Coordinates are recorded on both thread/start and thread/resume so
    // process restarts can rebuild the in-memory map from session state later.
    this.privateThreadCoordinates.set(threadId, {
      threadId,
      platform: session.platform,
      channelId: session.channelId,
      conversationId: session.conversationId,
      conversationKind: session.conversationKind,
      rootThreadTs: session.rootThreadTs,
      rootMessageId: session.rootMessageId,
      platformThreadId: session.platformThreadId,
      workspacePath: session.workspacePath,
      sessionKey: session.sessionKey,
    });
  }

  async privateBuildBaseInstructions(session: PromptSessionCoordinates): Promise<string> {
    const personalMemory = await this.privateReadPersonalMemory();
    return await buildSlackThreadBaseInstructions({
      platform: session.platform,
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      conversationId: session.conversationId,
      conversationKind: session.conversationKind,
      rootMessageId: session.rootMessageId,
      platformThreadId: session.platformThreadId,
      workspacePath: session.workspacePath,
      reposRoot: this.options.reposRoot,
      codexGeneratedImagesRoot: this.options.codexGeneratedImagesRoot ?? "$CODEX_HOME/generated_images",
      slackBotIdentity: this.privateSlackBotIdentity,
      personalMemory,
    });
  }

  async privateReadPersonalMemory(): Promise<string | undefined> {
    if (!this.options.personalMemoryFilePath) {
      return undefined;
    }

    const content = await fs.readFile(this.options.personalMemoryFilePath, "utf8").catch(() => "");
    const normalized = content.trim();
    return normalized ? normalized : undefined;
  }

  async readTurnResult(threadId: string, turnId: string, options?: ReadTurnResultOptions): Promise<ReadTurnResult | null> {
    const result = (await this.request("thread/read", {
      threadId,
      includeTurns: true,
    })) as {
      thread?: {
        turns?: Array<{
          id?: string;
          status?: string;
          usage?: unknown;
          token_usage?: unknown;
          tokenUsage?: unknown;
          error?: {
            message?: string;
            additionalDetails?: string | null;
          } | null;
          items?: Array<{
            type?: string;
            id?: string;
            text?: string;
            status?: string;
            result?: string;
            savedPath?: string | null;
            saved_path?: string | null;
            revisedPrompt?: string | null;
            revised_prompt?: string | null;
          }>;
        }>;
      };
    };

    const turn = result.thread?.turns?.find((entry) => entry.id === turnId);

    if (!turn) {
      if (options?.syncActiveTurn && options.treatMissingAsStale) {
        this.privateSettleMissingActiveTurn(turnId);
      }
      return null;
    }

    const agentMessages = (turn.items ?? []).filter((item) => item.type === "agentMessage");
    const lastAgentMessage = agentMessages.at(-1);
    const generatedImages = (turn.items ?? []).map((item, index) => normalizeGeneratedImageArtifact(item, index)).filter((item): item is GeneratedImageArtifact => item !== null);
    const status = normalizeTurnStatus(turn.status);
    const usage = this.privateWithThreadRuntimeDefaults(threadId, normalizeAgentTurnUsageFromThreadTurn(turn));

    const normalizedResult: ReadTurnResult = {
      status,
      finalMessage: String(lastAgentMessage?.text ?? "").trim(),
      generatedImages,
      errorMessage: turn.error?.additionalDetails ?? turn.error?.message ?? undefined,
    };
    const resultWithUsage = usage ? { ...normalizedResult, usage } : normalizedResult;

    if (options?.syncActiveTurn) {
      this.privateSyncActiveTurn(turnId, resultWithUsage);
    }

    return resultWithUsage;
  }

  async request(method: string, params?: JsonValue): Promise<JsonValue> {
    if (!this.privateSocket || this.privateSocket.readyState !== WebSocket.OPEN) {
      throw new Error("Codex app-server websocket is not connected");
    }

    const requestId = String(++this.privateRequestCounter);
    logger.debug("App-server request", {
      method,
      requestId,
    });
    logger.raw(
      "codex-rpc",
      {
        direction: "request",
        id: requestId,
        method,
        params,
      },
      {
        requestId,
        method,
      },
    );
    const payload = JSON.stringify(
      params === undefined
        ? {
            id: requestId,
            method,
          }
        : {
            id: requestId,
            method,
            params,
          },
    );

    return await new Promise<JsonValue>((resolve, reject) => {
      this.privatePendingRequests.set(requestId, { resolve, reject });
      this.privateSocket?.send(payload, (error) => {
        if (error) {
          this.privatePendingRequests.delete(requestId);
          reject(error);
        }
      });
    });
  }

  privateHandleMessage(raw: string): void {
    const message = JSON.parse(raw) as {
      readonly id?: string | number;
      readonly result?: JsonValue;
      readonly error?: { readonly message: string };
      readonly method?: string;
      readonly params?: Record<string, any>;
    };
    logger.raw(
      "codex-rpc",
      {
        direction: "response",
        message,
      },
      {
        requestId: message.id,
        method: message.method,
      },
    );

    const hasId = message.id !== undefined && message.id !== null;

    if (hasId && message.method) {
      // JSON-RPC request from the app-server to us (e.g. item/tool/call).
      const handler = this.privateServerRequestHandlers.get(message.method);
      if (!handler) {
        this.sendServerError(message.id, -32601, `Method not found: ${message.method}`);
        return;
      }

      void handler(message.params ?? {})
        .then((result) => {
          this.sendServerResponse(message.id!, result);
        })
        .catch((error: unknown) => {
          logger.warn("App-server request handler failed", {
            method: message.method,
            error: error instanceof Error ? error.message : String(error),
          });
          this.sendServerError(message.id!, -32000, error instanceof Error ? error.message : String(error));
        });
      return;
    }

    if (hasId) {
      const requestId = String(message.id);
      const pending = this.privatePendingRequests.get(requestId);
      if (!pending) {
        return;
      }

      this.privatePendingRequests.delete(requestId);
      if (message.error) {
        pending.reject(new Error(message.error.message));
        return;
      }

      pending.resolve(message.result ?? null);
      logger.debug("App-server response", {
        requestId,
      });
      return;
    }

    if (!message.method) {
      return;
    }

    this.emit("notification", message.method, message.params);
    this.privateHandleTurnEvent(message.method, message.params ?? {});
  }
}

function withOptionalUsage(result: Omit<CodexTurnResult, "usage">, usage: AgentTurnTokenUsage | undefined): CodexTurnResult {
  return usage ? { ...result, usage } : result;
}

function normalizeAgentTurnUsageFromTurnEvent(params: Record<string, any>): AgentTurnTokenUsage | undefined {
  const turn = isRecord(params.turn) ? params.turn : {};
  return normalizeAgentTurnTokenUsage(turn.usage) ?? normalizeAgentTurnTokenUsage(turn.token_usage) ?? normalizeAgentTurnTokenUsage(turn.tokenUsage) ?? normalizeAgentTurnTokenUsage(params.usage) ?? normalizeAgentTurnTokenUsage(params.token_usage) ?? normalizeAgentTurnTokenUsage(params.tokenUsage);
}

function normalizeAgentTurnUsageFromTokenCountEvent(params: Record<string, any>): CodexTokenCountUsageEvent | undefined {
  const event = isRecord(params.msg) ? params.msg : isRecord(params.payload) ? params.payload : params;
  const info = isRecord(event.info) ? event.info : isRecord(params.info) ? params.info : undefined;

  const usage =
    normalizeAgentTurnTokenUsage(info?.last_token_usage) ??
    normalizeAgentTurnTokenUsage(info?.lastTokenUsage) ??
    normalizeAgentTurnTokenUsage(event.last_token_usage) ??
    normalizeAgentTurnTokenUsage(event.lastTokenUsage) ??
    normalizeAgentTurnTokenUsage(params.last_token_usage) ??
    normalizeAgentTurnTokenUsage(params.lastTokenUsage) ??
    normalizeAgentTurnTokenUsage(event.usage) ??
    normalizeAgentTurnTokenUsage(params.usage);
  if (!usage) {
    return undefined;
  }

  const totalUsage = isRecord(info?.total_token_usage)
    ? info.total_token_usage
    : isRecord(info?.totalTokenUsage)
      ? info.totalTokenUsage
      : isRecord(event.total_token_usage)
        ? event.total_token_usage
        : isRecord(event.totalTokenUsage)
          ? event.totalTokenUsage
          : isRecord(params.total_token_usage)
            ? params.total_token_usage
            : isRecord(params.totalTokenUsage)
              ? params.totalTokenUsage
              : undefined;

  return {
    usage,
    cumulativeTotalTokens: totalUsage ? readTokenNumber(totalUsage, ["total_tokens", "totalTokens"]) : undefined,
  };
}

function normalizeAgentTurnUsageFromThreadTokenUsageUpdated(params: Record<string, any>): CodexTokenCountUsageEvent | undefined {
  const tokenUsage = isRecord(params.tokenUsage) ? params.tokenUsage : isRecord(params.token_usage) ? params.token_usage : undefined;
  if (!tokenUsage) {
    return undefined;
  }

  const lastUsage = isRecord(tokenUsage.last) ? tokenUsage.last : isRecord(tokenUsage.last_token_usage) ? tokenUsage.last_token_usage : isRecord(tokenUsage.lastTokenUsage) ? tokenUsage.lastTokenUsage : tokenUsage;
  const usage = normalizeAgentTurnTokenUsage(lastUsage);
  if (!usage) {
    return undefined;
  }

  const totalUsage = isRecord(tokenUsage.total) ? tokenUsage.total : isRecord(tokenUsage.total_token_usage) ? tokenUsage.total_token_usage : isRecord(tokenUsage.totalTokenUsage) ? tokenUsage.totalTokenUsage : undefined;

  return {
    usage,
    cumulativeTotalTokens: totalUsage ? readTokenNumber(totalUsage, ["total_tokens", "totalTokens"]) : undefined,
  };
}

function readCodexEventTurnId(params: Record<string, any>): string | undefined {
  const event = isRecord(params.msg) ? params.msg : isRecord(params.payload) ? params.payload : params;

  return normalizeOptionalString(params.turnId) ?? normalizeOptionalString(params.turn_id) ?? normalizeOptionalString(event.turnId) ?? normalizeOptionalString(event.turn_id);
}

function addCodexTurnUsage(current: AgentTurnTokenUsage | undefined, next: AgentTurnTokenUsage): AgentTurnTokenUsage {
  if (!current) {
    return next;
  }
  const aggregate = {
    source: current.source === "exact" || next.source === "exact" ? ("exact" as const) : next.source,
    inputTokens: current.inputTokens + next.inputTokens,
    cachedInputTokens: current.cachedInputTokens + next.cachedInputTokens,
    outputTokens: current.outputTokens + next.outputTokens,
    reasoningTokens: current.reasoningTokens + next.reasoningTokens,
    totalTokens: current.totalTokens + next.totalTokens,
    model: next.model ?? current.model,
    effort: next.effort ?? current.effort,
  };

  return {
    ...aggregate,
    rawUsage: aggregateRawTokenUsage(current.rawUsage, next.rawUsage, aggregate),
  };
}

function aggregateRawTokenUsage(current: JsonLike | undefined, next: JsonLike | undefined, aggregate: Omit<AgentTurnTokenUsage, "rawUsage">): JsonLike | undefined {
  if (current === undefined && next === undefined) {
    return undefined;
  }

  const events = [...rawTokenUsageEvents(current), ...rawTokenUsageEvents(next)];
  return {
    kind: "aggregated_token_usage",
    eventCount: rawTokenUsageEventCount(current) + rawTokenUsageEventCount(next),
    totalTokens: aggregate.totalTokens,
    inputTokens: aggregate.inputTokens,
    cachedInputTokens: aggregate.cachedInputTokens,
    outputTokens: aggregate.outputTokens,
    reasoningTokens: aggregate.reasoningTokens,
    latest: next ?? rawTokenUsageLatest(current) ?? null,
    events: events.slice(-20),
  };
}

function rawTokenUsageEvents(value: JsonLike | undefined): JsonLike[] {
  if (value === undefined) {
    return [];
  }
  if (isAggregatedRawTokenUsage(value)) {
    const events = value.events;
    return Array.isArray(events) ? events.filter(isJsonLike) : [];
  }
  return [value];
}

function rawTokenUsageEventCount(value: JsonLike | undefined): number {
  if (value === undefined) {
    return 0;
  }
  if (isAggregatedRawTokenUsage(value)) {
    const count = value.eventCount;
    return typeof count === "number" && Number.isFinite(count) ? count : rawTokenUsageEvents(value).length;
  }
  return 1;
}

function rawTokenUsageLatest(value: JsonLike | undefined): JsonLike | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (isAggregatedRawTokenUsage(value)) {
    return isJsonLike(value.latest) ? value.latest : undefined;
  }
  return value;
}

function isAggregatedRawTokenUsage(value: JsonLike | undefined): value is Record<string, JsonLike> {
  return isRecord(value) && value.kind === "aggregated_token_usage";
}

function isJsonLike(value: unknown): value is JsonLike {
  if (value === null) {
    return true;
  }
  const type = typeof value;
  if (type === "string" || type === "number" || type === "boolean") {
    return true;
  }
  if (Array.isArray(value)) {
    return value.every(isJsonLike);
  }
  if (!isRecord(value)) {
    return false;
  }
  return Object.values(value).every(isJsonLike);
}

function shouldApplyTokenCountUsage(previousCumulativeTotalTokens: number | undefined, nextCumulativeTotalTokens: number | undefined): boolean {
  return nextCumulativeTotalTokens === undefined || previousCumulativeTotalTokens === undefined || nextCumulativeTotalTokens > previousCumulativeTotalTokens;
}

function updateTokenCountCumulativeTotal(previousCumulativeTotalTokens: number | undefined, nextCumulativeTotalTokens: number | undefined): number | undefined {
  if (nextCumulativeTotalTokens === undefined) {
    return previousCumulativeTotalTokens;
  }
  if (previousCumulativeTotalTokens === undefined) {
    return nextCumulativeTotalTokens;
  }
  return Math.max(previousCumulativeTotalTokens, nextCumulativeTotalTokens);
}

function normalizeThreadRuntimeDefaults(value: unknown): ThreadRuntimeDefaults | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const thread = isRecord(value.thread) ? value.thread : {};
  const model = normalizeOptionalString(value.model) ?? normalizeOptionalString(thread.model) ?? normalizeOptionalString(value.modelName) ?? normalizeOptionalString(thread.modelName);
  const effort = normalizeOptionalString(value.reasoningEffort) ?? normalizeOptionalString(value.reasoning_effort) ?? normalizeOptionalString(value.effort) ?? normalizeOptionalString(thread.reasoningEffort) ?? normalizeOptionalString(thread.reasoning_effort) ?? normalizeOptionalString(thread.effort);

  if (!model && !effort) {
    return undefined;
  }

  return {
    ...(model ? { model } : {}),
    ...(effort ? { effort } : {}),
  };
}

function normalizeAgentTurnUsageFromThreadTurn(turn: { readonly usage?: unknown; readonly token_usage?: unknown; readonly tokenUsage?: unknown }): AgentTurnTokenUsage | undefined {
  return normalizeAgentTurnTokenUsage(turn.usage) ?? normalizeAgentTurnTokenUsage(turn.token_usage) ?? normalizeAgentTurnTokenUsage(turn.tokenUsage);
}

function normalizeAgentTurnTokenUsage(value: unknown): AgentTurnTokenUsage | undefined {
  if (!isRecord(value)) {
    return undefined;
  }

  const inputTokenValue = readTokenNumber(value, ["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"]);
  const cachedTokenValue =
    readTokenNumber(value, ["cached_input_tokens", "cachedInputTokens", "cached_tokens", "cachedTokens"]) ??
    readNestedTokenNumber(value, [
      ["input_token_details", "cached_tokens"],
      ["inputTokenDetails", "cachedTokens"],
      ["input_tokens_details", "cached_tokens"],
      ["inputTokensDetails", "cachedTokens"],
    ]);
  const outputTokenValue = readTokenNumber(value, ["output_tokens", "outputTokens", "completion_tokens", "completionTokens"]);
  const reasoningTokenValue =
    readTokenNumber(value, ["reasoning_tokens", "reasoningTokens", "reasoning_output_tokens", "reasoningOutputTokens"]) ??
    readNestedTokenNumber(value, [
      ["output_token_details", "reasoning_tokens"],
      ["output_token_details", "reasoning_output_tokens"],
      ["outputTokenDetails", "reasoningTokens"],
      ["outputTokenDetails", "reasoningOutputTokens"],
      ["output_tokens_details", "reasoning_tokens"],
      ["output_tokens_details", "reasoning_output_tokens"],
      ["outputTokensDetails", "reasoningTokens"],
      ["outputTokensDetails", "reasoningOutputTokens"],
    ]);
  const totalTokenValue = readTokenNumber(value, ["total_tokens", "totalTokens"]);

  if (inputTokenValue === undefined && cachedTokenValue === undefined && outputTokenValue === undefined && reasoningTokenValue === undefined && totalTokenValue === undefined) {
    return undefined;
  }

  const computedTotal = (inputTokenValue ?? 0) + (outputTokenValue ?? 0) + (reasoningTokenValue ?? 0);
  const totalTokens = totalTokenValue ?? (computedTotal > 0 ? computedTotal : (cachedTokenValue ?? 0));

  return {
    source: "exact",
    inputTokens: inputTokenValue ?? 0,
    cachedInputTokens: cachedTokenValue ?? 0,
    outputTokens: outputTokenValue ?? 0,
    reasoningTokens: reasoningTokenValue ?? 0,
    totalTokens,
    model: normalizeOptionalString(value.model) ?? normalizeOptionalString(value.modelName),
    effort: normalizeOptionalString(value.effort) ?? normalizeOptionalString(value.reasoning_effort) ?? normalizeOptionalString(value.reasoningEffort),
    rawUsage: toJsonLike(value),
  };
}

function readTokenNumber(record: Record<string, unknown>, keys: readonly string[]): number | undefined {
  for (const key of keys) {
    const normalized = normalizeTokenNumber(record[key]);
    if (normalized !== undefined) {
      return normalized;
    }
  }
  return undefined;
}

function readNestedTokenNumber(record: Record<string, unknown>, paths: readonly (readonly [string, string])[]): number | undefined {
  for (const [objectKey, valueKey] of paths) {
    const container = record[objectKey];
    if (!isRecord(container)) {
      continue;
    }
    const normalized = normalizeTokenNumber(container[valueKey]);
    if (normalized !== undefined) {
      return normalized;
    }
  }
  return undefined;
}

function normalizeTokenNumber(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) {
    return Math.max(0, Math.trunc(value));
  }
  if (typeof value === "string" && value.trim()) {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return Math.max(0, Math.trunc(parsed));
    }
  }
  return undefined;
}

function toJsonLike(value: unknown): JsonLike | undefined {
  if (value === null || typeof value === "boolean" || typeof value === "string") {
    return value;
  }
  if (typeof value === "number") {
    return Number.isFinite(value) ? value : undefined;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => toJsonLike(entry) ?? null);
  }
  if (isRecord(value)) {
    const normalized: Record<string, JsonLike> = {};
    for (const [key, entry] of Object.entries(value)) {
      const normalizedEntry = toJsonLike(entry);
      if (normalizedEntry !== undefined) {
        normalized[key] = normalizedEntry;
      }
    }
    return normalized;
  }
  return undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeTurnStatus(status: unknown): ReadTurnResult["status"] {
  if (status === "completed" || status === "failed" || status === "interrupted" || status === "inProgress") {
    return status;
  }

  return "unknown";
}

function normalizeRateLimitSnapshot(snapshot: RawRateLimitSnapshot): AppServerRateLimitSnapshot {
  return {
    limitId: snapshot.limitId ?? null,
    limitName: snapshot.limitName ?? null,
    primary: normalizeRateLimitWindow(snapshot.primary),
    secondary: normalizeRateLimitWindow(snapshot.secondary),
    credits: normalizeCreditsSnapshot(snapshot.credits),
    planType: snapshot.planType ?? null,
  };
}

function normalizeRateLimitSnapshotMap(snapshots: Record<string, RawRateLimitSnapshot> | null | undefined): Readonly<Record<string, AppServerRateLimitSnapshot>> | null {
  if (!snapshots) {
    return null;
  }

  return Object.fromEntries(Object.entries(snapshots).map(([limitId, snapshot]) => [limitId, normalizeRateLimitSnapshot(snapshot)]));
}

function normalizeRateLimitWindow(window: RawRateLimitWindow | null | undefined): AppServerRateLimitWindow | null {
  if (!window) {
    return null;
  }

  return {
    usedPercent: Number(window.usedPercent ?? 0),
    windowDurationMins: window.windowDurationMins ?? null,
    resetsAt: window.resetsAt ?? null,
  };
}

function normalizeCreditsSnapshot(credits: RawCreditsSnapshot | null | undefined): AppServerCreditsSnapshot | null {
  if (!credits) {
    return null;
  }

  return {
    hasCredits: Boolean(credits.hasCredits),
    unlimited: Boolean(credits.unlimited),
    balance: credits.balance ?? null,
  };
}

function normalizeGeneratedImageArtifact(item: Record<string, unknown> | undefined, index = 0): GeneratedImageArtifact | null {
  if (!item) {
    return null;
  }

  const type = normalizeOptionalString(item.type);
  if (type !== "imageGeneration" && type !== "image_generation_call") {
    return null;
  }

  const savedPath = normalizeOptionalString(item.savedPath) ?? normalizeOptionalString(item.saved_path);
  const result = normalizeOptionalString(item.result);
  const { contentBase64, contentType } = normalizeImageResult(result);
  const id = normalizeOptionalString(item.id) ?? savedPath ?? `generated-image-${index + 1}`;
  const revisedPrompt = normalizeOptionalString(item.revisedPrompt) ?? normalizeOptionalString(item.revised_prompt);

  if (!savedPath && !contentBase64) {
    return null;
  }

  return {
    id,
    contentBase64,
    contentType,
    savedPath,
    revisedPrompt,
  };
}

function normalizeImageResult(value: string | undefined): {
  readonly contentBase64?: string;
  readonly contentType?: string;
} {
  const normalized = value?.trim();
  if (!normalized) {
    return {};
  }

  const dataUrlMatch = normalized.match(/^data:(image\/[^;]+);base64,(.+)$/i);
  if (dataUrlMatch) {
    return {
      contentType: dataUrlMatch[1]!,
      contentBase64: dataUrlMatch[2]!.replace(/\s+/g, ""),
    };
  }

  if (!/^[A-Za-z0-9+/=\s]+$/.test(normalized)) {
    return {};
  }

  return {
    contentType: "image/png",
    contentBase64: normalized.replace(/\s+/g, ""),
  };
}

function normalizeOptionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function upsertGeneratedImage(target: GeneratedImageArtifact[], image: GeneratedImageArtifact): void {
  const existingIndex = target.findIndex((entry) => entry.id === image.id);
  if (existingIndex === -1) {
    target.push(image);
    return;
  }

  target[existingIndex] = {
    ...target[existingIndex],
    ...image,
  };
}
