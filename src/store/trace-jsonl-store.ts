import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

import { logger } from "../logger.js";
import type { JsonLike, PersistedAgentTraceEvent } from "../types.js";

export const TRACE_DIRECTORY_NAME = "trace";
export const DEFAULT_TRACE_SEGMENT_MAX_BYTES = 8 * 1024 * 1024;
export const TRACE_SEGMENT_INDEX_DIGITS = 6;
const ZSTD_DECODE_MAX_BUFFER_BYTES = 64 * 1024 * 1024;

export interface TraceSummarySnapshotRecord {
  readonly type: "summary_snapshot";
  readonly sessionKey: string;
  readonly sequence: number;
  readonly summary: {
    readonly eventCount: number;
    readonly modelRequestCount: number;
    readonly categories: Record<string, number>;
    readonly sources: Record<string, number>;
  };
}

export type TraceJsonlRecord = { readonly kind: "event"; readonly event: PersistedAgentTraceEvent } | { readonly kind: "snapshot"; readonly snapshot: TraceSummarySnapshotRecord };

export interface TraceJsonlStoreOptions {
  readonly stateDir: string;
  readonly segmentMaxBytes?: number | undefined;
}

export interface AgentTraceEventsPage {
  readonly events: PersistedAgentTraceEvent[];
  readonly hasMore: boolean;
  readonly nextBeforeSequence: number | null;
}

interface TraceSegment {
  readonly index: number;
  readonly filePath: string;
  readonly compressed: boolean;
}

interface SessionTraceState {
  eventsById: Map<string, PersistedAgentTraceEvent>;
  eventAppendCount: number;
  activeIndex: number;
  activeSize: number;
  loaded: boolean;
}

let zstdAvailability: boolean | undefined;
let zstdMissingWarned = false;

export function sanitizeTraceSessionKey(sessionKey: string): string {
  return encodeURIComponent(sessionKey);
}

export function traceSessionDirectory(stateDir: string, sessionKey: string): string {
  return path.join(stateDir, TRACE_DIRECTORY_NAME, sanitizeTraceSessionKey(sessionKey));
}

export function isZstdAvailable(): boolean {
  if (zstdAvailability !== undefined) {
    return zstdAvailability;
  }
  const result = spawnSync("zstd", ["-V"], { encoding: "utf8" });
  zstdAvailability = result.error === undefined && result.status === 0;
  return zstdAvailability;
}

export class TraceJsonlStore {
  readonly #stateDir: string;
  readonly #segmentMaxBytes: number;
  readonly #sessions = new Map<string, SessionTraceState>();

  constructor(options: TraceJsonlStoreOptions) {
    this.#stateDir = options.stateDir;
    this.#segmentMaxBytes = options.segmentMaxBytes ?? DEFAULT_TRACE_SEGMENT_MAX_BYTES;
  }

