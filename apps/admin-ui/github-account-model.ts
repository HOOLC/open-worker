import { AdminStatus } from "./admin-types.js";

export function normalizeGitHubAccounts(status: AdminStatus): Array<Record<string, any>> {
  const accounts = status.githubAccounts?.accounts;
  if (Array.isArray(accounts) && accounts.length > 0) return accounts;
  const fallback = buildFallbackGitHubAccounts(status);
  return fallback;
}

export function buildFallbackGitHubAccounts(status: AdminStatus): Array<Record<string, any>> {
  const rows = new Map<string, Record<string, any>>();
  const bindings = Array.isArray(status.githubPrIdentities?.bindings) ? status.githubPrIdentities.bindings : [];
  const sessions = Array.isArray(status.state?.sessions) ? status.state.sessions : [];
  const defaultAccount = status.githubAccounts?.defaultPrAccount;
  const defaultSlackUserId = defaultAccount?.available === true && defaultAccount.source === "bound" ? String(defaultAccount.slackUserId || "") : "";

  function addSlackUser(userId: unknown, identity?: Record<string, any> | null): void {
    const slackUserId = String(userId || "").trim();
    if (!slackUserId || slackUserId.startsWith("username:")) return;
    const normalizedIdentity = normalizeSlackIdentity(slackUserId, identity);
    const existing = rows.get(slackUserId);
    if (!rows.has(slackUserId)) {
      rows.set(slackUserId, {
        slackUserId,
        slackIdentity: normalizedIdentity,
        isDefaultPrAccount: slackUserId === defaultSlackUserId,
        prBinding: {
          state: "unbound",
        },
      });
      return;
    }
    if (existing) {
      rows.set(slackUserId, {
        ...existing,
        slackIdentity: mergeSlackIdentity(existing.slackIdentity, normalizedIdentity),
      });
    }
  }

  for (const session of sessions) {
    addSlackUser(session.initiatorUserId);
    addSlackUser(session.firstUserMessage?.userId, session.firstUserMessage?.slackIdentity || identityFromSessionMessage(session.firstUserMessage));
    addSlackUser(session.lastUserMessage?.userId, session.lastUserMessage?.slackIdentity || identityFromSessionMessage(session.lastUserMessage));
  }

  for (const binding of bindings) {
    const slackUserId = String(binding.slackUserId || "").trim();
    if (!slackUserId) continue;
    rows.set(slackUserId, {
      ...(rows.get(slackUserId) || {
        slackUserId,
        slackIdentity: normalizeSlackIdentity(slackUserId),
      }),
      isDefaultPrAccount: slackUserId === defaultSlackUserId,
      prBinding: {
        state: binding.revokedAt ? "revoked" : "bound",
        githubLogin: binding.githubLogin,
        githubUserId: binding.githubUserId,
        githubEmail: binding.githubEmail ?? null,
        githubName: binding.githubName ?? null,
        scopes: binding.scopes || [],
        createdAt: binding.createdAt,
        updatedAt: binding.updatedAt,
        lastValidatedAt: binding.lastValidatedAt ?? null,
        revokedAt: binding.revokedAt ?? null,
      },
    });
  }

  return [...rows.values()].sort((left, right) => {
    if (Boolean(left.isDefaultPrAccount) !== Boolean(right.isDefaultPrAccount)) {
      return left.isDefaultPrAccount ? -1 : 1;
    }
    const leftBound = left.prBinding?.state === "bound";
    const rightBound = right.prBinding?.state === "bound";
    if (leftBound !== rightBound) return leftBound ? -1 : 1;
    return String(left.slackUserId).localeCompare(String(right.slackUserId));
  });
}

export function normalizeSlackIdentity(slackUserId: string, identity?: Record<string, any> | null): Record<string, any> {
  return {
    userId: slackUserId,
    mention: `<@${slackUserId}>`,
    ...(identity?.username ? { username: identity.username } : {}),
    ...(identity?.displayName ? { displayName: identity.displayName } : {}),
    ...(identity?.realName ? { realName: identity.realName } : {}),
    ...(identity?.email ? { email: identity.email } : {}),
  };
}

export function mergeSlackIdentity(previous: Record<string, any> | undefined, next: Record<string, any>): Record<string, any> {
  return {
    ...normalizeSlackIdentity(String(next.userId || previous?.userId || "")),
    ...previous,
    ...Object.fromEntries(Object.entries(next).filter(([, value]) => value !== undefined && value !== null && value !== "")),
  };
}

export function identityFromSessionMessage(message: Record<string, any> | null | undefined): Record<string, any> | null {
  if (!message) return null;
  return normalizeSlackIdentity(String(message.userId || ""), message.senderUsername ? { username: message.senderUsername } : {});
}
