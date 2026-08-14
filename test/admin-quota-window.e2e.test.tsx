import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, describe, expect, it, vi } from "vitest";

import { AuthProfilesPanel } from "../src/admin-ui/admin-shell-helpers-1.js";
import { TopbarQuota } from "../src/admin-ui/admin-shell-helpers-2.js";
import { authProfileQuotaItems, profileQuotaSummary } from "../src/admin-ui/admin-shell-helpers-3.js";
import { profileQuotaLabel } from "../src/admin-ui/auth-profile-display.js";
import { serializeRateLimits } from "../src/services/codex/account-status.js";
import { readChatGptUsageSnapshot, type ChatGptUsageSnapshot } from "../src/services/codex/chatgpt-usage-api.js";
import { evaluateAuthProfile } from "../src/services/session-auth-profile-selector.js";
import type { AuthProfileSummary } from "../src/services/auth-profile-service.js";

describe("admin quota window end to end", () => {
  const tempDirs: string[] = [];
  const now = new Date("2026-08-14T08:00:00.000Z");

  afterEach(async () => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    await Promise.all(tempDirs.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
  });

  it("renders and scores a single weekly primary window from the live usage shape", async () => {
    const authJsonPath = await writeAuthJson({
      tokens: {
        access_token: "active-access-token",
        account_id: "account-1",
      },
    });
    vi.stubGlobal(
      "fetch",
      vi.fn(async () =>
        jsonResponse({
          email: "operator@example.com",
          plan_type: "pro",
          rate_limit: {
            allowed: true,
            limit_reached: false,
            primary_window: {
              used_percent: 52,
              limit_window_seconds: 604_800,
              reset_after_seconds: 509_396,
              reset_at: 1_787_203_912,
            },
            secondary_window: null,
          },
          additional_rate_limits: [],
        }),
      ),
    );

    const usage = await readChatGptUsageSnapshot(authJsonPath);
    const profile = authProfile(usage.account, serializeRateLimits(usage.rateLimits));

    vi.useFakeTimers();
    vi.setSystemTime(now);

    expect(profileQuotaLabel(profile, { now })).toBe("7d 48% / 0.57");
    const quotaSummary = profileQuotaSummary(profile.rateLimits);
    expect(quotaSummary).toEqual({
      ok: true,
      fullLabel: "7d 48% / 0.57",
      remainingLabel: "48%",
      scoreLabel: "0.57",
      resetLabel: "6 天后",
      shortLabel: null,
      tone: "",
    });
    expect(authProfileQuotaItems([profile])).toMatchObject([
      {
        label: "7d 48% / 0.57",
        remaining: 48,
      },
    ]);
    const cardMarkup = renderToStaticMarkup(
      <AuthProfilesPanel
        status={{
          authProfiles: {
            profiles: [profile],
          },
        }}
        message={null}
        setMessage={() => {}}
        onAdd={() => {}}
      />,
    );
    expect(cardMarkup).toContain("账号池");
    expect(cardMarkup).toContain("operator@example.com");
    expect(cardMarkup).toContain("7d 剩余");
    expect(cardMarkup).toContain("48%");
    expect(cardMarkup).toContain("0.57");
    expect(cardMarkup).toContain("6 天后");
    expect(cardMarkup).not.toContain("--");
    expect(cardMarkup).not.toContain("未知");

    const topbarMarkup = renderToStaticMarkup(<TopbarQuota profiles={[profile]} />);
    expect(topbarMarkup).toContain("7d 48% / 0.57");
    expect(topbarMarkup).not.toContain("账号池额度未知");

    const evaluation = evaluateAuthProfile(profile, { now });
    expect(evaluation.usable).toBe(true);
    expect(evaluation.weightedWeeklyQuotaScore).toBeCloseTo(0.57, 2);
    expect(evaluation.effectiveQuotaScore).toBeCloseTo(0.57, 2);
  });

  async function writeAuthJson(content: Record<string, unknown>): Promise<string> {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), "admin-quota-window-"));
    tempDirs.push(directory);
    const authJsonPath = path.join(directory, "auth.json");
    await fs.writeFile(authJsonPath, JSON.stringify(content), "utf8");
    return authJsonPath;
  }
});

function authProfile(account: ChatGptUsageSnapshot["account"], rateLimits: AuthProfileSummary["rateLimits"]): AuthProfileSummary {
  return {
    name: "profile-1",
    path: "/tmp/auth-profiles/profile-1.json",
    source: "probe",
    checkedAt: "2026-08-14T08:00:00.000Z",
    account: {
      ok: true,
      account,
      requiresOpenaiAuth: false,
    },
    rateLimits,
  };
}

function jsonResponse(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    headers: {
      "Content-Type": "application/json",
    },
  });
}
