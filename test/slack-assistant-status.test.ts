import { afterEach, describe, expect, it, vi } from "vitest";

import { SlackAssistantStatusController } from "../src/services/slack/slack-assistant-status.js";

describe("SlackAssistantStatusController", () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("retries the same status after a transient Slack API failure", async () => {
    vi.useFakeTimers();
    const setAssistantThreadStatus = vi.fn().mockRejectedValueOnce(new Error("Slack API request failed (500 Internal Server Error) for assistant.threads.setStatus")).mockResolvedValueOnce(undefined);

    const controller = new SlackAssistantStatusController({
      slackApi: {
        setAssistantThreadStatus,
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
      } as never,
      channelId: "C123",
      threadTs: "111.222",
    });

    controller.setThinking();
    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenCalledTimes(1);
    });

    controller.setThinking();
    await vi.advanceTimersByTimeAsync(2_000);

    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenCalledTimes(2);
    });
  });

  it("drops stale tool state when a cleared turn is followed by a new tool lifecycle", async () => {
    vi.useFakeTimers();
    const setAssistantThreadStatus = vi.fn(async () => undefined);

    const controller = new SlackAssistantStatusController({
      slackApi: {
        setAssistantThreadStatus,
        addReaction: vi.fn(),
        removeReaction: vi.fn(),
      } as never,
      channelId: "C123",
      threadTs: "111.222",
    });

    controller.handleToolStart({
      id: "stale-tool",
      name: "apply_patch",
    });
    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenNthCalledWith(1, {
        channelId: "C123",
        threadTs: "111.222",
        status: "Updating files...",
      });
    });

    controller.clear();
    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenNthCalledWith(2, {
        channelId: "C123",
        threadTs: "111.222",
        status: "",
      });
    });

    controller.handleToolStart({
      id: "fresh-tool",
      name: "search_query",
    });
    await vi.advanceTimersByTimeAsync(2_000);
    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenNthCalledWith(3, {
        channelId: "C123",
        threadTs: "111.222",
        status: "Searching the web...",
      });
    });

    controller.handleToolEnd({
      id: "fresh-tool",
    });
    await vi.advanceTimersByTimeAsync(2_000);
    await vi.waitFor(() => {
      expect(setAssistantThreadStatus).toHaveBeenNthCalledWith(4, {
        channelId: "C123",
        threadTs: "111.222",
        status: "Thinking...",
      });
    });
  });
});
