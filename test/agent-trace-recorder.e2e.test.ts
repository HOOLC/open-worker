import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { afterEach, describe, expect, it } from "vitest";

import { AgentTraceRecorder } from "../src/services/agent-runtime/agent-trace-recorder.js";
import { STATE_DATABASE_FILENAME, StateStore } from "../src/store/state-store.js";
import { isZstdAvailable, sessionTraceDirectory } from "../src/store/trace-jsonl-store.js";
import type { PersistedAgentTraceEvent } from "../src/types.js";
import { readJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

describe("agent trace recorder e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("records visible assistant, input, and tool traces through admin timeline resources", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-recorder-",
    });
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-1");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-1");
    const recorder = new AgentTraceRecorder({ sessions });

    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-1",
      turnId: "turn-1",
      messageId: "message-empty",
      role: "assistant",
      text: "   \n  ",
      at: "2026-03-19T00:00:02.000Z",
    });
    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-1",
      turnId: "turn-1",
      messageId: "message-1",
      role: "assistant",
      text: "我会先检查状态。",
      at: "2026-03-19T00:00:03.000Z",
    });
    await recorder.record({
      type: "agent.input.received",
      inputId: "input-1",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      source: "slack_user",
      textPreview: "A newer Slack message arrived while the current turn is still active. Treat it as the latest instruction...",
      text: [
        "A newer Slack message arrived while the current turn is still active.",
        "Treat it as the latest instruction and adjust the ongoing work accordingly.",
        "",
        "A new message arrived in the active Slack thread. Carefully judge whether it requires a reply or action from you.",
        "structured_message_json:",
        "```json",
        JSON.stringify(
          {
            source: "app_mention",
            message_ts: "1778316208.809479",
            sender: {
              kind: "user",
              user_id: "U123",
              mention: "<@U123>",
              display_name: "Jc",
            },
            text: "<@U0ALY77RMJL> 结合 willow repo，分析图中问题",
            text_with_resolved_mentions: "@codex-3720 结合 willow repo，分析图中问题",
            images: [],
          },
          null,
          2,
        ),
        "```",
      ].join("\n"),
      at: "2026-03-19T00:00:03.100Z",
    });
    await recorder.record({
      type: "agent.input.received",
      inputId: "input-runtime-1",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      source: "background_job",
      textPreview: "A broker-managed background job reported a new asynchronous event for this session.",
      text: [
        "A broker-managed background job reported a new asynchronous event for this session.",
        "background_job_event_json:",
        "```json",
        JSON.stringify(
          {
            source: "background_job_event",
            message_ts: "1778316208.809479",
            job: {
              job_id: "job-1",
              job_kind: "watch_ci",
              event_kind: "job_completed",
            },
            summary: "PR #1873 checks 13 pass.",
          },
          null,
          2,
        ),
        "```",
      ].join("\n"),
      at: "2026-03-19T00:00:03.200Z",
    });
    await recorder.record({
      type: "agent.tool.started",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      callId: "call-1",
      name: "exec_command",
      input: {
        command: '/bin/zsh -lc "cd /tmp/workspace/app && pnpm test"',
        cwd: "/tmp/workspace",
        commandActions: [
          {
            type: "test",
            name: "unit",
          },
        ],
      },
      at: "2026-03-19T00:00:03.300Z",
    });

    const started = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect((started.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(["agent_assistant_message", "agent_input_received", "agent_input_received", "agent_tool_call"]);
    expect(started.events).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          type: "agent_assistant_message",
          title: "Assistant 消息",
          summary: "我会先检查状态。",
          turnId: "turn-1",
        }),
        expect.objectContaining({
          type: "agent_input_received",
          title: "@codex-3720 结合 willow repo，分析图中问题",
          summary: "Jc · 提及",
          role: "user",
          metadata: expect.objectContaining({
            inputId: "input-1",
            source: "app_mention",
            sender: "Jc",
            messageTs: "1778316208.809479",
          }),
        }),
        expect.objectContaining({
          type: "agent_input_received",
          title: "PR #1873 checks 13 pass.",
          summary: "watch_ci · job_completed · Job job-1",
          role: "system",
          metadata: expect.objectContaining({
            inputId: "input-runtime-1",
            source: "background_job_event",
            jobKind: "watch_ci",
            eventKind: "job_completed",
          }),
        }),
        expect.objectContaining({
          type: "agent_tool_call",
          title: "pnpm test",
          summary: "测试 unit · cwd app · 运行中",
          metadata: expect.objectContaining({
            commandPreview: "pnpm test",
            cwdLabel: "app",
            actionSummary: "测试 unit",
          }),
        }),
      ]),
    );

    await recorder.record({
      type: "agent.tool.completed",
      agentSessionId: "thread-1",
      brokerSessionKey: session.key,
      turnId: "turn-1",
      callId: "call-1",
      name: "exec_command",
      output: {
        command: '/bin/zsh -lc "cd /tmp/workspace/app && pnpm test"',
        cwd: "/tmp/workspace",
        exitCode: 0,
        durationMs: 1200,
        aggregatedOutput: "PASS unit tests",
      },
      status: "completed",
      at: "2026-03-19T00:00:04.000Z",
    });

    const completed = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect((completed.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(["agent_assistant_message", "agent_input_received", "agent_input_received", "agent_tool_result"]);
    const toolResult = (completed.events as Array<Record<string, any>>).find((event) => event.type === "agent_tool_result");
    expect(toolResult).toMatchObject({
      title: "pnpm test",
      summary: "exit 0 · 1.2s · 输出 PASS unit tests",
      metadata: expect.objectContaining({
        exitCode: 0,
        durationMs: 1200,
        outputPreview: "PASS unit tests",
      }),
    });
    const detail = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline-events/${encodeURIComponent(String(toolResult?.id))}`);
    expect(detail).toMatchObject({
      ok: true,
      event: {
        type: "agent_tool_result",
        metadata: expect.objectContaining({
          outputPreview: "PASS unit tests",
        }),
      },
    });
  });

  it("records late events from an old agent turn after the session switches runtime ids", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-recorder-late-",
    });
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-old");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-old");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "thread-new");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "turn-new");
    const recorder = new AgentTraceRecorder({ sessions });
    await recorder.record({
      type: "agent.message.completed",
      agentSessionId: "thread-old",
      turnId: "turn-old",
      messageId: "message-late",
      role: "assistant",
      text: "旧 session 的迟到事件不能断链。",
      at: "2026-03-19T00:00:03.000Z",
    });

    const timeline = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline`);
    expect(timeline.events).toEqual([
      expect.objectContaining({
        type: "agent_assistant_message",
        summary: "旧 session 的迟到事件不能断链。",
        turnId: "turn-old",
      }),
    ]);
  });
});

