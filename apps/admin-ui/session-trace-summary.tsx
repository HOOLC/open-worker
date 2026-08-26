import { classSafeValue, statusTone } from "./session-formatters.js";

import React from "react";

export function TraceSummary({ trace }: { readonly trace: Record<string, any> }): React.JSX.Element {
  const categories = trace.categories || {};
  const eventCount = Number(trace.eventCount || 0);
  const items = [
    ["agent_system_prompt", "系统"],
    ["agent_memory", "记忆"],
    ["agent_user_message", "用户"],
    ["agent_runtime_reminder", "提醒"],
    ["agent_assistant_message", "助手"],
    ["agent_tool_call", "工具"],
  ];
  const summary = [
    ["agent_user_message", "用户"],
    ["agent_assistant_message", "助手"],
    ["agent_tool_call", "工具"],
  ]
    .map(([key, label]) => label + " " + Number(categories[key] || 0))
    .join(" · ");
  return (
    <details className="side-disclosure">
      <summary title={summary}>{summary}</summary>
      <div className="trace-stat-panel">
        <div className="trace-stat-head">
          <strong>{eventCount}</strong>
          <span>条 Agent 事件</span>
        </div>
        <div className="trace-stat-grid">
          {items.map(([key, label]) => (
            <div key={key} className={"trace-stat " + classSafeValue(statusTone(key), "")}>
              <span>{label}</span>
              <strong>{Number(categories[key] || 0)}</strong>
            </div>
          ))}
        </div>
      </div>
    </details>
  );
}
