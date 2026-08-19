import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { appendCoAuthorTrailers } from "../src/services/git/github-author-utils.js";
import { runCommitMsgHook } from "../src/tools/git-coauthor.js";
import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { getFreePort, readSessionRecord, removeTempRoot, startBrokerProcess, waitForSessionIdle, writeGitHubPrBinding } from "./e2e-broker-helpers.js";

describe("git coauthor helper", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("appends co-author trailers idempotently and skips the primary author email", () => {
    const message = appendCoAuthorTrailers("feat(test): demo\n", {
      primaryAuthorEmail: "alice@example.com",
      coAuthors: ["Alice Example <alice@example.com>", "Bob Example <bob@example.com>", "Bob Example <bob@example.com>"],
    });

    expect(message).not.toContain("Alice Example <alice@example.com>");
    expect(message).toContain("Co-authored-by: Bob Example <bob@example.com>");
    expect(message.match(/Co-authored-by:/g)).toHaveLength(1);
  });

  it("rewrites the commit message file with the broker-resolved trailers", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "git-coauthor-helper-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });
    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U123",
      githubLogin: "alice",
      githubUserId: 101,
      token: "alice-token",
      githubEmail: "alice@example.com",
      githubName: "Alice Example",
    });

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

    await mockSlack.sendEvent("evt-coauthor", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "816.220",
      ts: "816.221",
      text: "<@UBOT> commit this",
    });
    await waitForSessionIdle(tempRoot, "C123:816.220");
    const session = await readSessionRecord(tempRoot, "C123:816.220");
    const messagePath = path.join(session.workspacePath, "COMMIT_EDITMSG");
    await fs.writeFile(messagePath, "feat(test): demo\n");

    const previousAuthor = process.env.GIT_AUTHOR_EMAIL;
    process.env.GIT_AUTHOR_EMAIL = "other@example.com";
    try {
      await runCommitMsgHook({
        brokerApiBase: broker.baseUrl,
        cwd: session.workspacePath,
        commitMessagePath: messagePath,
      });
    } finally {
      if (previousAuthor === undefined) {
        delete process.env.GIT_AUTHOR_EMAIL;
      } else {
        process.env.GIT_AUTHOR_EMAIL = previousAuthor;
      }
    }
    await expect(fs.readFile(messagePath, "utf8")).resolves.toContain("Co-authored-by: Alice Example <alice@example.com>");
  }, 90_000);
});
