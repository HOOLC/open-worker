import { Tone } from "./admin-types.js";

export function githubBindingLabel(binding: Record<string, any>): string {
  if (binding.state === "bound") return "已绑定 " + (binding.githubLogin || "");
  if (binding.state === "revoked") return "绑定失效";
  return "未绑定";
}

export function githubBindingTone(binding: Record<string, any>): Tone {
  if (binding.state === "bound") return "good";
  if (binding.state === "revoked") return "danger";
  return "warn";
}

export function githubAccountOptionLabel(account: Record<string, any>): string {
  const identity = account.slackIdentity || {};
  const binding = account.prBinding || {};
  const slackLabel = identity.realName || identity.displayName || identity.username || account.slackUserId;
  const githubLabel = binding.githubLogin || "GitHub";
  return String(slackLabel) + " · " + String(githubLabel);
}

export function quotaTone(remaining: number): Tone {
  if (remaining < 10) return "danger";
  if (remaining < 30) return "warn";
  return "";
}

export function statusTone(status: unknown): Tone {
  const value = String(status || "").toLowerCase();
  if (["succeeded", "running", "active", "ok", "completed", "done"].includes(value)) return "good";
  if (["pending", "inflight", "registered", "starting", "idle", "started", "wait"].includes(value)) return "warn";
  if (["failed", "error", "stopped", "cancelled", "blocked"].includes(value)) return "danger";
  if (["agent_system_prompt", "agent_memory", "agent_runtime_instruction"].includes(value)) return "purple";
  if (value.startsWith("agent_")) return "info";
  if (["deploy", "rollback"].includes(value)) return "info";
  return "";
}

export function operationLabel(value: unknown): string {
  const labels: Record<string, string> = {
    deploy: "发布",
    rollback: "回滚",
    github_author_upsert: "保存 GitHub 作者",
    github_author_delete: "删除 GitHub 作者",
    github_pr_default_set: "设置默认 PR 账号",
  };
  return labels[String(value || "")] || String(value || "");
}

export function pickOperationLabel(operation: Record<string, any>): string {
  return operation?.request?.version || operation?.request?.ref || operation?.request?.name || operation?.request?.slackUserId || operation?.id || "-";
}

export function fmtTime(value: unknown): string {
  if (!value) return "--";
  const date = new Date(String(value));
  if (!Number.isFinite(date.getTime())) return String(value);
  return [String(date.getHours()).padStart(2, "0"), String(date.getMinutes()).padStart(2, "0"), String(date.getSeconds()).padStart(2, "0")].join(":");
}

export function fmtDateTime(value: unknown): string {
  if (!value) return "--";
  const date = new Date(String(value));
  if (!Number.isFinite(date.getTime())) return String(value);
  return [date.getFullYear(), String(date.getMonth() + 1).padStart(2, "0"), String(date.getDate()).padStart(2, "0")].join("-") + " " + fmtTime(value);
}

export function shortRevision(value: unknown): string {
  const text = String(value || "").trim();
  return text.length > 12 ? text.slice(0, 12) : text;
}

export function formatRelativeDuration(ms: number): string {
  const absMs = Math.abs(ms);
  const minutes = Math.round(absMs / 60_000);
  if (minutes < 60) return minutes + " 分钟";
  const hours = Math.round(absMs / 3_600_000);
  if (hours < 48) return hours + " 小时";
  return Math.round(absMs / 86_400_000) + " 天";
}

export function formatResetTime(seconds: unknown): string {
  const value = Number(seconds);
  if (!Number.isFinite(value)) return "未知";
  const delta = value * 1000 - Date.now();
  const relative = formatRelativeDuration(delta);
  return delta > 0 ? relative + "后" : relative + "前";
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
