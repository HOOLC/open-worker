import { afterEach, describe, expect, it } from "vitest";

import { createChatSessionKey } from "../src/services/chat/chat-session-key.js";
import { collectTextInput, createDeferred, findStartedTurnTextContaining, readSessionRecord, waitFor, waitForSessionIdle } from "./e2e-broker-helpers.js";
import { feishuSessionKey, readBrokerJsonl, startFeishuE2eRuntime, type FeishuE2eRuntime } from "./feishu-e2e-helpers.js";
import { createFeishuGroupTextEvent, createFeishuImageEvent } from "./helpers/mock-feishu-server.js";
import type { MockTurnContext } from "./helpers/mock-codex-app-server.js";

describe.sequential("feishu broker e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("starts a Feishu group @ mention as a persisted Codex session through the running broker", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_root",
        text: "@_user_1 please check this",
        mentionBot: true,
        threadId: "omt_thread",
      }),
      "evt_group_at",
    );

    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "Feishu mention turn");
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_root"));

    const sessionKey = feishuSessionKey("oc_group", "om_root");
    await expect(readSessionRecord(runtime.tempRoot, sessionKey)).resolves.toMatchObject({
      platform: "feishu",
      conversationId: "oc_group",
      conversationKind: "group",
      rootMessageId: "om_root",
      platformThreadId: "omt_thread",
      initiatorUserId: "ou_user",
    });
    expect(findStartedTurnTextContaining(runtime.mockCodex, "please check this")).toBeDefined();
    const logs = await readBrokerJsonl(runtime.tempRoot);
    expect(logs).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          message: "chat.message.accepted",
          meta: expect.objectContaining({
            platform: "feishu",
            conversationId: "oc_group",
            messageId: "om_root",
            eventId: "evt_group_at",
            route: "bot_mention",
          }),
        }),
        expect.objectContaining({
          message: "chat.session.created",
          meta: expect.objectContaining({
            platform: "feishu",
            sessionKey,
            conversationId: "oc_group",
            rootMessageId: "om_root",
          }),
        }),
      ]),
    );
  }, 60_000);

  it("deduplicates repeated Feishu deliveries of the same group message", async () => {
    const runtime = await startRuntime(cleanups);
    const event = createFeishuGroupTextEvent({
      messageId: "om_dup",
      text: "@_user_1 only once",
      mentionBot: true,
    });
    await runtime.mockFeishu.sendReceiveMessage(event, "evt_dup_1");
    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "first Feishu turn");
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_dup"));

    await runtime.mockFeishu.sendReceiveMessage(event, "evt_dup_2");
    await waitFor(async () => {
      const logs = await readBrokerJsonl(runtime.tempRoot);
      return logs.some((record) => record.message === "chat.message.deduped");
    }, "Feishu duplicate log");

    expect(runtime.mockCodex.turnsStarted).toHaveLength(1);
  }, 60_000);

  it("ignores private chats and bot/app senders without creating a Feishu session", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_private",
        text: "hello privately",
        chatId: "oc_private",
        chatType: "p2p",
      }),
      "evt_private",
    );
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_self",
        text: "bot noise",
        mentionBot: true,
        senderType: "app",
      }),
      "evt_self",
    );

    await waitFor(async () => {
      const logs = await readBrokerJsonl(runtime.tempRoot);
      return logs.some((record) => record.message === "chat.message.ignored" && (record.meta as { ignoredReason?: string }).ignoredReason === "ignored_private_chat") && logs.some((record) => record.message === "chat.message.ignored" && (record.meta as { ignoredReason?: string }).ignoredReason === "ignored_self");
    }, "ignored private and self logs");

    expect(runtime.mockCodex.turnsStarted).toHaveLength(0);
    await expect(readSessionRecord(runtime.tempRoot, feishuSessionKey("oc_private", "om_private"))).rejects.toThrow(/Unknown session/);
    await expect(readSessionRecord(runtime.tempRoot, feishuSessionKey("oc_group", "om_self"))).rejects.toThrow(/Unknown session/);
  }, 60_000);

  it("steers a non-@ follow-up into the active Feishu session in all mode", async () => {
    const hold = createDeferred<void>();
    const runtime = await startRuntime(cleanups, {
      onTurnStart: async () => {
        await hold.promise;
      },
    });
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_root",
        text: "@_user_1 start work",
        mentionBot: true,
      }),
      "evt_followup_start",
    );
    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "mention turn start");

    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_followup",
        text: "more context",
      }),
      "evt_followup",
    );
    await waitFor(() => runtime.mockCodex.steers.length >= 1, "Feishu follow-up steer");
    hold.resolve();
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_root"));

    expect(collectTextInput(runtime.mockCodex.steers[0]?.input ?? [])).toContain("more context");
    expect(runtime.mockCodex.turnsStarted).toHaveLength(1);
  }, 60_000);

  it("does not steer non-@ follow-ups when Feishu group mode is at_only", async () => {
    const hold = createDeferred<void>();
    const runtime = await startRuntime(cleanups, {
      extraEnv: {
        FEISHU_GROUP_MESSAGE_MODE: "at_only",
      },
      onTurnStart: async () => {
        await hold.promise;
      },
    });
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_root",
        text: "@_user_1 start work",
        mentionBot: true,
      }),
      "evt_at_only_start",
    );
    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "at_only mention turn");

    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_followup",
        text: "should stay ignored",
      }),
      "evt_at_only_followup",
    );
    await waitFor(async () => {
      const logs = await readBrokerJsonl(runtime.tempRoot);
      return logs.some((record) => record.message === "chat.message.ignored" && (record.meta as { messageId?: string; ignoredReason?: string }).messageId === "om_followup" && (record.meta as { ignoredReason?: string }).ignoredReason === "ignored_no_active_session");
    }, "at_only ignored follow-up");
    hold.resolve();
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_root"));

    expect(runtime.mockCodex.steers).toHaveLength(0);
    expect(runtime.mockCodex.turnsStarted).toHaveLength(1);
  }, 60_000);

  it("interrupts the active Feishu turn when the group sends -stop", async () => {
    const hold = createDeferred<void>();
    const runtime = await startRuntime(cleanups, {
      onTurnStart: async () => {
        await hold.promise;
      },
    });
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_root",
        text: "@_user_1 long task",
        mentionBot: true,
      }),
      "evt_stop_start",
    );
    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "stop mention turn");

    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_stop",
        text: "-stop",
      }),
      "evt_stop",
    );
    await waitFor(() => runtime.mockCodex.interrupts.length >= 1, "Feishu stop interrupt");
    hold.resolve();
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_root"));

    expect(runtime.mockCodex.interrupts).toHaveLength(1);
  }, 60_000);

  it("downloads Feishu image attachments into Codex image input", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuImageEvent({
        messageId: "om_image",
        imageKey: "img_v2_key",
      }),
      "evt_image",
    );

    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 1, "Feishu image turn");
    const turn = runtime.mockCodex.turnsStarted[0];
    expect(turn?.input.some((item) => item.type === "image" && "url" in item && item.url === "data:image/png;base64,aGVsbG8=")).toBe(true);
    expect(findStartedTurnTextContaining(runtime.mockCodex, "transfer_status: downloaded_as_image_input")).toBeDefined();
    expect(runtime.mockFeishu.resourceRequests).toEqual([
      expect.objectContaining({
        messageId: "om_image",
        fileKey: "img_v2_key",
        type: "image",
      }),
    ]);
  }, 60_000);

  it("records Feishu card callback coordinates through the long-connection mock", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_root",
        text: "@_user_1 start",
        mentionBot: true,
      }),
      "evt_card_start",
    );
    await waitForSessionIdle(runtime.tempRoot, feishuSessionKey("oc_group", "om_root"));

    const sessionKey = createChatSessionKey({
      platform: "feishu",
      conversationId: "oc_group",
      rootMessageId: "om_root",
    });
    await runtime.mockFeishu.sendEvent(
      "card.action.trigger",
      {
        open_message_id: "om_card",
        action: {
          value: {
            kind: "coauthor_confirm_all",
            sessionKey,
            conversationId: "oc_group",
            rootMessageId: "om_root",
            candidateRevision: 1,
          },
        },
      },
      "evt_card_action",
    );

    await waitFor(async () => {
      const logs = await readBrokerJsonl(runtime.tempRoot);
      return logs.some((record) => record.message === "chat.card.callback.received" && (record.meta as { eventId?: string }).eventId === "evt_card_action");
    }, "Feishu card callback log");
  }, 60_000);

  it("starts Slack and Feishu sessions in one spawned broker process", async () => {
    const runtime = await startRuntime(cleanups);
    await runtime.mockSlack.sendEvent("evt-dual-slack", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "220.330",
      ts: "220.331",
      text: "<@UBOT> slack side",
    });
    await runtime.mockFeishu.sendReceiveMessage(
      createFeishuGroupTextEvent({
        messageId: "om_dual",
        text: "@_user_1 feishu side",
        mentionBot: true,
      }),
      "evt_dual_feishu",
    );

    await waitFor(() => runtime.mockCodex.turnsStarted.length >= 2, "dual-platform turns");
    expect(findStartedTurnTextContaining(runtime.mockCodex, "slack side")).toBeDefined();
    expect(findStartedTurnTextContaining(runtime.mockCodex, "feishu side")).toBeDefined();
  }, 60_000);
});

async function startRuntime(
  cleanups: Array<() => Promise<void>>,
  options?: {
    readonly extraEnv?: Record<string, string>;
    readonly onTurnStart?: ((context: MockTurnContext) => Promise<void> | void) | undefined;
  },
): Promise<FeishuE2eRuntime> {
  const runtime = await startFeishuE2eRuntime(options);
  cleanups.push(() => runtime.stop());
  return runtime;
}
