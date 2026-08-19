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

  it("syncs an active turn completion from thread/read", async () => {
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

      if (message.method === "turn/start") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              turn: {
                id: "turn-1",
              },
            },
          }),
        );
        return;
      }

      if (message.method === "thread/read") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              thread: {
                turns: [
                  {
                    id: "turn-1",
                    status: "completed",
                    items: [
                      {
                        type: "agentMessage",
                        text: "done",
                      },
                    ],
                  },
                ],
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
    const started = await client.startTurn("thread-1", "/tmp", [
      {
        type: "text",
        text: "hello",
        text_elements: [],
      },
    ]);

    await expect(
      client.readTurnResult("thread-1", "turn-1", {
        syncActiveTurn: true,
      }),
    ).resolves.toEqual({
      status: "completed",
      finalMessage: "done",
      errorMessage: undefined,
      generatedImages: [],
    });
    await expect(started.completion).resolves.toEqual({
      threadId: "thread-1",
      turnId: "turn-1",
      finalMessage: "done",
      aborted: false,
      generatedImages: [],
    });
  });

  it("rejects an active turn when thread/read shows it is missing", async () => {
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

      if (message.method === "turn/start") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              turn: {
                id: "turn-1",
              },
            },
          }),
        );
        return;
      }

      if (message.method === "thread/read") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              thread: {
                turns: [],
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
    const started = await client.startTurn("thread-1", "/tmp", [
      {
        type: "text",
        text: "hello",
        text_elements: [],
      },
    ]);

    await expect(
      client.readTurnResult("thread-1", "turn-1", {
        syncActiveTurn: true,
        treatMissingAsStale: true,
      }),
    ).resolves.toBeNull();
    await expect(started.completion).rejects.toThrow(/missing from thread snapshot/i);
  });

  it("keeps the app-server websocket alive with heartbeat pings", async () => {
    let pingCount = 0;
    const server = await createServer((socket, message) => {
      socket.on("ping", () => {
        pingCount += 1;
      });

      if (message.method === "initialize") {
        socket.send(
          JSON.stringify({
            id: message.id,
            result: { ok: true },
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
      heartbeatIntervalMs: 20,
    });

    await client.connect();
    await new Promise((resolve) => setTimeout(resolve, 75));
    await client.close();

    expect(pingCount).toBeGreaterThan(0);
  });
});
