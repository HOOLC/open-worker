import http from "node:http";

import { afterEach, describe, expect, it } from "vitest";

import { createHttpHandler } from "../src/http/router.js";

describe("chat routes", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("documents canonical chat coordinate names in missing-field errors", async () => {
    const server = http.createServer(
      createHttpHandler({
        bridge: {} as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const response = await fetch(`${baseUrl}/chat/post-state`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        kind: "wait",
        reason: "waiting for approval",
      }),
    });

    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toEqual({
      ok: false,
      error: "missing_required_body",
      required: ["platform", "conversationId (alias: conversation_id)", "rootMessageId (alias: root_message_id)", "kind"],
    });
  });

  it("rejects invalid generic chat platforms before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          readChatThreadHistory: async (payload: unknown) => {
            calls.push(["history", payload]);
            return {
              messages: [],
              hasMore: false,
            };
          },
          postChatMessage: async (payload: unknown) => {
            calls.push(["message", payload]);
          },
          postChatState: async (payload: unknown) => {
            calls.push(["state", payload]);
          },
          postChatFile: async (payload: unknown) => {
            calls.push(["file", payload]);
            return {
              platform: "slack",
              fileId: "F123",
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const expected = {
      ok: false,
      error: "invalid_platform",
      allowed: ["slack", "feishu"],
    };

    const history = await fetch(`${baseUrl}/chat/thread-history?platform=teams&conversation_id=C123&root_message_id=111.222`);
    expect(history.status).toBe(400);
    await expect(history.json()).resolves.toEqual(expected);

    const message = await fetch(`${baseUrl}/chat/post-message`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "teams",
        conversationId: "C123",
        rootMessageId: "111.222",
        text: "done",
      }),
    });
    expect(message.status).toBe(400);
    await expect(message.json()).resolves.toEqual(expected);

    const nonStringPlatform = await fetch(`${baseUrl}/chat/post-message`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: 123,
        conversationId: "C123",
        rootMessageId: "111.222",
        text: "done",
      }),
    });
    expect(nonStringPlatform.status).toBe(400);
    await expect(nonStringPlatform.json()).resolves.toEqual(expected);

    const state = await fetch(`${baseUrl}/chat/post-state`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "teams",
        conversationId: "C123",
        rootMessageId: "111.222",
        kind: "final",
      }),
    });
    expect(state.status).toBe(400);
    await expect(state.json()).resolves.toEqual(expected);

    const file = await fetch(`${baseUrl}/chat/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "teams",
        conversationId: "C123",
        rootMessageId: "111.222",
        filePath: "/tmp/report.txt",
      }),
    });
    expect(file.status).toBe(400);
    await expect(file.json()).resolves.toEqual(expected);

    expect(calls).toEqual([]);
  });

  it("rejects non-positive or fractional generic chat history limits before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          readChatThreadHistory: async (payload: unknown) => {
            calls.push(payload);
            return {
              messages: [],
              hasMore: false,
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
          feishuHistoryApiMaxLimit: 20,
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    for (const limit of ["abc", "0", "-1", "1.5"]) {
      const response = await fetch(`${baseUrl}/chat/thread-history?platform=feishu&conversation_id=oc_group&root_message_id=om_root&limit=${encodeURIComponent(limit)}`);

      expect(response.status).toBe(400);
      await expect(response.json()).resolves.toEqual({
        ok: false,
        error: "invalid_limit",
        message: "limit must be a positive integer",
      });
    }

    expect(calls).toEqual([]);
  });

  it("rejects invalid generic chat history response formats before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          readChatThreadHistory: async (payload: unknown) => {
            calls.push(payload);
            return {
              messages: [],
              hasMore: false,
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
          feishuHistoryApiMaxLimit: 20,
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    for (const format of ["markdown", "JSON", ""]) {
      const response = await fetch(`${baseUrl}/chat/thread-history?platform=feishu&conversation_id=oc_group&root_message_id=om_root&format=${encodeURIComponent(format)}`);

      expect(response.status).toBe(400);
      await expect(response.json()).resolves.toEqual({
        ok: false,
        error: "invalid_format",
        allowed: ["json", "text"],
      });
    }

    expect(calls).toEqual([]);
  });

  it("rejects invalid rich/card JSON fields before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          postChatMessage: async (payload: unknown) => {
            calls.push(payload);
          },
        } as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const invalidRichText = await fetch(`${baseUrl}/chat/post-message`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
        text: "deploy ready",
        format: "rich_text",
        richText: "{not json",
      }),
    });
    expect(invalidRichText.status).toBe(400);
    await expect(invalidRichText.json()).resolves.toEqual({
      ok: false,
      error: "invalid_json_field",
      field: "richText (alias: rich_text)",
    });

    const invalidCard = await fetch(`${baseUrl}/chat/post-message`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
        text: "deploy ready",
        format: "card",
        card: "{not json",
      }),
    });
    expect(invalidCard.status).toBe(400);
    await expect(invalidCard.json()).resolves.toEqual({
      ok: false,
      error: "invalid_json_field",
      field: "card",
    });

    expect(calls).toEqual([]);
  });

  it("uploads Slack files through generic chat coordinates", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          postChatFile: async (payload: unknown) => {
            calls.push(payload);
            return {
              platform: "slack",
              fileId: "F123",
              title: "report",
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const response = await fetch(`${baseUrl}/chat/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "slack",
        conversation_id: "C123",
        root_message_id: "111.222",
        file_path: "/tmp/report.txt",
        title: "report",
        initial_comment: "see attached",
      }),
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({
      ok: true,
      file: {
        platform: "slack",
        fileId: "F123",
        title: "report",
      },
    });
    expect(calls).toEqual([
      {
        platform: "slack",
        conversationId: "C123",
        rootMessageId: "111.222",
        filePath: "/tmp/report.txt",
        contentBase64: undefined,
        filename: undefined,
        title: "report",
        initialComment: "see attached",
        altText: undefined,
        snippetType: undefined,
        contentType: undefined,
      },
    ]);
  });

  it("documents canonical chat file source names in missing-source errors", async () => {
    const server = http.createServer(
      createHttpHandler({
        bridge: {} as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const response = await fetch(`${baseUrl}/chat/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
      }),
    });

    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toEqual({
      ok: false,
      error: "provide_exactly_one_file_source",
      required: ["filePath (alias: file_path)", "contentBase64 (alias: content_base64)"],
    });
  });

  it("rejects inline chat file uploads without a filename before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          postChatFile: async (payload: unknown) => {
            calls.push(payload);
            return {
              platform: "feishu",
              fileId: "file_uploaded",
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const response = await fetch(`${baseUrl}/chat/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
        contentBase64: Buffer.from("pdf").toString("base64"),
      }),
    });

    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toEqual({
      ok: false,
      error: "missing_required_body",
      message: "filename is required when using contentBase64 (alias: content_base64)",
      required: ["filename"],
    });
    expect(calls).toEqual([]);
  });

  it("rejects invalid inline chat file content before delegation", async () => {
    const calls: unknown[] = [];
    const server = http.createServer(
      createHttpHandler({
        bridge: {
          postChatFile: async (payload: unknown) => {
            calls.push(payload);
            return {
              platform: "feishu",
              fileId: "file_uploaded",
            };
          },
        } as never,
        config: {
          serviceName: "test-broker",
        } as never,
      }),
    );
    const baseUrl = await listen(server);
    cleanups.push(() => close(server));

    const response = await fetch(`${baseUrl}/chat/post-file`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        platform: "feishu",
        conversationId: "oc_group",
        rootMessageId: "om_root",
        contentBase64: "!!!!",
        filename: "report.pdf",
      }),
    });

    expect(response.status).toBe(400);
    await expect(response.json()).resolves.toEqual({
      ok: false,
      error: "invalid_content_base64",
      message: "contentBase64 (alias: content_base64) must decode to non-empty file content",
      required: ["contentBase64 (alias: content_base64)"],
    });
    expect(calls).toEqual([]);
  });
});

async function listen(server: http.Server): Promise<string> {
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to bind test server");
  }
  return `http://127.0.0.1:${address.port}`;
}

async function close(server: http.Server): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    server.close((error) => {
      if (error) {
        reject(error);
        return;
      }
      resolve();
    });
  });
}
