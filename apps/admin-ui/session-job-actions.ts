export async function requestCancelSessionJob(sessionKey: string, jobId: string): Promise<Record<string, any>> {
  const response = await fetch(sessionJobCancelApiPath(sessionKey, jobId), {
    method: "POST",
  });
  const payload = (await response.json().catch(() => ({}))) as Record<string, any>;
  if (!response.ok || payload.ok === false) {
    throw new Error(payload.error || response.statusText || "请求失败");
  }
  return payload;
}

function sessionJobCancelApiPath(sessionKey: string, jobId: string): string {
  return "/admin/api/sessions/" + encodeURIComponent(sessionKey) + "/jobs/" + encodeURIComponent(jobId) + "/cancel";
}
