import http from "node:http";
import { URL } from "node:url";

import { logger } from "../logger.js";
import type { SessionManager } from "../services/session-manager.js";
import { respondJson } from "./common.js";

export async function handleCliRequest(
  method: string,
  url: URL,
  response: http.ServerResponse,
  options: {
    readonly sessions: SessionManager;
  },
): Promise<boolean> {
  if (method === "GET" && url.pathname === "/cli/context") {
    await handleCliContext(url, response, options);
    return true;
  }

  return false;
}

async function handleCliContext(
  url: URL,
  response: http.ServerResponse,
  options: {
    readonly sessions: SessionManager;
  },
): Promise<void> {
  const threadId = url.searchParams.get("threadId")?.trim() || url.searchParams.get("thread_id")?.trim();
  logger.raw(
    "http-requests",
    {
      method: "GET",
      path: "/cli/context",
      query: { threadId },
    },
    { threadId },
  );

  if (!threadId) {
    respondJson(response, 400, {
      ok: false,
      error: "missing_thread_id",
    });
    return;
  }

  const session = options.sessions.findSessionByAgentActivity({ agentSessionId: threadId });
  if (!session) {
    respondJson(response, 404, {
      ok: false,
      error: "unknown_thread",
    });
    return;
  }

  const platform = session.platform === "feishu" ? "feishu" : "slack";
  respondJson(response, 200, {
    ok: true,
    platform,
    conversationId: session.conversationId ?? session.channelId,
    rootMessageId: session.rootMessageId ?? session.rootThreadTs,
    channelId: session.channelId,
    rootThreadTs: session.rootThreadTs,
  });
}
