import http from "node:http";
import { URL } from "node:url";

import { logger } from "../logger.js";
import type { JobManager } from "../services/job-manager.js";
import type { SlackAgentBridge } from "../services/slack/slack-agent-bridge.js";
import { readJsonBody, readString, respondJson } from "./common.js";
import { redactHttpRequestBody } from "./request-log-redaction.js";

export async function handleNotifyRequest(
  method: string,
  url: URL,
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly bridge: SlackAgentBridge;
    readonly jobManager?: JobManager | undefined;
  },
): Promise<boolean> {
  if (method === "POST" && url.pathname === "/notify") {
    await handleNotify(request, response, options);
    return true;
  }

  return false;
}

async function handleNotify(
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly bridge: SlackAgentBridge;
    readonly jobManager?: JobManager | undefined;
  },
): Promise<void> {
  let body: Record<string, unknown>;
  try {
    body = await readJsonBody(request);
  } catch (error) {
    respondJson(response, 400, {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
    return;
  }

  const platform = readString(body.platform) === "feishu" ? "feishu" : readString(body.platform) === "slack" ? "slack" : undefined;
  const conversationId = readString(body.conversation_id) ?? readString(body.conversationId);
  const rootMessageId = readString(body.root_message_id) ?? readString(body.rootMessageId);
  const text = readString(body.text);
  const jobId = readString(body.jobId) ?? readString(body.job_id);

  logger.raw(
    "http-requests",
    {
      method: "POST",
      path: "/notify",
      body: redactHttpRequestBody(body),
    },
    {
      platform,
      conversationId,
      rootMessageId,
      jobId,
    },
  );

  if (!platform || !conversationId || !rootMessageId || !text) {
    respondJson(response, 400, {
      ok: false,
      error: "missing_required_body",
      required: ["platform", "conversationId", "rootMessageId", "text"],
    });
    return;
  }

  if (options.jobManager) {
    const result = await options.jobManager.notify({
      jobId,
      summary: text,
      platform,
      conversationId,
      rootMessageId,
    });
    respondJson(response, 200, { ok: true, suppressed: result === "suppressed" });
    return;
  }

  await options.bridge.acceptBackgroundJobEvent({
    platform,
    conversationId,
    rootMessageId,
    payload: {
      jobId: jobId ?? "notify",
      jobKind: "notify",
      eventKind: "notify",
      summary: text,
    },
  });

  respondJson(response, 200, { ok: true });
}
