import type { AuthProfileSummary, AuthProfilesStatus } from "./auth-profile-service.js";
import { daysUntilReset, remainingPercent, resolveQuotaWindowRoles, timestampMs, weightedWeeklyQuotaScore } from "../auth-profile-quota.js";

export type AuthProfileUnavailableReason = "profile_not_found" | "account_probe_failed" | "rate_limits_probe_failed" | "primary_quota_exhausted" | "secondary_quota_exhausted" | "credits_exhausted" | "no_usable_auth_profiles";

export interface AuthProfileEvaluation {
  readonly profileName: string;
  readonly usable: boolean;
  readonly reason?: AuthProfileUnavailableReason | undefined;
  readonly effectiveQuotaScore: number;
  readonly primaryRemainingPercent?: number | undefined;
  readonly secondaryRemainingPercent?: number | undefined;
  readonly weeklyRemainingPercent?: number | undefined;
  readonly shortRemainingPercent?: number | undefined;
  readonly secondaryRefreshDays?: number | undefined;
  readonly weeklyRefreshDays?: number | undefined;
  readonly weightedWeeklyQuotaScore?: number | undefined;
}

export function isAuthProfileProbeFailureReason(reason: string | undefined): reason is "account_probe_failed" | "rate_limits_probe_failed" {
  return reason === "account_probe_failed" || reason === "rate_limits_probe_failed";
}

export function isAuthProfileProbeFailure(evaluation: AuthProfileEvaluation): boolean {
  return !evaluation.usable && isAuthProfileProbeFailureReason(evaluation.reason);
}

export function selectBestAuthProfile(status: AuthProfilesStatus, options: { readonly now?: Date | number | string | undefined } = {}): AuthProfileSummary | null {
  const nowMs = timestampMs(options.now);
  const candidates = status.profiles
    .map((profile) => ({
      profile,
      evaluation: evaluateAuthProfile(profile, { now: nowMs }),
    }))
    .filter((candidate) => candidate.evaluation.usable)
    .sort((left, right) => {
      const scoreDelta = right.evaluation.effectiveQuotaScore - left.evaluation.effectiveQuotaScore;
      if (scoreDelta) {
        return scoreDelta;
      }

      const weeklyDelta = (right.evaluation.weeklyRemainingPercent ?? 100) - (left.evaluation.weeklyRemainingPercent ?? 100);
      if (weeklyDelta) {
        return weeklyDelta;
      }

      const shortDelta = (right.evaluation.shortRemainingPercent ?? 100) - (left.evaluation.shortRemainingPercent ?? 100);
      if (shortDelta) {
        return shortDelta;
      }

      return left.profile.name.localeCompare(right.profile.name);
    });

  return candidates[0]?.profile ?? null;
}

export function findAuthProfile(status: AuthProfilesStatus, profileName: string): AuthProfileSummary | null {
  return status.profiles.find((profile) => profile.name === profileName) ?? null;
}

export function evaluateAuthProfile(profile: AuthProfileSummary, options: { readonly now?: Date | number | string | undefined } = {}): AuthProfileEvaluation {
  if (!profile.account.ok) {
    return unavailable(profile.name, "account_probe_failed");
  }

  if (!profile.rateLimits.ok) {
    return unavailable(profile.name, "rate_limits_probe_failed");
  }

  const limits = profile.rateLimits.rateLimits;
  const primaryRemaining = remainingPercent(limits?.primary?.usedPercent);
  const secondaryRemaining = remainingPercent(limits?.secondary?.usedPercent);
  const windows = resolveQuotaWindowRoles({
    primary: limits?.primary,
    secondary: limits?.secondary,
  });
  const weeklyRemaining = remainingPercent(windows.weekly?.usedPercent);
  const shortRemaining = remainingPercent(windows.short?.usedPercent);
  const weeklyRefreshDays = daysUntilReset(windows.weekly?.resetsAt, timestampMs(options.now));
  const weightedWeeklyQuota = weightedWeeklyQuotaScore(weeklyRemaining, weeklyRefreshDays);
  const credits = limits?.credits;

  if (primaryRemaining !== undefined && primaryRemaining <= 0) {
    return unavailable(profile.name, "primary_quota_exhausted", {
      primaryRemainingPercent: primaryRemaining,
      secondaryRemainingPercent: secondaryRemaining,
    });
  }

  if (secondaryRemaining !== undefined && secondaryRemaining <= 0) {
    return unavailable(profile.name, "secondary_quota_exhausted", {
      primaryRemainingPercent: primaryRemaining,
      secondaryRemainingPercent: secondaryRemaining,
    });
  }

  if (credits && !credits.unlimited && credits.hasCredits === false) {
    return unavailable(profile.name, "credits_exhausted", {
      primaryRemainingPercent: primaryRemaining,
      secondaryRemainingPercent: secondaryRemaining,
    });
  }

  return {
    profileName: profile.name,
    usable: true,
    effectiveQuotaScore: weightedWeeklyQuota ?? (weeklyRemaining !== undefined ? weeklyRemaining / 100 : undefined) ?? (shortRemaining !== undefined ? shortRemaining / 100 : undefined) ?? 1,
    primaryRemainingPercent: primaryRemaining,
    secondaryRemainingPercent: secondaryRemaining,
    weeklyRemainingPercent: weeklyRemaining,
    shortRemainingPercent: shortRemaining,
    secondaryRefreshDays: daysUntilReset(limits?.secondary?.resetsAt, timestampMs(options.now)),
    weeklyRefreshDays,
    weightedWeeklyQuotaScore: weightedWeeklyQuota,
  };
}

export function authProfileReasonLabel(reason: string | undefined): string {
  switch (reason) {
    case "profile_not_found":
      return "绑定的账号不存在";
    case "account_probe_failed":
      return "账号状态读取失败";
    case "rate_limits_probe_failed":
      return "额度状态读取失败";
    case "primary_quota_exhausted":
      return "当前额度已耗尽";
    case "secondary_quota_exhausted":
      return "周额度已耗尽";
    case "credits_exhausted":
      return "账号 credits 不可用";
    case "no_usable_auth_profiles":
      return "没有可用账号";
    default:
      return reason || "账号不可用";
  }
}

function unavailable(
  profileName: string,
  reason: AuthProfileUnavailableReason,
  partial?: {
    readonly primaryRemainingPercent?: number | undefined;
    readonly secondaryRemainingPercent?: number | undefined;
  },
): AuthProfileEvaluation {
  return {
    profileName,
    usable: false,
    reason,
    effectiveQuotaScore: 0,
    primaryRemainingPercent: partial?.primaryRemainingPercent,
    secondaryRemainingPercent: partial?.secondaryRemainingPercent,
  };
}