  ensureLayout(): void {
    fs.mkdirSync(this.#traceRoot(), { recursive: true });
  }

  appendEvent(event: PersistedAgentTraceEvent): void {
    this.#appendRecord(event.sessionKey, event);
    const state = this.#loadSession(event.sessionKey);
    state.eventsById.set(event.id, event);
    state.eventAppendCount += 1;
  }

  appendSummarySnapshot(snapshot: TraceSummarySnapshotRecord): void {
    this.#appendRecord(snapshot.sessionKey, snapshot);
  }

  eventAppendCount(sessionKey: string): number {
    return this.#loadSession(sessionKey).eventAppendCount;
  }

  listAgentTraceEvents(sessionKey: string, limit = 1000): PersistedAgentTraceEvent[] {
    const events = this.#uniqueEvents(sessionKey);
    events.sort(compareTraceEventsOldestFirst);
    return events.slice(0, Math.max(0, limit));
  }

  getAgentTraceEvent(sessionKey: string, id: string): PersistedAgentTraceEvent | undefined {
    return this.#loadSession(sessionKey).eventsById.get(id);
  }

  listAgentTraceEventsPage(
    sessionKey: string,
    options?: {
      readonly limit?: number | undefined;
      readonly beforeSequence?: number | undefined;
    },
  ): AgentTraceEventsPage {
    const limit = clampPositiveInteger(options?.limit ?? 100, 1, 500);
    const beforeSequence = Number(options?.beforeSequence ?? 0);
    const hasBefore = Number.isFinite(beforeSequence) && beforeSequence > 0;
    const cutoff = hasBefore ? Math.floor(beforeSequence) : undefined;
    const events = this.#uniqueEvents(sessionKey).filter((event) => cutoff === undefined || event.sequence < cutoff);
    events.sort(compareTraceEventsNewestFirst);
    const pageRows = events.slice(0, limit);
    const hasMore = events.length > limit;
    const page = pageRows.slice().reverse();
    return {
      events: page,
      hasMore,
      nextBeforeSequence: page.length ? Math.min(...page.map((event) => event.sequence)) : null,
    };
  }

  getMatchingToolCallForResult(record: PersistedAgentTraceEvent): PersistedAgentTraceEvent | undefined {
    const key = traceToolEventKeyParts(record);
    if (!key) {
      return undefined;
    }
    const matches = [...this.#loadSession(record.sessionKey).eventsById.values()].filter((event) => event.type === "agent_tool_call" && traceEventMatchesToolKey(event, key));
    matches.sort(compareTraceEventsNewestFirst);
    return matches[0];
  }

  hasCompletedToolResultForToolCall(record: PersistedAgentTraceEvent, excludeEventId?: string | undefined): boolean {
    if (record.type !== "agent_tool_call" && record.type !== "agent_tool_result") {
      return false;
    }
    const key = traceToolEventKeyParts(record);
    if (!key) {
      return false;
    }
    for (const event of this.#loadSession(record.sessionKey).eventsById.values()) {
      if (event.type !== "agent_tool_result" || event.id === excludeEventId || !traceEventMatchesToolKey(event, key)) {
        continue;
      }
      return true;
    }
    return false;
  }

  *iterateRecords(sessionKey: string): Generator<TraceJsonlRecord> {
    for (const segment of this.#listSegments(sessionKey)) {
      for (const record of parseTraceRecords(readSegmentText(segment))) {
        yield record;
      }
    }
  }

  listSessionKeys(): string[] {
    this.ensureLayout();
    const sessionKeys: string[] = [];
    for (const entry of fs.readdirSync(this.#traceRoot(), { withFileTypes: true })) {
      if (!entry.isDirectory()) {
        continue;
      }
      sessionKeys.push(sessionKeyFromDirectoryName(entry.name));
    }
    return sessionKeys;
  }

  deleteSession(sessionKey: string): void {
    this.#sessions.delete(sessionKey);
    fs.rmSync(this.#sessionDir(sessionKey), { recursive: true, force: true });
  }

  clearCache(): void {
    this.#sessions.clear();
  }

  #appendRecord(sessionKey: string, record: PersistedAgentTraceEvent | TraceSummarySnapshotRecord): void {
    const state = this.#loadSession(sessionKey);
    const line = `${JSON.stringify(record)}\n`;
    const lineBytes = Buffer.byteLength(line, "utf8");
    if (state.activeSize > 0 && state.activeSize + lineBytes > this.#segmentMaxBytes) {
      this.#sealActiveSegment(sessionKey, state);
    }
    fs.mkdirSync(this.#sessionDir(sessionKey), { recursive: true });
    const activePath = this.#plainSegmentPath(sessionKey, state.activeIndex);
    fs.appendFileSync(activePath, line, "utf8");
    state.activeSize += lineBytes;
  }

  #sealActiveSegment(sessionKey: string, state: SessionTraceState): void {
    const plainPath = this.#plainSegmentPath(sessionKey, state.activeIndex);
    if (fs.existsSync(plainPath) && isZstdAvailable()) {
      const zstPath = `${plainPath}.zst`;
      const result = spawnSync("zstd", ["-T0", "-f", "-o", zstPath, plainPath], { encoding: "utf8" });
      if (result.error === undefined && result.status === 0 && fs.existsSync(zstPath)) {
        fs.rmSync(plainPath, { force: true });
      } else {
        logger.warn("Failed to compress sealed trace segment; leaving it uncompressed", {
          sessionKey,
          segment: path.basename(plainPath),
          error: result.error ? String(result.error) : result.stderr.trim(),
        });
      }
    } else if (fs.existsSync(plainPath) && !isZstdAvailable()) {
      warnZstdMissing();
    }
    state.activeIndex += 1;
    state.activeSize = 0;
  }

  #uniqueEvents(sessionKey: string): PersistedAgentTraceEvent[] {
    return [...this.#loadSession(sessionKey).eventsById.values()];
  }

  #loadSession(sessionKey: string): SessionTraceState {
    const cached = this.#sessions.get(sessionKey);
    if (cached?.loaded) {
      return cached;
    }
    const eventsById = new Map<string, PersistedAgentTraceEvent>();
    let eventAppendCount = 0;
    for (const record of this.iterateRecords(sessionKey)) {
      if (record.kind !== "event") {
        continue;
      }
      eventsById.set(record.event.id, record.event);
      eventAppendCount += 1;
    }
    const segments = this.#listSegments(sessionKey);
    const active = resolveActiveSegment(segments);
    const state: SessionTraceState = {
      eventsById,
      eventAppendCount,
      activeIndex: active.index,
      activeSize: active.size,
      loaded: true,
    };
    this.#sessions.set(sessionKey, state);
    return state;
  }

  #listSegments(sessionKey: string): TraceSegment[] {
    const directory = this.#sessionDir(sessionKey);
    if (!fs.existsSync(directory)) {
      return [];
    }
    const byIndex = new Map<number, TraceSegment>();
    for (const name of fs.readdirSync(directory)) {
      const compressedMatch = name.match(/^(\d{6})\.jsonl\.zst$/);
      if (compressedMatch?.[1]) {
        const index = Number(compressedMatch[1]);
        byIndex.set(index, {
          index,
          filePath: path.join(directory, name),
          compressed: true,
        });
        continue;
      }
      const plainMatch = name.match(/^(\d{6})\.jsonl$/);
      if (!plainMatch?.[1]) {
        continue;
      }
      const index = Number(plainMatch[1]);
      if (byIndex.get(index)?.compressed) {
        continue;
      }
      byIndex.set(index, {
        index,
        filePath: path.join(directory, name),
        compressed: false,
      });
    }
    return [...byIndex.values()].sort((left, right) => left.index - right.index);
  }

  #plainSegmentPath(sessionKey: string, index: number): string {
    return path.join(this.#sessionDir(sessionKey), `${String(index).padStart(TRACE_SEGMENT_INDEX_DIGITS, "0")}.jsonl`);
  }

  #sessionDir(sessionKey: string): string {
    return traceSessionDirectory(this.#stateDir, sessionKey);
  }

  #traceRoot(): string {
    return path.join(this.#stateDir, TRACE_DIRECTORY_NAME);
  }
}

function resolveActiveSegment(segments: readonly TraceSegment[]): { index: number; size: number } {
  const plainSegments = segments.filter((segment) => !segment.compressed);
  const active = plainSegments.reduce<TraceSegment | undefined>((current, segment) => {
    if (!current || segment.index >= current.index) {
      return segment;
    }
    return current;
  }, undefined);
  if (!active) {
    const last = segments.at(-1);
    return {
      index: last ? last.index + 1 : 0,
      size: 0,
    };
  }
  return {
    index: active.index,
    size: fs.existsSync(active.filePath) ? fs.statSync(active.filePath).size : 0,
  };
}

function readSegmentText(segment: TraceSegment): string {
  if (!segment.compressed) {
    return fs.readFileSync(segment.filePath, "utf8");
  }
  if (!isZstdAvailable()) {
    logger.warn("Cannot read compressed trace segment because zstd is not on PATH", {
      path: segment.filePath,
    });
    return "";
  }
  const result = spawnSync("zstd", ["-dc", segment.filePath], {
    encoding: "utf8",
    maxBuffer: ZSTD_DECODE_MAX_BUFFER_BYTES,
  });
  if (result.error || result.status !== 0) {
    logger.warn("Failed to decompress trace segment", {
      path: segment.filePath,
      error: result.error ? String(result.error) : result.stderr.trim(),
    });
    return "";
  }
  return result.stdout;
}

function parseTraceRecords(text: string): TraceJsonlRecord[] {
  const records: TraceJsonlRecord[] = [];
  for (const rawLine of text.split("\n")) {
    const line = rawLine.replace(/\r$/, "").trim();
    if (!line) {
      continue;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(line) as unknown;
    } catch {
      continue;
    }
    const snapshot = parseSummarySnapshot(parsed);
    if (snapshot) {
      records.push({ kind: "snapshot", snapshot });
      continue;
    }
    const event = parseTraceEvent(parsed);
    if (event) {
      records.push({ kind: "event", event });
    }
  }
  return records;
}

function parseSummarySnapshot(value: unknown): TraceSummarySnapshotRecord | undefined {
  if (!value || typeof value !== "object") {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  if (record.type !== "summary_snapshot" || !record.summary || typeof record.summary !== "object") {
    return undefined;
  }
  const summary = record.summary as Record<string, unknown>;
  return {
    type: "summary_snapshot",
    sessionKey: String(record.sessionKey ?? ""),
    sequence: normalizeFiniteNumber(record.sequence) ?? 0,
    summary: {
      eventCount: normalizeFiniteNumber(summary.eventCount) ?? 0,
      modelRequestCount: normalizeFiniteNumber(summary.modelRequestCount) ?? 0,
      categories: readCountMap(summary.categories),
      sources: readCountMap(summary.sources),
    },
  };
}

function parseTraceEvent(value: unknown): PersistedAgentTraceEvent | undefined {
  if (!value || typeof value !== "object") {
    return undefined;
  }
  const record = value as Record<string, unknown>;
  if (record.type === "summary_snapshot" || !record.id || !record.sessionKey || !record.source || !record.type || !record.at || !record.title) {
    return undefined;
  }
  const now = new Date().toISOString();
  return {
    id: String(record.id),
    sessionKey: String(record.sessionKey),
    source: record.source === "broker" ? "broker" : "agent_runtime",
    type: String(record.type),
    at: String(record.at),
    sequence: normalizeFiniteNumber(record.sequence) ?? 0,
    title: String(record.title),
    summary: String(record.summary ?? ""),
    detail: typeof record.detail === "string" ? record.detail : undefined,
    status: typeof record.status === "string" ? record.status : undefined,
    role: typeof record.role === "string" ? record.role : undefined,
    toolName: typeof record.toolName === "string" ? record.toolName : undefined,
    callId: typeof record.callId === "string" ? record.callId : undefined,
    turnId: typeof record.turnId === "string" ? record.turnId : undefined,
    detailTruncated: typeof record.detailTruncated === "boolean" ? record.detailTruncated : undefined,
    detailOriginalChars: normalizeFiniteNumber(record.detailOriginalChars),
    metadata: record.metadata as JsonLike | undefined,
    createdAt: String(record.createdAt ?? now),
    updatedAt: String(record.updatedAt ?? record.createdAt ?? now),
  };
}

function sessionKeyFromDirectoryName(name: string): string {
  try {
    return decodeURIComponent(name);
  } catch {
    return name;
  }
}

function warnZstdMissing(): void {
  if (zstdMissingWarned) {
    return;
  }
  zstdMissingWarned = true;
  logger.warn("zstd not found on PATH; leaving sealed trace segments uncompressed");
}

function compareTraceEventsNewestFirst(left: PersistedAgentTraceEvent, right: PersistedAgentTraceEvent): number {
  if (left.sequence !== right.sequence) {
    return right.sequence - left.sequence;
  }
  if (left.at !== right.at) {
    return left.at < right.at ? 1 : -1;
  }
  if (left.id === right.id) {
    return 0;
  }
  return left.id < right.id ? 1 : -1;
}

function compareTraceEventsOldestFirst(left: PersistedAgentTraceEvent, right: PersistedAgentTraceEvent): number {
  return compareTraceEventsNewestFirst(right, left);
}

export function traceToolEventKeyParts(event: PersistedAgentTraceEvent):
  | {
      readonly turnId: string;
      readonly callId?: string | undefined;
      readonly toolName?: string | undefined;
    }
  | undefined {
  const turnId = event.turnId ?? "";
  if (event.callId) {
    return {
      turnId,
      callId: event.callId,
    };
  }
  if (!turnId && !event.toolName) {
    return undefined;
  }
  return {
    turnId,
    toolName: event.toolName ?? "",
  };
}

export function traceToolEventKey(event: PersistedAgentTraceEvent): string {
  const key = traceToolEventKeyParts(event);
  if (!key) {
    return "";
  }
  return key.callId ? [key.turnId, key.callId].join("\u001f") : [key.turnId, key.toolName ?? ""].join("\u001f");
}

function traceEventMatchesToolKey(
  event: PersistedAgentTraceEvent,
  key: {
    readonly turnId: string;
    readonly callId?: string | undefined;
    readonly toolName?: string | undefined;
  },
): boolean {
  if ((event.turnId ?? "") !== key.turnId) {
    return false;
  }
  if (key.callId) {
    return event.callId === key.callId;
  }
  return (event.toolName ?? "") === (key.toolName ?? "");
}

function readCountMap(value: unknown): Record<string, number> {
  if (!value || typeof value !== "object") {
    return {};
  }
  const mapped: Record<string, number> = {};
  for (const [key, count] of Object.entries(value as Record<string, unknown>)) {
    const parsed = normalizeFiniteNumber(count);
    if (parsed !== undefined) {
      mapped[key] = parsed;
    }
  }
  return mapped;
}

function normalizeFiniteNumber(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }
  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }
  return undefined;
}

function clampPositiveInteger(value: unknown, min: number, max: number): number {
  const number = Number(value);
  if (!Number.isFinite(number)) {
    return min;
  }
  return Math.max(min, Math.min(max, Math.floor(number)));
}
