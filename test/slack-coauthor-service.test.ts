import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { GitHubPrIdentityService } from "../src/services/github-pr-identity-service.js";
import { SessionManager } from "../src/services/session-manager.js";
import { SlackCoauthorService } from "../src/services/slack/slack-coauthor-service.js";
import { StateStore } from "../src/store/state-store.js";

describe("SlackCoauthorService", () => {
  const tempDirs: string[] = [];

  afterEach(async () => {
    vi.restoreAllMocks();
    await Promise.all(
      tempDirs.splice(0).map((directory) =>
        fs.rm(directory, {
          recursive: true,
          force: true,
        }),
      ),
    );
  });

  it("opens a co-author modal without manual GitHub author fields", async () => {
    const { sessions, githubPrIdentity } = await createHarness();
    const session = await sessions.ensureSession("C555", "222.333");
    await githubPrIdentity.upsertBinding({
      slackUserId: "U1",
      githubLogin: "alice",
      githubUserId: 101,
      githubEmail: "alice@github.example",
      githubName: "Alice GitHub",
      token: "alice-token",
      scopes: ["repo", "read:user", "user:email"],
    });

    const openView = vi.fn(async () => {});
    const service = new SlackCoauthorService({
      sessions,
      githubPrIdentity,
      slackApi: {
        getUserIdentity: vi.fn(async () => ({
          userId: "U1",
          mention: "<@U1>",
          realName: "Alice Example",
          email: "alice@slack.example",
        })),
        postEphemeral: vi.fn(async () => "111.444"),
        openView,
      } as never,
    });

    const latestSession = await service.noteIncomingSlackInput(session, {
      source: "thread_reply",
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      messageTs: "222.334",
      userId: "U1",
      senderKind: "user",
      text: "first request",
    });

    await service.handleInteractivePayload({
      type: "block_actions",
      trigger_id: "trigger-1",
      actions: [
        {
          action_id: "coauthor_configure",
          value: JSON.stringify({
            session_key: latestSession.key,
            candidate_revision: latestSession.coAuthorCandidateRevision,
          }),
        },
      ],
    });

    expect(openView).toHaveBeenCalledTimes(1);
    const modalView = (openView.mock.calls[0] as unknown as [Record<string, unknown>])?.[0]?.view as Record<string, unknown>;
    expect(modalView).toMatchObject({
      callback_id: "coauthor_confirm",
    });
    expect((modalView.blocks as Array<Record<string, unknown>>).filter((block) => String(block.block_id || "").startsWith("author__"))).toEqual([]);
  });

  async function createHarness(): Promise<{
    readonly stateDir: string;
    readonly sessions: SessionManager;
    readonly githubPrIdentity: GitHubPrIdentityService;
  }> {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-coauthor-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-coauthor-sessions-"));
    tempDirs.push(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore: new StateStore(stateDir, sessionsRoot),
      sessionsRoot,
    });
    await sessions.load();
    const githubPrIdentity = new GitHubPrIdentityService({ stateDir });
    await githubPrIdentity.load();
    return {
      stateDir,
      sessions,
      githubPrIdentity,
    };
  }
});
