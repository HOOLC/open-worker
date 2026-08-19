import { describe, expect, it } from "vitest";

import { formatSlackHistoryContextForAgent, formatSlackMessageForAgent } from "../src/services/slack/slack-message-format.js";

describe("formatSlackMessageForAgent", () => {
  it("includes sender identity and thread metadata", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "thread_reply",
        channelId: "C123",
        channelType: "channel",
        rootThreadTs: "111.222",
        messageTs: "111.223",
        userId: "U123",
        text: "Please fix the flaky test.",
        senderKind: "user",
        mentionedUserIds: ["U456"],
        mentionedUsers: [
          {
            userId: "U456",
            mention: "<@U456>",
            displayName: "claude",
            username: "claude",
          },
        ],
        images: [
          {
            fileId: "F123",
            title: "Screenshot",
            mimetype: "image/png",
            width: 1280,
            height: 720,
            url: "https://example.com/file.png",
          },
        ],
        slackMessage: {
          text: "Please fix the flaky test.",
          blocks: [
            {
              type: "section",
              text: {
                type: "mrkdwn",
                text: "Please fix the flaky test.",
              },
            },
          ],
        },
      },
      {
        userId: "U123",
        mention: "<@U123>",
        username: "alice",
        displayName: "Alice",
        realName: "Alice Zhang",
      },
    );

    expect(result).toContain("A new message arrived in the active Slack thread.");
    expect(result).toContain("structured_message_json:");
    expect(result).toContain('"source": "thread_reply"');
    expect(result).toContain('"message_ts": "111.223"');
    expect(result).toContain('"user_id": "U123"');
    expect(result).toContain('"mention": "<@U123>"');
    expect(result).toContain("Carefully judge whether it requires a reply or action from you.");
    expect(result).toContain('"display_name": "Alice"');
    expect(result).toContain('"real_name": "Alice Zhang"');
    expect(result).toContain('"username": "alice"');
    expect(result).toContain('"mentioned_user_ids": [');
    expect(result).toContain('"U456"');
    expect(result).toContain('"mentioned_user_mentions": [');
    expect(result).toContain('"<@U456>"');
    expect(result).toContain('"mentioned_users": [');
    expect(result).toContain('"attachments": [');
    expect(result).toContain('"title": "Screenshot"');
    expect(result).toContain('"dimensions": "1280x720"');
    expect(result).toContain('"text": "Please fix the flaky test."');
    expect(result).not.toContain('"slack_message":');
    expect(result).not.toContain('"blocks": [');
  });

  it("falls back to ids when profile lookup is unavailable", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "direct_message",
        channelId: "D123",
        rootThreadTs: "222.333",
        userId: "U999",
        senderKind: "user",
        text: "status?",
      },
      null,
    );

    expect(result).toContain('"source": "direct_message"');
    expect(result).toContain('"user_id": "U999"');
    expect(result).toContain('"mention": "<@U999>"');
    expect(result).not.toContain("sender_display_name:");
    expect(result).toContain('"text": "status?"');
  });

  it("prepends earlier thread context when provided", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "app_mention",
        channelId: "C123",
        rootThreadTs: "111.222",
        messageTs: "111.224",
        userId: "U123",
        senderKind: "user",
        text: "What happened before this?",
        contextText: "Earlier Slack thread context before the current message.",
      },
      {
        userId: "U123",
        mention: "<@U123>",
      },
    );

    expect(result).toContain("Earlier Slack thread context before the current message.");
    expect(result).toContain("Current Slack message requiring a response:");
    expect(result).toContain('"source": "app_mention"');
  });

  it("includes resolved mentioned users and readable mention text", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "thread_reply",
        channelId: "C123",
        rootThreadTs: "111.222",
        messageTs: "111.227",
        userId: "U123",
        senderKind: "user",
        text: "<@U456> preview 呢？",
        mentionedUserIds: ["U456"],
        mentionedUsers: [
          {
            userId: "U456",
            mention: "<@U456>",
            displayName: "claude",
            username: "claude",
          },
        ],
      },
      {
        userId: "U123",
        mention: "<@U123>",
        displayName: "Alice",
      },
    );

    expect(result).toContain('"mentioned_users": [');
    expect(result).toContain('"display_name": "claude"');
    expect(result).toContain('"text_with_resolved_mentions": "@claude preview 呢？"');
  });

  it("renders image-only messages without dropping the body block", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "thread_reply",
        channelId: "C123",
        rootThreadTs: "111.222",
        messageTs: "111.225",
        userId: "U123",
        senderKind: "user",
        text: "",
        images: [
          {
            fileId: "F999",
            name: "paste.png",
            mimetype: "image/png",
            url: "https://example.com/paste.png",
          },
        ],
      },
      null,
    );

    expect(result).toContain('"attachments": [');
    expect(result).toContain('"text": "[no text body]"');
  });

  it("renders recovered missed messages as one chronological batch", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "recovered_thread_batch",
        channelId: "C123",
        rootThreadTs: "111.222",
        messageTs: "111.226",
        userId: "U123",
        text: "",
        recoveryKind: "missed_thread_messages",
        batchMessages: [
          {
            source: "thread_reply",
            messageTs: "111.224",
            userId: "U123",
            senderKind: "user",
            text: "first missed message",
            sender: {
              userId: "U123",
              mention: "<@U123>",
              displayName: "Alice",
            },
          },
          {
            source: "thread_reply",
            messageTs: "111.225",
            userId: "U456",
            senderKind: "user",
            text: "second missed message",
            mentionedUserIds: ["U789"],
            mentionedUsers: [
              {
                userId: "U789",
                mention: "<@U789>",
                displayName: "claude",
              },
            ],
            sender: {
              userId: "U456",
              mention: "<@U456>",
              displayName: "Bob",
            },
          },
        ],
      },
      null,
    );

    expect(result).toContain("The broker server restarted or reconnected.");
    expect(result).toContain("recovered_message_batch_json:");
    expect(result).toContain('"source": "recovered_thread_batch"');
    expect(result).toContain('"recovery_kind": "missed_thread_messages"');
    expect(result).toContain('"batch_message_count": 2');
    expect(result).toContain('"text": "first missed message"');
    expect(result).toContain('"text": "second missed message"');
    expect(result).toContain('"mentioned_user_mentions": [');
    expect(result).toContain('"<@U789>"');
    expect(result).toContain('"mentioned_users": [');
  });

  it("includes only selected Slack payload fields for bot cards", () => {
    const result = formatSlackMessageForAgent(
      {
        source: "thread_reply",
        channelId: "C123",
        rootThreadTs: "111.222",
        messageTs: "111.226",
        userId: "bot:B123",
        text: "issue created",
        senderKind: "bot",
        botId: "B123",
        appId: "A123",
        senderUsername: "Linear",
        slackMessage: {
          subtype: "bot_message",
          bot_id: "B123",
          app_id: "A123",
          username: "Linear",
          text: "issue created",
          channel: "C123",
          team: "T123",
          attachments: [
            {
              title: "CUE-1180",
              title_link: "https://linear.app/cue/issue/CUE-1180",
            },
          ],
          metadata: {
            noisy: "not needed by app-server",
          },
        },
      },
      null,
    );

    expect(result).toContain('"slack_message": {');
    expect(result).toContain('"attachments": [');
    expect(result).not.toContain('"metadata"');
    expect(result).not.toContain('"team"');
  });
});

describe("formatSlackHistoryContextForAgent", () => {
  it("renders a readable thread history block", () => {
    const result = formatSlackHistoryContextForAgent([
      {
        channelId: "C123",
        channelType: "channel",
        rootThreadTs: "111.222",
        messageTs: "111.220",
        userId: "U234",
        text: "Earlier note",
        senderKind: "user",
        mentionedUserIds: ["U345"],
        images: [
          {
            fileId: "F234",
            title: "Earlier screenshot",
            mimetype: "image/jpeg",
            url: "https://example.com/earlier.jpg",
          },
        ],
        slackMessage: {
          text: "Earlier note",
        },
        sender: {
          userId: "U234",
          mention: "<@U234>",
          displayName: "Bob",
        },
      },
    ]);

    expect(result).toContain("history_count: 1");
    expect(result).toContain("[history 1]");
    expect(result).toContain('"source": "thread_history"');
    expect(result).toContain('"mentioned_user_ids": [');
    expect(result).toContain('"U345"');
    expect(result).toContain('"display_name": "Bob"');
    expect(result).toContain('"attachments": [');
    expect(result).toContain('"text": "Earlier note"');
    expect(result).not.toContain('"slack_message":');
  });
});
