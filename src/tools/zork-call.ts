#!/usr/bin/env node

import path from "node:path";
import { pathToFileURL } from "node:url";

import { readEnv, requestBroker, resolveChatCoordinates, resolveCoauthorCwd } from "./broker-http.js";

const USAGE = `Usage: zork-call <command>

Chat:
  zork-call chat post-message --text TEXT --kind progress|final|block|wait [--reason REASON]
  zork-call chat post-state --kind wait|block|final [--reason REASON]
  zork-call chat post-file --file-path ABS [--initial-comment TEXT]
  zork-call chat thread-history [--before-message-id ID | --before-cursor C] [--limit N] [--format json|text]

Notify:
  zork-call notify --text TEXT

Coauthor:
  zork-call coauthor status
  zork-call coauthor configure --coauthor NAME [--coauthor NAME ...] [--ignore-missing]

Jobs:
  zork-call job register --kind KIND --script SCRIPT [--cwd CWD] [--restart-on-boot true|false]

Integrations:
  zork-call integration list-tools --server linear|notion
  zork-call integration call --server linear|notion --name NAME [--json '{...}']

Session identity is resolved from CHAT_PLATFORM/CHAT_CONVERSATION_ID/CHAT_ROOT_MESSAGE_ID
or CODEX_THREAD_ID. Do not pass channel, conversation, or thread ids.

BROKER_API_BASE is required.
`;

const BOOLEAN_FLAGS = new Set(["ignore-missing", "help"]);
const MESSAGE_KINDS = new Set(["progress", "final", "block", "wait"]);
const STATE_KINDS = new Set(["wait", "block", "final"]);
const INTEGRATION_SERVERS = new Set(["linear", "notion"]);

interface ParsedFlags {
  readonly values: Map<string, string[]>;
  readonly bools: Set<string>;
}

export async function runZorkCall(argv: readonly string[]): Promise<void> {
  if (argv.length === 0 || argv[0] === "help" || argv[0] === "--help" || argv[0] === "-h") {
    process.stdout.write(USAGE);
    return;
  }

  const group = argv[0]!;
  if (group === "notify") {
    const flags = parseFlags(argv.slice(1));
    if (flags.bools.has("help")) {
      process.stdout.write(USAGE);
      return;
    }
    await runNotifyCommand(flags);
    return;
  }

  const action = argv[1];
  if (!action || action.startsWith("--")) {
    throw new Error(`unknown command: ${argv.join(" ")}`);
  }

  const flags = parseFlags(argv.slice(2));
  if (flags.bools.has("help")) {
    process.stdout.write(USAGE);
    return;
  }

  switch (group) {
    case "chat":
      await runChatCommand(action, flags);
      return;
    case "coauthor":
      await runCoauthorCommand(action, flags);
      return;
    case "job":
      await runJobCommand(action, flags);
      return;
    case "integration":
      await runIntegrationCommand(action, flags);
      return;
    default:
      throw new Error(`unknown command: ${group} ${action}`);
  }
}

