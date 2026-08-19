import { EventEmitter } from "node:events";

import WebSocket from "ws";

import type { AgentTurnTokenUsage, GeneratedImageArtifact, JsonLike, SlackUserIdentity } from "../../types.js";
import type { ThreadCoordinates } from "./thread-coordinates.js";

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
export interface AppServerClientBase {
  [key: string]: any;
}

export class AppServerClientBase extends EventEmitter {
  privateSocket: WebSocket | undefined;

  privateRequestCounter = 0;

  readonly privatePendingRequests = new Map<string, PendingRequest>();

  readonly privateActiveTurns = new Map<string, ActiveTurn>();

  readonly privateBufferedTurnEvents = new Map<string, BufferedTurnEvents>();

  readonly privateThreadRuntimeDefaults = new Map<string, ThreadRuntimeDefaults>();

  privatePendingAnonymousTokenUsage: AgentTurnTokenUsage | undefined;

  privatePendingAnonymousTokenUsageCumulativeTokens: number | undefined;

  privateConnected = false;

  privateDisconnectHandled = false;

  privateSlackBotIdentity: SlackUserIdentity | null = null;

  privateHeartbeatTimer: NodeJS.Timeout | undefined;

  privateAwaitingPong = false;

  // Handlers for JSON-RPC requests the app-server sends TO the client.
  readonly privateServerRequestHandlers = new Map<string, (params: Record<string, any>) => Promise<Record<string, unknown>>>();

  // threadId -> platform coordinates, populated by both thread/start and
  // thread/resume.
  readonly privateThreadCoordinates = new Map<string, ThreadCoordinates>();

  constructor(
    readonly options: {
      readonly url: string;
      readonly serviceName: string;
      readonly brokerHttpBaseUrl: string;
      readonly reposRoot: string;
      readonly codexGeneratedImagesRoot?: string | undefined;
      readonly openAiApiKey?: string | undefined;
      readonly personalMemoryFilePath?: string | undefined;
      readonly heartbeatIntervalMs?: number | undefined;
    },
  ) {
    super();
  }

  setServerRequestHandler(method: string, handler: (params: Record<string, any>) => Promise<Record<string, unknown>>): void {
    this.privateServerRequestHandlers.set(method, handler);
  }

  sendServerResponse(id: string | number, result: Record<string, unknown>): void {
    this.privateSocket?.send(JSON.stringify({ id, result }));
  }

  sendServerError(id: string | number, code: number, message: string): void {
    this.privateSocket?.send(JSON.stringify({ id, error: { code, message } }));
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
