import { afterEach, describe, expect, it } from "vitest";

import { readSessionRecord, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";
import { feishuSessionKey, postChatJson, startFeishuE2eRuntime, type FeishuE2eRuntime } from "./feishu-e2e-helpers.js";
import { createFeishuGroupTextEvent } from "./helpers/mock-feishu-server.js";

describe.sequential("chat routes e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("posts Slack messages through generic chat coordinates on the running broker", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockSlack.sendEvent("evt-chat-post", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "111.222",
      ts: "111.223",
      text: "<@UBOT> start",
    });
    await waitForSessionIdle(runtime.tempRoot, "C123:111.222");

    const response = await postChatJson(runtime.baseUrl, "/chat/post-message", {
      platform: "slack",
      conversation_id: "C123",
      root_message_id: "111.222",
      text: "done",
      kind: "final",
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true });
    await waitFor(() => runtime.mockSlack.postedMessages.some((message) => message.text === "done" && message.threadTs === "111.222"), "Slack chat post");
  }, 60_000);

  it("accepts canonical camelCase chat coordinate fields for Feishu state", async () => {
    const runtime = await startRuntime(cleanups);
    await startFeishuSession(runtime, "om_camel");

    const response = await postChatJson(runtime.baseUrl, "/chat/post-state", {
      platform: "feishu",
      conversationId: "oc_group",
      rootMessageId: "om_camel",
      kind: "wait",
      reason: "waiting for approval",
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true });
    await expect(readSessionRecord(runtime.tempRoot, feishuSessionKey("oc_group", "om_camel"))).resolves.toMatchObject({
      lastTurnSignalKind: "wait",
    });
  }, 60_000);

  it("reads Slack thread history through generic chat coordinates", async () => {
    const runtime = await startRuntime(cleanups);
    runtime.mockSlack.recordThreadMessage({
      channel: "C123",
      threadTs: "111.222",
      ts: "111.221",
      text: "history text",
      user: "U123",
    });

    const response = await fetch(`${runtime.baseUrl}/chat/thread-history?platform=slack&conversation_id=C123&root_message_id=111.222&before_message_id=111.223&limit=20&format=text`);

    expect(response.status).toBe(200);
    const body = await response.text();
    expect(body).toContain("history text");
  }, 60_000);

  it("returns Feishu chat history pagination cursors from the platform adapter", async () => {
    const runtime = await startRuntime(cleanups);
    runtime.mockFeishu.historyHasMore = true;
    runtime.mockFeishu.historyPageToken = "page_next";
    runtime.mockFeishu.historyItems = [
      {
        message_id: "om_history",
        root_id: "om_root",
        parent_id: "om_root",
        thread_id: "omt_thread",
        msg_type: "text",
        create_time: "1710000001000",
        chat_id: "oc_group",
        body: {
          content: JSON.stringify({
            text: "recovered history",
          }),
        },
        raw: {
          sender: {
            id: "ou_user",
            id_type: "open_id",
            sender_type: "user",
          },
        },
      },
    ];

    const response = await fetch(`${runtime.baseUrl}/chat/thread-history?platform=feishu&conversation_id=oc_group&root_message_id=om_root&before_cursor=page_current&limit=20`);

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      ok: true,
      platform: "feishu",
      returnedCount: 1,
      hasMore: true,
      nextCursor: "page_next",
      maxLimit: 50,
    });
    expect(runtime.mockFeishu.listRequests.some((entry) => entry.params.page_token === "page_current")).toBe(true);
  }, 60_000);

  it("posts Feishu card messages through generic chat coordinates", async () => {
    const runtime = await startRuntime(cleanups);
    const response = await postChatJson(runtime.baseUrl, "/chat/post-message", {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      text: "deploy ready",
      format: "card",
      card: {
        config: {
          wide_screen_mode: true,
        },
        header: {
          title: "Deploy",
        },
      },
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true });
    const posted = await runtime.mockFeishu.waitForPostedMessage((message) => message.msgType === "interactive");
    expect(posted.replyToMessageId).toBe("om_root");
    expect(posted.content).toEqual(
      expect.objectContaining({
        header: expect.objectContaining({
          title: "Deploy",
        }),
      }),
    );
  }, 60_000);

  it("records Feishu visible final lifecycle through generic chat coordinates", async () => {
    const runtime = await startRuntime(cleanups);
    await startFeishuSession(runtime, "om_final");

    const response = await postChatJson(runtime.baseUrl, "/chat/post-message", {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_final",
      text: "Final answer is visible.",
      kind: "final",
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true });
    await runtime.mockFeishu.waitForPostedMessage((message) => {
      return Boolean(message.content && typeof message.content === "object" && "text" in message.content && (message.content as { text?: string }).text === "Final answer is visible.");
    });
    await expect(readSessionRecord(runtime.tempRoot, feishuSessionKey("oc_group", "om_final"))).resolves.toMatchObject({
      lastTurnSignalKind: "final",
    });
  }, 60_000);

  it("returns Feishu send failures from generic chat message posts", async () => {
    const runtime = await startRuntime(cleanups);
    runtime.mockFeishu.replyError = {
      code: 1,
      msg: "Feishu send failed",
    };

    const response = await postChatJson(runtime.baseUrl, "/chat/post-message", {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      text: "done",
      format: "text",
    });

    expect(response.status).toBe(500);
    await expect(response.json()).resolves.toEqual({
      ok: false,
      error: "Feishu API error for im.v1.message.reply: Feishu send failed",
    });
  }, 60_000);

  it("records Slack state through generic chat coordinates", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockSlack.sendEvent("evt-state", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "111.222",
      ts: "111.223",
      text: "<@UBOT> start",
    });
    await waitForSessionIdle(runtime.tempRoot, "C123:111.222");

    const response = await postChatJson(runtime.baseUrl, "/chat/post-state", {
      platform: "slack",
      conversation_id: "C123",
      root_message_id: "111.222",
      kind: "wait",
      reason: "watching CI",
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toEqual({ ok: true });
    await expect(readSessionRecord(runtime.tempRoot, "C123:111.222")).resolves.toMatchObject({
      lastTurnSignalKind: "wait",
    });
  }, 60_000);

  it("uploads Feishu inline files through generic chat coordinates", async () => {
    const runtime = await startRuntime(cleanups);
    const contentBase64 = Buffer.from("pdf").toString("base64");
    const response = await postChatJson(runtime.baseUrl, "/chat/post-file", {
      platform: "feishu",
      conversation_id: "oc_group",
      root_message_id: "om_root",
      content_base64: contentBase64,
      filename: "report.pdf",
      content_type: "application/pdf",
      title: "report",
    });

    expect(response.status).toBe(200);
    await expect(response.json()).resolves.toMatchObject({
      ok: true,
      file: {
        platform: "feishu",
        fileId: "file_uploaded_1",
        title: "report",
      },
    });
    expect(runtime.mockFeishu.uploadedFiles).toEqual([
      {
        kind: "file",
        key: "file_uploaded_1",
      },
    ]);
    expect(runtime.mockFeishu.postedMessages.some((message) => message.msgType === "file")).toBe(true);
  }, 60_000);
});

async function startRuntime(cleanups: Array<() => Promise<void>>): Promise<FeishuE2eRuntime> {
  const runtime = await startFeishuE2eRuntime();
  cleanups.push(() => runtime.stop());
  return runtime;
}

async function startFeishuSession(runtime: FeishuE2eRuntime, messageId: string): Promise<void> {
  await runtime.mockFeishu.sendReceiveMessage(
    createFeishuGroupTextEvent({
      messageId,
      text: "@_user_1 start",
      mentionBot: true,
    }),
    `evt-${messageId}`,
  );
  await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "Feishu session turn");
  await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", messageId));
}
