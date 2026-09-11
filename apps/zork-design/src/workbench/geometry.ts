import type { Rect } from "./types";
export function roundedPath(rect: Rect, origin: Rect, scale: number, radius: number) {
  const x = (rect.x - origin.x) * scale,
    y = (rect.y - origin.y) * scale,
    w = rect.width * scale,
    h = rect.height * scale,
    r = Math.min(radius * scale, w / 2, h / 2);
  return `M ${x + r} ${y} H ${x + w - r} A ${r} ${r} 0 0 1 ${x + w} ${y + r} V ${y + h - r} A ${r} ${r} 0 0 1 ${x + w - r} ${y + h} H ${x + r} A ${r} ${r} 0 0 1 ${x} ${y + h - r} V ${y + r} A ${r} ${r} 0 0 1 ${x + r} ${y} Z`;
}
