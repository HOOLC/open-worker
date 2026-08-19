import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { runGhWrapper } from "../src/tools/gh.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { fetchJson, getFreePort, readSessionRecord, removeTempRoot, startBrokerProcess, waitForSessionIdle, writeGitHubPrBinding } from "./e2e-broker-helpers.js";

describe.sequential("broker gh wrapper", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("asks the broker for the current session token and execs the real gh with GH_TOKEN", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "broker-gh-wrapper-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U123",
      githubLogin: "alice",
      githubUserId: 101,
      token: "starter-token",
      githubEmail: "alice@example.com",
    });
    const harness = await startHarness(tempRoot, cleanups);
    const session = await mentionAndWait(harness.mockSlack, tempRoot, "814.220");

    const capturePath = path.join(tempRoot, "capture.json");
    const realGhPath = path.join(tempRoot, "real-gh.mjs");
    await fs.writeFile(realGhPath, ["#!/usr/bin/env node", "import fs from 'node:fs/promises';", "await fs.writeFile(process.env.CAPTURE_PATH, JSON.stringify({", "  argv: process.argv.slice(2),", "  ghToken: process.env.GH_TOKEN,", "  githubToken: process.env.GITHUB_TOKEN,", "  cwd: process.cwd()", "}));"].join("\n"));
    await fs.chmod(realGhPath, 0o755);

    const result = await runGhWrapper({
      brokerApiBase: harness.baseUrl,
      realGhPath,
      cwd: session.workspacePath,
      argv: ["pr", "create", "--fill"],
      env: {
        ...process.env,
        CAPTURE_PATH: capturePath,
        GH_TOKEN: "inherited-gh-token",
        GITHUB_TOKEN: "inherited-github-token",
      },
    });

    expect(result.status, `${result.stdout}\n${result.stderr}`).toBe(0);
    const captured = JSON.parse(await fs.readFile(capturePath, "utf8")) as Record<string, unknown>;
    expect(captured).toMatchObject({
      argv: ["pr", "create", "--fill"],
      ghToken: "starter-token",
      cwd: await fs.realpath(session.workspacePath),
    });
    expect(captured).not.toHaveProperty("githubToken");
  }, 90_000);

  it("does not call the real gh when broker token resolution blocks", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "broker-gh-wrapper-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U123",
      githubLogin: "alice",
      githubUserId: 101,
      token: "alice-token",
      revokedAt: "2026-05-13T00:00:00.000Z",
    });
    const harness = await startHarness(tempRoot, cleanups);
    const session = await mentionAndWait(harness.mockSlack, tempRoot, "815.220");
    const realGhPath = path.join(tempRoot, "real-gh.mjs");
    await fs.writeFile(realGhPath, ["#!/usr/bin/env node", "throw new Error('real gh should not run');"].join("\n"));
    await fs.chmod(realGhPath, 0o755);

    const result = await runGhWrapper({
      brokerApiBase: harness.baseUrl,
      realGhPath,
      cwd: session.workspacePath,
      argv: ["pr", "create"],
      env: process.env,
    });

    expect(result.status, `${result.stdout}\n${result.stderr}`).toBe(1);
    expect(result.stderr).toContain("GitHub token for alice is invalid.");

    const resolved = await fetchJson(`${harness.baseUrl}/slack/github-token/resolve`, {
      cwd: session.workspacePath,
      command: ["pr", "create"],
    });
    expect(resolved.status).toBe(409);
    expect(resolved.body).toMatchObject({
      ok: false,
      mode: "blocked",
      reason: "initiator_token_invalid",
      githubLogin: "alice",
    });
  }, 90_000);
});

async function startHarness(
  tempRoot: string,
  cleanups: Array<() => Promise<void>>,
): Promise<{
  readonly baseUrl: string;
  readonly mockSlack: MockSlackServer;
}> {
  const mockSlack = new MockSlackServer("UBOT", {
    botId: "BBOT",
    appId: "AAPP",
  });
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
  return {
    baseUrl: broker.baseUrl,
    mockSlack,
  };
}

async function mentionAndWait(mockSlack: MockSlackServer, tempRoot: string, threadTs: string) {
  await mockSlack.sendEvent(`evt-gh-${threadTs}`, {
    type: "app_mention",
    user: "U123",
    channel: "C123",
    thread_ts: threadTs,
    ts: `${threadTs}1`,
    text: "<@UBOT> open a PR",
  });
  await waitForSessionIdle(tempRoot, `C123:${threadTs}`);
  return await readSessionRecord(tempRoot, `C123:${threadTs}`);
}
