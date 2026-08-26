import { describe, expect, it } from "vite-plus/test";

import { AUTOMATIC_PROFILE_ID, modelOptions, profileOptions, thinkingOptions } from "../apps/admin-ui/profile-selection.js";

const profiles = [
  {
    profile_id: "subscription",
    provider: "xai",
    billing: "subscription",
    auth_configured: true,
    account: { ok: true },
    rateLimits: { ok: true, rateLimits: { secondary: { usedPercent: 60 } } },
    models: [
      {
        id: "grok",
        thinking: ["low", "high"],
        default_thinking: "high",
        capabilities: { input: ["text"] },
        default: true,
      },
    ],
  },
  {
    profile_id: "usage",
    provider: "xai",
    billing: "usage",
    auth_configured: true,
    account: { ok: true },
    rateLimits: { ok: true, rateLimits: { credits: { balance: "100" } } },
    models: [
      {
        id: "grok",
        thinking: ["high"],
        default_thinking: "high",
        capabilities: { input: ["text", "image"] },
      },
    ],
  },
  {
    profile_id: "other",
    provider: "anthropic",
    billing: "subscription",
    auth_configured: true,
    account: { ok: true },
    rateLimits: { ok: true, rateLimits: { secondary: { usedPercent: 10 } } },
    models: [
      {
        id: "claude",
        thinking: ["off", "high"],
        default_thinking: "off",
        capabilities: { input: ["text"] },
        default: true,
      },
    ],
  },
  {
    profile_id: "unconfigured",
    provider: "openai",
    billing: "usage",
    auth_configured: false,
    models: [
      {
        id: "unavailable",
        thinking: ["minimal"],
        default_thinking: "minimal",
        capabilities: { input: ["text"] },
        default: true,
      },
      {
        id: "grok",
        thinking: ["minimal"],
        default_thinking: "minimal",
        capabilities: { input: ["text"] },
      },
    ],
  },
];

describe("Profile-backed model selection", () => {
  it("builds model, thinking, and compatible Profile choices independently", () => {
    expect(modelOptions(profiles)).toEqual(["claude", "grok"]);
    expect(thinkingOptions(profiles, "grok")).toEqual(["low", "high"]);
    expect(profileOptions(profiles, "grok", "low").map((profile) => profile.profile_id)).toEqual(["subscription"]);
    expect(profileOptions(profiles, "grok", "high").map((profile) => profile.profile_id)).toEqual(["subscription", "usage"]);
  });

  it("adds auto only to the Profile choice", () => {
    expect([AUTOMATIC_PROFILE_ID, ...profileOptions(profiles, "grok", "high").map((profile) => profile.profile_id)]).toEqual(["auto", "subscription", "usage"]);
  });
});
