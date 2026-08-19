import fs from "node:fs/promises";

import { afterEach, describe, expect, it } from "vitest";

import { WebSocketServer, type WebSocket } from "ws";

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

  it("uses thread runtime defaults when token usage events omit model settings", async () => {
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
        socket.send(
          JSON.stringify({
            id: message.id,
            result: {
              thread: {
                id: "thread-runtime-defaults",
              },
              model: "gpt-5.5",
              reasoningEffort: "xhigh",
            },
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
                id: "turn-runtime-defaults",
              },
            },
          }),
        );
        socket.send(
          JSON.stringify({
            method: "thread/tokenUsage/updated",
            params: {
              threadId: "thread-runtime-defaults",
              turnId: "turn-runtime-defaults",
              tokenUsage: {
                total: {
                  totalTokens: 500,
                  inputTokens: 320,
                  cachedInputTokens: 100,
                  outputTokens: 180,
                },
                last: {
                  totalTokens: 500,
                  inputTokens: 320,
                  cachedInputTokens: 100,
                  outputTokens: 180,
                },
              },
            },
          }),
        );
        socket.send(
          JSON.stringify({
            method: "turn/completed",
            params: {
              turn: {
                id: "turn-runtime-defaults",
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
    const threadId = await client.ensureThread({
      workspacePath: "/tmp",
      channelId: "C1",
      rootThreadTs: "1.000001",
    });
    const started = await client.startTurn(threadId, "/tmp", [
      {
        type: "text",
        text: "hello",
        text_elements: [],
      },
    ]);

    await expect(started.completion).resolves.toMatchObject({
      threadId: "thread-runtime-defaults",
      turnId: "turn-runtime-defaults",
      usage: {
        source: "exact",
        inputTokens: 320,
        cachedInputTokens: 100,
        outputTokens: 180,
        reasoningTokens: 0,
        totalTokens: 500,
        model: "gpt-5.5",
        effort: "xhigh",
      },
    });
  });

  it("does not emit an unhandled rejection when a turn disconnects before completion is awaited", async () => {
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
                id: "turn-early-close",
              },
            },
          }),
        );
        setTimeout(() => {
          socket.close();
        }, 0);
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

    const unhandled: unknown[] = [];
    const onUnhandled = (reason: unknown) => {
      unhandled.push(reason);
    };
    process.on("unhandledRejection", onUnhandled);

    try {
      await new Promise((resolve) => setTimeout(resolve, 25));
      await expect(started.completion).rejects.toThrow(/closed/i);
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(unhandled).toEqual([]);
    } finally {
      process.off("unhandledRejection", onUnhandled);
    }
  });

  it("can recover a completed turn result from thread/read", async () => {
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
    await expect(client.readTurnResult("thread-1", "turn-1")).resolves.toEqual({
      status: "completed",
      finalMessage: "done",
      errorMessage: undefined,
      generatedImages: [],
    });
  });

  it("parses generated images from thread/read results", async () => {
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
                      {
                        type: "image_generation_call",
                        id: "ig-1",
                        revised_prompt: "blue cat",
                        result: "QUJDREVGRw==",
                        saved_path: "/tmp/ig-1.png",
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
    await expect(client.readTurnResult("thread-1", "turn-1")).resolves.toEqual({
      status: "completed",
      finalMessage: "done",
      errorMessage: undefined,
      generatedImages: [
        {
          id: "ig-1",
          contentBase64: "QUJDREVGRw==",
          contentType: "image/png",
          savedPath: "/tmp/ig-1.png",
          revisedPrompt: "blue cat",
        },
      ],
    });
  });

  it("captures image generation results from live turn notifications", async () => {
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
        socket.send(
          JSON.stringify({
            method: "item/completed",
            params: {
              turnId: "turn-1",
              item: {
                type: "imageGeneration",
                id: "ig-1",
                revisedPrompt: "blue cat",
                result: "QUJDREVGRw==",
                savedPath: "/tmp/ig-1.png",
              },
            },
          }),
        );
        socket.send(
          JSON.stringify({
            method: "turn/completed",
            params: {
              turn: {
                id: "turn-1",
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

    await expect(started.completion).resolves.toEqual({
      threadId: "thread-1",
      turnId: "turn-1",
      finalMessage: "",
      aborted: false,
      generatedImages: [
        {
          id: "ig-1",
          contentBase64: "QUJDREVGRw==",
          contentType: "image/png",
          savedPath: "/tmp/ig-1.png",
          revisedPrompt: "blue cat",
        },
      ],
    });
  });
});
