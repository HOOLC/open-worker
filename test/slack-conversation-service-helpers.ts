import { afterEach, vi } from "vitest";

import type { AppConfig } from "../src/config.js";

import type { SlackSessionRecord } from "../src/types.js";

export const TEST_SESSION: SlackSessionRecord = {
  key: "C123:111.222",
  channelId: "C123",
  rootThreadTs: "111.222",
  workspacePath: "/tmp/workspace",
  agentSessionId: "thread-1",
  activeTurnId: "turn-1",
  createdAt: new Date().toISOString(),
  updatedAt: new Date().toISOString(),
};

export const TEST_CONFIG = {
  slackInitialThreadHistoryCount: 8,
  slackHistoryApiMaxLimit: 50,
  slackActiveTurnReconcileIntervalMs: 15_000,
  slackMissedThreadRecoveryIntervalMs: 15_000,
  adminBaseUrl: "https://admin.example",
} as AppConfig;

afterEach(() => {
  vi.restoreAllMocks();
});