async function runChatCommand(action: string, flags: ParsedFlags): Promise<void> {
  switch (action) {
    case "post-message": {
      rejectUnknownFlags(flags, ["text", "kind", "reason"]);
      const kind = requireOneOf(flags, "kind", MESSAGE_KINDS);
      const reason = optionalFlag(flags, "reason");
      requireReasonForStopKind(kind, reason);
      const coords = await resolveChatCoordinates();
      await requestBroker({
        method: "POST",
        path: "/chat/post-message",
        body: {
          platform: coords.platform,
          conversationId: coords.conversationId,
          rootMessageId: coords.rootMessageId,
          text: requiredFlag(flags, "text"),
          kind,
          ...(reason ? { reason } : {}),
        },
      });
      return;
    }
    case "post-state": {
      rejectUnknownFlags(flags, ["kind", "reason"]);
      const kind = requireOneOf(flags, "kind", STATE_KINDS);
      const reason = optionalFlag(flags, "reason");
      requireReasonForStopKind(kind, reason);
      const coords = await resolveChatCoordinates();
      await requestBroker({
        method: "POST",
        path: "/chat/post-state",
        body: {
          platform: coords.platform,
          conversationId: coords.conversationId,
          rootMessageId: coords.rootMessageId,
          kind,
          ...(reason ? { reason } : {}),
        },
      });
      return;
    }
    case "post-file": {
      rejectUnknownFlags(flags, ["file-path", "initial-comment"]);
      const filePath = requiredFlag(flags, "file-path");
      if (!path.isAbsolute(filePath)) {
        throw new Error("--file-path must be an absolute path");
      }
      const initialComment = optionalFlag(flags, "initial-comment");
      const coords = await resolveChatCoordinates();
      await requestBroker({
        method: "POST",
        path: "/chat/post-file",
        body: {
          platform: coords.platform,
          conversationId: coords.conversationId,
          rootMessageId: coords.rootMessageId,
          filePath,
          ...(initialComment ? { initialComment } : {}),
        },
      });
      return;
    }
    case "thread-history": {
      rejectUnknownFlags(flags, ["before-message-id", "before-cursor", "limit", "format"]);
      const beforeMessageId = optionalFlag(flags, "before-message-id");
      const beforeCursor = optionalFlag(flags, "before-cursor");
      if (beforeMessageId && beforeCursor) {
        throw new Error("provide only one of --before-message-id or --before-cursor");
      }
      const format = optionalFlag(flags, "format");
      if (format && format !== "json" && format !== "text") {
        throw new Error("--format must be json or text");
      }
      const limit = optionalFlag(flags, "limit");
      if (limit && !isPositiveInteger(limit)) {
        throw new Error("--limit must be a positive integer");
      }
      const coords = await resolveChatCoordinates();
      await requestBroker({
        method: "GET",
        path: "/chat/thread-history",
        query: {
          platform: coords.platform,
          conversation_id: coords.conversationId,
          root_message_id: coords.rootMessageId,
          before_message_id: beforeMessageId,
          before_cursor: beforeCursor,
          limit,
          format,
        },
      });
      return;
    }
    default:
      throw new Error(`unknown command: chat ${action}`);
  }
}

async function runCoauthorCommand(action: string, flags: ParsedFlags): Promise<void> {
  const cwd = resolveCoauthorCwd();
  switch (action) {
    case "status":
      rejectUnknownFlags(flags, []);
      await requestBroker({
        method: "GET",
        path: "/slack/git-coauthors/session-status",
        query: { cwd },
      });
      return;
    case "configure": {
      rejectUnknownFlags(flags, ["coauthor"], ["ignore-missing"]);
      const coauthors = allFlags(flags, "coauthor");
      if (coauthors.length === 0) {
        throw new Error("missing required argument --coauthor");
      }
      await requestBroker({
        method: "POST",
        path: "/slack/git-coauthors/configure-session",
        body: {
          cwd,
          coauthors,
          ...(flags.bools.has("ignore-missing") ? { ignoreMissing: true } : {}),
        },
      });
      return;
    }
    default:
      throw new Error(`unknown command: coauthor ${action}`);
  }
}

async function runNotifyCommand(flags: ParsedFlags): Promise<void> {
  rejectUnknownFlags(flags, ["text"]);
  const coords = await resolveChatCoordinates();
  const jobId = readEnv("BROKER_JOB_ID");
  await requestBroker({
    method: "POST",
    path: "/notify",
    body: {
      platform: coords.platform,
      conversationId: coords.conversationId,
      rootMessageId: coords.rootMessageId,
      text: requiredFlag(flags, "text"),
      ...(jobId ? { jobId } : {}),
    },
  });
}

async function runJobCommand(action: string, flags: ParsedFlags): Promise<void> {
  if (action !== "register") {
    throw new Error(`unknown command: job ${action}`);
  }

  rejectUnknownFlags(flags, ["kind", "script", "cwd", "restart-on-boot"]);
  const coords = await resolveChatCoordinates();
  const restartOnBoot = optionalFlag(flags, "restart-on-boot");
  await requestBroker({
    method: "POST",
    path: "/jobs/register",
    body: {
      platform: coords.platform,
      conversationId: coords.conversationId,
      rootMessageId: coords.rootMessageId,
      kind: requiredFlag(flags, "kind"),
      script: requiredFlag(flags, "script"),
      ...(optionalFlag(flags, "cwd") ? { cwd: optionalFlag(flags, "cwd") } : {}),
      ...(restartOnBoot === undefined ? {} : { restart_on_boot: parseBooleanFlag("restart-on-boot", restartOnBoot) }),
    },
  });
}