describe("agent trace jsonl store e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("pages traces newest-first and applies last-write-wins by id", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-jsonl-page-",
    });
    const session = await sessions.ensureSession("C123", "111.222");
    for (let sequence = 1; sequence <= 10; sequence += 1) {
      await sessions.upsertAgentTraceEvent(fakeTraceEvent(session.key, sequence));
    }
    await sessions.upsertAgentTraceEvent(
      fakeTraceEvent(session.key, 11, {
        id: "evt-1",
        title: "updated title",
        summary: "updated summary",
      }),
    );

    const unique = sessions.listAgentTraceEvents(session.key, 100);
    expect(unique).toHaveLength(10);
    expect(unique.find((event) => event.id === "evt-1")).toMatchObject({
      sequence: 11,
      title: "updated title",
    });
    expect(unique.filter((event) => event.id === "evt-1")).toHaveLength(1);

    const page = sessions.listAgentTraceEventsPage(session.key, { limit: 3 });
    expect(page.events.map((event) => event.sequence)).toEqual([9, 10, 11]);
    expect(page.events.at(-1)).toMatchObject({ id: "evt-1", title: "updated title" });
    expect(page.hasMore).toBe(true);
    expect(page.nextBeforeSequence).toBe(9);

    const older = sessions.listAgentTraceEventsPage(session.key, { limit: 3, beforeSequence: 9 });
    expect(older.events.map((event) => event.sequence)).toEqual([6, 7, 8]);
    expect(older.hasMore).toBe(true);
    expect(older.nextBeforeSequence).toBe(6);

    const timeline = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/timeline?limit=3`);
    expect((timeline.events as Array<{ sequence: number }>).map((event) => event.sequence)).toEqual([9, 10, 11]);
  });

  it("rebuilds sqlite trace summaries from jsonl snapshots", async () => {
    const store = await openTraceStore(cleanups, {
      traceSummarySnapshotInterval: 5,
    });
    const sessionKey = "C123:111.222";
    await seedStoreSession(store, sessionKey);
    for (let sequence = 1; sequence <= 7; sequence += 1) {
      await store.upsertAgentTraceEvent(fakeTraceEvent(sessionKey, sequence));
    }
    await store.upsertAgentTraceEvent(
      fakeTraceEvent(sessionKey, 8, {
        id: "tool-call-1",
        type: "agent_tool_call",
        title: "pnpm test",
        summary: "running",
        toolName: "exec_command",
        callId: "call-1",
        turnId: "turn-1",
        status: "running",
      }),
    );
    await store.upsertAgentTraceEvent(
      fakeTraceEvent(sessionKey, 9, {
        id: "tool-result-1",
        type: "agent_tool_result",
        title: "pnpm test",
        summary: "exit 0",
        toolName: "exec_command",
        callId: "call-1",
        turnId: "turn-1",
        status: "completed",
      }),
    );

    const before = store.getAgentSessionTraceSummary(sessionKey);
    expect(before?.eventCount).toBeGreaterThan(0);
    const snapshotLines = readTraceLines(store, sessionKey).filter((line) => {
      try {
        return (JSON.parse(line) as { type?: string }).type === "summary_snapshot";
      } catch {
        return false;
      }
    });
    expect(snapshotLines.length).toBeGreaterThan(0);

    const database = new DatabaseSync(path.join(storeStateDir(store), STATE_DATABASE_FILENAME));
    database.exec("PRAGMA busy_timeout = 5000; DELETE FROM agent_session_trace_summaries");
    database.close();
    expect(store.getAgentSessionTraceSummary(sessionKey)).toBeUndefined();

    store.rebuildTraceSummaries();
    expect(store.getAgentSessionTraceSummary(sessionKey)).toEqual(before);
  });

  it("migrates legacy sqlite agent_trace_events into jsonl and drops the table", async () => {
    const store = await openTraceStore(cleanups);
    const sessionKey = "C123:111.222";
    await seedStoreSession(store, sessionKey);
    store.close();

    const database = new DatabaseSync(path.join(storeStateDir(store), STATE_DATABASE_FILENAME));
    database.exec(`
      CREATE TABLE IF NOT EXISTS agent_trace_events (
        id TEXT PRIMARY KEY,
        session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
        source TEXT NOT NULL,
        type TEXT NOT NULL,
        at TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        title TEXT NOT NULL,
        summary TEXT NOT NULL,
        detail TEXT,
        status TEXT,
        role TEXT,
        tool_name TEXT,
        call_id TEXT,
        turn_id TEXT,
        detail_truncated INTEGER NOT NULL DEFAULT 0,
        detail_original_chars INTEGER,
        metadata TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
    `);
    const insertLegacy = database.prepare(`
        INSERT INTO agent_trace_events (
          id, session_key, source, type, at, sequence, title, summary, detail,
          status, role, tool_name, call_id, turn_id, detail_truncated,
          detail_original_chars, metadata, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
    // Inserted out of sequence order; the migration must stream oldest-first
    // so the newest row lands in the active segment.
    insertLegacy.run("legacy-newest", sessionKey, "agent_runtime", "agent_assistant_message", "2026-03-19T00:00:03.000Z", 3, "legacy newest", "legacy newest summary", "legacy newest detail", "completed", "assistant", null, null, "turn-1", 0, null, null, "2026-03-19T00:00:03.000Z", "2026-03-19T00:00:03.000Z");
    insertLegacy.run("legacy-1", sessionKey, "agent_runtime", "agent_assistant_message", "2026-03-19T00:00:01.000Z", 1, "legacy title", "legacy summary", "legacy detail", "completed", "assistant", null, null, "turn-1", 0, null, null, "2026-03-19T00:00:01.000Z", "2026-03-19T00:00:01.000Z");
    insertLegacy.run("legacy-middle", sessionKey, "agent_runtime", "agent_token_count", "2026-03-19T00:00:02.000Z", 2, "legacy middle", "legacy middle summary", null, "completed", "assistant", null, null, "turn-1", 0, null, null, "2026-03-19T00:00:02.000Z", "2026-03-19T00:00:02.000Z");
    database.prepare("DELETE FROM schema_migrations WHERE version = 18").run();
    database.close();

    const migrated = new StateStore(storeStateDir(store), storeSessionsRoot(store));
    await migrated.load();
    cleanups.push(async () => {
      migrated.close();
    });

    expect(migrated.listAgentTraceEvents(sessionKey)).toEqual([
      expect.objectContaining({
        id: "legacy-1",
        title: "legacy title",
        summary: "legacy summary",
        sequence: 1,
      }),
      expect.objectContaining({
        id: "legacy-middle",
        sequence: 2,
      }),
      expect.objectContaining({
        id: "legacy-newest",
        sequence: 3,
      }),
    ]);
    const lines = readTraceLines(migrated, sessionKey);
    expect(lines.some((line) => line.includes("legacy-1"))).toBe(true);
    // Rows were streamed oldest-first, so the newest row is the last JSONL line.
    expect(lines.at(-1)).toContain("legacy-newest");
    expect(lines.findIndex((line) => line.includes("legacy-1"))).toBeLessThan(lines.findIndex((line) => line.includes("legacy-newest")));

    // The SQLite summary cache is derived state and must be refilled from the
    // migrated JSONL without waiting for a manual rebuild.
    expect(migrated.getAgentSessionTraceSummary(sessionKey)).toEqual(
      expect.objectContaining({
        sessionKey,
        eventCount: 2,
        modelRequestCount: 1,
        categories: {
          agent_assistant_message: 2,
        },
      }),
    );

    const verify = new DatabaseSync(path.join(storeStateDir(store), STATE_DATABASE_FILENAME));
    try {
      expect(verify.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'agent_trace_events'").get()).toBeUndefined();
      expect(verify.prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_agent_trace_events%'").all()).toEqual([]);
    } finally {
      verify.close();
    }
  });

  it("leaves the newest migrated rows in the active segment when sealing forces multiple segments", async () => {
    const stateRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), "agent-trace-migrate-seal-"));
    cleanups.push(async () => {
      await fs.promises.rm(stateRoot, { recursive: true, force: true });
    });
    const stateDir = path.join(stateRoot, "state");
    const sessionsRoot = path.join(stateRoot, "sessions");
    const sessionKey = "C123:111.222";

    const seed = new StateStore(stateDir, sessionsRoot, { traceSegmentMaxBytes: 300 });
    await seed.load();
    storeRoots.set(seed, { stateDir, sessionsRoot });
    await seedStoreSession(seed, sessionKey);
    seed.close();

    const database = new DatabaseSync(path.join(stateDir, STATE_DATABASE_FILENAME));
    database.exec(`
      CREATE TABLE IF NOT EXISTS agent_trace_events (
        id TEXT PRIMARY KEY,
        session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
        source TEXT NOT NULL,
        type TEXT NOT NULL,
        at TEXT NOT NULL,
        sequence INTEGER NOT NULL,
        title TEXT NOT NULL,
        summary TEXT NOT NULL,
        detail TEXT,
        status TEXT,
        role TEXT,
        tool_name TEXT,
        call_id TEXT,
        turn_id TEXT,
        detail_truncated INTEGER NOT NULL DEFAULT 0,
        detail_original_chars INTEGER,
        metadata TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
    `);
    const insertLegacy = database.prepare(`
        INSERT INTO agent_trace_events (
          id, session_key, source, type, at, sequence, title, summary, detail,
          status, role, tool_name, call_id, turn_id, detail_truncated,
          detail_original_chars, metadata, created_at, updated_at
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
    for (let sequence = 1; sequence <= 8; sequence += 1) {
      const at = new Date(Date.UTC(2026, 2, 19, 0, 0, sequence)).toISOString();
      insertLegacy.run(`legacy-${sequence}`, sessionKey, "agent_runtime", "agent_assistant_message", at, sequence, `legacy title ${sequence}`, `legacy summary ${sequence}`, `legacy detail ${sequence} ${"x".repeat(120)}`, "completed", "assistant", null, null, null, 0, null, null, at, at);
    }
    database.prepare("DELETE FROM schema_migrations WHERE version = 18").run();
    database.close();

    const migrated = new StateStore(stateDir, sessionsRoot, { traceSegmentMaxBytes: 300 });
    await migrated.load();
    cleanups.push(async () => {
      migrated.close();
    });

    const files = listTraceSegmentFiles(migrated, sessionKey);
    expect(files.length).toBeGreaterThan(1);
    if (isZstdAvailable()) {
      expect(files.some((name) => name.endsWith(".jsonl.zst"))).toBe(true);
    }

    // Last-write-wins reads and reverse pagination still work across the
    // migrated segments.
    expect(migrated.listAgentTraceEvents(sessionKey).map((event) => event.id)).toEqual(Array.from({ length: 8 }, (_, index) => `legacy-${index + 1}`));
    const page = migrated.listAgentTraceEventsPage(sessionKey, { limit: 3 });
    expect(page.events.map((event) => event.sequence)).toEqual([6, 7, 8]);
    expect(page.hasMore).toBe(true);
    expect(page.nextBeforeSequence).toBe(6);
    expect(migrated.getAgentSessionTraceSummary(sessionKey)).toEqual(
      expect.objectContaining({
        eventCount: 8,
      }),
    );

    // The active (uncompressed, highest-index) segment holds the newest rows.
    const activeSegment = files.filter((name) => name.endsWith(".jsonl")).at(-1);
    expect(activeSegment).toBeTruthy();
    const activeLines = readTraceSegmentLines(migrated, sessionKey, activeSegment!);
    expect(activeLines.length).toBeGreaterThan(0);
    expect(activeLines.at(-1)).toContain("legacy-8");
    expect(activeLines.some((line) => line.includes("legacy-1"))).toBe(false);
  });

  it("seals oversized segments and reads them back, compressing with zstd when available", async () => {
    const store = await openTraceStore(cleanups, {
      traceSegmentMaxBytes: 200,
    });
    const sessionKey = "C123:111.222";
    await seedStoreSession(store, sessionKey);
    const written: PersistedAgentTraceEvent[] = [];
    for (let sequence = 1; sequence <= 6; sequence += 1) {
      const event = fakeTraceEvent(sessionKey, sequence, {
        detail: `payload-${sequence}-${"x".repeat(180)}`,
      });
      written.push(event);
      await store.upsertAgentTraceEvent(event);
    }

    const files = listTraceSegmentFiles(store, sessionKey);
    expect(files.length).toBeGreaterThan(1);
    if (isZstdAvailable()) {
      expect(files.some((name) => name.endsWith(".jsonl.zst"))).toBe(true);
    }

    expect(store.listAgentTraceEvents(sessionKey, 100).map((event) => event.id)).toEqual(written.map((event) => event.id));
    const page = store.listAgentTraceEventsPage(sessionKey, { limit: 2 });
    expect(page.events.map((event) => event.sequence)).toEqual([5, 6]);
    expect(page.hasMore).toBe(true);
  });

  it("removes jsonl traces when a session is deleted", async () => {
    const { sessions, config } = await startAdminFixture(cleanups, {
      tempPrefix: "agent-trace-jsonl-delete-",
    });
    const session = await sessions.ensureSession("C123", "111.222");
    await sessions.upsertAgentTraceEvent(fakeTraceEvent(session.key, 1));
    expect(fs.existsSync(sessionTraceDirectory(session.workspacePath))).toBe(true);

    await sessions.deleteSessionByKey(session.key);
    expect(sessions.listAgentTraceEvents(session.key)).toEqual([]);
    expect(fs.existsSync(sessionTraceDirectory(session.workspacePath))).toBe(false);
  });
});

function fakeTraceEvent(sessionKey: string, sequence: number, overrides: Partial<PersistedAgentTraceEvent> = {}): PersistedAgentTraceEvent {
  const at = new Date(Date.UTC(2026, 2, 19, 0, 0, sequence)).toISOString();
  return {
    id: `evt-${sequence}`,
    sessionKey,
    source: "agent_runtime",
    type: "agent_assistant_message",
    at,
    sequence,
    title: `title ${sequence}`,
    summary: `summary ${sequence}`,
    detail: `detail ${sequence}`,
    status: "completed",
    role: "assistant",
    createdAt: at,
    updatedAt: at,
    ...overrides,
  };
}

const storeRoots = new WeakMap<StateStore, { stateDir: string; sessionsRoot: string }>();

async function openTraceStore(
  cleanups: Array<() => Promise<void>>,
  options?: {
    readonly traceSegmentMaxBytes?: number;
    readonly traceSummarySnapshotInterval?: number;
  },
): Promise<StateStore> {
  const tempRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), "agent-trace-jsonl-"));
  cleanups.push(async () => {
    await fs.promises.rm(tempRoot, { recursive: true, force: true });
  });
  const stateDir = path.join(tempRoot, "state");
  const sessionsRoot = path.join(tempRoot, "sessions");
  const store = new StateStore(stateDir, sessionsRoot, options);
  await store.load();
  storeRoots.set(store, { stateDir, sessionsRoot });
  cleanups.push(async () => {
    store.close();
  });
  return store;
}

async function seedStoreSession(store: StateStore, sessionKey: string): Promise<void> {
  const [channelId, rootThreadTs] = sessionKey.split(":") as [string, string];
  await store.upsertSession({
    key: sessionKey,
    channelId,
    rootThreadTs,
    workspacePath: path.join(storeSessionsRoot(store), `${channelId}-${rootThreadTs}`, "workspace"),
    createdAt: "2026-03-19T00:00:00.000Z",
    updatedAt: "2026-03-19T00:00:00.000Z",
  });
}

function storeStateDir(store: StateStore): string {
  const roots = storeRoots.get(store);
  if (!roots) {
    throw new Error("Unknown StateStore temp root");
  }
  return roots.stateDir;
}

function storeSessionsRoot(store: StateStore): string {
  const roots = storeRoots.get(store);
  if (!roots) {
    throw new Error("Unknown StateStore temp root");
  }
  return roots.sessionsRoot;
}

function sessionWorkspacePath(store: StateStore, sessionKey: string): string {
  const session = store.getSession(sessionKey);
  if (!session) {
    throw new Error(`Unknown session: ${sessionKey}`);
  }
  return session.workspacePath;
}

function readTraceLines(store: StateStore, sessionKey: string): string[] {
  const directory = sessionTraceDirectory(sessionWorkspacePath(store, sessionKey));
  if (!fs.existsSync(directory)) {
    return [];
  }
  const lines: string[] = [];
  for (const name of fs.readdirSync(directory).sort()) {
    if (!name.endsWith(".jsonl")) {
      continue;
    }
    const text = fs.readFileSync(path.join(directory, name), "utf8");
    for (const line of text.split("\n")) {
      if (line.trim()) {
        lines.push(line);
      }
    }
  }
  return lines;
}

function listTraceSegmentFiles(store: StateStore, sessionKey: string): string[] {
  const directory = sessionTraceDirectory(sessionWorkspacePath(store, sessionKey));
  if (!fs.existsSync(directory)) {
    return [];
  }
  return fs.readdirSync(directory).sort();
}

function readTraceSegmentLines(store: StateStore, sessionKey: string, segmentName: string): string[] {
  const directory = sessionTraceDirectory(sessionWorkspacePath(store, sessionKey));
  const text = fs.readFileSync(path.join(directory, segmentName), "utf8");
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}
