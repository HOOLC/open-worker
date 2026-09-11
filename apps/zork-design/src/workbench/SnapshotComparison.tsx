import { useEffect, useRef, useState } from "react";
import { asset } from "../content/assets";
import type { Mode, Story } from "./types";
const source = (path: string) => asset("components/" + path);
export function SnapshotComparison({ story, mode, scale }: { story: Story; mode: Mode; scale: number }) {
  const canvas = useRef<HTMLCanvasElement>(null),
    [opacity, setOpacity] = useState(0.5),
    [error, setError] = useState("");
  useEffect(() => {
    if (mode !== "diff" || !story.design.image) return;
    let cancelled = false;
    Promise.all(
      [story.design.image, story.native.image].map(
        (path) =>
          new Promise<HTMLImageElement>((resolve, reject) => {
            const image = new Image();
            image.onload = () => resolve(image);
            image.onerror = () => reject(new Error("快照加载失败"));
            image.src = source(path);
          }),
      ),
    )
      .then(([a, b]) => {
        if (cancelled || !canvas.current) return;
        const width = Math.ceil(Math.max(story.native.bounds.width, story.design.bounds!.width)),
          height = Math.ceil(Math.max(story.native.bounds.height, story.design.bounds!.height));
        canvas.current.width = width;
        canvas.current.height = height;
        const context = canvas.current.getContext("2d")!;
        const temporary = document.createElement("canvas");
        temporary.width = width;
        temporary.height = height;
        const second = temporary.getContext("2d")!;
        for (const c of [context, second]) {
          c.fillStyle = "#fff";
          c.fillRect(0, 0, width, height);
        }
        context.drawImage(a, 0, 0, story.design.bounds!.width, story.design.bounds!.height);
        second.drawImage(b, 0, 0, story.native.bounds.width, story.native.bounds.height);
        const left = context.getImageData(0, 0, width, height),
          right = second.getImageData(0, 0, width, height);
        for (let i = 0; i < left.data.length; i += 4) {
          const difference = Math.max(...[0, 1, 2].map((j) => Math.abs(left.data[i + j] - right.data[i + j])));
          left.data.set([...(difference > 30 ? [200, 58, 50] : [245, 245, 242]), 255], i);
        }
        context.putImageData(left, 0, 0);
      })
      .catch((cause) => setError(String(cause)));
    return () => {
      cancelled = true;
    };
  }, [story, mode]);
  if (story.design.status !== "captured") return <p className="missing-reference">此状态没有设计快照。</p>;
  if (mode === "side")
    return (
      <div className="snapshot-panes">
        <section>
          <h3>HTML 快照</h3>
          <img src={source(story.design.image!)} style={{ width: story.design.bounds!.width * scale }} alt="设计快照" />
        </section>
        <section>
          <h3>原生 GPUI 快照</h3>
          <img src={source(story.native.image)} style={{ width: story.native.bounds.width * scale }} alt="原生快照" />
        </section>
      </div>
    );
  const width = Math.max(story.native.bounds.width, story.design.bounds!.width) * scale,
    height = Math.max(story.native.bounds.height, story.design.bounds!.height) * scale;
  return (
    <section className="snapshot-stage">
      {mode === "overlay" && (
        <label>
          原生透明度
          <input
            aria-label="原生透明度"
            type="range"
            min="0"
            max="1"
            step=".05"
            value={opacity}
            onChange={(e) => setOpacity(Number(e.target.value))}
          />
        </label>
      )}
      {error && <p role="alert">{error}</p>}
      <div className="snapshot-overlay" style={{ width, height }}>
        {mode === "diff" ? (
          <canvas ref={canvas} style={{ width, height }} />
        ) : (
          <>
            <img
              src={source(story.design.image!)}
              style={{ width: story.design.bounds!.width * scale }}
              alt="设计快照"
            />
            <img
              src={source(story.native.image)}
              style={{ width: story.native.bounds.width * scale, opacity }}
              alt="原生快照"
            />
          </>
        )}
      </div>
    </section>
  );
}
