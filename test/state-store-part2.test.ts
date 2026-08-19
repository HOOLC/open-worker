import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { DatabaseSync } from "node:sqlite";

import { describe, expect, it } from "vitest";

import { STATE_DATABASE_FILENAME, StateStore } from "../src/store/state-store.js";

describe("StateStore", () => {
  it("persists historical agent activity bindings when the current session runtime changes", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-agent-bindings-"));
    const sessionsRoot = path.join(stateDir, "sessions");
    const store = new StateStore(stateDir, sessionsRoot);
    await store.load();
    await store.upsertSession({
      key: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      workspacePath: "/tmp/sessions/C123-111.222/workspace",
      createdAt: "2026-03-15T00:00:00.000Z",
      updatedAt: "2026-03-15T00:00:00.000Z",
      agentSessionId: "thread-current",
      activeTurnId: "turn-current",
    });
    expect(
      store.getSessionKeyForAgentActivity({
        agentSessionId: "thread-current",
        turnId: "turn-current",
      }),
    ).toBe("C123:111.222");
    await store.bindAgentSession({
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      agentSessionId: "thread-old",
      at: "2026-03-15T00:00:01.000Z",
    });
    await store.bindAgentTurn({
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      agentSessionId: "thread-old",
      turnId: "turn-old",
      at: "2026-03-15T00:00:02.000Z",
    });

    expect(
      store.getSessionKeyForAgentActivity({
        agentSessionId: "thread-old",
      }),
    ).toBe("C123:111.222");
    expect(
      store.getSessionKeyForAgentActivity({
        turnId: "turn-old",
      }),
    ).toBe("C123:111.222");

    await store.patchSession("C123:111.222", {
      agentSessionId: "thread-new",
      activeTurnId: "turn-new",
    });
    expect(
      store.getSessionKeyForAgentActivity({
        agentSessionId: "thread-old",
        turnId: "turn-old",
      }),
    ).toBe("C123:111.222");

    await store.deleteSession("C123:111.222");
    expect(
      store.getSessionKeyForAgentActivity({
        agentSessionId: "thread-old",
        turnId: "turn-old",
      }),
    ).toBeUndefined();
    store.close();
  });

  it("filters inbound messages needing Slack mention identity backfill in SQLite", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-mention-backfill-"));
    const sessionsRoot = path.join(stateDir, "sessions");
    const store = new StateStore(stateDir, sessionsRoot);
    await store.load();
    await store.upsertSession({
      key: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      workspacePath: "/tmp/sessions/C123-111.222/workspace",
      createdAt: "2026-03-15T00:00:00.000Z",
      updatedAt: "2026-03-15T00:00:00.000Z",
    });

    await store.upsertInboundMessage({
      key: "needs-backfill",
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.223",
      source: "thread_reply",
      userId: "U123",
      text: "<@U234> follow up",
      mentionedUserIds: ["U234"],
      mentionedUsers: [],
      status: "pending",
      createdAt: "2026-03-15T00:00:01.000Z",
      updatedAt: "2026-03-15T00:00:01.000Z",
    });
    await store.upsertInboundMessage({
      key: "already-complete",
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.224",
      source: "thread_reply",
      userId: "U123",
      text: "<@U234> already resolved",
      mentionedUserIds: ["U234"],
      mentionedUsers: [
        {
          userId: "U234",
          mention: "<@U234>",
          username: "mock-user-234",
          displayName: "Mock Display 234",
          realName: "Mock User 234",
        },
      ],
      status: "pending",
      createdAt: "2026-03-15T00:00:02.000Z",
      updatedAt: "2026-03-15T00:00:02.000Z",
    });
    await store.upsertInboundMessage({
      key: "no-mentions",
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.225",
      source: "thread_reply",
      userId: "U123",
      text: "plain message",
      mentionedUserIds: [],
      mentionedUsers: [],
      status: "pending",
      createdAt: "2026-03-15T00:00:03.000Z",
      updatedAt: "2026-03-15T00:00:03.000Z",
    });
    await store.upsertInboundMessage({
      key: "wrong-source",
      sessionKey: "C123:111.222",
      channelId: "C123",
      rootThreadTs: "111.222",
      messageTs: "111.226",
      source: "background_job_event",
      userId: "U123",
      text: "<@U234> synthetic",
      mentionedUserIds: ["U234"],
      mentionedUsers: [],
      status: "pending",
      createdAt: "2026-03-15T00:00:04.000Z",
      updatedAt: "2026-03-15T00:00:04.000Z",
    });

    expect(
      store
        .listInboundMessages({
          source: ["app_mention", "direct_message", "thread_reply"],
          needsMentionUserBackfill: true,
        })
        .map((message) => message.key),
    ).toEqual(["needs-backfill"]);
    store.close();
  });

  it("migrates old session Codex thread ids into agent session ids", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-old-session-"));
    const sessionsRoot = path.join(stateDir, "sessions");
    await fs.mkdir(stateDir, { recursive: true });

    const database = new DatabaseSync(path.join(stateDir, STATE_DATABASE_FILENAME));
    try {
      database.exec(`
        CREATE TABLE schema_migrations (
          version INTEGER PRIMARY KEY,
          name TEXT NOT NULL,
          applied_at TEXT NOT NULL
        );

        INSERT INTO schema_migrations (version, name, applied_at) VALUES
          (1, 'initial_sqlite_state', '2026-03-15T00:00:00.000Z'),
          (2, 'admin_operations', '2026-03-15T00:00:00.000Z'),
          (3, 'codex_turn_usage', '2026-03-15T00:00:00.000Z');

        CREATE TABLE sessions (
          key TEXT PRIMARY KEY,
          channel_id TEXT NOT NULL,
          root_thread_ts TEXT NOT NULL,
          workspace_path TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          codex_thread_id TEXT,
          active_turn_id TEXT,
          active_turn_started_at TEXT,
          last_observed_message_ts TEXT,
          last_delivered_message_ts TEXT,
          last_slack_reply_at TEXT,
          last_progress_reminder_at TEXT,
          last_turn_signal_turn_id TEXT,
          last_turn_signal_kind TEXT,
          last_turn_signal_reason TEXT,
          last_turn_signal_at TEXT,
          co_author_candidate_user_ids TEXT,
          co_author_candidate_revision INTEGER,
          co_author_confirmed_user_ids TEXT,
          co_author_confirmed_revision INTEGER,
          co_author_ignore_missing_revision INTEGER,
          co_author_prompt_revision INTEGER,
          co_author_prompted_at TEXT,
          UNIQUE(channel_id, root_thread_ts)
        );

        INSERT INTO sessions (
          key, channel_id, root_thread_ts, workspace_path, created_at, updated_at, codex_thread_id
        ) VALUES (
          'C123:111.222', 'C123', '111.222', '/tmp/workspace',
          '2026-03-15T00:00:00.000Z', '2026-03-15T00:00:00.000Z', 'thread-old'
        );
      `);
    } finally {
      database.close();
    }

    const store = new StateStore(stateDir, sessionsRoot);
    await store.load();
    expect(store.getSession("C123:111.222")).toEqual(
      expect.objectContaining({
        agentSessionId: "thread-old",
      }),
    );

    await expect(
      store.patchSession("C123:111.222", {
        updatedAt: "2026-03-15T00:00:01.000Z",
      }),
    ).resolves.toEqual(
      expect.objectContaining({
        agentSessionId: "thread-old",
      }),
    );
    store.close();
  });

  it("migrates the old turn usage table into the agent schema and removes the old table", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-old-usage-"));
    const sessionsRoot = path.join(stateDir, "sessions");
    await fs.mkdir(stateDir, { recursive: true });

    const database = new DatabaseSync(path.join(stateDir, STATE_DATABASE_FILENAME));
    try {
      database.exec(`
        PRAGMA foreign_keys = ON;

        CREATE TABLE schema_migrations (
          version INTEGER PRIMARY KEY,
          name TEXT NOT NULL,
          applied_at TEXT NOT NULL
        );

        INSERT INTO schema_migrations (version, name, applied_at) VALUES
          (1, 'initial_sqlite_state', '2026-03-15T00:00:00.000Z'),
          (2, 'admin_operations', '2026-03-15T00:00:00.000Z'),
          (3, 'agent_turn_usage', '2026-03-15T00:00:00.000Z'),
          (4, 'agent_trace_events', '2026-03-15T00:00:00.000Z');

        CREATE TABLE sessions (
          key TEXT PRIMARY KEY,
          channel_id TEXT NOT NULL,
          root_thread_ts TEXT NOT NULL,
          workspace_path TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          agent_session_id TEXT,
          active_turn_id TEXT,
          active_turn_started_at TEXT,
          last_observed_message_ts TEXT,
          last_delivered_message_ts TEXT,
          last_slack_reply_at TEXT,
          last_progress_reminder_at TEXT,
          last_turn_signal_turn_id TEXT,
          last_turn_signal_kind TEXT,
          last_turn_signal_reason TEXT,
          last_turn_signal_at TEXT,
          co_author_candidate_user_ids TEXT,
          co_author_candidate_revision INTEGER,
          co_author_confirmed_user_ids TEXT,
          co_author_confirmed_revision INTEGER,
          co_author_ignore_missing_revision INTEGER,
          co_author_prompt_revision INTEGER,
          co_author_prompted_at TEXT
        );

        INSERT INTO sessions (
          key, channel_id, root_thread_ts, workspace_path, created_at, updated_at
        ) VALUES (
          'C123:111.222', 'C123', '111.222', '/tmp/workspace',
          '2026-03-15T00:00:00.000Z', '2026-03-15T00:00:00.000Z'
        );

        CREATE TABLE codex_turn_usage (
          turn_id TEXT PRIMARY KEY,
          session_key TEXT NOT NULL REFERENCES sessions(key) ON DELETE CASCADE,
          channel_id TEXT NOT NULL,
          root_thread_ts TEXT NOT NULL,
          codex_thread_id TEXT,
          status TEXT NOT NULL,
          source TEXT NOT NULL,
          model TEXT,
          effort TEXT,
          input_tokens INTEGER NOT NULL DEFAULT 0,
          cached_input_tokens INTEGER NOT NULL DEFAULT 0,
          output_tokens INTEGER NOT NULL DEFAULT 0,
          reasoning_tokens INTEGER NOT NULL DEFAULT 0,
          total_tokens INTEGER NOT NULL DEFAULT 0,
          raw_usage TEXT,
          started_at TEXT,
          completed_at TEXT,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL
        );

        INSERT INTO codex_turn_usage (
          turn_id, session_key, channel_id, root_thread_ts, codex_thread_id,
          status, source, model, effort,
          input_tokens, cached_input_tokens, output_tokens, reasoning_tokens, total_tokens,
          raw_usage, started_at, completed_at, created_at, updated_at
        ) VALUES (
          'turn-1', 'C123:111.222', 'C123', '111.222', 'thread-1',
          'completed', 'exact', 'gpt-5.5', 'xhigh',
          1200, 300, 450, 75, 1725,
          '{"total_tokens":1725}',
          '2026-03-15T00:00:01.000Z',
          '2026-03-15T00:00:09.000Z',
          '2026-03-15T00:00:01.000Z',
          '2026-03-15T00:00:09.000Z'
        );
      `);
    } finally {
      database.close();
    }

    const store = new StateStore(stateDir, sessionsRoot);
    await store.load();
    expect(store.listAgentTurnUsage()).toEqual([
      expect.objectContaining({
        turnId: "turn-1",
        agentSessionId: "thread-1",
        totalTokens: 1725,
      }),
    ]);
    store.close();

    const migrated = new DatabaseSync(path.join(stateDir, STATE_DATABASE_FILENAME));
    try {
      expect(migrated.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'codex_turn_usage'").get()).toBeUndefined();
      expect(migrated.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'agent_turn_usage'").get()).toBeTruthy();
    } finally {
      migrated.close();
    }
  });
});
