import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

import { afterEach, describe, expect, it } from "vitest";

import { CURRENT_STATE_SCHEMA_VERSION, STATE_DATABASE_FILENAME } from "../src/store/state-store.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { getFreePort, pathExists, readAgentTraceEvents, readAgentTurnUsage, readBackgroundJobs, readHasProcessedEvent, readInboundMessages, readPendingSlackEvents, readSessionRecord, removeTempRoot, startBrokerProcess, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";

describe.sequential("store behavior e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      const cleanup = cleanups.pop();
      await cleanup?.();
    }
  });

  it("persists session, events, traces, usage, and jobs, then cascades delete through the worker API", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-store-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const sessionKey = "C123:410.220";
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
      channels: [
        {
          id: "C123",
          name: "deep-review",
          is_channel: true,
        },
      ],
    });
    const mockCodex = new MockCodexAppServer({
      emitThreadTokenUsage: true,
      onTurnStart: (context) => {
        context.complete("STORE_BEHAVIOR_DONE", {
          input_tokens: 1_200,
          cached_input_tokens: 300,
          output_tokens: 450,
          reasoning_tokens: 75,
          total_tokens: 1_725,
          model: "gpt-5.5",
          effort: "xhigh",
        });
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await waitFor(async () => {
      const rows = await readSchemaMigrations(tempRoot);
      return rows.length === expectedSchemaMigrations().length && rows.at(-1)?.version === CURRENT_STATE_SCHEMA_VERSION;
    }, "schema migrations recorded");
    await expect(readSchemaMigrations(tempRoot)).resolves.toEqual(expectedSchemaMigrations());

    await mockSlack.sendEvent("evt-store-behavior", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "410.220",
      ts: "410.221",
      text: "<@UBOT> please review <@U234>",
    });

    await waitFor(() => mockCodex.turnsStarted.length >= 1, "store-behavior first turn");
    await waitForSessionIdle(tempRoot, sessionKey);

    const session = await readSessionRecord(tempRoot, sessionKey);
    expect(session).toMatchObject({
      key: sessionKey,
      channelName: "deep-review",
      channelType: "channel",
      initiatorUserId: "U123",
      initiatorMessageTs: "410.221",
      workspacePath: path.join(tempRoot, "sessions", "C123-410-220", "workspace"),
    });
    expect(session.agentSessionId).toBeTruthy();
    expect(session.activeTurnId).toBeUndefined();
    await expect(pathExists(session.workspacePath)).resolves.toBe(true);
    await expect(readHasProcessedEvent(tempRoot, "evt-store-behavior")).resolves.toBe(true);
    await expect(readPendingSlackEvents(tempRoot)).resolves.toEqual([]);
    await expect(readInboundMessages(tempRoot, sessionKey)).resolves.toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          messageTs: "410.221",
          mentionedUserIds: expect.arrayContaining(["U234"]),
          mentionedUsers: expect.arrayContaining([
            expect.objectContaining({
              userId: "U234",
              displayName: "Mock Display 234",
            }),
          ]),
        }),
      ]),
    );

    await waitFor(async () => {
      const traces = await readAgentTraceEvents(tempRoot, sessionKey);
      return traces.some((event) => event.type === "agent_turn_completed");
    }, "agent traces persisted");
    const traces = await readAgentTraceEvents(tempRoot, sessionKey);
    expect(traces).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          sessionKey,
          type: "agent_turn_completed",
        }),
      ]),
    );

    await waitFor(async () => {
      const usage = await readAgentTurnUsage(tempRoot);
      return usage.some((record) => record.sessionKey === sessionKey && record.totalTokens === 1_725);
    }, "agent turn usage persisted");
    await expect(readAgentTurnUsage(tempRoot)).resolves.toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          sessionKey,
          source: "exact",
          totalTokens: 1_725,
          model: "gpt-5.5",
        }),
      ]),
    );

    const sessionListResponse = await fetch(`${broker.baseUrl}/admin/api/sessions`);
    expect(sessionListResponse.ok).toBe(true);
    const sessionList = (await sessionListResponse.json()) as {
      readonly sessions?: Array<{ readonly key?: string }>;
    };
    expect(sessionList.sessions?.some((item) => item.key === sessionKey)).toBe(true);

    const registerResponse = await fetch(`${broker.baseUrl}/jobs/register`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        channel_id: "C123",
        thread_ts: "410.220",
        kind: "watch_ci",
        script: "#!/bin/sh\nsleep 30",
      }),
    });
    const registerBody = (await registerResponse.json()) as {
      job?: { id: string; token: string };
    };
    expect(registerResponse.ok).toBe(true);
    expect(registerBody.job?.id).toBeTruthy();
    await waitFor(async () => {
      const jobs = await readBackgroundJobs(tempRoot, sessionKey);
      return jobs.some((job) => job.id === registerBody.job!.id && (job.status === "registered" || job.status === "running"));
    }, "background job persisted");

    const deleteResponse = await fetch(`${broker.baseUrl}/slack/sessions/${encodeURIComponent(sessionKey)}`, {
      method: "DELETE",
    });
    expect(deleteResponse.ok).toBe(true);
    await expect(deleteResponse.json()).resolves.toMatchObject({
      ok: true,
      sessionKey,
      delete: {
        deleted: true,
      },
    });

    await waitFor(async () => {
      try {
        await readSessionRecord(tempRoot, sessionKey);
        return false;
      } catch {
        return true;
      }
    }, "session record deleted");
    await expect(readInboundMessages(tempRoot, sessionKey)).resolves.toHaveLength(0);
    await expect(readBackgroundJobs(tempRoot, sessionKey)).resolves.toHaveLength(0);
    await expect(readAgentTraceEvents(tempRoot, sessionKey)).resolves.toHaveLength(0);
    await expect(readAgentTurnUsage(tempRoot)).resolves.toEqual([]);
    await expect(pathExists(path.dirname(session.workspacePath))).resolves.toBe(false);

    const deletedSessionListResponse = await fetch(`${broker.baseUrl}/admin/api/sessions`);
    expect(deletedSessionListResponse.ok).toBe(true);
    const deletedSessionList = (await deletedSessionListResponse.json()) as {
      readonly sessions?: Array<{ readonly key?: string }>;
    };
    expect(deletedSessionList.sessions?.some((item) => item.key === sessionKey)).toBe(false);
  }, 90_000);
});

