import { describe, expect, it } from "vitest";

import { DEFAULT_WEEKLY_WINDOW_MINS, resolveQuotaWindowRoles } from "../src/auth-profile-quota.js";

describe("resolveQuotaWindowRoles", () => {
  it("recognizes a single weekly primary window", () => {
    const primary = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS);

    expect(resolveQuotaWindowRoles({ primary, secondary: null })).toEqual({
      weekly: primary,
      short: null,
    });
  });

  it("keeps the legacy short-primary and weekly-secondary pair", () => {
    const primary = quotaWindow(300);
    const secondary = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS);

    expect(resolveQuotaWindowRoles({ primary, secondary })).toEqual({
      weekly: secondary,
      short: primary,
    });
  });

  it("finds weekly and short windows when upstream reverses their positions", () => {
    const primary = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS);
    const secondary = quotaWindow(300);

    expect(resolveQuotaWindowRoles({ primary, secondary })).toEqual({
      weekly: primary,
      short: secondary,
    });
  });

  it("accepts the upstream weekly duration tolerance", () => {
    const lowerBound = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS * 0.95);
    const upperBound = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS * 1.05);

    expect(resolveQuotaWindowRoles({ primary: lowerBound, secondary: null }).weekly).toBe(lowerBound);
    expect(resolveQuotaWindowRoles({ primary: upperBound, secondary: null }).weekly).toBe(upperBound);
  });

  it("rejects durations immediately outside the weekly tolerance", () => {
    const belowLowerBound = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS * 0.95 - 1);
    const aboveUpperBound = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS * 1.05 + 1);

    expect(resolveQuotaWindowRoles({ primary: belowLowerBound, secondary: null })).toEqual({
      weekly: null,
      short: belowLowerBound,
    });
    expect(resolveQuotaWindowRoles({ primary: aboveUpperBound, secondary: null })).toEqual({
      weekly: null,
      short: aboveUpperBound,
    });
  });

  it("uses legacy positions when both windows look weekly", () => {
    const primary = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS);
    const secondary = quotaWindow(DEFAULT_WEEKLY_WINDOW_MINS);

    expect(resolveQuotaWindowRoles({ primary, secondary })).toEqual({
      weekly: secondary,
      short: primary,
    });
  });

  it("does not reinterpret a single monthly primary window as weekly", () => {
    const primary = quotaWindow(30 * 24 * 60);

    expect(resolveQuotaWindowRoles({ primary, secondary: null })).toEqual({
      weekly: null,
      short: primary,
    });
  });

  it("falls back to legacy positions when durations do not identify weekly", () => {
    const primary = quotaWindow(null);
    const secondary = quotaWindow(null);

    expect(resolveQuotaWindowRoles({ primary, secondary })).toEqual({
      weekly: secondary,
      short: primary,
    });
    expect(resolveQuotaWindowRoles({ primary, secondary: null })).toEqual({
      weekly: null,
      short: primary,
    });
    expect(resolveQuotaWindowRoles({ primary: null, secondary })).toEqual({
      weekly: secondary,
      short: null,
    });
  });
});

function quotaWindow(windowDurationMins: number | null) {
  return {
    usedPercent: 52,
    windowDurationMins,
    resetsAt: 1_787_203_912,
  };
}
