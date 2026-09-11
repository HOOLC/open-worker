import { roundedPath } from "./geometry";
import { useCallback, useEffect, useRef, useState } from "react";
import { asset } from "../content/assets";
import type { GpuiWindow, Rect, Snapshot, Story, WasmState } from "./types";
interface Props {
  desired: string;
  story: Story;
  width: number;
  height: number;
  basic: boolean;
  hidden: boolean;
  scale: number;
  bounds: Rect | null;
  resetToken: number;
  onFrame: (bounds: Rect, state: WasmState) => void;
  onStatus: (status: "loading" | "ready" | "error") => void;
}
export function GpuiCanvas({
  desired,
  story,
  width,
  height,
  basic,
  hidden,
  scale,
  bounds,
  resetToken,
  onFrame,
  onStatus,
}: Props) {
  const frame = useRef<HTMLIFrameElement>(null),
    initial = useRef(desired),
    desiredRef = useRef(desired),
    currentStory = useRef(story),
    callback = useRef(onFrame);
  const [loaded, setLoaded] = useState(false),
    [error, setError] = useState(""),
    [generation, setGeneration] = useState(0),
    [clip, setClip] = useState<{ origin: Rect; areas: Array<{ rect: Rect; radius: number }> } | null>(null);
  useEffect(() => {
    onStatus(error ? "error" : loaded && (basic || bounds) ? "ready" : "loading");
  }, [error, loaded, basic, bounds, onStatus]);
  const lastSent = useRef(initial.current + "|0");
  const resetRef = useRef(resetToken);
  resetRef.current = resetToken;
  const lastRequestAt = useRef(0);
  const resetSeen = useRef(resetToken);
  const failed = useRef(false);
  desiredRef.current = desired;
  currentStory.current = story;
  callback.current = onFrame;
  useEffect(() => {
    function message(event: MessageEvent) {
      if (event.origin !== location.origin || event.source !== frame.current?.contentWindow) return;
      if (event.data?.type === "zork-story-ready") {
        setLoaded(true);
      }
      if (event.data?.type === "zork-story-error") {
        failed.current = true;
        setError(String(event.data.message));
      }
    }
    window.addEventListener("message", message);
    return () => window.removeEventListener("message", message);
  }, []);
  const retry = useCallback(() => {
    onStatus("loading");
    initial.current = desiredRef.current;
    lastSent.current = initial.current + "|" + resetRef.current;
    lastRequestAt.current = 0;
    failed.current = false;
    setLoaded(false);
    setError("");
    setClip(null);
    setGeneration((value) => value + 1);
  }, [onStatus]);
  useEffect(() => {
    if (resetSeen.current === resetToken) return;
    resetSeen.current = resetToken;
    if (failed.current) retry();
  }, [resetToken, retry]);
  useEffect(() => {
    if (hidden) return;
    const started = Date.now();
    let waitingSince = started;
    let waitingKey = "";
    const timer = setInterval(() => {
      if (failed.current) return;
      try {
        const child = frame.current?.contentWindow as GpuiWindow | null;
        const api = child?.zorkStory;
        const state = api ? (JSON.parse(api.story_state()) as WasmState | null) : null;
        const snapshot = state && api ? (JSON.parse(api.snapshot()) as Snapshot | null) : null;
        if (!state || !snapshot?.elements.length) {
          setLoaded(false);
          if (Date.now() - started > 45000) throw new Error("画布启动超时，请重新加载。");
          return;
        }
        // The iframe can initialize before a listener is attached, or return from a hidden tab.
        // Read its actual state instead of relying on a one-shot ready message.
        setLoaded(true);
        if (state.action_error) throw new Error(state.action_error);
        const wanted = desiredRef.current;
        const key = wanted + "|" + resetRef.current;
        if (waitingKey !== key) {
          waitingKey = key;
          waitingSince = Date.now();
        }
        if ((state.id !== wanted || lastSent.current !== key) && Date.now() - lastRequestAt.current > 750) {
          lastSent.current = key;
          lastRequestAt.current = Date.now();
          child?.postMessage({ type: "zork-select-story", id: wanted }, location.origin);
        }
        if (state.id !== wanted || state.pending_actions) {
          if (Date.now() - waitingSince > 15000) throw new Error("示例切换未完成，请重新加载。");
          return;
        }
        const dialogs = [
          "model-editor-dialog",
          "profile-detail-dialog",
          "profile-create-dialog",
          "agent-editor-dialog",
          "agent-create-dialog",
          "mesh-peer-dialog",
          "add-device-dialog",
        ];
        const active = dialogs.map((id) => snapshot.elements.find((e) => e.id === id && e.visible)).find(Boolean);
        const target =
          active ??
          snapshot.elements.find((e) => e.id === currentStory.current.target && e.visible) ??
          snapshot.elements.find((e) => e.id === "desktop-settings-column" && e.visible);
        const rect = desiredRef.current.startsWith("family-")
          ? { x: 0, y: 0, width: snapshot.viewport.width, height: snapshot.viewport.height }
          : target?.bounds;
        if (!rect && Date.now() - waitingSince > 15000) throw new Error("画布未生成可见内容，请重新加载。");
        if (rect) {
          waitingSince = Date.now();
          let crop = { ...rect };
          const menus = snapshot.elements.filter((e) => e.visible && e.id.endsWith("-menu"));
          for (const menu of menus) {
            const right = Math.max(crop.x + crop.width, menu.visible_bounds.x + menu.visible_bounds.width);
            const bottom = Math.max(crop.y + crop.height, menu.visible_bounds.y + menu.visible_bounds.height);
            crop.x = Math.min(crop.x, menu.visible_bounds.x);
            crop.y = Math.min(crop.y, menu.visible_bounds.y);
            crop.width = right - crop.x;
            crop.height = bottom - crop.y;
          }
          const nextClip = active
            ? {
                origin: crop,
                areas: [
                  { rect: active.bounds, radius: 32 },
                  ...menus.map((menu) => ({ rect: menu.visible_bounds, radius: 20 })),
                ],
              }
            : null;
          setClip((old) => (JSON.stringify(old) === JSON.stringify(nextClip) ? old : nextClip));
          callback.current(crop, state);
        }
      } catch (cause) {
        failed.current = true;
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    }, 100);
    return () => clearInterval(timer);
  }, [hidden, generation]);
  const transform = basic
    ? "none"
    : `translate(${-(bounds?.x ?? 0) * scale}px,${-(bounds?.y ?? 0) * scale}px) scale(${scale})`;
  return (
    <>
      <div
        className="gpui-canvas-host"
        hidden={hidden || Boolean(error)}
        style={
          basic
            ? { height }
            : {
                width: (bounds?.width ?? width) * scale,
                height: (bounds?.height ?? height) * scale,
                clipPath: clip
                  ? `path("${clip.areas.map((area) => roundedPath(area.rect, clip.origin, scale, area.radius)).join(" ")}")`
                  : undefined,
              }
        }
      >
        <iframe
          key={generation}
          ref={frame}
          title="共享 Rust / GPUI 组件"
          src={asset(`components/web/index.html?story=${encodeURIComponent(initial.current)}&reload=${generation}`)}
          width={width}
          height={height}
          style={{ width, height, transform }}
          allow="clipboard-read; clipboard-write"
        />
        {(!loaded || (!basic && !bounds)) && !error && <div className="canvas-message">正在加载组件…</div>}
      </div>
      {error && !hidden && (
        <div className="gpui-recovery" role="alert">
          <p>组件加载失败：{error}</p>
          <button type="button" className="quiet-button" onClick={retry}>
            重新加载组件
          </button>
        </div>
      )}
    </>
  );
}