function expectedSchemaMigrations(): Array<{ version: number; name: string }> {
  return [
    { version: 1, name: "initial_sqlite_state" },
    { version: 2, name: "admin_operations" },
    { version: 3, name: "agent_turn_usage" },
    { version: 4, name: "agent_trace_events" },
    { version: 5, name: "agent_schema_repair" },
    { version: 6, name: "session_agent_schema_repair" },
    { version: 7, name: "session_channel_metadata" },
    { version: 8, name: "inbound_mentioned_users" },
    { version: 9, name: "admin_realtime_events" },
    { version: 10, name: "session_page_link_announcement" },
    { version: 11, name: "session_auth_profile_binding" },
    { version: 12, name: "agent_activity_bindings" },
    { version: 13, name: "session_initiator" },
    { version: 14, name: "agent_session_derived_summaries" },
    { version: 15, name: "slack_event_retention_indexes" },
    { version: 16, name: "inbound_mention_backfill_indexes" },
    { version: 17, name: "chat_platform_columns" },
    { version: CURRENT_STATE_SCHEMA_VERSION, name: "trace_jsonl_ssot" },
  ];
}

async function readSchemaMigrations(tempRoot: string): Promise<Array<{ version: number; name: string }>> {
  const database = new DatabaseSync(path.join(tempRoot, "state", STATE_DATABASE_FILENAME));
  try {
    return database.prepare("SELECT version, name FROM schema_migrations ORDER BY version ASC").all() as Array<{ version: number; name: string }>;
  } finally {
    database.close();
  }
}
