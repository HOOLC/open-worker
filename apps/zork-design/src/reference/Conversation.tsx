import { HeaderBrand } from "./HeaderBrand";
import { HoverDetails } from "./HoverDetails";
import { useRef, useState } from "react";
import type { Catalog, Story } from "../workbench/types";
import { asset } from "../content/assets";
import { Avatar, Button, Empty, Glyph, Notice } from "./controls";
type RecordValue = Record<string, unknown>;
const object = (value: unknown) => (value ?? {}) as RecordValue;
export interface HistoryEntry {
  id: string;
  lane: number;
  action: string;
  summary: string;
  start: number;
  end?: number;
  state: string;
  model?: string;
  input?: number;
  output?: number;
  cache?: number;
  outcome?: string;
  raw: unknown[];
}
export function historyEntries(catalog: Catalog): HistoryEntry[] {
  const entries = new Map<string, HistoryEntry>();
  let model = "";
  for (const record of catalog.fixture.history.records) {
    const event = record.event,
      kind = String(event.kind);
    if (kind === "session_created") {
      model = String(object(event.selection).model ?? "");
      continue;
    }
    let id = "",
      lane = 0,
      action = "",
      summary = "",
      start = 0,
      end: number | undefined,
      state = "";
    if (kind === "input_appended") {
      const input = object(event.input);
      id = record.event_id;
      action = "用户输入";
      summary = String(input.content);
      start = Number(input.received_at_ms);
      end = start;
      state = "received";
    } else if (kind.startsWith("step_")) {
      id = "model:" + event.step_id;
      lane = 1;
      action = "模型请求";
      start = Number(event.started_at_ms ?? 0);
      end = event.completed_at_ms
        ? Number(event.completed_at_ms)
        : event.failed_at_ms
          ? Number(event.failed_at_ms)
          : undefined;
      state = kind === "step_started" ? "running" : kind === "step_failed" ? "failed" : "succeeded";
      summary = String(event.assistant_text ?? object(event.error).message ?? "");
    } else if (kind === "tool_result") {
      const result = object(event.result),
        data = object(result.data);
      id = "tool:" + result.invocation_id;
      lane = 2;
      action = String(result.tool);
      end = Number(result.finished_at_ms);
      state = String(result.outcome);
      summary = String(data.error ?? data.content ?? data.stdout ?? "");
    }
    if (!id) continue;
    const previous = entries.get(id);
    entries.set(id, {
      id,
      lane,
      action,
      summary: previous?.summary || summary,
      start: start || previous?.start || end || 0,
      end: end ?? previous?.end,
      state,
      model: lane === 1 ? model : undefined,
      input: Number(object(event.usage).input_tokens) || previous?.input,
      output: Number(object(event.usage).output_tokens) || previous?.output,
      cache: Number(object(event.usage).cached_input_tokens) || previous?.cache,
      outcome: lane === 2 ? summary : undefined,
      raw: [...(previous?.raw ?? []), record],
    });
    if (kind === "step_completed" && Array.isArray(event.invocations)) {
      for (const raw of event.invocations) {
        const call = object(raw),
          args = object(call.arguments),
          id = "tool:" + call.invocation_id;
        entries.set(id, {
          id,
          lane: 2,
          action: String(call.tool),
          summary: String(args.path ?? args.command ?? ""),
          start: Number(call.started_at_ms),
          state: "running",
          raw: [call],
        });
      }
    }
  }
  return [...entries.values()];
}
const color = (entry: HistoryEntry) =>
  entry.state === "failed"
    ? "#b42318"
    : entry.state === "running" && entry.lane === 2
      ? "#b54708"
      : ["#175cd3", "#5925dc", "#067647"][entry.lane];
const clock = (time: number) => new Date(time).toLocaleTimeString("en-GB", { hour12: false, timeZone: "UTC" });
const duration = (ms: number) =>
  ms < 1000
    ? `${ms}ms`
    : ms < 60000
      ? `${Math.round(ms / 100) / 10}s`
      : `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`;
