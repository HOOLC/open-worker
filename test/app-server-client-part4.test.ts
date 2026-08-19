import fs from "node:fs/promises";

import { afterEach, describe, expect, it } from "vitest";

import { AppServerClient } from "../src/services/codex/app-server-client.js";

import { TestServer, createServer } from "./app-server-client-helpers.js";

describe("AppServerClient disconnect handling", () => {
  const servers: TestServer[] = [];

  const tempDirs: string[] = [];

  afterEach(async () => {
    await Promise.all(servers.splice(0).map((server) => server.close()));
    await Promise.all(
      tempDirs.splice(0).map((directory) =>
        fs.rm(directory, {
          force: true,
          recursive: true,
        }),
      ),
    );
  });

  it("uses Feishu-aware broker commands in Feishu thread/start base instructions", async () => {
    let threadStartParams: Record<string, unknown> | undefined;
    const server = await createServer((socket, message) => {
      if (message.method === "initialize") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: { ok: true },
          }),
        );
        return;
      }

      if (message.method === "thread/start") {
        threadStartParams = (message as { params?: Record<string, unknown> }).params;
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              thread: {
                id: "thread-feishu",
              },
            },
          }),
        );
      }
    });
    servers.push(server);

    const client = new AppServerClient({
      url: server.url,
      serviceName: "test",
      brokerHttpBaseUrl: "http://127.0.0.1:3000",
      reposRoot: "/tmp/repos",
    });

    await client.connect();
    await expect(
      client.ensureThread({
        platform: "feishu",
        conversationId: "oc_group",
        conversationKind: "group",
        rootMessageId: "om_root",
        platformThreadId: "omt_thread",
        channelId: "oc_group",
        rootThreadTs: "om_root",
        workspacePath: "/tmp/feishu-workspace",
      }),
    ).resolves.toBe("thread-feishu");

    const baseInstructions = String(threadStartParams?.baseInstructions ?? "");
    expect(baseInstructions).toContain("Current Feishu thread coordinates");
    expect(baseInstructions).toContain("platform: feishu");
    expect(baseInstructions).toContain("conversation_id: oc_group");
    expect(baseInstructions).toContain("root_message_id: om_root");
    expect(baseInstructions).toContain("platform_thread_id: omt_thread");
    expect(baseInstructions).toContain("this session is anchored to one Feishu topic");
    expect(baseInstructions).toContain("Feishu equivalent of a Slack thread");
    expect(baseInstructions).toContain("/chat/post-message");
    expect(baseInstructions).toContain('"platform":"feishu"');
    expect(baseInstructions).toContain("/chat/post-state");
    expect(baseInstructions).toContain("/chat/thread-history?platform=feishu");
    expect(baseInstructions).not.toContain("/slack/post-message");
    expect(baseInstructions).not.toContain("channel_id: oc_group");
  });
});
