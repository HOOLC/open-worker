import { Badge } from "./admin-badge.js";

import { fmtDateTime, fmtTime, operationLabel, pickOperationLabel, statusTone } from "./admin-formatters.js";

import { AdminStatus } from "./admin-types.js";

import { statusLabel } from "./timeline-display";

import React from "react";

export function OperationRecords({ status }: { readonly status: AdminStatus }): React.JSX.Element {
  const operations = Array.isArray(status.operations) ? status.operations : [];
  const events = Array.isArray(status.auditEvents) ? status.auditEvents : [];
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">操作记录</div>
        <span className="badge purple">审计</span>
      </div>
      <div className="panel-body">
        <div className="operation-list">
          {operations.length ? (
            operations.slice(0, 5).map((operation: Record<string, any>) => (
              <div className="operation-row" key={operation.id || `${operation.kind}-${operation.updatedAt}`}>
                <Badge label={operation.status || "unknown"} tone={statusTone(operation.status)} />
                <div className="operation-main">
                  <div className="operation-title">{operationLabel(operation.kind)}</div>
                  <div className="operation-detail">{pickOperationLabel(operation)}</div>
                </div>
                <div className="summary-detail">{fmtTime(operation.updatedAt)}</div>
              </div>
            ))
          ) : (
            <div className="empty-state">暂无管理操作</div>
          )}
        </div>
        <div className="audit-list">
          {events.slice(0, 6).map((event: Record<string, any>) => (
            <div key={event.id || `${event.action}-${event.createdAt}`}>
              {fmtTime(event.createdAt)} · {operationLabel(event.action)} · {statusLabel(event.status)}
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

export function LogsPanel({ logs }: { readonly logs: readonly Record<string, any>[] }): React.JSX.Element {
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">系统日志</div>
      </div>
      <div className="log-list">
        {logs.length ? (
          logs.slice(0, 10).map((entry, index) => (
            <div className={"log-entry " + statusTone(entry.level)} key={`${entry.ts || index}-${entry.message || entry.raw || ""}`}>
              <span>{fmtTime(entry.ts)}</span>
              <span>{entry.message || entry.raw || ""}</span>
            </div>
          ))
        ) : (
          <div className="empty-state">暂无日志</div>
        )}
      </div>
    </section>
  );
}

export function ServicePanel({ service }: { readonly service: Record<string, any> }): React.JSX.Element {
  return (
    <section className="panel ops-panel">
      <div className="panel-head">
        <div className="panel-title">运行信息</div>
      </div>
      <div className="panel-body summary-detail" style={{ display: "grid", gap: 6 }}>
        <div>名称：{service.name || "--"}</div>
        <div>模式：{statusLabel(service.mode || "--")}</div>
        <div>端口：{service.port || "--"}</div>
        <div>启动：{fmtDateTime(service.startedAt)}</div>
        <div style={{ wordBreak: "break-all" }}>会话目录：{service.sessionsRoot || "--"}</div>
        <div style={{ wordBreak: "break-all" }}>DATA_ROOT: {service.dataRoot || "--"}</div>
      </div>
    </section>
  );
}
