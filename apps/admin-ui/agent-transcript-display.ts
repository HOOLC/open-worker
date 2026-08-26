import type { TimelineEvent } from "./timeline-display.js";
import { mergeToolTracePayloads, parseToolTraceDetail, summarizeToolTraceDisplay } from "./tool-trace-summary";

export type AgentTranscriptKind = "user" | "assistant" | "bot" | "tool" | "system" | "session";

const runtimeInputSources = new Set(["background_job", "background_job_event", "runtime_reminder", "broker_recovery", "recovered_thread_batch"]);

export function agentTranscriptKind(event: TimelineEvent): AgentTranscriptKind {
  const type = String(event.type || "").toLowerCase();
  const source = timelineInputSource(event);
  const semanticType = timelineSemanticType(event);

  if (source === "admin_session_reset") {
    return "session";
  }
  if (semanticType === "slack_message" || semanticType === "slack_file") {
    return "bot";
  }
  if (runtimeInputSources.has(source)) {
    return "system";
  }
  if (type === "agent_user_message") {
    return "user";
  }
  if (type === "agent_input_received") {
    return "user";
  }
  if (type.includes("assistant")) {
    return "assistant";
  }
  if (type.includes("tool") || event.toolName) {
    return "tool";
  }
  if (type.includes("runtime") || type.includes("system") || type.includes("memory") || type.includes("reasoning")) {
    return "system";
  }
  if (type.includes("session")) {
    return "session";
  }
  return "system";
}

export function agentTranscriptSpeaker(kind: AgentTranscriptKind, event: TimelineEvent): string {
  if (kind === "user") {
    return "用户";
  }
  if (kind === "assistant") {
    return "Agent";
  }
  if (kind === "bot") {
    return "Bot";
  }
  if (kind === "tool") {
    return event.toolName ? "工具 · " + String(event.toolName) : "工具";
  }
  if (kind === "session") {
    return "Session";
  }
  return "Runtime";
}

export function agentTranscriptAvatar(kind: AgentTranscriptKind): string {
  const labels: Record<AgentTranscriptKind, string> = {
    user: "你",
    assistant: "AI",
    bot: "Bot",
    tool: "$",
    system: "i",
    session: "S",
  };
  return labels[kind];
}

function timelineInputSource(event: TimelineEvent): string {
  return String(event.metadata?.source || event.source || "").toLowerCase();
}

function timelineSemanticType(event: TimelineEvent): string {
  const stored = String(event.metadata?.semanticType || "").toLowerCase();
  if (stored) {
    return stored;
  }

  const type = String(event.type || "").toLowerCase();
  if (!type.includes("tool") && !event.toolName) {
    return "";
  }

  const toolSummary = summarizeToolTraceDisplay({
    eventType: type,
    toolName: String(event.toolName || ""),
    status: String(event.status || ""),
    payload: mergeToolTracePayloads(event.metadata, parseToolTraceDetail(event.detail), rawDetailCommandPayload(event.detail)),
    fallbackTitle: String(event.title || ""),
    fallbackSummary: String(event.summary || ""),
  });
  return String(toolSummary?.metadata?.semanticType || "").toLowerCase();
}

function rawDetailCommandPayload(detail: unknown): Record<string, string> | undefined {
  if (typeof detail !== "string") {
    return undefined;
  }
  const text = detail.trim();
  if (!text.includes("/chat/") && !text.includes("zork-call chat")) {
    return undefined;
  }
  return { command: text };
}
