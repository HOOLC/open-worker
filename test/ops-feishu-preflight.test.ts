import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

// @ts-expect-error ops scripts are plain ESM JavaScript without generated declarations.
const opsLib = await import("../scripts/ops/lib.mjs");
const { getAdminHeadersFromInspect, getEnvObjectFromInspect, readDetailedStateFromHost, runCommand, sanitizeOpsLogsForEvidence, summarizeOpsDisplayPath, summarizeOpsEvidencePath, summarizeOpsHostPath, summarizePlatformHealth, shouldRunFeishuPreflight, writeRolloutMetadata } = opsLib;

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(tempDirs.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
});

describe("ops Feishu preflight helpers", () => {
  it("parses inspected env entries without losing values containing equals signs", () => {
    const env = getEnvObjectFromInspect({
      Config: {
        Env: ["FEISHU_ENABLED=true", "BROKER_ADMIN_TOKEN=abc=def", "MALFORMED"],
      },
    });

    expect(env).toEqual({
      FEISHU_ENABLED: "true",
      BROKER_ADMIN_TOKEN: "abc=def",
    });
  });

  it("runs Feishu preflight only for Feishu-enabled rollout containers", () => {
    expect(
      shouldRunFeishuPreflight({
        Config: {
          Env: ["FEISHU_ENABLED=true"],
        },
      }),
    ).toBe(true);
    expect(
      shouldRunFeishuPreflight({
        Config: {
          Env: ["FEISHU_ENABLED=false"],
        },
      }),
    ).toBe(false);
    expect(
      shouldRunFeishuPreflight({
        Config: {
          Env: [],
        },
      }),
    ).toBe(false);
  });

  it("builds admin auth headers from the inspected container without exposing them in summaries", () => {
    expect(
      getAdminHeadersFromInspect({
        Config: {
          Env: ["BROKER_ADMIN_TOKEN=secret-token"],
        },
      }),
    ).toEqual({
      "x-admin-token": "secret-token",
    });
    expect(
      getAdminHeadersFromInspect({
        Config: {
          Env: [],
        },
      }),
    ).toEqual({});
  });

  it("summarizes platform health from admin status without copying recent logs", () => {
    expect(
      summarizePlatformHealth({
        platforms: {
          slack: {
            platform: "slack",
            enabled: true,
            state: "ready",
            startupRequired: true,
            connection: {
              mode: "socket_mode",
              connected: true,
            },
            lastEvent: {
              messageId: "slack-message",
            },
          },
          feishu: {
            platform: "feishu",
            enabled: true,
            state: "degraded",
            startupRequired: true,
            groupMessageMode: "all",
            allMessageDeliveryVerified: false,
            degradedReason: "all_message_delivery_unverified",
            connection: {
              mode: "long_connection",
              connected: true,
            },
            permissions: [
              {
                name: "im:message.group_msg",
                requiredFor: "non-@ follow-ups",
                status: "configured",
              },
            ],
            lastError: {
              message: "do not copy this",
            },
          },
        },
        state: {
          recentBrokerLogs: [
            {
              message: "chat.message.accepted",
            },
          ],
        },
      }),
    ).toEqual({
      slack: {
        platform: "slack",
        enabled: true,
        state: "ready",
        startupRequired: true,
        groupMessageMode: undefined,
        allMessageDeliveryVerified: undefined,
        degradedReason: undefined,
        connection: {
          mode: "socket_mode",
          connected: true,
        },
        permissions: undefined,
      },
      feishu: {
        platform: "feishu",
        enabled: true,
        state: "degraded",
        startupRequired: true,
        groupMessageMode: "all",
        allMessageDeliveryVerified: false,
        degradedReason: "all_message_delivery_unverified",
        connection: {
          mode: "long_connection",
          connected: true,
        },
        permissions: [
          {
            name: "im:message.group_msg",
            status: "configured",
          },
        ],
      },
    });
  });

  it("allowlists platform-health summary fields to posture-safe values", () => {
    const summary = summarizePlatformHealth({
      platforms: {
        feishu: {
          platform: "FEISHU_OPS_PLATFORM_SECRET",
          enabled: true,
          state: "ready FEISHU_OPS_STATE_SECRET",
          startupRequired: true,
          groupMessageMode: "all FEISHU_OPS_MODE_SECRET",
          allMessageDeliveryVerified: "FEISHU_OPS_BOOLEAN_SECRET",
          degradedReason: "FEISHU_OPS_REASON_SECRET",
          connection: {
            mode: "long_connection FEISHU_OPS_CONNECTION_SECRET",
            connected: true,
          },
          permissions: [
            {
              name: "im:message.group_msg",
              requiredFor: "FEISHU_OPS_REQUIRED_FOR_SECRET",
              status: "verified",
            },
            {
              name: "im:message.group_msg FEISHU_OPS_PERMISSION_SECRET",
              status: "configured",
            },
            {
              name: "bot_identity",
              status: "configured FEISHU_OPS_PERMISSION_STATUS_SECRET",
            },
          ],
        },
      },
    });

    expect(summary.feishu).toEqual({
      platform: "feishu",
      enabled: true,
      state: "unknown",
      startupRequired: true,
      groupMessageMode: undefined,
      allMessageDeliveryVerified: undefined,
      degradedReason: undefined,
      connection: {
        mode: undefined,
        connected: true,
      },
      permissions: [
        {
          name: "im:message.group_msg",
          status: "verified",
        },
      ],
    });
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_PLATFORM_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_STATE_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_MODE_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_BOOLEAN_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_REASON_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_CONNECTION_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_REQUIRED_FOR_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_PERMISSION_SECRET");
    expect(JSON.stringify(summary)).not.toContain("FEISHU_OPS_PERMISSION_STATUS_SECRET");
  });

  it("summarizes detailed host state without copying raw inbound bodies, job tokens, or raw broker logs", async () => {
    const dataRoot = await fs.mkdtemp(path.join(os.tmpdir(), "ops-status-redaction-"));
    tempDirs.push(dataRoot);

    await fs.mkdir(path.join(dataRoot, "state", "sessions"), { recursive: true });
    await fs.mkdir(path.join(dataRoot, "state", "inbound-messages"), { recursive: true });
    await fs.mkdir(path.join(dataRoot, "state", "background-jobs"), { recursive: true });
    await fs.mkdir(path.join(dataRoot, "logs"), { recursive: true });

    await fs.writeFile(
      path.join(dataRoot, "state", "sessions", "feishu-session.json"),
      JSON.stringify({
        key: "feishu:oc_group:om_root",
        platform: "feishu",
        conversationId: "oc_group",
        conversationKind: "group",
        rootMessageId: "om_root",
        platformThreadId: "om_root",
        channelId: "oc_group",
        rootThreadTs: "om_root",
        workspacePath: "/srv/broker/.data/sessions/feishu",
        codexThreadId: "codex-thread-secret",
        activeTurnId: "turn_123",
        coAuthorCandidateUserIds: ["ou_secret_user"],
        createdAt: "2026-03-19T00:00:00.000Z",
        updatedAt: "2026-03-19T00:00:10.000Z",
      }),
      "utf8",
    );
    await fs.writeFile(
      path.join(dataRoot, "state", "inbound-messages", "feishu-session.json"),
      JSON.stringify([
        {
          key: "message-key",
          sessionKey: "feishu:oc_group:om_root",
          channelId: "oc_group",
          channelType: "group",
          rootThreadTs: "om_root",
          messageTs: "om_msg",
          source: "thread_reply",
          userId: "ou_secret_user",
          senderKind: "user",
          botId: "ou_secret_bot",
          appId: "cli_secret_app",
          text: "OPS_STATUS_SECRET_BODY",
          contextText: "OPS_STATUS_CONTEXT_SECRET",
          slackMessage: {
            text: "OPS_STATUS_SLACK_MESSAGE_SECRET",
          },
          images: [
            {
              alt: "OPS_STATUS_IMAGE_SECRET",
            },
          ],
          backgroundJob: {
            summary: "OPS_STATUS_JOB_EVENT_SECRET",
          },
          status: "pending",
          batchId: "batch_123",
          createdAt: "2026-03-19T00:00:01.000Z",
          updatedAt: "2026-03-19T00:00:02.000Z",
        },
      ]),
      "utf8",
    );
    await fs.writeFile(
      path.join(dataRoot, "state", "background-jobs", "job.json"),
      JSON.stringify({
        id: "job_123",
        token: "OPS_STATUS_JOB_TOKEN_SECRET",
        sessionKey: "feishu:oc_group:om_root",
        channelId: "oc_group",
        rootThreadTs: "om_root",
        kind: "background_job_event",
        shell: "/bin/zsh OPS_STATUS_SHELL_SECRET",
        cwd: "/srv/broker/.data/repos/example",
        scriptPath: "/tmp/OPS_STATUS_SCRIPT_SECRET.sh",
        restartOnBoot: true,
        status: "running",
        error: "OPS_STATUS_JOB_ERROR_SECRET",
        lastEventSummary: "OPS_STATUS_LAST_EVENT_SECRET",
        createdAt: "2026-03-19T00:00:03.000Z",
        updatedAt: "2026-03-19T00:00:04.000Z",
      }),
      "utf8",
    );
    await fs.writeFile(
      path.join(dataRoot, "logs", "broker.jsonl"),
      [
        JSON.stringify({
          ts: "2026-03-19T00:00:05.000Z",
          type: "log",
          level: "info",
          message: "chat.message.accepted",
          meta: {
            platform: "feishu",
            conversationId: "oc_group",
            jobId: "job_123",
            messageId: "om_msg",
            route: "group_message",
            payloadRef: "Bearer ops-log-token",
            fileId: "xoxb-ops-secret",
            content: "OPS_STATUS_LOG_CONTENT_SECRET",
            rawPayload: {
              text: "OPS_STATUS_LOG_PAYLOAD_SECRET",
              email: "ops-status@example.com",
            },
          },
        }),
        "not json OPS_STATUS_RAW_LINE_SECRET",
      ].join("\n") + "\n",
      "utf8",
    );

    const state = await readDetailedStateFromHost(dataRoot, {
      openInboundLimit: 10,
      logLineLimit: 10,
    });
    const serialized = JSON.stringify(state);

    expect(serialized).not.toContain("OPS_STATUS_SECRET_BODY");
    expect(serialized).not.toContain("OPS_STATUS_CONTEXT_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_SLACK_MESSAGE_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_IMAGE_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_JOB_EVENT_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_JOB_TOKEN_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_SHELL_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_SCRIPT_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_JOB_ERROR_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_LAST_EVENT_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_LOG_CONTENT_SECRET");
    expect(serialized).not.toContain("OPS_STATUS_LOG_PAYLOAD_SECRET");
    expect(serialized).not.toContain("Bearer ops-log-token");
    expect(serialized).not.toContain("xoxb-ops-secret");
    expect(serialized).not.toContain("ops-status@example.com");
    expect(serialized).not.toContain("OPS_STATUS_RAW_LINE_SECRET");
    expect(serialized).not.toContain("/srv/broker/.data/sessions/feishu");
    expect(serialized).not.toContain("/srv/broker/.data/repos/example");
    expect(serialized).not.toContain("ou_secret_user");
    expect(serialized).not.toContain("ou_secret_bot");
    expect(serialized).not.toContain("cli_secret_app");
    expect(serialized).not.toContain("codex-thread-secret");

    expect(state).toMatchObject({
      activeSessions: [
        expect.objectContaining({
          sessionKey: "feishu:oc_group:om_root",
          activeTurnId: "turn_123",
          workspacePathBasename: "feishu",
        }),
      ],
      openInbound: [
        expect.objectContaining({
          sessionKey: "feishu:oc_group:om_root",
          messageTs: "om_msg",
          textPreview: "message body redacted (22 chars)",
          textLength: 22,
          textRedacted: true,
        }),
      ],
      backgroundJobs: [
        expect.objectContaining({
          id: "job_123",
          jobId: "job_123",
          kind: "background_job_event",
          cwdBasename: "example",
          errorLength: "OPS_STATUS_JOB_ERROR_SECRET".length,
          errorRedacted: true,
        }),
      ],
      recentBrokerLogs: [
        expect.objectContaining({
          type: "log",
          message: "chat.message.accepted",
          meta: expect.objectContaining({
            platform: "feishu",
            conversationId: "oc_group",
            jobId: "job_123",
            messageId: "om_msg",
            route: "group_message",
          }),
        }),
        expect.objectContaining({
          type: "log_parse_error",
          message: "unparseable broker log line",
        }),
      ],
    });
    expect(state.openInbound[0]).not.toHaveProperty("text");
    expect(state.openInbound[0]).not.toHaveProperty("contextText");
    expect(state.backgroundJobs[0]).not.toHaveProperty("token");
    expect(state.backgroundJobs[0]).not.toHaveProperty("cwd");
    expect(state.backgroundJobs[0]).not.toHaveProperty("scriptPath");
    expect(state.activeSessions[0]).not.toHaveProperty("workspacePath");
    expect(state.recentBrokerLogs[0].meta).not.toHaveProperty("content");
    expect(state.recentBrokerLogs[0].meta).not.toHaveProperty("rawPayload");
    expect(state.recentBrokerLogs[0].meta).not.toHaveProperty("payloadRef");
    expect(state.recentBrokerLogs[0].meta).not.toHaveProperty("fileId");
  });

  it("summarizes ops host paths without exposing full filesystem paths", () => {
    expect(summarizeOpsHostPath("/Users/operator@example.com/OPS_STATUS_PATH_SECRET/.data")).toEqual({
      basename: ".data",
      redacted: true,
    });
    expect(summarizeOpsHostPath("/tmp/OPS_STATUS_PATH_SECRET")).toEqual({
      basename: "[redacted-path]",
      redacted: true,
    });
  });

  it("summarizes rollout evidence paths as safe repo-relative coordinates", () => {
    const rolloutPath = path.join(process.cwd(), ".backups", "rollouts", "2026-05-29T10-00-00-000Z", "feishu-preflight");

    expect(summarizeOpsEvidencePath(rolloutPath)).toEqual({
      relativePath: ".backups/rollouts/2026-05-29T10-00-00-000Z/feishu-preflight",
      basename: "feishu-preflight",
      redacted: true,
    });
    expect(JSON.stringify(summarizeOpsEvidencePath(rolloutPath))).not.toContain(process.cwd());
    expect(summarizeOpsEvidencePath("/tmp/OPS_STATUS_PATH_SECRET/feishu-preflight")).toEqual({
      basename: "feishu-preflight",
      redacted: true,
    });
  });

  it("formats operator-facing paths without exposing full host filesystem paths", () => {
    const repoEvidencePath = path.join(process.cwd(), ".backups", "auth-switches", "stamp", "auth.json");

    expect(summarizeOpsDisplayPath(repoEvidencePath)).toBe(".backups/auth-switches/stamp/auth.json");
    expect(summarizeOpsDisplayPath("/Users/operator@example.com/.codex/auth.json")).toBe("auth.json (path redacted)");
    expect(summarizeOpsDisplayPath("/tmp/OPS_STATUS_PATH_SECRET")).toBe("[redacted-path] (path redacted)");
    expect(summarizeOpsDisplayPath(repoEvidencePath)).not.toContain(process.cwd());
  });

  it("redacts command failure details before surfacing ops errors", () => {
    const repoEvidencePath = path.join(process.cwd(), ".backups", "rollouts", "stamp", "feishu-preflight");

    let message = "";
    try {
      runCommand(process.execPath, ["-e", [`console.error("repo path ${repoEvidencePath}");`, "console.error(\"host path '/tmp/OPS_STATUS_PATH_SECRET/operator@example.com/setup.json'\");", "console.error('Bearer ops-secret-token xoxb-ops-secret FEISHU_APP_SECRET=missing');", "process.exit(7);"].join("")], {
        capture: true,
      });
    } catch (error) {
      message = error instanceof Error ? error.message : String(error);
    }

    expect(message).toContain("Command failed (7):");
    expect(message).toContain(".backups/rollouts/stamp/feishu-preflight");
    expect(message).toContain("setup.json (path redacted)");
    expect(message).toContain("FEISHU_APP_SECRET=missing");
    expect(message).not.toContain(process.cwd());
    expect(message).not.toContain(process.execPath);
    expect(message).not.toContain("/tmp/");
    expect(message).not.toContain("OPS_STATUS_PATH_SECRET");
    expect(message).not.toContain("operator@example.com");
    expect(message).not.toContain("Bearer ops-secret-token");
    expect(message).not.toContain("xoxb-ops-secret");
  });

  it("sanitizes rollout preflight broker logs before writing evidence", () => {
    const rawLogs = [
      JSON.stringify({
        ts: "2026-05-29T00:00:00.000Z",
        type: "log",
        level: "info",
        message: "chat.message.accepted",
        meta: {
          platform: "feishu",
          conversationId: "oc_group",
          messageId: "om_msg",
          route: "group_message",
          text: "OPS_ROLLOUT_LOG_BODY_SECRET",
          payloadRef: "Bearer rollout-log-secret",
        },
      }),
      '2026-05-29T00:00:01.000Z INFO chat.platform.ready {"platform":"feishu","source":"long_connection","groupMessageMode":"all","durationMs":3,"body":"OPS_ROLLOUT_TEXT_LOG_BODY_SECRET","email":"operator@example.com"}',
      "Connected to Slack Socket Mode with token xoxb-rollout-secret",
      "raw line with /tmp/OPS_ROLLOUT_PATH_SECRET/operator@example.com/message-body.txt and OPS_ROLLOUT_BODY_SECRET",
    ].join("\n");

    const sanitized = sanitizeOpsLogsForEvidence(rawLogs);
    const records = (sanitized as string)
      .trim()
      .split("\n")
      .map((line: string) => JSON.parse(line));
    const serialized = JSON.stringify(records);

    expect(records).toEqual([
      expect.objectContaining({
        type: "log",
        message: "chat.message.accepted",
        meta: {
          platform: "feishu",
          conversationId: "oc_group",
          messageId: "om_msg",
          route: "group_message",
        },
      }),
      expect.objectContaining({
        type: "log",
        message: "chat.platform.ready",
        meta: {
          platform: "feishu",
          source: "long_connection",
          groupMessageMode: "all",
          durationMs: 3,
        },
      }),
      expect.objectContaining({
        type: "log_text_redacted",
        message: "Connected to Slack Socket Mode",
      }),
      expect.objectContaining({
        type: "log_text_redacted",
        message: "non-structured log line redacted",
      }),
    ]);
    expect(serialized).not.toContain("OPS_ROLLOUT_LOG_BODY_SECRET");
    expect(serialized).not.toContain("OPS_ROLLOUT_TEXT_LOG_BODY_SECRET");
    expect(serialized).not.toContain("OPS_ROLLOUT_PATH_SECRET");
    expect(serialized).not.toContain("OPS_ROLLOUT_BODY_SECRET");
    expect(serialized).not.toContain("Bearer rollout-log-secret");
    expect(serialized).not.toContain("xoxb-rollout-secret");
    expect(serialized).not.toContain("operator@example.com");
    expect(serialized).not.toContain("/tmp/");
  });

  it("sanitizes rollout metadata before writing it to evidence files", async () => {
    const outputDir = await fs.mkdtemp(path.join(os.tmpdir(), "ops-rollout-metadata-"));
    tempDirs.push(outputDir);
    const repoEvidencePath = path.join(process.cwd(), ".backups", "rollouts", "stamp", "feishu-preflight");

    await writeRolloutMetadata(outputDir, {
      containerName: "slack-codex-broker-real",
      rolloutDir: repoEvidencePath,
      dataRootSource: "/tmp/OPS_METADATA_PATH_SECRET/operator@example.com/.data",
      diagnostic: "Bearer ops-metadata-secret xoxb-ops-metadata-secret",
      feishuPreflight: {
        report: {
          checks: [
            {
              id: "preflight.feishu_app_secret_present",
              evidence: ["FEISHU_APP_SECRET=missing", "operator@example.com", "/tmp/OPS_METADATA_REPORT_PATH_SECRET/report.json"],
            },
          ],
        },
      },
    });

    const metadata = await fs.readFile(path.join(outputDir, "metadata.json"), "utf8");

    expect(metadata).toContain("slack-codex-broker-real");
    expect(metadata).toContain(".backups/rollouts/stamp/feishu-preflight");
    expect(metadata).toContain("FEISHU_APP_SECRET=missing");
    expect(metadata).not.toContain(process.cwd());
    expect(metadata).not.toContain("/tmp/");
    expect(metadata).not.toContain("OPS_METADATA_PATH_SECRET");
    expect(metadata).not.toContain("OPS_METADATA_REPORT_PATH_SECRET");
    expect(metadata).not.toContain("operator@example.com");
    expect(metadata).not.toContain("Bearer ops-metadata-secret");
    expect(metadata).not.toContain("xoxb-ops-metadata-secret");
  });
});