export function HistoryPanel({
  catalog,
  onClose = () => undefined,
  empty = false,
}: {
  catalog: Catalog;
  onClose?: () => void;
  empty?: boolean;
}) {
  const entries = empty ? [] : historyEntries(catalog),
    [expanded, setExpanded] = useState<string[]>([]),
    [selected, setSelected] = useState(""),
    [zoom, setZoom] = useState(1),
    [pan, setPan] = useState(0),
    [range, setRange] = useState<[number, number] | null>(null),
    drag = useRef<number | null>(null);
  const start = entries.length ? Math.min(...entries.map((e) => e.start)) : catalog.fixture.history.now,
    span = Math.max(1, catalog.fixture.history.now - start);
  const updateZoom = (next: number, pointer = 0.5) => {
    next = Math.max(1, Math.min(64, next));
    setPan(Math.max(0, Math.min(1 - 1 / next, pan + pointer * (1 / zoom - 1 / next))));
    setZoom(next);
    setRange(null);
  };
  const fraction = (event: React.PointerEvent<HTMLDivElement>) => {
    const r = event.currentTarget.getBoundingClientRect();
    return Math.max(0, Math.min(1, (event.clientX - r.x - 44) / (r.width - 44))) / zoom + pan;
  };
  return (
    <aside className="ref-history">
      <header>
        <span>执行历史 · {catalog.fixture.agents[0].name}</span>
        <button className="ref-close" aria-label="关闭执行历史" onClick={onClose}>
          <Glyph name="x" size={14} />
        </button>
      </header>
      <div className="ref-history-toolbar">
        <b>时序图</b>
        <small>真实时间</small>
        <button onClick={() => updateZoom(zoom * 1.5)}>放大</button>
        <button disabled={zoom === 1} onClick={() => updateZoom(zoom / 1.5)}>
          缩小
        </button>
        <button
          onClick={() => {
            setZoom(1);
            setPan(0);
            setRange(null);
          }}
        >
          全部
        </button>
      </div>
      <div
        className="ref-history-chart"
        onWheel={(event) => {
          const r = event.currentTarget.getBoundingClientRect();
          if (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY))
            setPan(Math.max(0, Math.min(1 - 1 / zoom, pan + (event.deltaX || event.deltaY) / (r.width - 44) / zoom)));
          else
            updateZoom(
              zoom * Math.exp(-event.deltaY * 0.006),
              Math.max(0, Math.min(1, (event.clientX - r.x - 44) / (r.width - 44))),
            );
        }}
        onPointerDown={(event) => {
          if ((event.target as HTMLElement).closest("button")) return;
          drag.current = fraction(event);
          setRange([drag.current, drag.current]);
          event.currentTarget.setPointerCapture(event.pointerId);
        }}
        onPointerMove={(event) => {
          if (drag.current !== null) setRange([drag.current, fraction(event)]);
        }}
        onPointerUp={(event) => {
          if (drag.current !== null && Math.abs(drag.current - fraction(event)) < 0.004) setRange(null);
          drag.current = null;
        }}
      >
        <div className="ref-history-axis">
          {[0, 1, 2, 3, 4].map((i) => {
            const time = Math.floor(start + span * (pan + i / 4 / zoom));
            return (
              <span key={i}>
                {clock(time)}
                {span / zoom < 10000 ? "." + String(time % 1000).padStart(3, "0") : ""}
              </span>
            );
          })}
        </div>
        {["输入", "模型", "工具"].map((label, lane) => (
          <div className="ref-history-lane" key={label}>
            <small>{label}</small>
            <div>
              {range && (
                <i
                  className="ref-history-range"
                  style={{
                    left: `${Math.max(0, (Math.min(...range) - pan) * zoom) * 100}%`,
                    width: `${Math.max(0, Math.min(1, (Math.max(...range) - pan) * zoom) - Math.max(0, (Math.min(...range) - pan) * zoom)) * 100}%`,
                  }}
                />
              )}
              {entries
                .filter((e) => e.lane === lane)
                .map((e) => (
                  <button
                    title={`${e.action} · ${e.summary}`}
                    key={e.id}
                    className={selected === e.id ? "selected" : ""}
                    onClick={() => {
                      setSelected(e.id);
                      setExpanded((old) => (old.includes(e.id) ? old : [...old, e.id]));
                    }}
                    style={{
                      left: `${((e.start - start) / span - pan) * 100 * zoom}%`,
                      width: `${Math.max(1, (((e.end ?? catalog.fixture.history.now) - e.start) / span) * 100 * zoom)}%`,
                      background: color(e) + "bf",
                    }}
                  />
                ))}
            </div>
          </div>
        ))}
      </div>
      <div className="ref-history-meta">
        <span>已加载 {entries.length} 条记录</span>
        <span>滚轮缩放 · Shift + 滚轮平移 · 拖动选区</span>
      </div>
      <div className="ref-history-columns">
        <span>行为</span>
        <span>操作内容</span>
        <span>时间</span>
      </div>
      <div className="ref-history-records">
        <div className="ref-history-start">已到 Session 开始处</div>
        {entries.length ? (
          entries.map((e) => (
            <section key={e.id} className={selected === e.id ? "selected" : ""}>
              <button
                className="ref-history-row"
                onClick={() => {
                  setSelected(e.id);
                  setExpanded((old) => (old.includes(e.id) ? old.filter((id) => id !== e.id) : [...old, e.id]));
                }}
                aria-expanded={expanded.includes(e.id)}
              >
                <div>
                  <b style={{ background: color(e) + "1a", color: color(e) }}>{e.action}</b>
                </div>
                <div>
                  {e.model && <small className="ref-history-model">{e.model}</small>}
                  {e.input && (
                    <div className="ref-history-metrics">
                      <span>输入 token {e.input}</span>
                      <span>输出 token {e.output}</span>
                      {e.cache !== undefined && <span>缓存 {Math.floor((e.cache * 100) / e.input)}%</span>}
                    </div>
                  )}
                  <p>{e.summary}</p>
                  {e.lane === 2 && (
                    <small style={{ color: color(e) }}>
                      {e.state === "failed" ? "失败" : e.state === "running" ? "执行中" : "已完成"}{" "}
                      <span style={{ color: "var(--ref-muted)" }}>{e.outcome}</span>
                    </small>
                  )}
                </div>
                <div>
                  <time>{clock(e.start)}</time>
                  {e.lane !== 0 && <span>{duration((e.end ?? catalog.fixture.history.now) - e.start)}</span>}
                </div>
              </button>
              {expanded.includes(e.id) && (
                <div className="ref-history-details">
                  <small>详情</small>
                  <p>{e.summary}</p>
                  <details>
                    <summary>事件数据</summary>
                    <pre>{JSON.stringify(e.raw, null, 2)}</pre>
                  </details>
                </div>
              )}
            </section>
          ))
        ) : (
          <Empty title="还没有执行记录">执行开始后，记录会显示在这里。</Empty>
        )}
      </div>
    </aside>
  );
}
export function Attachment({
  name = "组件规范.md",
  detail = "Markdown · 3.2 KB",
  onOpen = () => undefined,
}: {
  name?: string;
  detail?: string;
  onOpen?: () => void;
}) {
  return (
    <button className="ref-attachment" onClick={onOpen}>
      <Glyph name="file" size={24} />
      <span>
        <b>{name}</b>
        <small>{detail}</small>
      </span>
      <Glyph name="download" size={16} />
    </button>
  );
}
export function ConversationPage({
  catalog,
  story,
  onNavigate,
}: {
  catalog: Catalog;
  story: Story;
  onNavigate?: (story: string) => void;
}) {
  const fixture = catalog.fixture,
    [history, setHistory] = useState(story.state.startsWith("history")),
    [draft, setDraft] = useState(""),
    [messages, setMessages] = useState(fixture.conversation.messages),
    [comment, setComment] = useState(""),
    [comments, setComments] = useState<string[]>([]),
    [popover, setPopover] = useState(false),
    [selection, setSelection] = useState(""),
    [notice, setNotice] = useState("");
  const leader = fixture.agents[0],
    composerOnly = story.state.startsWith("composer");
  const send = () => {
    if (!draft.trim() && !comments.length) return;
    setMessages((old) => [
      ...old,
      {
        id: "sent-" + Date.now(),
        role: "user",
        who: "me",
        content: [draft, ...comments.map((text) => "评论：" + text)].filter(Boolean).join("\n"),
        time: "09:44",
        created_at: "2026-09-06T09:44:00Z",
      },
    ]);
    setDraft("");
    setComments([]);
  };
  const composer = (
    <div className="ref-composer" data-reference-target={composerOnly ? "" : undefined}>
      {comments.length > 0 && (
        <div className="ref-comment-queue">
          {comments.map((text, index) => (
            <div key={index}>
              <Glyph name="task-chat" size={14} />
              <span>{text}</span>
              <button aria-label="移除评论" onClick={() => setComments((old) => old.filter((_, i) => i !== index))}>
                ×
              </button>
            </div>
          ))}
        </div>
      )}
      <textarea
        aria-label="消息"
        rows={1}
        placeholder={fixture.conversation.placeholder}
        value={draft}
        onChange={(e) => {
          setDraft(e.target.value);
          e.currentTarget.style.height = "32px";
          e.currentTarget.style.height = Math.min(160, e.currentTarget.scrollHeight) + "px";
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            send();
          }
        }}
      />
      <div>
        <button
          className="ref-close"
          aria-label="添加附件"
          onClick={() => setNotice("设计示例：文件将附在当前对话中。")}
        >
          <Glyph name="paperclip" size={16} />
        </button>
        <span className="ref-composer-spacer" aria-hidden="true" />
        <button className="ref-send" aria-label="发送" disabled={!draft.trim() && !comments.length} onClick={send}>
          <Glyph name="arrow-up" size={16} />
        </button>
      </div>
    </div>
  );
  if (composerOnly) return <div className="ref-composer-only">{composer}</div>;
  return (
    <div className="ref-conversation-root" data-reference-target>
      <div className="ref-window">
        <aside className="ref-sidebar">
          <div className="ref-brand ref-window-brand">
            <span className="ref-traffic-lights" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            <HeaderBrand />
          </div>
          <button className="ref-nav" onClick={() => onNavigate?.("device-running-wide")}>
            <Glyph name="node" size={20} />
            {fixture.device.name}
            <span className="ref-badge" style={{ marginLeft: "auto" }}>
              在线
            </span>
          </button>
          <HoverDetails
            title={leader.name}
            kind="领队"
            avatar={leader.avatar}
            rows={[
              ["设备", fixture.device.name],
              ["连接", fixture.profile.profile_id],
              ["模型", leader.model ?? ""],
            ]}
          >
            <button className="ref-nav child selected">
              <Avatar name={leader.avatar} size={20} />
              {leader.name}
            </button>
          </HoverDetails>
          <div className="ref-sidebar-bottom">
            <button className="ref-nav" onClick={() => onNavigate?.("enrollment-start-wide")}>
              <Glyph name="plus" />
              添加设备
            </button>
            <button className="ref-nav" onClick={() => onNavigate?.("client-signed-out-wide")}>
              <Glyph name="settings" />
              设置
            </button>
          </div>
        </aside>
        <main className="ref-chat">
          <header>
            <button className="ref-participant" onClick={() => setHistory(!history)} title="查看执行历史">
              <Avatar name={leader.avatar} size={26} />
            </button>
            <span>
              <Glyph name="node" size={12} />
              {fixture.device.name}
            </span>
          </header>
          <div
            className="ref-messages"
            onMouseUp={() => {
              const text = window.getSelection()?.toString().trim();
              if (text) {
                setSelection(text);
                setPopover(true);
              }
            }}
          >
            {messages.map((message) => (
              <article key={message.id} className={message.role === "user" ? "user" : "assistant"}>
                {message.role !== "user" && <Avatar name={leader.avatar} size={26} />}
                <div>
                  {message.role !== "user" && (
                    <header>
                      <b>{leader.name}</b>
                      <small>{fixture.device.name}</small>
                      <time>{message.time}</time>
                    </header>
                  )}
                  <p>{message.content}</p>
                  {message.role === "user" && <time>{message.time}</time>}
                </div>
              </article>
            ))}
          </div>
          <div className="ref-composer-dock">
            {notice && <Notice>{notice}</Notice>}
            {composer}
          </div>
        </main>
        {history && <HistoryPanel catalog={catalog} onClose={() => setHistory(false)} />}
      </div>
      {popover && (
        <div className="ref-comment-popover">
          <b>评论所选文字</b>
          <blockquote>{selection}</blockquote>
          <textarea
            aria-label="评论"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="写下你的意见…"
          />
          <div className="ref-actions">
            <Button onClick={() => setPopover(false)}>取消</Button>
            <Button
              primary
              disabled={!comment.trim()}
              onClick={() => {
                setComments((old) => [...old, comment.trim()]);
                setComment("");
                setPopover(false);
              }}
            >
              加入评论
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}
