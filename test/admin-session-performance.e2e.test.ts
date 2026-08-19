import { afterEach, describe, expect, it } from "vitest";

import { readJson, requestJson, startAdminFixture } from "./admin-control-plane.e2e-helpers.js";

async function seedAssistantEvents(
  sessions: {
    upsertAgentTraceEvent: (event: {
      readonly id: string;
      readonly sessionKey: string;
      readonly source: "agent_runtime";
      readonly type: string;
      readonly at: string;
      readonly sequence: number;
      readonly title: string;
      readonly summary: string;
      readonly detail: string;
      readonly status: string;
      readonly role: string;
      readonly createdAt: string;
      readonly updatedAt: string;
    }) => Promise<void>;
  },
  sessionKey: string,
  start: number,
  end: number,
  type: string,
  titlePrefix: string,
): Promise<void> {
  for (let index = start; index <= end; index += 1) {
    await sessions.upsertAgentTraceEvent({
      id: `${titlePrefix}-${index}`,
      sessionKey,
      source: "agent_runtime",
      type,
      at: new Date(Date.UTC(2030, 2, 19, 0, 0, index)).toISOString(),
      sequence: index,
      title: `${titlePrefix} ${index}`,
      summary: `${titlePrefix} summary ${index}`,
      detail: `${titlePrefix} detail ${index}`,
      status: "completed",
      role: "assistant",
      createdAt: "2026-03-19T00:00:00.000Z",
      updatedAt: "2026-03-19T00:00:00.000Z",
    });
  }
}

describe("admin session performance e2e", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("serves timeline pages from newest to older with bounded responses", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-session-timeline-page-",
    });
    await sessions.ensureSession("C123", "111.222");
    await seedAssistantEvents(sessions, "C123:111.222", 1, 120, "agent_assistant_message", "event");

    const firstResponse = await requestJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/timeline?limit=25`);
    expect(firstResponse.status).toBe(200);
    expect(firstResponse.headers.get("server-timing")).toContain("session-timeline");
    expect(Number(firstResponse.headers.get("x-admin-duration-ms"))).toBeGreaterThanOrEqual(0);
    const first = firstResponse.payload;
    const firstTraceSequences = (first.events as Array<Record<string, unknown>>).map((event) => event.sequence).filter((sequence): sequence is number => typeof sequence === "number");
    expect(first.events).toHaveLength(25);
    expect((first.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(Array.from({ length: 25 }, () => "agent_assistant_message"));
    expect(firstTraceSequences.slice(0, 3)).toEqual([96, 97, 98]);
    expect(firstTraceSequences.at(-1)).toBe(120);
    expect((first.events as Array<Record<string, unknown>>)[0]).toMatchObject({
      detailAvailable: true,
    });
    expect((first.events as Array<Record<string, unknown>>)[0]).not.toHaveProperty("detail");
    expect(first.page).toMatchObject({
      limit: 25,
      hasMore: true,
      nextBeforeSequence: 96,
    });
    expect(first.trace).toMatchObject({
      eventCount: 120,
      categories: {
        agent_assistant_message: 120,
      },
    });

    const older = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/timeline?limit=25&before_sequence=96`);
    const olderTraceSequences = (older.events as Array<Record<string, unknown>>).map((event) => event.sequence).filter((sequence): sequence is number => typeof sequence === "number");
    expect(olderTraceSequences.slice(0, 3)).toEqual([71, 72, 73]);
    expect(olderTraceSequences.at(-1)).toBe(95);
    expect(older.page).toMatchObject({
      hasMore: true,
      nextBeforeSequence: 71,
    });

    const detail = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/timeline-events/${encodeURIComponent("event-120")}`);
    expect(detail).toMatchObject({
      ok: true,
      event: {
        id: "event-120",
        detail: "event detail 120",
      },
    });
  });

  it("fills timeline pages with visible events instead of raw hidden trace rows", async () => {
    const { baseUrl, sessions } = await startAdminFixture(cleanups, {
      tempPrefix: "admin-session-visible-page-",
    });
    await sessions.ensureSession("C123", "111.222");
    await seedAssistantEvents(sessions, "C123:111.222", 1, 60, "agent_assistant_message", "visible");
    await seedAssistantEvents(sessions, "C123:111.222", 61, 90, "agent_token_count", "hidden");

    const page = await readJson(`${baseUrl}/admin/api/sessions/${encodeURIComponent("C123:111.222")}/timeline?limit=25`);
    const sequences = (page.events as Array<Record<string, unknown>>).map((event) => event.sequence).filter((sequence): sequence is number => typeof sequence === "number");
    expect(page.events).toHaveLength(25);
    expect((page.events as Array<Record<string, unknown>>).map((event) => event.type)).toEqual(Array.from({ length: 25 }, () => "agent_assistant_message"));
    expect(sequences[0]).toBe(36);
    expect(sequences.at(-1)).toBe(60);
    expect(page.page).toMatchObject({
      hasMore: true,
      nextBeforeSequence: 36,
    });
  });
});