async function runIntegrationCommand(action: string, flags: ParsedFlags): Promise<void> {
  switch (action) {
    case "list-tools":
      rejectUnknownFlags(flags, ["server"]);
      await requestBroker({
        method: "GET",
        path: "/integrations/mcp-tools",
        query: { server: requireOneOf(flags, "server", INTEGRATION_SERVERS) },
      });
      return;
    case "call": {
      rejectUnknownFlags(flags, ["server", "name", "json"]);
      const rawJson = optionalFlag(flags, "json");
      await requestBroker({
        method: "POST",
        path: "/integrations/mcp-call",
        body: {
          server: requireOneOf(flags, "server", INTEGRATION_SERVERS),
          name: requiredFlag(flags, "name"),
          arguments: parseArgumentsObject(rawJson),
        },
      });
      return;
    }
    default:
      throw new Error(`unknown command: integration ${action}`);
  }
}

function parseFlags(argv: readonly string[]): ParsedFlags {
  const values = new Map<string, string[]>();
  const bools = new Set<string>();
  for (let index = 0; index < argv.length; index += 1) {
    const entry = argv[index]!;
    if (!entry.startsWith("--")) {
      throw new Error(`unexpected argument: ${entry}`);
    }

    const key = entry.slice(2);
    if (!key) {
      throw new Error("unexpected argument: --");
    }

    if (BOOLEAN_FLAGS.has(key)) {
      bools.add(key);
      continue;
    }

    const next = argv[index + 1];
    if (!next || next.startsWith("--")) {
      throw new Error(`missing value for --${key}`);
    }

    const current = values.get(key) ?? [];
    current.push(next);
    values.set(key, current);
    index += 1;
  }

  return { values, bools };
}

function rejectUnknownFlags(flags: ParsedFlags, allowedValues: readonly string[], allowedBools: readonly string[] = []): void {
  for (const key of flags.values.keys()) {
    if (!allowedValues.includes(key)) {
      throw new Error(`unknown argument: --${key}`);
    }
  }

  for (const key of flags.bools) {
    if (key === "help") {
      continue;
    }
    if (!allowedBools.includes(key)) {
      throw new Error(`unknown argument: --${key}`);
    }
  }
}

function requiredFlag(flags: ParsedFlags, key: string): string {
  const value = optionalFlag(flags, key);
  if (!value) {
    throw new Error(`missing required argument --${key}`);
  }
  return value;
}

function optionalFlag(flags: ParsedFlags, key: string): string | undefined {
  const values = flags.values.get(key);
  if (!values || values.length === 0) {
    return undefined;
  }
  if (values.length > 1 && key !== "coauthor") {
    throw new Error(`duplicate argument: --${key}`);
  }
  const value = values.at(-1)?.trim();
  return value ? value : undefined;
}

function allFlags(flags: ParsedFlags, key: string): string[] {
  return (flags.values.get(key) ?? []).map((value) => value.trim()).filter(Boolean);
}

function requireOneOf(flags: ParsedFlags, key: string, allowed: ReadonlySet<string>): string {
  const value = requiredFlag(flags, key);
  if (!allowed.has(value)) {
    throw new Error(`invalid --${key}: expected ${[...allowed].join("|")}`);
  }
  return value;
}

function requireReasonForStopKind(kind: string, reason: string | undefined): void {
  if ((kind === "block" || kind === "wait") && !reason) {
    throw new Error("missing required argument --reason");
  }
}

function parseBooleanFlag(key: string, value: string): boolean {
  if (value === "true") {
    return true;
  }
  if (value === "false") {
    return false;
  }
  throw new Error(`invalid --${key}: expected true|false`);
}

function isPositiveInteger(value: string): boolean {
  return /^\d+$/u.test(value) && Number(value) > 0;
}

function parseArgumentsObject(value: string | undefined): Record<string, unknown> {
  if (!value) {
    return {};
  }

  const parsed: unknown = JSON.parse(value);
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("--json must be a JSON object");
  }
  return parsed as Record<string, unknown>;
}

async function main(): Promise<void> {
  await runZorkCall(process.argv.slice(2));
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  void main().catch((error: unknown) => {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`${message}\n`);
    process.exitCode = 1;
  });
}
