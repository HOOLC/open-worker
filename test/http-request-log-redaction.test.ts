import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { delay, fetchJson, getFreePort, removeTempRoot, seedBrokerSessions, startBrokerProcess, waitFor } from "./e2e-broker-helpers.js";

describe.sequential("raw HTTP request log redaction", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("redacts body-like fields from generic chat route raw request logs", async () => {
    const { baseUrl, logDir } = await startLoggedBroker(cleanups);
    const inlineContent = Buffer.from("CHAT_SECRET_INLINE_FILE").toString("base64");

    await fetchJson(`${baseUrl}/chat/post-message`, {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      text: "CHAT_SECRET_TEXT",
      kind: "wait",
      reason: "CHAT_SECRET_REASON",
      stop_reason: "CHAT_SECRET_STOP_REASON",
      format: "card",
      card: {
        title: "CHAT_SECRET_CARD",
      },
      rich_text: {
        content: "CHAT_SECRET_RICH",
      },
      richText: {
        content: "CHAT_SECRET_RICH_CAMEL",
      },
    });
    await fetchJson(`${baseUrl}/chat/post-state`, {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      kind: "block",
      reason: "CHAT_SECRET_STATE_REASON",
    });
    await fetchJson(`${baseUrl}/chat/post-file`, {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      content_base64: inlineContent,
      contentBase64: inlineContent,
      filename: "report.txt",
      initial_comment: "CHAT_SECRET_COMMENT",
      initialComment: "CHAT_SECRET_COMMENT_CAMEL",
      text: "CHAT_SECRET_FILE_TEXT",
      alt_text: "CHAT_SECRET_ALT",
      altText: "CHAT_SECRET_ALT_CAMEL",
    });

    const { raw, records } = await readRawHttpLog(logDir);
    expect(raw).not.toContain("CHAT_SECRET_TEXT");
    expect(raw).not.toContain("CHAT_SECRET_REASON");
    expect(raw).not.toContain("CHAT_SECRET_STOP_REASON");
    expect(raw).not.toContain("CHAT_SECRET_CARD");
    expect(raw).not.toContain("CHAT_SECRET_RICH");
    expect(raw).not.toContain("CHAT_SECRET_STATE_REASON");
    expect(raw).not.toContain("CHAT_SECRET_COMMENT");
    expect(raw).not.toContain("CHAT_SECRET_FILE_TEXT");
    expect(raw).not.toContain("CHAT_SECRET_ALT");
    expect(raw).not.toContain(inlineContent);

    const messageBody = findRawBody(records, "/chat/post-message");
    expect(messageBody.text).toMatch(/^\[redacted-text:\d+\]$/u);
    expect(messageBody.reason).toMatch(/^\[redacted-reason:\d+\]$/u);
    expect(messageBody.stop_reason).toMatch(/^\[redacted-reason:\d+\]$/u);
    expect(messageBody.card).toBe("[redacted-card]");
    expect(messageBody.rich_text).toBe("[redacted-rich-text]");
    expect(messageBody.richText).toBe("[redacted-rich-text]");

    const stateBody = findRawBody(records, "/chat/post-state");
    expect(stateBody.reason).toMatch(/^\[redacted-reason:\d+\]$/u);

    const fileBody = findRawBody(records, "/chat/post-file");
    expect(fileBody.content_base64).toMatch(/^\[redacted-base64:\d+\]$/u);
    expect(fileBody.contentBase64).toMatch(/^\[redacted-base64:\d+\]$/u);
    expect(fileBody.initial_comment).toMatch(/^\[redacted-comment:\d+\]$/u);
    expect(fileBody.initialComment).toMatch(/^\[redacted-comment:\d+\]$/u);
    expect(fileBody.text).toMatch(/^\[redacted-text:\d+\]$/u);
    expect(fileBody.alt_text).toMatch(/^\[redacted-alt-text:\d+\]$/u);
    expect(fileBody.altText).toMatch(/^\[redacted-alt-text:\d+\]$/u);
  }, 60_000);

  it("redacts body-like fields from legacy Slack route raw request logs", async () => {
    const { baseUrl, logDir } = await startLoggedBroker(cleanups);
    const inlineContent = Buffer.from("SLACK_SECRET_INLINE_FILE").toString("base64");

    await fetch(`${baseUrl}/slack/post-message`, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded; charset=utf-8",
      },
      body: new URLSearchParams({
        channel_id: "C123",
        thread_ts: "111.222",
        text: "SLACK_SECRET_TEXT",
        kind: "wait",
        reason: "SLACK_SECRET_REASON",
        stop_reason: "SLACK_SECRET_STOP_REASON",
      }).toString(),
    });
    await fetch(`${baseUrl}/slack/post-state`, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded; charset=utf-8",
      },
      body: new URLSearchParams({
        channel_id: "C123",
        thread_ts: "111.222",
        kind: "block",
        reason: "SLACK_SECRET_STATE_REASON",
      }).toString(),
    });
    await fetch(`${baseUrl}/slack/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded; charset=utf-8",
      },
      body: new URLSearchParams({
        channel_id: "C123",
        thread_ts: "111.222",
        content_base64: inlineContent,
        filename: "report.txt",
        initial_comment: "SLACK_SECRET_COMMENT",
        text: "SLACK_SECRET_FILE_TEXT",
        alt_text: "SLACK_SECRET_ALT",
      }).toString(),
    });

    const { raw, records } = await readRawHttpLog(logDir);
    expect(raw).not.toContain("SLACK_SECRET_TEXT");
    expect(raw).not.toContain("SLACK_SECRET_REASON");
    expect(raw).not.toContain("SLACK_SECRET_STOP_REASON");
    expect(raw).not.toContain("SLACK_SECRET_STATE_REASON");
    expect(raw).not.toContain("SLACK_SECRET_COMMENT");
    expect(raw).not.toContain("SLACK_SECRET_FILE_TEXT");
    expect(raw).not.toContain("SLACK_SECRET_ALT");
    expect(raw).not.toContain(inlineContent);

    const messageBody = findRawBody(records, "/slack/post-message");
    expect(messageBody.text).toMatch(/^\[redacted-text:\d+\]$/u);
    expect(messageBody.reason).toMatch(/^\[redacted-reason:\d+\]$/u);
    expect(messageBody.stop_reason).toMatch(/^\[redacted-reason:\d+\]$/u);

    const stateBody = findRawBody(records, "/slack/post-state");
    expect(stateBody.reason).toMatch(/^\[redacted-reason:\d+\]$/u);

    const fileBody = findRawBody(records, "/slack/post-file");
    expect(fileBody.content_base64).toMatch(/^\[redacted-base64:\d+\]$/u);
    expect(fileBody.initial_comment).toMatch(/^\[redacted-comment:\d+\]$/u);
    expect(fileBody.text).toMatch(/^\[redacted-text:\d+\]$/u);
    expect(fileBody.alt_text).toMatch(/^\[redacted-alt-text:\d+\]$/u);
  }, 60_000);

  it("redacts scripts, tokens, and event bodies from job route raw request logs", async () => {
    const { baseUrl, logDir } = await startLoggedBroker(cleanups, [
      {
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
      },
    ]);

    await fetchJson(`${baseUrl}/jobs/register`, {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      kind: "watch_ci",
      script: "JOB_SECRET_SCRIPT",
      cwd: ".",
    });
    await fetchJson(`${baseUrl}/jobs/job-1/event`, {
      token: "JOB_SECRET_TOKEN",
      event_kind: "state_changed",
      summary: "JOB_SECRET_SUMMARY",
      details_text: "JOB_SECRET_DETAILS",
      details_json: {
        value: "JOB_SECRET_JSON",
      },
    });
    await fetchJson(`${baseUrl}/jobs/job-1/fail`, {
      token: "JOB_SECRET_TOKEN",
      summary: "JOB_SECRET_FAIL_SUMMARY",
      error: "JOB_SECRET_ERROR",
    });

    const { raw, records } = await readRawHttpLog(logDir);
    expect(raw).not.toContain("JOB_SECRET_SCRIPT");
    expect(raw).not.toContain("JOB_SECRET_TOKEN");
    expect(raw).not.toContain("JOB_SECRET_SUMMARY");
    expect(raw).not.toContain("JOB_SECRET_DETAILS");
    expect(raw).not.toContain("JOB_SECRET_JSON");
    expect(raw).not.toContain("JOB_SECRET_ERROR");

    const registerBody = findRawBody(records, "/jobs/register");
    expect(registerBody.script).toMatch(/^\[redacted-script:\d+\]$/u);

    const eventBody = findRawBody(records, "/jobs/job-1/event");
    expect(eventBody.token).toMatch(/^\[redacted-token:\d+\]$/u);
    expect(eventBody.summary).toMatch(/^\[redacted-summary:\d+\]$/u);
    expect(eventBody.details_text).toMatch(/^\[redacted-details-text:\d+\]$/u);
    expect(eventBody.details_json).toBe("[redacted-details-json]");

    const failBody = findRawBody(records, "/jobs/job-1/fail");
    expect(failBody.error).toMatch(/^\[redacted-error:\d+\]$/u);
  }, 60_000);

  it("redacts MCP call arguments from integration route raw request logs", async () => {
    const { baseUrl, logDir } = await startLoggedBroker(cleanups);

    await fetchJson(`${baseUrl}/integrations/mcp-call`, {
      server: "linear",
      name: "search",
      arguments: {
        query: "INTEGRATION_SECRET_QUERY",
        apiToken: "INTEGRATION_SECRET_TOKEN",
      },
    });
    await fetchJson(`${baseUrl}/integrations/mcp-call`, {
      server: "linear",
      name: "search",
      arguments: JSON.stringify({
        query: "INTEGRATION_SECRET_STRING_QUERY",
      }),
    });

    const { raw, records } = await readRawHttpLog(logDir);
    expect(raw).not.toContain("INTEGRATION_SECRET_QUERY");
    expect(raw).not.toContain("INTEGRATION_SECRET_TOKEN");
    expect(raw).not.toContain("INTEGRATION_SECRET_STRING_QUERY");

    const bodies = records.filter((record) => record.payload.path === "/integrations/mcp-call").map((record) => record.payload.body);
    expect(bodies).toHaveLength(2);
    expect(bodies[0]?.arguments).toBe("[redacted-arguments]");
    expect(bodies[1]?.arguments).toMatch(/^\[redacted-arguments:\d+\]$/u);
  }, 60_000);
});

async function startLoggedBroker(
  cleanups: Array<() => Promise<void>>,
  sessions: Parameters<typeof seedBrokerSessions>[1] = [],
): Promise<{
  readonly baseUrl: string;
  readonly logDir: string;
}> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "http-redaction-e2e-"));
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
    extraEnv: {
      LOG_RAW_HTTP_REQUESTS: "true",
    },
  });
  cleanups.push(() => broker.stop());
  return {
    baseUrl: broker.baseUrl,
    logDir: path.join(tempRoot, "logs"),
  };
}

async function readRawHttpLog(logDir: string): Promise<{
  raw: string;
  records: Array<{ payload: { path: string; body: Record<string, unknown> } }>;
}> {
  const logPath = path.join(logDir, "raw", "http-requests.jsonl");
  await waitFor(async () => {
    try {
      const raw = await fs.readFile(logPath, "utf8");
      return raw.includes('"path":');
    } catch {
      return false;
    }
  }, "raw HTTP request log");
  await delay(100);
  const raw = await fs.readFile(logPath, "utf8");
  const records = raw
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line) as { payload: { path: string; body: Record<string, unknown> } });
  return { raw, records };
}

function findRawBody(records: Array<{ payload: { path: string; body: Record<string, unknown> } }>, requestPath: string): Record<string, unknown> {
  const record = records.find((candidate) => candidate.payload.path === requestPath);
  if (!record) {
    throw new Error(`missing raw HTTP record for ${requestPath}`);
  }
  return record.payload.body;
}
