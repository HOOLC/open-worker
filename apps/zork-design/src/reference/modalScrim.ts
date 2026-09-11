// Keep the backdrop alive through exit and reuse its displayed opacity on reversal.
type Scrim = { element: HTMLDivElement; users: number; animation?: Animation };
let active: Scrim | undefined;
function animate(scrim: Scrim, target: number) {
  const current = Number(getComputedStyle(scrim.element).opacity);
  scrim.animation?.cancel();
  scrim.element.style.opacity = String(target);
  const duration = matchMedia("(prefers-reduced-motion: reduce)").matches
    ? 0
    : (target ? 220 : 180) * Math.abs(target - current);
  const animation = scrim.element.animate([{ opacity: current }, { opacity: target }], {
    duration,
    easing: "cubic-bezier(.2,0,0,1)",
  });
  scrim.animation = animation;
  animation.finished
    .then(() => {
      if (scrim.animation !== animation) return;
      animation.cancel();
      if (!scrim.users) {
        scrim.element.remove();
        if (active === scrim) active = undefined;
      }
    })
    .catch(() => {});
}
export function mountModalScrim() {
  if (!active) {
    const element = document.createElement("div");
    element.className = "ref-modal-scrim";
    Object.assign(element.style, {
      position: "fixed",
      inset: "0",
      zIndex: "2147483646",
      pointerEvents: "none",
      backgroundColor: "rgba(0,0,0,.55)",
      opacity: "0",
      willChange: "opacity",
    });
    document.body.appendChild(element);
    active = { element, users: 0 };
  }
  const scrim = active;
  if (++scrim.users === 1) animate(scrim, 1);
  return () => {
    if (--scrim.users === 0) animate(scrim, 0);
  };
}
