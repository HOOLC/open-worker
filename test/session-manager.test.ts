import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { StateStore } from "../src/store/state-store.js";
import { SessionManager } from "../src/services/session-manager.js";

describe("SessionManager", () => {
  it("persists auth profile binding and blocked state", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-sessions-"));
    const store = new StateStore(stateDir, sessionsRoot);
    const manager = new SessionManager({
      stateStore: store,
      sessionsRoot,
    });

    await manager.load();
    const session = await manager.ensureSession("C123", "111.222");
    await manager.setSessionAuthProfile(session.key, "profile-a", {
      boundAt: "2026-05-09T00:00:00.000Z",
    });
    await manager.markSessionAuthBlocked(session.key, {
      reason: "primary_quota_exhausted",
      blockedAt: "2026-05-09T01:00:00.000Z",
    });
    await manager.setSessionAuthBlockedNoticePostedAt(session.key, "2026-05-09T01:00:05.000Z");

    const reloadedStore = new StateStore(stateDir, sessionsRoot);
    const reloadedManager = new SessionManager({
      stateStore: reloadedStore,
      sessionsRoot,
    });
    await reloadedManager.load();

    expect(reloadedManager.getSession("C123", "111.222")).toMatchObject({
      authProfileName: "profile-a",
      authProfileBoundAt: "2026-05-09T00:00:00.000Z",
      authBlockedAt: "2026-05-09T01:00:00.000Z",
      authBlockReason: "primary_quota_exhausted",
      authBlockedNoticePostedAt: "2026-05-09T01:00:05.000Z",
    });

    const switched = await reloadedManager.switchSessionAuthProfileAndClearBlock(session.key, "profile-b", {
      boundAt: "2026-05-09T02:00:00.000Z",
    });

    expect(switched).toMatchObject({
      authProfileName: "profile-b",
      authProfileBoundAt: "2026-05-09T02:00:00.000Z",
      authBlockedAt: undefined,
      authBlockReason: undefined,
      authBlockedNoticePostedAt: undefined,
      agentSessionId: undefined,
      activeTurnId: undefined,
    });

    await reloadedManager.markSessionAuthBlocked(session.key, {
      reason: "primary_quota_exhausted",
      blockedAt: "2026-05-09T03:00:00.000Z",
    });
    await reloadedManager.setSessionAuthBlockedNoticePostedAt(session.key, "2026-05-09T03:00:05.000Z");

    const recovered = await (reloadedManager as any).clearSessionAuthBlock(session.key);

    expect(recovered).toMatchObject({
      authProfileName: "profile-b",
      authProfileBoundAt: "2026-05-09T02:00:00.000Z",
      authBlockedAt: undefined,
      authBlockReason: undefined,
      authBlockedNoticePostedAt: undefined,
    });
  });

  it("does not restore a stale active turn when turn state and turn signal write concurrently", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-sessions-"));
    const store = new StateStore(stateDir, sessionsRoot);
    const manager = new SessionManager({
      stateStore: store,
      sessionsRoot,
    });

    await manager.load();
    await manager.ensureSession("C123", "444.555");
    await manager.setActiveTurnId("C123", "444.555", "turn-1");

    await Promise.all([
      manager.recordTurnSignal("C123", "444.555", {
        turnId: "turn-1",
        kind: "final",
        occurredAt: "2026-03-18T10:00:00.000Z",
      }),
      manager.setActiveTurnId("C123", "444.555", undefined),
    ]);

    expect(manager.getSession("C123", "444.555")).toMatchObject({
      activeTurnId: undefined,
      lastTurnSignalTurnId: "turn-1",
      lastTurnSignalKind: "final",
      lastTurnSignalAt: "2026-03-18T10:00:00.000Z",
    });
  });

  it("persists co-author candidate and confirmed revision state", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-sessions-"));
    const store = new StateStore(stateDir, sessionsRoot);
    const manager = new SessionManager({
      stateStore: store,
      sessionsRoot,
    });

    await manager.load();
    await manager.ensureSession("C123", "999.000");
    let session = await manager.addCoAuthorCandidates("C123", "999.000", ["U1"]);
    expect(session).toMatchObject({
      coAuthorCandidateUserIds: ["U1"],
      coAuthorCandidateRevision: 1,
    });

    session = await manager.confirmCoAuthors("C123", "999.000", {
      userIds: ["U1"],
      candidateRevision: 1,
      ignoreMissing: true,
    });
    expect(session).toMatchObject({
      coAuthorConfirmedUserIds: ["U1"],
      coAuthorConfirmedRevision: 1,
      coAuthorIgnoreMissingRevision: 1,
    });

    session = await manager.addCoAuthorCandidates("C123", "999.000", ["U2"]);
    expect(session).toMatchObject({
      coAuthorCandidateUserIds: ["U1", "U2"],
      coAuthorCandidateRevision: 2,
      coAuthorConfirmedRevision: 1,
      coAuthorIgnoreMissingRevision: undefined,
    });

    const reloadedStore = new StateStore(stateDir, sessionsRoot);
    const reloadedManager = new SessionManager({
      stateStore: reloadedStore,
      sessionsRoot,
    });
    await reloadedManager.load();
    expect(reloadedManager.getSession("C123", "999.000")).toMatchObject({
      coAuthorCandidateUserIds: ["U1", "U2"],
      coAuthorCandidateRevision: 2,
      coAuthorConfirmedUserIds: ["U1"],
      coAuthorConfirmedRevision: 1,
      coAuthorIgnoreMissingRevision: undefined,
    });
  });
});
