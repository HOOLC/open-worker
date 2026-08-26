type ProfileRecord = Record<string, any>;

export const AUTOMATIC_PROFILE_ID = "auto";

const thinkingOrder = ["off", "minimal", "low", "medium", "high", "xhigh"];

export function modelOptions(profiles: readonly ProfileRecord[]): string[] {
  return [
    ...new Set(
      selectableProfiles(profiles)
        .flatMap((profile) => modelsOf(profile).map((model) => String(model.id || "").trim()))
        .filter(Boolean),
    ),
  ].sort();
}

export function thinkingOptions(profiles: readonly ProfileRecord[], modelId: string): string[] {
  const supported = new Set(
    selectableProfiles(profiles)
      .flatMap((profile) =>
        modelsOf(profile)
          .filter((model) => model.id === modelId)
          .flatMap((model) => (Array.isArray(model.thinking) ? model.thinking : [])),
      )
      .map(String),
  );
  return [...supported].sort((left, right) => {
    const leftRank = thinkingOrder.indexOf(left);
    const rightRank = thinkingOrder.indexOf(right);
    if (leftRank >= 0 || rightRank >= 0) {
      return (leftRank < 0 ? thinkingOrder.length : leftRank) - (rightRank < 0 ? thinkingOrder.length : rightRank);
    }
    return left.localeCompare(right);
  });
}

export function defaultThinking(profiles: readonly ProfileRecord[], modelId: string): string {
  for (const profile of selectableProfiles(profiles)) {
    const model = modelsOf(profile).find((candidate) => candidate.id === modelId);
    if (model && Array.isArray(model.thinking) && model.thinking.includes(model.default_thinking)) {
      return String(model.default_thinking);
    }
  }
  return thinkingOptions(profiles, modelId)[0] || "";
}

export function profileOptions(profiles: readonly ProfileRecord[], modelId: string, thinking: string): ProfileRecord[] {
  return selectableProfiles(profiles)
    .filter((profile) => profileSupports(profile, modelId, thinking))
    .sort((left, right) => String(left.profile_id || "").localeCompare(String(right.profile_id || "")));
}

function selectableProfiles(profiles: readonly ProfileRecord[]): ProfileRecord[] {
  return profiles.filter((profile) => profile.auth_configured === true);
}

function profileSupports(profile: ProfileRecord, modelId: string, thinking: string): boolean {
  return modelsOf(profile).some((model) => model.id === modelId && Array.isArray(model.thinking) && model.thinking.includes(thinking));
}

function modelsOf(profile: ProfileRecord): ProfileRecord[] {
  return Array.isArray(profile.models) ? profile.models : [];
}
