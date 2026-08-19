import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";
import { JobManager } from "../src/services/job-manager.js";
import { StateStore } from "../src/store/state-store.js";
import type { PersistedBackgroundJob } from "../src/types.js";

describe("JobManager", () => {
  it("automatically cancels jobs that exceed the runtime limit", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-"));
    const jobsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-jobs-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-sessions-"));
    const reposRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-repos-"));
    const store = new StateStore(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore: store,
      sessionsRoot,
    });
    await sessions.load();
    const session = await sessions.ensureSession("C123", "111.333");
    await fs.mkdir(session.workspacePath, { recursive: true });

    const seenEvents: Array<{ payload: { summary: string; jobId: string; eventKind: string } }> = [];
    const jobs = new JobManager({
      sessions,
      jobsRoot,
      reposRoot,
      brokerHttpBaseUrl: "http://127.0.0.1:3000",
      maxRuntimeMs: 100,
      onEvent: async (event) => {
        seenEvents.push({
          payload: {
            summary: event.payload.summary,
            jobId: event.payload.jobId,
            eventKind: event.payload.eventKind,
          },
        });
      },
    });

    const job = await jobs.registerJob({
      channelId: "C123",
      rootThreadTs: "111.333",
      kind: "watch_ci",
      script: "#!/usr/bin/env bash\nsleep 30",
    });

    const timedOut = await waitForJobStatus(sessions, job.id, "cancelled");
    expect(timedOut.cancelledAt).toEqual(expect.any(String));
    expect(timedOut.completedAt).toEqual(expect.any(String));
    expect(timedOut.error).toContain("runtime limit");
    expect(seenEvents).toEqual([
      {
        payload: {
          summary: expect.stringContaining("runtime limit"),
          jobId: job.id,
          eventKind: "job_cancelled",
        },
      },
    ]);

    await jobs.stop();
  }, 15_000);

  it("cancels restartable jobs that are already past the runtime limit on startup", async () => {
    const stateDir = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-state-"));
    const jobsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-jobs-"));
    const sessionsRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-sessions-"));
    const reposRoot = await fs.mkdtemp(path.join(os.tmpdir(), "slack-codex-repos-"));
    const store = new StateStore(stateDir, sessionsRoot);
    const sessions = new SessionManager({
      stateStore: store,
      sessionsRoot,
    });
    await sessions.load();
    const session = await sessions.ensureSession("C123", "111.444");
    await fs.mkdir(session.workspacePath, { recursive: true });
    const jobDir = path.join(jobsRoot, "expired-job");
    const scriptPath = path.join(jobDir, "run.sh");
    await fs.mkdir(jobDir, { recursive: true });
    await fs.writeFile(scriptPath, "#!/usr/bin/env bash\nsleep 30\n", { mode: 0o755 });

    await sessions.upsertBackgroundJob({
      id: "expired-job",
      token: "job-token",
      sessionKey: session.key,
      channelId: session.channelId,
      rootThreadTs: session.rootThreadTs,
      kind: "watch_ci",
      shell: "bash",
      cwd: session.workspacePath,
      scriptPath,
      restartOnBoot: true,
      status: "running",
      createdAt: new Date(Date.now() - 1_000).toISOString(),
      updatedAt: new Date(Date.now() - 1_000).toISOString(),
    });

    const seenEvents: string[] = [];
    const jobs = new JobManager({
      sessions,
      jobsRoot,
      reposRoot,
      brokerHttpBaseUrl: "http://127.0.0.1:3000",
      maxRuntimeMs: 100,
      onEvent: async (event) => {
        seenEvents.push(event.payload.eventKind);
      },
    });

    await jobs.start();

    expect(sessions.getBackgroundJob("expired-job")).toMatchObject({
      id: "expired-job",
      status: "cancelled",
      cancelledAt: expect.any(String),
      completedAt: expect.any(String),
      error: expect.stringContaining("runtime limit"),
    });
    expect(seenEvents).toEqual(["job_cancelled"]);

    await jobs.stop();
  });
});

async function waitForJobStatus(sessions: SessionManager, jobId: string, status: string): Promise<PersistedBackgroundJob> {
  const deadline = Date.now() + 5_000;
  while (Date.now() < deadline) {
    const job = sessions.getBackgroundJob(jobId);
    if (job?.status === status) {
      return job;
    }
    await new Promise((resolve) => setTimeout(resolve, 25));
  }

  throw new Error(`Timed out waiting for ${jobId} to become ${status}`);
}
