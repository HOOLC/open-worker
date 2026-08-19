import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { AppServerClient } from "../src/services/codex/app-server-client.js";
import { getFreePort, removeTempRoot, startBrokerProcess, waitForSessionIdle } from "./e2e-broker-helpers.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";

describe.sequential("cli instead of dynamic tools e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("thread/start does not declare dynamicTools", async () => {
    const workspace = await fs.mkdtemp(path.join(os.tmpdir(), "cli-client-"));
    cleanups.push(async () => removeTempRoot(workspace));
    const mock = new MockCodexAppServer();
    const url = await mock.start();
    cleanups.push(async () => mock.stop());
    const client = new AppServerClient({
      url,
      serviceName: "cli-e2e",
      brokerHttpBaseUrl: "http://127.0.0.1:9",
      reposRoot: path.join(workspace, "repos"),
    });
    await client.connect();
    cleanups.push(async () => client.close());
    await client.ensureThread({
      channelId: "C123",
      rootThreadTs: "930.220",
      workspacePath: workspace,
      sessionKey: "C123:930.220",
      platform: "slack",
    });
    expect(mock.lastThreadStartParams?.dynamicTools).toBeUndefined();
    expect(mock.dynamicTools).toBeUndefined();
  }, 30_000);

  it("injects zork-call CLI into base instructions and never curl routes", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "cli-broker-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const mockSlack = new MockSlackServer("UBOT", { botId: "BBOT", appId: "AAPP" });
    const mockCodex = new MockCodexAppServer();
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      await mockCodex.stop();
      await mockSlack.stop();
    });
    const broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
    });
    cleanups.push(() => broker.stop());

    await mockSlack.sendEvent("evt-cli-prompt", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "940.220",
      ts: "940.221",
      text: "<@UBOT> start",
    });
    await waitForSessionIdle(tempRoot, "C123:940.220");

    expect(mockCodex.threadStarts).toHaveLength(1);
    expect(mockCodex.threadStarts[0]?.params.dynamicTools).toBeUndefined();
    const baseInstructions = String(mockCodex.threadStarts[0]?.baseInstructions ?? "");
    expect(baseInstructions).toContain("zork-call chat post-message");
    expect(baseInstructions).toContain("zork-call notify");
    expect(baseInstructions).toContain("zork-call job register");
    expect(baseInstructions).not.toContain("chat.post_message");
    expect(baseInstructions).not.toContain("/chat/post-message");
    expect(baseInstructions).not.toContain("BROKER_JOB_HELPER");
  }, 90_000);
});
