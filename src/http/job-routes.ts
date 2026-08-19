import http from "node:http";
import { URL } from "node:url";

import type { JobManager } from "../services/job-manager.js";
import { logger } from "../logger.js";
import { readBoolean, readJsonBody, readString, respondJson } from "./common.js";
import { redactHttpRequestBody } from "./request-log-redaction.js";

const JOB_COORDINATE_REQUIRED_FIELDS = ["platform", "conversationId (alias: conversation_id)", "rootMessageId (alias: root_message_id)", "kind", "script"];
const JOB_LEGACY_COORDINATE_ALIASES = ["channel_id", "thread_ts"] as const;

export async function handleJobRequest(
  method: string,
  url: URL,
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly jobManager: JobManager;
  },
): Promise<boolean> {
  if (method === "POST" && url.pathname === "/jobs/register") {
    await handleJobRegisterRequest(request, response, options);
    return true;
  }

  const matchedAdminCancel = matchJobAdminCancel(url.pathname);
  if (method === "POST" && matchedAdminCancel) {
    await handleJobAdminCancelRequest(request, response, options, matchedAdminCancel);
    return true;
  }

  return false;
}

async function handleJobRegisterRequest(
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly jobManager: JobManager;
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
  logger.raw(
    "http-requests",
    {
      method: "POST",
      path: "/jobs/register",
      body: redactHttpRequestBody(body),
    },
    {
      channelId: readString(body.channel_id),
      rootThreadTs: readString(body.thread_ts),
    },
  );

  const platform = readPlatform(body.platform);
  const platformValue = body.platform;
  const conversationId = readString(body.conversation_id) ?? readString(body.conversationId);
  const rootMessageId = readString(body.root_message_id) ?? readString(body.rootMessageId);
  const channelId = readString(body.channel_id);
  const rootThreadTs = readString(body.thread_ts);
  const kind = readString(body.kind);
  const script = readString(body.script);

  if (isInvalidPlatformValue(platformValue)) {
    respondJson(response, 400, {
      ok: false,
      error: "invalid_platform",
      allowed: ["slack", "feishu"],
    });
    return;
  }

  const hasLegacySlackCoordinates = (!platformValue || platform === "slack") && channelId && rootThreadTs;
  const hasCanonicalCoordinates = platform && conversationId && rootMessageId;
  if (!kind || !script || (!hasLegacySlackCoordinates && !hasCanonicalCoordinates)) {
    respondJson(response, 400, {
      ok: false,
      error: "missing_required_body",
      required: JOB_COORDINATE_REQUIRED_FIELDS,
      legacyAliases: JOB_LEGACY_COORDINATE_ALIASES,
    });
    return;
  }

  try {
    const job = await options.jobManager.registerJob({
      platform,
      conversationId,
      rootMessageId,
      channelId,
      rootThreadTs,
      kind,
      script,
      cwd: readString(body.cwd) || undefined,
      shell: readString(body.shell) || undefined,
      restartOnBoot: readBoolean(body.restart_on_boot, true),
    });
    respondJson(response, 200, {
      ok: true,
      job: {
        id: job.id,
        token: job.token,
        status: job.status,
        kind: job.kind,
        cwd: job.cwd,
        shell: job.shell,
        scriptPath: job.scriptPath,
        restartOnBoot: job.restartOnBoot,
        platform: job.platform,
        conversationId: job.conversationId,
        rootMessageId: job.rootMessageId,
        channelId: job.channelId,
        rootThreadTs: job.rootThreadTs,
        createdAt: job.createdAt,
      },
    });
  } catch (error) {
    respondJson(response, 500, {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

async function handleJobAdminCancelRequest(
  request: http.IncomingMessage,
  response: http.ServerResponse,
  options: {
    readonly jobManager: JobManager;
  },
  action: {
    readonly jobId: string;
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
  logger.raw(
    "http-requests",
    {
      method: "POST",
      path: `/jobs/${action.jobId}/admin-cancel`,
      body: redactHttpRequestBody(body),
    },
    {
      jobId: action.jobId,
    },
  );

  const sessionKey = readString(body.session_key);
  if (!sessionKey) {
    respondJson(response, 400, {
      ok: false,
      error: "missing_required_body",
      required: ["session_key"],
    });
    return;
  }

  try {
    const job = await options.jobManager.cancelJobFromAdmin(action.jobId, {
      sessionKey,
    });
    respondJson(response, 200, {
      ok: true,
      job,
    });
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    respondJson(response, message === "job_session_mismatch" ? 400 : 500, {
      ok: false,
      error: message,
    });
  }
}

function readPlatform(value: unknown): "slack" | "feishu" | undefined {
  return value === "slack" || value === "feishu" ? value : undefined;
}

function isInvalidPlatformValue(value: unknown): boolean {
  return value != null && value !== "" && !readPlatform(value);
}

function matchJobAdminCancel(pathname: string): {
  readonly jobId: string;
} | null {
  const match = pathname.match(/^\/jobs\/([^/]+)\/admin-cancel$/);
  if (!match?.[1]) {
    return null;
  }

  return {
    jobId: decodeURIComponent(match[1]),
  };
}
