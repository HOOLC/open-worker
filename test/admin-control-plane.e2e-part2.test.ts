import http from "node:http";

import { afterEach, describe, expect, it } from "vitest";

import { SessionManager } from "../src/services/session-manager.js";

import { seedActiveSession, backgroundJob, readJson, postJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

describe("admin control plane e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("exposes a tracked session reset operation and delegates history clearing to the worker", async () => {
    const workerPaths: string[] = [];
    let sessionsRef: SessionManager | undefined;
    const worker = http.createServer((request, response) => {
      void (async () => {
        workerPaths.push(request.url ?? "");
        const session = sessionsRef?.getSessionByKey("C123:111.222");
        if (session) {
          await sessionsRef?.resetSessionRuntimeState(session.key);
        }
        response.writeHead(200, { "content-type": "application/json" });
        response.end(
          JSON.stringify({
            ok: true,
            sessionKey: "C123:111.222",
            reset: {
              clearedInboundCount: 2,
              resetMessageTs: "1778316208.809479",
              resumedCount: 1,
              interruptedActiveTurn: true,
              previousAgentSessionId: "old-thread",
              previousActiveTurnId: "old-turn",
              historyMessageCount: 4,
              authBlocked: false,
            },
          }),
        );
      })().catch((error) => {
        response.writeHead(500, { "content-type": "application/json" });
        response.end(JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error) }));
      });
    });
    await new Promise<void>((resolve) => worker.listen(0, "127.0.0.1", resolve));
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        worker.close((error) => {
          if (error) reject(error);
          else resolve();
        });
      });
    });
    const address = worker.address();
    if (!address || typeof address === "string") {
      throw new Error("failed to start worker fixture");
    }

    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      workerBaseUrl: `http://127.0.0.1:${address.port}`,
    });
    sessionsRef = sessions;
    let session = await sessions.ensureSession("C123", "111.222");
    session = await sessions.setAgentSessionId(session.channelId, session.rootThreadTs, "old-thread");
    session = await sessions.setActiveTurnId(session.channelId, session.rootThreadTs, "old-turn");

    const result = await postJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/reset`, {});

    expect(result).toMatchObject({
      ok: true,
      operation: {
        kind: "session_reset",
        status: "succeeded",
        request: {
          sessionKey: session.key,
        },
      },
      workerReset: {
        ok: true,
        reset: {
          clearedInboundCount: 2,
          resumedCount: 1,
          previousAgentSessionId: "old-thread",
          previousActiveTurnId: "old-turn",
        },
      },
      session: {
        key: session.key,
        agentSessionId: null,
        activeTurnId: null,
      },
    });
    expect(workerPaths).toEqual([`/slack/sessions/${encodeURIComponent(session.key)}/reset`]);
  });

  it("cancels a session background job through the admin control plane", async () => {
    const workerRequests: Array<{ readonly url: string; readonly body: Record<string, unknown> }> = [];
    let sessionsRef: SessionManager | undefined;
    const worker = http.createServer((request, response) => {
      void (async () => {
        const chunks: Buffer[] = [];
        for await (const chunk of request) {
          chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
        }
        const bodyText = Buffer.concat(chunks).toString("utf8");
        const body = bodyText ? (JSON.parse(bodyText) as Record<string, unknown>) : {};
        workerRequests.push({
          url: request.url ?? "",
          body,
        });

        const current = sessionsRef?.getBackgroundJob("job-1");
        const completedAt = "2026-03-19T00:00:05.000Z";
        if (current) {
          await sessionsRef?.upsertBackgroundJob({
            ...current,
            status: "cancelled",
            cancelledAt: completedAt,
            completedAt,
            updatedAt: completedAt,
          });
        }

        response.writeHead(200, { "content-type": "application/json" });
        response.end(
          JSON.stringify({
            ok: true,
            job: sessionsRef?.getBackgroundJob("job-1") ?? {
              id: "job-1",
              sessionKey: "C123:111.222",
              status: "cancelled",
            },
          }),
        );
      })().catch((error) => {
        response.writeHead(500, { "content-type": "application/json" });
        response.end(JSON.stringify({ ok: false, error: error instanceof Error ? error.message : String(error) }));
      });
    });
    await new Promise<void>((resolve) => worker.listen(0, "127.0.0.1", resolve));
    cleanups.push(async () => {
      await new Promise<void>((resolve, reject) => {
        worker.close((error) => {
          if (error) reject(error);
          else resolve();
        });
      });
    });
    const address = worker.address();
    if (!address || typeof address === "string") {
      throw new Error("failed to start worker fixture");
    }

    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      workerBaseUrl: `http://127.0.0.1:${address.port}`,
    });
    sessionsRef = sessions;
    const session = await sessions.ensureSession("C123", "111.222");
    await sessions.upsertBackgroundJob(
      backgroundJob({
        sessionKey: session.key,
        status: "running",
        updatedAt: "2026-03-19T00:00:02.000Z",
        startedAt: "2026-03-19T00:00:02.000Z",
      }),
    );

    const result = await postJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent(session.key)}/jobs/job-1/cancel`, {});

    expect(result).toMatchObject({
      ok: true,
      operation: {
        kind: "session_job_cancel",
        status: "succeeded",
        request: {
          sessionKey: session.key,
          jobId: "job-1",
        },
      },
      job: {
        id: "job-1",
        sessionKey: session.key,
        status: "cancelled",
      },
      session: {
        key: session.key,
        runningBackgroundJobCount: 0,
        backgroundJobs: [
          expect.objectContaining({
            id: "job-1",
            status: "cancelled",
          }),
        ],
      },
    });
    expect(workerRequests).toEqual([
      {
        url: `/jobs/job-1/admin-cancel`,
        body: {
          session_key: session.key,
        },
      },
    ]);
  });

  it("records deploy requests as durable admin operations with audit events", async () => {
    const { baseUrl, deploymentCalls } = await startAdminFixture(cleanups);

    const deploy = await postJson(`${baseUrl}/admin/api/deploy`, {
      target: "worker",
      version: "0.2.0",
      allow_active: false,
    });
    expect(deploy).toMatchObject({
      ok: true,
      operation: {
        kind: "deploy",
        status: "succeeded",
        request: {
          target: "worker",
          version: "0.2.0",
        },
      },
    });
    expect(deploymentCalls).toEqual([{ kind: "deploy", target: "worker", version: "0.2.0" }]);

    const operations = await readJson(`${baseUrl}/admin/api/operations`);
    expect(operations).toMatchObject({
      ok: true,
      operations: [
        {
          id: deploy.operation.id,
          kind: "deploy",
          status: "succeeded",
          request: {
            target: "worker",
            version: "0.2.0",
          },
        },
      ],
    });

    const audit = await readJson(`${baseUrl}/admin/api/audit`);
    expect(audit).toMatchObject({
      ok: true,
      events: [
        {
          operationId: deploy.operation.id,
          action: "deploy",
          status: "succeeded",
        },
        {
          operationId: deploy.operation.id,
          action: "deploy",
          status: "started",
        },
      ],
    });
  });

  it("records refused deploy preflight checks as failed admin operations", async () => {
    const { baseUrl, deploymentCalls, sessions } = await startAdminFixture(cleanups);
    await seedActiveSession(sessions);

    const response = await fetch(`${baseUrl}/admin/api/deploy`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        target: "worker",
        version: "0.2.0",
        allow_active: false,
      }),
    });
    const payload = (await response.json()) as Record<string, unknown>;
    expect(response.status).toBe(500);
    expect(payload).toMatchObject({
      ok: false,
    });
    expect(String(payload.error)).toContain("Refusing deploy");
    expect(deploymentCalls).toEqual([]);

    const operations = await readJson(`${baseUrl}/admin/api/operations`);
    expect(operations).toMatchObject({
      ok: true,
      operations: [
        {
          kind: "deploy",
          status: "failed",
          request: {
            target: "worker",
            version: "0.2.0",
          },
        },
      ],
    });
    const operation = (operations.operations as Array<{ id: string }>)[0];
    expect(operation?.id).toBeTruthy();

    const audit = await readJson(`${baseUrl}/admin/api/audit`);
    expect(audit).toMatchObject({
      ok: true,
      events: [
        {
          operationId: operation?.id,
          action: "deploy",
          status: "failed",
        },
        {
          operationId: operation?.id,
          action: "deploy",
          status: "started",
        },
      ],
    });
  });
});
