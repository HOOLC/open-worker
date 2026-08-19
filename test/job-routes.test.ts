import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { fetchJson, getFreePort, removeTempRoot, seedBrokerSessions, startBrokerProcess } from "./e2e-broker-helpers.js";

describe.sequential("job routes", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("registers jobs with canonical platform-aware chat coordinates", async () => {
    const { baseUrl } = await startJobBroker(cleanups, [
      {
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
      },
    ]);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      platform: "feishu",
      conversationId: "oc_group",
      rootMessageId: "om_root",
      kind: "watch_ci",
      cwd: ".",
      script: "sleep 30",
      restart_on_boot: false,
    });

    expect(response.status).toBe(200);
    expect(response.body).toMatchObject({
      ok: true,
      job: {
        status: "running",
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
        channelId: "oc_group",
        rootThreadTs: "om_root",
        kind: "watch_ci",
        restartOnBoot: false,
      },
    });
  }, 60_000);

  it("keeps legacy Slack job coordinates working", async () => {
    const { baseUrl } = await startJobBroker(cleanups, [
      {
        conversationId: "C123",
        rootMessageId: "111.222",
      },
    ]);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "111.222",
      kind: "watch_ci",
      script: "sleep 30",
    });

    expect(response.status).toBe(200);
    expect(response.body).toMatchObject({
      ok: true,
      job: {
        platform: "slack",
        channelId: "C123",
        rootThreadTs: "111.222",
        restartOnBoot: true,
      },
    });
  }, 60_000);

  it("accepts legacy Slack job coordinates when platform is explicitly slack", async () => {
    const { baseUrl } = await startJobBroker(cleanups, [
      {
        conversationId: "C123",
        rootMessageId: "111.222",
      },
    ]);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      platform: "slack",
      channel_id: "C123",
      thread_ts: "111.222",
      kind: "watch_ci",
      script: "sleep 30",
    });

    expect(response.status).toBe(200);
    expect(response.body).toMatchObject({
      ok: true,
      job: {
        platform: "slack",
        channelId: "C123",
        rootThreadTs: "111.222",
      },
    });
  }, 60_000);

  it("documents generic job coordinates in missing-field errors", async () => {
    const { baseUrl } = await startJobBroker(cleanups);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      kind: "watch_ci",
      script: "sleep 30",
    });

    expect(response.status).toBe(400);
    expect(response.body).toEqual({
      ok: false,
      error: "missing_required_body",
      required: ["platform", "conversationId (alias: conversation_id)", "rootMessageId (alias: root_message_id)", "kind", "script"],
      legacyAliases: ["channel_id", "thread_ts"],
    });
  }, 60_000);

  it("rejects invalid job platforms before missing-coordinate validation", async () => {
    const { baseUrl } = await startJobBroker(cleanups);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      platform: "teams",
      kind: "watch_ci",
      script: "sleep 30",
    });
    expect(response.status).toBe(400);
    expect(response.body).toEqual({
      ok: false,
      error: "invalid_platform",
      allowed: ["slack", "feishu"],
    });

    const nonStringPlatform = await fetchJson(`${baseUrl}/jobs/register`, {
      platform: 123,
      conversationId: "C123",
      rootMessageId: "111.222",
      kind: "watch_ci",
      script: "sleep 30",
    });
    expect(nonStringPlatform.status).toBe(400);
    expect(nonStringPlatform.body).toEqual({
      ok: false,
      error: "invalid_platform",
      allowed: ["slack", "feishu"],
    });
  }, 60_000);

  it("does not treat legacy Slack job coordinates as Feishu coordinates", async () => {
    const { baseUrl } = await startJobBroker(cleanups);

    const response = await fetchJson(`${baseUrl}/jobs/register`, {
      platform: "feishu",
      channel_id: "C123",
      thread_ts: "111.222",
      kind: "watch_ci",
      script: "sleep 30",
    });

    expect(response.status).toBe(400);
    expect(response.body).toEqual({
      ok: false,
      error: "missing_required_body",
      required: ["platform", "conversationId (alias: conversation_id)", "rootMessageId (alias: root_message_id)", "kind", "script"],
      legacyAliases: ["channel_id", "thread_ts"],
    });
  }, 60_000);

  it("does not expose script job callback routes", async () => {
    const { baseUrl } = await startJobBroker(cleanups, [
      {
        conversationId: "C123",
        rootMessageId: "111.222",
      },
    ]);
    const registered = await fetchJson(`${baseUrl}/jobs/register`, {
      channel_id: "C123",
      thread_ts: "111.222",
      kind: "watch_ci",
      script: "#!/bin/sh\nsleep 30",
    });
    const job = registered.body.job as { id: string };

    for (const action of ["heartbeat", "event", "complete", "fail", "cancel"]) {
      const response = await fetchJson(`${baseUrl}/jobs/${job.id}/${action}`, {});
      expect(response.status).toBe(405);
    }
  }, 60_000);
});

async function startJobBroker(
  cleanups: Array<() => Promise<void>>,
  sessions: Parameters<typeof seedBrokerSessions>[1] = [],
): Promise<{
  readonly baseUrl: string;
  readonly tempRoot: string;
}> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "job-routes-e2e-"));
  cleanups.push(async () => {
    await removeTempRoot(tempRoot);
  });
  if (sessions.length > 0) {
    await seedBrokerSessions(tempRoot, sessions);
  }

  const mockSlack = new MockSlackServer("UBOT", {
    botId: "BBOT",
    appId: "AAPP",
  });
  const mockCodex = new MockCodexAppServer();
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
  return {
    baseUrl: broker.baseUrl,
    tempRoot,
  };
}
