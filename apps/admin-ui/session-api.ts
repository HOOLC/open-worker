export async function requestJson(path: string, init?: RequestInit): Promise<unknown> {
  const response = await fetch(path, init);
  const payload = (await response.json().catch(() => ({}))) as Record<string, any>;
  if (!response.ok || payload.ok === false) throw new Error(payload.error || response.statusText || "请求失败");
  return payload;
}

export function sessionTimelineApiPath(
  sessionKey: string,
  options: {
    readonly limit?: number | undefined;
    readonly beforeSequence?: number | undefined;
  } = {},
): string {
  const params = new URLSearchParams();
  if (options.limit) {
    params.set("limit", String(options.limit));
  }
  if (options.beforeSequence) {
    params.set("before_sequence", String(options.beforeSequence));
  }
  const query = params.toString();
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/timeline" + (query ? "?" + query : "");
}

export function sessionTimelineEventApiPath(sessionKey: string, eventId: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/timeline-events/" + encodeURIComponent(eventId);
}

export function slackThreadUrlApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/slack-thread-url";
}

export function githubIdentityApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/github-identity";
}

export function githubDeviceStartApiPath(sessionKey: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/github-oauth/device/start";
}

export function githubDevicePollApiPath(deviceAuthorizationId: string): string {
  return "/admin/api/github-oauth/device/" + encodeURIComponent(deviceAuthorizationId);
}

export function adminSessionPath(sessionKey: string): string {
  return "/admin/sessions/" + encodeURIComponent(sessionKey);
}
