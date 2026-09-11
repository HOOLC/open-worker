import { useRef } from "react";
import { asset } from "../content/assets";

export function BrandMotion({ name, label }: { name: string; label: string }) {
  const object = useRef<HTMLObjectElement>(null);
  const root = () => object.current?.contentDocument?.documentElement as unknown as SVGSVGElement | undefined;
  const reset = () => {
    const svg = root();
    svg?.pauseAnimations();
    svg?.setCurrentTime(0);
  };
  const play = (explicit = false) => {
    const svg = root();
    if (!svg || (!explicit && matchMedia("(prefers-reduced-motion: reduce)").matches)) return;
    svg.setCurrentTime(0);
    svg.unpauseAnimations();
  };
  return (
    <figure onPointerEnter={() => play()} onPointerLeave={reset} onFocusCapture={() => play()} onBlurCapture={reset}>
      <button className="motion-preview" type="button" aria-label={`播放${label}`} onClick={() => play(true)}>
        <object
          ref={object}
          data={asset(`motion/${name}.svg`)}
          type="image/svg+xml"
          onLoad={reset}
          aria-hidden="true"
          tabIndex={-1}
        />
      </button>
      <figcaption>{label}</figcaption>
    </figure>
  );
}
