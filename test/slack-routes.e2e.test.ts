import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { MockCodexAppServer } from "./helpers/mock-codex-app-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { collectTextInput, createDeferred, fetchJson, findStartedTurnTextContaining, getFreePort, readInboundMessages, readSessionRecord, removeTempRoot, startBrokerProcess, waitFor, waitForSessionActive, writeGitHubPrBinding } from "./e2e-broker-helpers.js";

describe.sequential("slack routes e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      const cleanup = cleanups.pop();
      await cleanup?.();
    }
  });

  it("covers Slack HTTP contracts, co-author resolution, file upload, session reset, and delete", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-routes-e2e-"));
    cleanups.push(async () => {
      await removeTempRoot(tempRoot);
    });

    const releaseFirstTurn = createDeferred<void>();
    const mockSlack = new MockSlackServer("UBOT", {
      botId: "BBOT",
      appId: "AAPP",
    });
    const mockCodex = new MockCodexAppServer({
      onTurnStart: async (context) => {
        if (mockCodex.turnsStarted.length === 1) {
          await releaseFirstTurn.promise;
        }
        context.complete("");
      },
    });
    const slackPort = await mockSlack.start();
    const codexUrl = await mockCodex.start();
    cleanups.push(async () => {
      releaseFirstTurn.resolve();
      await mockCodex.stop();
      await mockSlack.stop();
    });

    const brokerPort = await getFreePort();
    const broker = await startBrokerProcess({
      port: brokerPort,
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: {
        ADMIN_BASE_URL: "https://admin.example.test",
      },
    });
    cleanups.push(() => broker.stop());

    const missingHistory = await fetchJson(`${broker.baseUrl}/slack/thread-history`);
    expect(missingHistory.status).toBe(400);
    expect(missingHistory.body).toMatchObject({
      ok: false,
      error: "missing_required_query",
    });

    const missingPost = await fetchJson(`${broker.baseUrl}/slack/post-message`, {
      channel_id: "C123",
    });
    expect(missingPost.status).toBe(400);
    expect(missingPost.body).toMatchObject({
      ok: false,
      error: "missing_required_body",
    });

    const invalidKind = await fetchJson(`${broker.baseUrl}/slack/post-message`, {
      channel_id: "C123",
      thread_ts: "101.220",
      text: "hello",
      kind: "nope",
    });
    expect(invalidKind.status).toBe(400);
    expect(invalidKind.body).toMatchObject({
      ok: false,
      error: "invalid_kind",
    });

    const unknownDelete = await fetchJson(`${broker.baseUrl}/slack/sessions/${encodeURIComponent("C123:missing")}`, undefined, {
      method: "DELETE",
    });
    expect(unknownDelete.status).toBe(404);
    expect(unknownDelete.body).toMatchObject({
      ok: false,
      error: "Unknown session runtime key: C123:missing",
    });

    await mockSlack.sendEvent("evt-routes-root", {
      type: "message",
      user: "U123",
      channel: "C123",
      ts: "101.220",
      text: "ROUTE_ROOT_CONTEXT",
    });
    await mockSlack.sendEvent("evt-routes-mention", {
      type: "app_mention",
      user: "U123",
      channel: "C123",
      thread_ts: "101.220",
      ts: "101.221",
      text: "<@UBOT> start routes session",
    });

    const sessionKey = "C123:101.220";
    await waitFor(() => mockCodex.turnsStarted.length >= 1, "routes session turn start");
    await waitForSessionActive(tempRoot, sessionKey);
    const session = await readSessionRecord(tempRoot, sessionKey);
    const cwd = session.workspacePath;

    const history = await fetchJson(`${broker.baseUrl}/slack/thread-history?channel_id=C123&thread_ts=101.220&before_ts=101.221&limit=8`);
    expect(history.status).toBe(200);
    expect(history.body).toMatchObject({
      ok: true,
      channelId: "C123",
      rootThreadTs: "101.220",
    });
    expect(JSON.stringify(history.body)).toContain("ROUTE_ROOT_CONTEXT");

    const historyText = await fetch(`${broker.baseUrl}/slack/thread-history?channel_id=C123&thread_ts=101.220&before_ts=101.221&limit=8&format=text`);
    expect(historyText.ok).toBe(true);
    expect(await historyText.text()).toContain("ROUTE_ROOT_CONTEXT");

    const uploaded = await fetchJson(`${broker.baseUrl}/slack/post-file`, {
      channel_id: "C123",
      thread_ts: "101.220",
      filename: "report.txt",
      content_base64: Buffer.from("hello world").toString("base64"),
      initial_comment: "## Summary\n- **done**\n- [docs](https://example.com)",
    });
    expect(uploaded.status).toBe(200);
    expect(uploaded.body).toMatchObject({
      ok: true,
      file: {
        fileId: expect.stringMatching(/^FUPLOAD/),
      },
    });
    expect(mockSlack.uploadedFiles).toEqual([
      expect.objectContaining({
        channelId: "C123",
        threadTs: "101.220",
        filename: "report.txt",
        initialComment: "*Summary*\n• *done*\n• <https://example.com|docs>",
      }),
    ]);

    const blankCoauthors = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/configure-session`, {
      cwd,
      coauthors: ["   "],
      user_ids: [""],
    });
    expect(blankCoauthors.status).toBe(200);
    expect(blankCoauthors.body).toMatchObject({
      ok: true,
      status: {
        sessionKey,
        needsUserInput: true,
      },
    });

    const legacyMappings = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/configure-session`, {
      cwd,
      mappings: [
        {
          slack_user: "Alice Example",
          github_author: "Alice Example <alice@example.com>",
        },
      ],
    });
    expect(legacyMappings.status).toBe(400);
    expect(legacyMappings.body).toMatchObject({
      ok: false,
      error: "Manual co-author mappings are no longer supported. Bind GitHub OAuth for Slack users instead.",
    });

    const firstResolve = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/resolve-commit-message`, {
      cwd,
      commit_message: "feat(test): demo",
    });
    expect(firstResolve.status).toBe(200);
    expect(firstResolve.body).toMatchObject({
      ok: true,
      status: "noop",
    });
    expect(String(firstResolve.body.message ?? "")).toContain("missing GitHub OAuth binding");
    expect(mockSlack.ephemeralPosts).toHaveLength(1);

    const secondResolve = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/resolve-commit-message`, {
      cwd,
      commit_message: "feat(test): demo",
    });
    expect(secondResolve.status).toBe(200);
    expect(secondResolve.body).toMatchObject({
      status: "noop",
    });
    expect(mockSlack.ephemeralPosts).toHaveLength(1);

    const ignoreMissing = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/configure-session`, {
      cwd,
      user_ids: ["U123"],
      ignore_missing: true,
    });
    expect(ignoreMissing.status).toBe(200);
    expect(ignoreMissing.body).toMatchObject({
      ok: true,
      status: {
        ignoreMissing: true,
        missingSelectedUserIds: ["U123"],
      },
    });
    const ignoredResolve = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/resolve-commit-message`, {
      cwd,
      commit_message: "feat(slack): ignore unresolved",
    });
    expect(ignoredResolve.status).toBe(200);
    expect(ignoredResolve.body).toMatchObject({
      status: "noop",
    });
    expect(String(ignoredResolve.body.message ?? "")).toContain("skipped for this commit");

    await mockSlack.sendEvent("evt-routes-second-user", {
      type: "message",
      user: "U234",
      channel: "C123",
      thread_ts: "101.220",
      ts: "101.222",
      text: "second contributor",
    });
    await waitFor(() => mockCodex.steers.some((steer) => collectTextInput(steer.input).includes("second contributor")), "second user joined active turn");

    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U123",
      githubLogin: "alice",
      githubUserId: 101,
      githubEmail: "alice@github.example",
      githubName: "Alice GitHub",
      token: "alice-token",
      scopes: ["repo", "read:user", "user:email"],
    });
    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U234",
      githubLogin: "bob",
      githubUserId: 102,
      githubEmail: "bob@github.example",
      githubName: "Bob GitHub",
      token: "bob-token",
      scopes: ["repo", "read:user", "user:email"],
    });

    const configured = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/configure-session`, {
      cwd,
      user_ids: ["U123", "U234"],
      ignore_missing: false,
    });
    expect(configured.status).toBe(200);
    expect(configured.body).toMatchObject({
      ok: true,
      status: {
        canCommitDirectly: true,
        missingSelectedUserIds: [],
      },
    });

    const resolvedTrailers = await fetchJson(`${broker.baseUrl}/slack/git-coauthors/resolve-commit-message`, {
      cwd,
      commit_message: "feat(slack): add coauthors",
      primary_author_email: "broker@example.com",
    });
    expect(resolvedTrailers.status).toBe(200);
    expect(resolvedTrailers.body).toMatchObject({
      status: "resolved",
    });
    expect(String(resolvedTrailers.body.commitMessage ?? "")).toContain("Co-authored-by: Alice GitHub <alice@github.example>");
    expect(String(resolvedTrailers.body.commitMessage ?? "")).toContain("Co-authored-by: Bob GitHub <bob@github.example>");

    const tokenOk = await fetchJson(`${broker.baseUrl}/slack/github-token/resolve`, {
      cwd,
      command: ["pr", "create", "--fill"],
    });
    expect(tokenOk.status).toBe(200);
    expect(tokenOk.body).toMatchObject({
      ok: true,
      mode: "initiator",
      slackUserId: "U123",
      githubLogin: "alice",
      token: "alice-token",
    });

    await writeGitHubPrBinding(tempRoot, {
      slackUserId: "U123",
      githubLogin: "alice",
      githubUserId: 101,
      githubEmail: "alice@github.example",
      githubName: "Alice GitHub",
      token: "alice-token",
      scopes: ["repo", "read:user", "user:email"],
      revokedAt: "2026-08-19T00:00:00.000Z",
    });
    const tokenBlocked = await fetchJson(`${broker.baseUrl}/slack/github-token/resolve`, {
      cwd,
      command: ["pr", "create"],
    });
    expect(tokenBlocked.status).toBe(409);
    expect(tokenBlocked.body).toMatchObject({
      ok: false,
      mode: "blocked",
      reason: "initiator_token_invalid",
    });

    const reset = await fetchJson(`${broker.baseUrl}/slack/sessions/${encodeURIComponent(sessionKey)}/reset`, {});
    expect(reset.status).toBe(200);
    expect(reset.body).toMatchObject({
      ok: true,
      sessionKey,
      reset: {
        interruptedActiveTurn: true,
        previousAgentSessionId: session.agentSessionId,
        authBlocked: false,
      },
    });
    expect(mockCodex.interrupts.length).toBeGreaterThan(0);
    releaseFirstTurn.resolve();

    await waitFor(() => Boolean(findStartedTurnTextContaining(mockCodex, "previous agent thread/history was intentionally discarded")), "reset wakeup turn");
    const resetTurnText = findStartedTurnTextContaining(mockCodex, "previous agent thread/history was intentionally discarded") ?? "";
    expect(resetTurnText).toContain("ROUTE_ROOT_CONTEXT");

    const afterReset = await readSessionRecord(tempRoot, sessionKey);
    expect(afterReset.agentSessionId).toBeTruthy();
    expect(afterReset.agentSessionId).not.toBe(session.agentSessionId);
    const resetInbound = await readInboundMessages(tempRoot, sessionKey);
    expect(resetInbound.some((message) => message.source === "admin_session_reset" && String(message.text).includes("丢弃旧 agent history"))).toBe(true);

    const deleted = await fetchJson(`${broker.baseUrl}/slack/sessions/${encodeURIComponent(sessionKey)}`, undefined, {
      method: "DELETE",
    });
    expect(deleted.status).toBe(200);
    expect(deleted.body).toMatchObject({
      ok: true,
      sessionKey,
      delete: {
        deleted: true,
      },
    });
    await expect(readSessionRecord(tempRoot, sessionKey)).rejects.toThrow(/Unknown session/);
  }, 120_000);
});
