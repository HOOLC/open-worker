import { roundedPath } from "./geometry";
import { useEffect, useRef, useState } from "react";
import { asset } from "../content/assets";
import type { Catalog, Rect, Story } from "./types";
export function HtmlReference({
  story,
  catalog,
  bounds,
  scale,
  onBounds,
}: {
  story: Story;
  catalog: Catalog;
  bounds: Rect | null;
  scale: number;
  onBounds: (rect: Rect) => void;
}) {
  const frame = useRef<HTMLIFrameElement>(null),
    [error, setError] = useState(""),
    [clip, setClip] = useState<{ origin: Rect; areas: Array<{ rect: Rect; radius: number }> } | null>(null),
    callback = useRef(onBounds);
  callback.current = onBounds;
  useEffect(() => {
    const listener = (event: MessageEvent) => {
      if (
        event.origin !== location.origin ||
        event.source !== frame.current?.contentWindow ||
        event.data?.id !== story.id
      )
        return;
      if (event.data.type === "zork-reference-bounds") {
        callback.current(event.data.bounds as Rect);
      }
      if (event.data.type === "zork-reference-error") setError(String(event.data.message));
    };
    window.addEventListener("message", listener);
    return () => window.removeEventListener("message", listener);
  }, [story.id]);
  const load = () => {
    setError("");
  };
  useEffect(() => {
    const timer = setInterval(() => {
      try {
        const child = frame.current?.contentWindow;
        const state = child?.zorkReference?.snapshot();
        if (state?.id === story.id) {
          const next =
            state.surface === "dialog"
              ? {
                  origin: state.bounds,
                  areas: [
                    { rect: state.surfaceBounds ?? state.bounds, radius: 32 },
                    ...(state.regions ?? []).map((rect) => ({ rect, radius: 20 })),
                  ],
                }
              : null;
          setClip((old) => (JSON.stringify(old) === JSON.stringify(next) ? old : next));
          callback.current(state.bounds);
        }
      } catch (cause) {
        setError(String(cause));
      }
    }, 120);
    return () => clearInterval(timer);
  }, [story.id]);
  if (story.design.status !== "captured")
    return <div className="missing-reference">{story.design.reason || "此页面状态尚未定义 HTML 参考。"}</div>;
  return (
    <div
      className="reference-canvas-host"
      style={{
        width: (bounds?.width ?? story.width) * scale,
        height: (bounds?.height ?? story.height) * scale,
        clipPath: clip
          ? `path("${clip.areas.map((area) => roundedPath(area.rect, clip.origin, scale, area.radius)).join(" ")}")`
          : undefined,
      }}
    >
      <iframe
        key={story.id}
        ref={frame}
        title="HTML 页面设计"
        src={asset(`reference.html?isolate=1&story=${story.id}`)}
        onLoad={load}
        width={story.width}
        height={story.height}
        style={{
          width: story.width,
          height: story.height,
          transform: `translate(${-(bounds?.x ?? 0) * scale}px,${-(bounds?.y ?? 0) * scale}px) scale(${scale})`,
        }}
      />
      {!bounds && <div className="canvas-message">正在准备设计参考…</div>}
      {error && (
        <div role="alert" className="canvas-error">
          {error}
        </div>
      )}
    </div>
  );
}
