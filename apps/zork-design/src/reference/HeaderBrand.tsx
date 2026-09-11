import { useEffect, useRef } from "react";
import { asset } from "../content/assets";
let source: Promise<string> | undefined;

/** Scrub the approved SVG timeline in either direction, keeping its current frame. */
export function HeaderBrand() {
  const host = useRef<HTMLDivElement>(null);
  const direction = useRef(0);
  const position = useRef(2);
  const frame = useRef(0);
  const svg = useRef<SVGSVGElement | null>(null);
  const reduced = useRef(false);
  useEffect(() => {
    let disposed = false;
    const preference = matchMedia("(prefers-reduced-motion: reduce)");
    reduced.current = preference.matches;
    const change = () => {
      reduced.current = preference.matches;
    };
    preference.addEventListener("change", change);
    source ??= fetch(asset("motion/icon-to-wordmark.svg")).then((r) => {
      if (!r.ok) throw new Error("Brand SVG unavailable");
      return r.text();
    });
    source
      .then((text) => {
        if (disposed || !host.current) return;
        const element = new DOMParser().parseFromString(text, "image/svg+xml")
          .documentElement as unknown as SVGSVGElement;
        element.setAttribute("aria-label", "Zork");
        element.style.visibility = "hidden";
        host.current.replaceChildren(element);
        element.pauseAnimations();
        element.setCurrentTime(position.current);
        element.style.visibility = "visible";
        svg.current = element;
      })
      .catch(() => {
        source = undefined;
      });
    return () => {
      disposed = true;
      cancelAnimationFrame(frame.current);
      preference.removeEventListener("change", change);
    };
  }, []);
  const play = (next: number) => {
    cancelAnimationFrame(frame.current);
    direction.current = next;
    if (reduced.current) {
      position.current = next < 0 ? 0 : 2;
      svg.current?.setCurrentTime(position.current);
      return;
    }
    let previous = performance.now();
    const tick = (now: number) => {
      position.current = Math.max(
        0,
        Math.min(2, position.current + (direction.current * Math.max(0, now - previous)) / 1000),
      );
      previous = now;
      svg.current?.setCurrentTime(position.current);
      if (host.current) host.current.dataset.progress = String(position.current / 2);
      if (direction.current < 0 ? position.current > 0 : position.current < 2)
        frame.current = requestAnimationFrame(tick);
    };
    frame.current = requestAnimationFrame(tick);
  };
  return (
    <div className="ref-header-brand" ref={host} onPointerEnter={() => play(-1)} onPointerLeave={() => play(1)}>
      <img src={asset("assets/brand/zork-wordmark-draft.svg")} alt="Zork" />
    </div>
  );
}
