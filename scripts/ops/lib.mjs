#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
export const repoRoot = path.resolve(scriptDir, "..", "..");

export function runCommand(command, args, options = {}) {
  const { capture = false, cwd = repoRoot, env = undefined, input = undefined } = options;

  const result = spawnSync(command, args, {
    cwd,
    encoding: "utf8",
    env: env ? { ...process.env, ...env } : process.env,
    input,
    stdio: capture ? ["pipe", "pipe", "pipe"] : "inherit",
  });

  if (result.status !== 0) {
    const details = capture ? sanitizeOpsCommandErrorText([result.stdout, result.stderr].filter(Boolean).join("\n").trim()) : "";
    throw new Error(`Command failed (${result.status ?? "null"}): ${formatCommandForError(command, args)}${details ? `\n${details}` : ""}`);
  }

  return capture ? result.stdout.trim() : "";
}

export function getEnvObjectFromInspect(inspect) {
  const env = {};
  for (const entry of inspect.Config?.Env ?? []) {
    const equalsIndex = entry.indexOf("=");
    if (equalsIndex <= 0) {
      continue;
    }

    env[entry.slice(0, equalsIndex)] = entry.slice(equalsIndex + 1);
  }

  return env;
}

export function shouldRunFeishuPreflight(inspect) {
  const env = getEnvObjectFromInspect(inspect);
  return env.FEISHU_ENABLED?.trim().toLowerCase() === "true";
}

export function getAdminHeadersFromInspect(inspect) {
  const env = getEnvObjectFromInspect(inspect);
  const adminToken = env.BROKER_ADMIN_TOKEN;
  return adminToken
    ? {
        "x-admin-token": adminToken,
      }
    : {};
}

const PLATFORM_HEALTH_STATES = new Set(["disabled", "starting", "ready", "degraded", "failed"]);
const PLATFORM_CONNECTION_MODES = new Set(["socket_mode", "long_connection", "http"]);
const FEISHU_GROUP_MESSAGE_MODES = new Set(["all", "at_only"]);
const PERMISSION_STATUSES = new Set(["unknown", "configured", "verified", "missing"]);
const SAFE_HEALTH_TOKEN = /^[a-z][a-z0-9_.:-]*$/u;
const SAFE_OPS_LOG_TOKEN = /^[a-z][a-z0-9_.:-]*$/u;
const SAFE_OPS_LOG_TIMESTAMP = /^[0-9TZ:.+-]+$/u;
const SECRET_LIKE_OPS_LOG_META_VALUE = /\b(?:xox[abprs]-[A-Za-z0-9_-]+|xapp-[A-Za-z0-9_-]+|Bearer\s+\S+|[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,})\b|-----BEGIN [A-Z ]*PRIVATE KEY-----/iu;
const SECRET_LIKE_OPS_COMMAND_ERROR_VALUE = /\b(?:xox[abprs]-[A-Za-z0-9_-]+|xapp-[A-Za-z0-9_-]+|Bearer\s+\S+|[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,})\b|-----BEGIN [A-Z ]*PRIVATE KEY-----/giu;
const SENTINEL_LIKE_OPS_PATH_VALUE = /\b[A-Z0-9_]*(?:SECRET|BODY|PAYLOAD)[A-Z0-9_]*\b/u;
const SENTINEL_LIKE_OPS_COMMAND_ERROR_VALUE = /\b[A-Z0-9_]*(?:SECRET|BODY|PAYLOAD)[A-Z0-9_]*\b/gu;
const OPS_SAFE_COMMAND_ERROR_LITERALS = ["FEISHU_APP_SECRET"];
const OPS_SAFE_TEXT_LOG_MARKERS = ["Codex app-server client connected", "Connected to Slack Socket Mode", "Service booted", "event-dispatch is ready"];
const OPS_SAFE_LOG_META_FIELDS = new Set([
  "ackDurationMs",
  "attempt",
  "attachmentId",
  "batchId",
  "candidateRevision",
  "codexThreadId",
  "confirmedCount",
  "conversationId",
  "conversationKind",
  "degradedReason",
  "durationMs",
  "errorClass",
  "eventId",
  "fileId",
  "format",
  "groupMessageMode",
  "hadActiveTurn",
  "handler",
  "ignoredReason",
  "jobId",
  "kind",
  "messageCursor",
  "messageId",
  "msgType",
  "payloadRef",
  "permission",
  "platform",
  "platformThreadId",
  "recoveredCount",
  "rootMessageId",
  "route",
  "senderKind",
  "sessionKey",
  "source",
  "startupRequired",
  "statusCode",
  "turnId",
]);

