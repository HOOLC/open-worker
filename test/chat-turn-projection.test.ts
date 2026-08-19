import { describe, expect, it } from "vitest";

import { createChatTurnProjection, createChatTurnProjectionFromOutboundMessage, createChatTurnProjectionFromUploadedFile } from "../src/services/chat/chat-turn-projection.js";
import { createFeishuTurnStateCard } from "../src/services/feishu/feishu-status-card.js";

describe("chat turn projection", () => {
  it("maps logical chat turn states without platform rendering details", () => {
    const target = {
      platform: "feishu" as const,
      conversationId: "oc_group",
      rootMessageId: "om_root",
    };

    expect(
      createChatTurnProjection(target, {
        kind: "wait",
        reason: "CI is still running",
      }),
    ).toEqual({
      target,
      status: "waiting",
      title: "Codex is waiting",
      summary: "Codex is waiting for another system or user input.",
      reason: "CI is still running",
    });

    expect(
      createChatTurnProjection(target, {
        kind: "block",
      }),
    ).toMatchObject({
      status: "blocked",
      title: "Codex is blocked",
    });

    expect(
      createChatTurnProjection(target, {
        kind: "final",
      }),
    ).toMatchObject({
      status: "final",
      title: "Codex finished",
    });
  });

  it("lets Slack and Feishu render the same logical projection with native formats", () => {
    const target = {
      platform: "feishu" as const,
      conversationId: "oc_group",
      rootMessageId: "om_root",
    };
    const projection = createChatTurnProjection(target, {
      kind: "block",
      reason: "approval needed",
    });

    const feishuCard = createFeishuTurnStateCard(projection);

    expect(feishuCard).toEqual(
      expect.objectContaining({
        header: expect.objectContaining({
          template: "red",
          title: expect.objectContaining({
            content: "Codex is blocked",
          }),
        }),
      }),
    );
    expect(typeof feishuCard).toBe("object");
  });

  it("projects progress, tool, and final messages into stable logical slots", () => {
    const target = {
      platform: "feishu" as const,
      conversationId: "oc_group",
      rootMessageId: "om_root",
    };

    expect(
      createChatTurnProjectionFromOutboundMessage(target, {
        text: "Thinking through the failing tests",
        kind: "progress",
      }),
    ).toMatchObject({
      status: "thinking",
      summary: "Codex shared a progress update.",
      slots: [
        {
          kind: "commentary",
          title: "Progress update",
          body: "Thinking through the failing tests",
        },
      ],
    });

    expect(
      createChatTurnProjectionFromOutboundMessage(target, {
        text: "tool: exec_command\npnpm test",
        kind: "progress",
      }),
    ).toMatchObject({
      status: "running_tool",
      summary: "Codex is using a tool to make progress.",
      slots: [
        {
          kind: "tool",
          title: "Tool: exec_command",
          metadata: {
            toolName: "exec_command",
          },
        },
      ],
    });

    expect(
      createChatTurnProjectionFromOutboundMessage(target, {
        text: "All done",
        kind: "final",
      }),
    ).toMatchObject({
      status: "final",
      slots: [
        {
          kind: "final",
          title: "Final answer",
          body: "All done",
        },
      ],
    });
  });

  it("projects uploaded files into platform-neutral artifact slots", () => {
    const target = {
      platform: "feishu" as const,
      conversationId: "oc_group",
      rootMessageId: "om_root",
    };

    expect(
      createChatTurnProjectionFromUploadedFile(
        target,
        {
          filename: "report.pdf",
          altText: "weekly report",
          contentType: "application/pdf",
        },
        {
          platform: "feishu",
          fileId: "file_uploaded",
          kind: "file",
          name: "report.pdf",
          mimetype: "application/pdf",
          size: 2048,
        },
      ),
    ).toMatchObject({
      status: "thinking",
      title: "Codex shared an artifact",
      slots: [
        {
          kind: "artifact",
          title: "Artifact: report.pdf",
          body: "Kind: file\nName: report.pdf\nType: application/pdf\nSize: 2.0 KiB\nAlt text: weekly report",
          metadata: {
            fileId: "file_uploaded",
            artifactKind: "file",
            name: "report.pdf",
            mimetype: "application/pdf",
            size: 2048,
          },
        },
      ],
    });
  });
});