export function summarizePlatformHealth(adminStatus) {
  const platforms = adminStatus?.platforms && typeof adminStatus.platforms === "object" ? adminStatus.platforms : {};

  return Object.fromEntries(
    ["slack", "feishu"].map((platform) => {
      const status = platforms[platform] && typeof platforms[platform] === "object" ? platforms[platform] : {};

      return [
        platform,
        {
          platform,
          enabled: Boolean(status.enabled),
          state: sanitizeKnownValue(status.state, PLATFORM_HEALTH_STATES) ?? "unknown",
          startupRequired: Boolean(status.startupRequired),
          groupMessageMode: sanitizeKnownValue(status.groupMessageMode, FEISHU_GROUP_MESSAGE_MODES),
          allMessageDeliveryVerified: typeof status.allMessageDeliveryVerified === "boolean" ? status.allMessageDeliveryVerified : undefined,
          degradedReason: sanitizeHealthToken(status.degradedReason),
          connection:
            status.connection && typeof status.connection === "object"
              ? {
                  mode: sanitizeKnownValue(status.connection.mode, PLATFORM_CONNECTION_MODES),
                  connected: Boolean(status.connection.connected),
                }
              : undefined,
          permissions: Array.isArray(status.permissions) ? status.permissions.map(summarizePermissionHealth).filter(Boolean) : undefined,
        },
      ];
    }),
  );
}

function summarizePermissionHealth(permission) {
  if (!permission || typeof permission !== "object") {
    return undefined;
  }

  const name = sanitizeHealthToken(permission.name);
  const status = sanitizeKnownValue(permission.status, PERMISSION_STATUSES);
  if (!name || !status) {
    return undefined;
  }

  return {
    name,
    status,
  };
}

function sanitizeKnownValue(value, allowedValues) {
  return typeof value === "string" && allowedValues.has(value) ? value : undefined;
}

function sanitizeHealthToken(value) {
  return typeof value === "string" && SAFE_HEALTH_TOKEN.test(value) ? value : undefined;
}

export function summarizeOpsHostPath(filePath) {
  const basename = sanitizeOpsPathBasename(filePath);
  return withoutUndefined({
    basename,
    redacted: Boolean(readString(filePath)),
  });
}

export function summarizeOpsEvidencePath(filePath) {
  const basename = sanitizeOpsPathBasename(filePath);
  return withoutUndefined({
    relativePath: sanitizeOpsRepoRelativePath(filePath),
    basename,
    redacted: Boolean(readString(filePath)),
  });
}

export function summarizeOpsDisplayPath(filePath) {
  const evidencePath = summarizeOpsEvidencePath(filePath);
  if (evidencePath.relativePath) {
    return evidencePath.relativePath;
  }

  const hostPath = summarizeOpsHostPath(filePath);
  return hostPath.basename ? `${hostPath.basename} (path redacted)` : undefined;
}

export function sanitizeOpsLogsForEvidence(text) {
  const lines =
    readString(text)
      ?.split(/\r?\n/u)
      .filter((line) => line.trim()) ?? [];
  return lines.map((line) => JSON.stringify(sanitizeOpsLogLine(line))).join("\n") + (lines.length > 0 ? "\n" : "");
}

function formatCommandForError(command, args) {
  return [command, ...args].map(sanitizeOpsCommandErrorText).join(" ");
}

function sanitizeOpsCommandErrorText(value) {
  const text = readString(value);
  if (!text) {
    return "";
  }

  const protectedLiterals = new Map();
  const protectedText = OPS_SAFE_COMMAND_ERROR_LITERALS.reduce((current, literal, index) => {
    const placeholder = `__ops_safe_error_literal_${index}__`;
    protectedLiterals.set(placeholder, literal);
    return current.replaceAll(literal, placeholder);
  }, text);

  const sanitized = protectedText
    .replace(/(["'])(\/[^"']+)\1/gu, (_match, quote, filePath) => `${quote}${summarizeOpsDisplayPath(filePath) ?? "[redacted-path]"}${quote}`)
    .replace(/(^|[\s=])\/(?!\/)([^\s'"]+)/gu, (_match, prefix, pathTail) => `${prefix}${summarizeOpsDisplayPath(`/${pathTail}`) ?? "[redacted-path]"}`)
    .replace(SECRET_LIKE_OPS_COMMAND_ERROR_VALUE, "[redacted unsafe ops output]")
    .replace(SENTINEL_LIKE_OPS_COMMAND_ERROR_VALUE, "[redacted unsafe ops output]");

  return [...protectedLiterals.entries()].reduce((current, [placeholder, literal]) => current.replaceAll(placeholder, literal), sanitized);
}

function sanitizeOpsMetadataValue(value) {
  if (typeof value === "string") {
    return sanitizeOpsCommandErrorText(value);
  }

  if (Array.isArray(value)) {
    return value.map(sanitizeOpsMetadataValue);
  }

  if (value && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value)
        .map(([key, nested]) => [key, sanitizeOpsMetadataValue(nested)])
        .filter(([, nested]) => nested !== undefined),
    );
  }

  return value;
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

async function retryUntil(label, operation, options = {}) {
  const timeoutMs = options.timeoutMs ?? 30_000;
  const intervalMs = options.intervalMs ?? 1_000;
  const startedAt = Date.now();
  let lastError = undefined;

  while (Date.now() - startedAt < timeoutMs) {
    try {
      return await operation();
    } catch (error) {
      lastError = error;
      await sleep(intervalMs);
    }
  }

  const reason = lastError instanceof Error ? lastError.message : String(lastError);
  throw new Error(`${label} did not succeed within ${timeoutMs}ms: ${reason}`);
}

export async function writeRolloutMetadata(directory, payload) {
  await fsp.mkdir(directory, { recursive: true });
  await fsp.writeFile(path.join(directory, "metadata.json"), `${JSON.stringify(sanitizeOpsMetadataValue(payload), null, 2)}\n`);
}

const STATE_DATABASE_FILENAME = "broker.sqlite";
const BROKER_SQLITE_SESSIONS_SQL = `
  SELECT key, platform, conversation_id, conversation_kind, root_message_id, platform_thread_id,
         channel_id, root_thread_ts, workspace_path, created_at, updated_at, active_turn_id,
         active_turn_started_at, last_observed_message_ts, last_delivered_message_ts, last_slack_reply_at
  FROM sessions
`;
const BROKER_SQLITE_OPEN_INBOUND_SQL = `
  SELECT key, session_key, channel_id, channel_type, root_thread_ts, message_ts, source,
         sender_kind, status, batch_id, created_at, updated_at, text
  FROM inbound_messages
  WHERE status IN ('pending', 'inflight')
`;
const BROKER_SQLITE_BACKGROUND_JOBS_SQL = `
  SELECT id, session_key, channel_id, root_thread_ts, kind, status, cwd, restart_on_boot,
         created_at, updated_at, started_at, heartbeat_at, completed_at, cancelled_at,
         exit_code, error, last_event_at, last_event_kind
  FROM background_jobs
`;

function emptyBrokerSqliteState() {
  return {
    sessions: [],
    inboundMessages: [],
    backgroundJobs: [],
  };
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function logOpsStateReadReason(reason) {
  console.error(`ops: reporting empty session/inbound/job counts (${sanitizeOpsCommandErrorText(reason)})`);
}

function mapSqlRowToCamelRecord(row) {
  if (!row || typeof row !== "object") {
    return {};
  }

  return Object.fromEntries(Object.entries(row).map(([column, value]) => [sqlColumnToCamelCase(column), normalizeSqlColumnValue(column, value)]));
}

function sqlColumnToCamelCase(column) {
  return column.replaceAll(/_([a-z])/gu, (_match, letter) => letter.toUpperCase());
}

function normalizeSqlColumnValue(column, value) {
  if (value === null || value === undefined) {
    return undefined;
  }

  const scalar = typeof value === "bigint" ? Number(value) : value;
  if (column === "restart_on_boot") {
    return typeof scalar === "boolean" ? scalar : typeof scalar === "number" ? scalar !== 0 : undefined;
  }

  if (column === "exit_code") {
    return typeof scalar === "number" && Number.isFinite(scalar) ? scalar : undefined;
  }

  return scalar;
}

async function readBrokerSqliteState(stateRoot) {
  const databasePath = path.join(stateRoot, STATE_DATABASE_FILENAME);
  try {
    await fsp.access(databasePath);
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return emptyBrokerSqliteState();
    }

    logOpsStateReadReason(`unable to access ${STATE_DATABASE_FILENAME}: ${errorMessage(error)}`);
    return emptyBrokerSqliteState();
  }

  const nodeResult = await readBrokerSqliteStateWithNode(databasePath);
  if (nodeResult.ok) {
    return nodeResult;
  }

  const cliResult = readBrokerSqliteStateWithCli(databasePath);
  if (cliResult.ok) {
    return cliResult;
  }

  logOpsStateReadReason(`${nodeResult.reason}; ${cliResult.reason}`);
  return emptyBrokerSqliteState();
}

async function readBrokerSqliteStateWithNode(databasePath) {
  let DatabaseSync;
  try {
    ({ DatabaseSync } = await import("node:sqlite"));
  } catch (error) {
    return { ok: false, reason: `node:sqlite unavailable: ${errorMessage(error)}` };
  }

  let database;
  try {
    database = new DatabaseSync(databasePath, { readOnly: true });
    return {
      ok: true,
      sessions: database.prepare(BROKER_SQLITE_SESSIONS_SQL).all().map(mapSqlRowToCamelRecord),
      inboundMessages: database.prepare(BROKER_SQLITE_OPEN_INBOUND_SQL).all().map(mapSqlRowToCamelRecord),
      backgroundJobs: database.prepare(BROKER_SQLITE_BACKGROUND_JOBS_SQL).all().map(mapSqlRowToCamelRecord),
    };
  } catch (error) {
    return { ok: false, reason: `node:sqlite read failed: ${errorMessage(error)}` };
  } finally {
    try {
      database?.close();
    } catch {
      // ignore close failures after a successful or failed snapshot
    }
  }
}

function readBrokerSqliteStateWithCli(databasePath) {
  const sessions = sqlite3JsonQuery(databasePath, BROKER_SQLITE_SESSIONS_SQL);
  if (!sessions.ok) {
    return sessions;
  }

  const inboundMessages = sqlite3JsonQuery(databasePath, BROKER_SQLITE_OPEN_INBOUND_SQL);
  if (!inboundMessages.ok) {
    return inboundMessages;
  }

  const backgroundJobs = sqlite3JsonQuery(databasePath, BROKER_SQLITE_BACKGROUND_JOBS_SQL);
  if (!backgroundJobs.ok) {
    return backgroundJobs;
  }

  return {
    ok: true,
    sessions: sessions.rows.map(mapSqlRowToCamelRecord),
    inboundMessages: inboundMessages.rows.map(mapSqlRowToCamelRecord),
    backgroundJobs: backgroundJobs.rows.map(mapSqlRowToCamelRecord),
  };
}

function sqlite3JsonQuery(databasePath, sql) {
  const result = spawnSync("sqlite3", ["-readonly", "-json", databasePath, sql], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.error) {
    return { ok: false, reason: `sqlite3 CLI unavailable: ${errorMessage(result.error)}` };
  }

  if (result.status !== 0) {
    const details = [result.stderr, result.stdout].filter(Boolean).join("\n").trim();
    return { ok: false, reason: `sqlite3 CLI failed: ${details || `exit ${result.status}`}` };
  }

  const text = result.stdout.trim();
  if (!text) {
    return { ok: true, rows: [] };
  }

  try {
    const parsed = JSON.parse(text);
    return { ok: true, rows: Array.isArray(parsed) ? parsed : [] };
  } catch (error) {
    return { ok: false, reason: `sqlite3 CLI JSON parse failed: ${errorMessage(error)}` };
  }
}

async function readLastJsonlLines(filePath, limit) {
  try {
    const text = await fsp.readFile(filePath, "utf8");
    return text
      .trim()
      .split("\n")
      .filter(Boolean)
      .slice(-limit)
      .map((line) => {
        try {
          return JSON.parse(line);
        } catch {
          return {
            type: "log_parse_error",
            message: "unparseable broker log line",
          };
        }
      });
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return [];
    }

    throw error;
  }
}

function summarizeOpsSession(session) {
  return withoutUndefined({
    key: readString(session?.key),
    sessionKey: readString(session?.key),
    platform: readString(session?.platform ?? "slack"),
    conversationId: readString(session?.conversationId ?? session?.channelId),
    conversationKind: readString(session?.conversationKind ?? "channel"),
    rootMessageId: readString(session?.rootMessageId ?? session?.rootThreadTs),
    platformThreadId: readString(session?.platformThreadId),
    channelId: readString(session?.channelId),
    rootThreadTs: readString(session?.rootThreadTs),
    workspacePathBasename: sanitizeOpsPathBasename(session?.workspacePath),
    createdAt: readString(session?.createdAt),
    updatedAt: readString(session?.updatedAt),
    activeTurnId: readString(session?.activeTurnId),
    activeTurnStartedAt: readString(session?.activeTurnStartedAt),
    lastObservedMessageTs: readString(session?.lastObservedMessageTs),
    lastDeliveredMessageTs: readString(session?.lastDeliveredMessageTs),
    lastSlackReplyAt: readString(session?.lastSlackReplyAt),
  });
}

function summarizeOpsInboundMessage(message) {
  const text = readString(message?.text) ?? "";
  return withoutUndefined({
    key: readString(message?.key),
    sessionKey: readString(message?.sessionKey),
    channelId: readString(message?.channelId),
    channelType: readString(message?.channelType),
    rootThreadTs: readString(message?.rootThreadTs),
    messageTs: readString(message?.messageTs),
    source: readString(message?.source),
    senderKind: readString(message?.senderKind),
    status: readString(message?.status),
    batchId: readString(message?.batchId),
    createdAt: readString(message?.createdAt),
    updatedAt: readString(message?.updatedAt),
    textPreview: redactOpsInboundTextPreview(text),
    textLength: text.length,
    textRedacted: true,
  });
}

function summarizeOpsBackgroundJob(job) {
  const error = readString(job?.error);
  return withoutUndefined({
    id: readString(job?.id),
    jobId: readString(job?.id),
    sessionKey: readString(job?.sessionKey),
    channelId: readString(job?.channelId),
    rootThreadTs: readString(job?.rootThreadTs),
    kind: readString(job?.kind),
    status: readString(job?.status),
    cwdBasename: sanitizeOpsPathBasename(job?.cwd),
    restartOnBoot: typeof job?.restartOnBoot === "boolean" ? job.restartOnBoot : undefined,
    createdAt: readString(job?.createdAt),
    updatedAt: readString(job?.updatedAt),
    startedAt: readString(job?.startedAt),
    heartbeatAt: readString(job?.heartbeatAt),
    completedAt: readString(job?.completedAt),
    cancelledAt: readString(job?.cancelledAt),
    exitCode: typeof job?.exitCode === "number" ? job.exitCode : undefined,
    errorLength: error ? error.length : undefined,
    errorRedacted: error ? true : undefined,
    lastEventAt: readString(job?.lastEventAt),
    lastEventKind: readString(job?.lastEventKind),
  });
}

function sanitizeOpsBrokerLogRecord(record) {
  const parsed = asRecord(record);
  if (!parsed) {
    return {
      type: "log_parse_error",
      message: "non-object broker log line",
    };
  }

  if (parsed.type === "log_parse_error") {
    return {
      type: "log_parse_error",
      message: "unparseable broker log line",
    };
  }

  const meta = sanitizeOpsLogMeta(asRecord(parsed.meta));
  return withoutUndefined({
    ts: readSafeOpsLogTimestamp(parsed.ts),
    type: readSafeOpsLogToken(parsed.type),
    level: readSafeOpsLogToken(parsed.level),
    message: readSafeOpsLogToken(parsed.message),
    meta,
  });
}

function sanitizeOpsLogLine(line) {
  const trimmed = line.trim();
  try {
    return sanitizeOpsBrokerLogRecord(JSON.parse(trimmed));
  } catch {
    // Raw logs may include the human console formatter rather than JSONL.
  }

  const textRecord = parseOpsTextLogLine(trimmed);
  if (textRecord) {
    return textRecord;
  }

  const marker = OPS_SAFE_TEXT_LOG_MARKERS.find((candidate) => trimmed.includes(candidate));
  return withoutUndefined({
    type: "log_text_redacted",
    message: marker ?? "non-structured log line redacted",
    length: trimmed.length,
  });
}

function parseOpsTextLogLine(line) {
  const match = /^(\S+)\s+(DEBUG|INFO|WARN|ERROR)\s+([a-z][a-z0-9_.:-]*)(?:\s+(.+))?$/u.exec(line);
  if (!match) {
    return undefined;
  }

  const [, timestamp, level, message, rest] = match;
  const metaText = rest?.trim();
  const metaCandidate = metaText?.startsWith("{") ? parseJsonObject(metaText) : undefined;
  return withoutUndefined({
    ts: readSafeOpsLogTimestamp(timestamp),
    type: "log",
    level: level.toLowerCase(),
    message: readSafeOpsLogToken(message),
    meta: sanitizeOpsLogMeta(metaCandidate),
  });
}

function sanitizeOpsLogMeta(meta) {
  if (!meta) {
    return undefined;
  }

  const safeEntries = Object.entries(meta).filter(([key, value]) => OPS_SAFE_LOG_META_FIELDS.has(key) && isSafeOpsLogMetaValue(value));
  return safeEntries.length > 0 ? Object.fromEntries(safeEntries) : undefined;
}

function parseJsonObject(text) {
  try {
    return asRecord(JSON.parse(text));
  } catch {
    return undefined;
  }
}

function isSafeOpsLogMetaValue(value) {
  if (typeof value === "string") {
    return !SECRET_LIKE_OPS_LOG_META_VALUE.test(value);
  }

  return typeof value === "number" || typeof value === "boolean" || value === null;
}

function redactOpsInboundTextPreview(text) {
  return `message body redacted (${text.length} chars)`;
}

function sanitizeOpsPathBasename(filePath) {
  const text = readString(filePath);
  if (!text) {
    return undefined;
  }

  const basename = path.basename(text);
  if (!basename || basename === "." || SECRET_LIKE_OPS_LOG_META_VALUE.test(basename) || SENTINEL_LIKE_OPS_PATH_VALUE.test(basename)) {
    return "[redacted-path]";
  }

  return basename;
}

function sanitizeOpsRepoRelativePath(filePath) {
  const text = readString(filePath);
  if (!text) {
    return undefined;
  }

  const relativePath = path.relative(repoRoot, text);
  if (!relativePath || relativePath.startsWith("..") || path.isAbsolute(relativePath)) {
    return undefined;
  }

  const parts = relativePath.split(path.sep).filter(Boolean);
  if (parts.length === 0) {
    return undefined;
  }

  for (const part of parts) {
    if (sanitizeOpsPathBasename(part) !== part) {
      return undefined;
    }
  }

  return parts.join("/");
}

function readString(value) {
  return typeof value === "string" ? value : undefined;
}

function readSafeOpsLogToken(value) {
  const text = readString(value);
  return text && SAFE_OPS_LOG_TOKEN.test(text) ? text : undefined;
}

function readSafeOpsLogTimestamp(value) {
  const text = readString(value);
  return text && SAFE_OPS_LOG_TIMESTAMP.test(text) ? text : undefined;
}

function asRecord(value) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : undefined;
}

function withoutUndefined(record) {
  return Object.fromEntries(Object.entries(record).filter(([, value]) => value !== undefined));
}

export async function readDetailedStateFromHost(dataRootSource, options = {}) {
  const openInboundLimit = options.openInboundLimit ?? 20;
  const logLineLimit = options.logLineLimit ?? 40;
  const stateRoot = path.join(dataRootSource, "state");
  const logsRoot = path.join(dataRootSource, "logs");

  const { sessions, inboundMessages, backgroundJobs } = await readBrokerSqliteState(stateRoot);

  const activeSessions = sessions.filter((session) => session?.activeTurnId).sort((left, right) => String(right.updatedAt ?? "").localeCompare(String(left.updatedAt ?? "")));

  const openInbound = inboundMessages.filter((message) => message?.status === "pending" || message?.status === "inflight").sort((left, right) => String(left.updatedAt ?? "").localeCompare(String(right.updatedAt ?? "")));

  const brokerLogs = await readLastJsonlLines(path.join(logsRoot, "broker.jsonl"), logLineLimit);

  return {
    sessionCount: sessions.length,
    activeCount: activeSessions.length,
    activeSessions: activeSessions.map(summarizeOpsSession),
    openInboundCount: openInbound.length,
    openInbound: openInbound.slice(0, openInboundLimit).map(summarizeOpsInboundMessage),
    backgroundJobs: backgroundJobs.sort((left, right) => String(right.updatedAt ?? "").localeCompare(String(left.updatedAt ?? ""))).map(summarizeOpsBackgroundJob),
    recentBrokerLogs: brokerLogs.map(sanitizeOpsBrokerLogRecord),
  };
}
