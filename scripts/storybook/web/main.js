import init, * as wasm from "./pkg/zork_gui_web.js";
const initial = new URLSearchParams(location.search).get("story") || "button-primary";
const loading = document.getElementById("loading");
let current = initial;
let failed = false;
const reportFailure = (error) => {
  if (failed) return;
  failed = true;
  const message = error?.message || String(error);
  loading.hidden = false;
  loading.textContent = "GPUI 示例加载失败：" + message;
  document.documentElement.dataset.error = message;
  parent.postMessage({ type: "zork-story-error", message }, location.origin);
  window.requestAnimationFrame = () => 0;
};
window.addEventListener("error", (event) => reportFailure(event.error || event.message));
const ready = () => {
  let state;
  try {
    state = JSON.parse(wasm.snapshot());
  } catch {}
  return state && state.elements.length > 0;
};
// Dim the canvas backdrop through the browser compositor, keeping the dialog white.
const modalBackdrops = new Map();
const retiringBackdrops = new Map();
function animateScrim(layer, target, onComplete) {
  const current = Number(getComputedStyle(layer).opacity);
  layer.getAnimations().forEach((animation) => animation.cancel());
  layer.style.opacity = String(target);
  const duration = matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : (target ? 220 : 180) * Math.abs(target - current);
  const animation = layer.animate([{ opacity: current }, { opacity: target }], { duration, easing: "cubic-bezier(.2,0,0,1)" });
  animation.finished
    .then(() => {
      animation.cancel();
      onComplete?.();
    })
    .catch(() => {});
}
window.zorkModalBackdrop = (payload) => {
  const data = JSON.parse(payload);
  const key = data.id.replace(/-\d+$/, "");
  if (data.remove) {
    const layer = modalBackdrops.get(data.id);
    if (!layer) return;
    modalBackdrops.delete(data.id);
    retiringBackdrops.set(key, layer);
    layer.style.maskImage = layer.style.webkitMaskImage = "none";
    animateScrim(layer, 0, () => {
      if (retiringBackdrops.get(key) === layer) {
        retiringBackdrops.delete(key);
        layer.remove();
      }
    });
    return;
  }
  let layer = modalBackdrops.get(data.id);
  const arriving = !layer;
  if (!layer) {
    layer = retiringBackdrops.get(key);
    retiringBackdrops.delete(key);
    if (!layer) {
      layer = document.createElement("div");
      Object.assign(layer.style, { position: "fixed", inset: "0", zIndex: "3", pointerEvents: "none", backgroundColor: "rgba(0,0,0,0.35)", opacity: "0", willChange: "opacity" });
      document.body.appendChild(layer);
    }
    layer.dataset.zorkModalBlur = data.id;
    modalBackdrops.set(data.id, layer);
  }
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${innerWidth}" height="${innerHeight}"><defs><mask id="hole"><rect width="100%" height="100%" fill="white"/><rect x="${data.x}" y="${data.y}" width="${data.width}" height="${data.height}" rx="${data.radius}" fill="black"/></mask></defs><rect width="100%" height="100%" fill="white" mask="url(#hole)"/></svg>`;
  const mask = `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
  layer.style.maskImage = mask;
  layer.style.webkitMaskImage = mask;
  if (arriving) animateScrim(layer, 1);
};

try {
  const response = await fetch("./pkg/zork_gui_web_bg.wasm.gz");
  if (!response.ok) throw Error(`WASM download ${response.status}`);
  const payload = await response.arrayBuffer();
  const signature = new Uint8Array(payload, 0, Math.min(2, payload.byteLength));
  // Vite serves .gz with Content-Encoding, so fetch may already have decoded it.
  const bytes = signature[0] === 0x1f && signature[1] === 0x8b ? await new Response(new Blob([payload]).stream().pipeThrough(new DecompressionStream("gzip"))).arrayBuffer() : payload;
  await init({ module_or_path: bytes });
  window.zorkStory = wasm;
  const motionPreference = matchMedia("(prefers-reduced-motion: reduce)");
  wasm.set_reduce_motion(motionPreference.matches);
  wasm.start(initial);
  motionPreference.addEventListener("change", (event) => wasm.set_reduce_motion(event.matches));

  let initialSelectionApplied = false;
  const wait = () => {
    if (failed) return;
    if (ready()) {
      if (!initialSelectionApplied) {
        // Replay only after the first layout has usable hit-test bounds.
        if (initial.startsWith("family-")) wasm.select_family(initial.slice(7));
        else wasm.select_story(initial);
        initialSelectionApplied = true;
        requestAnimationFrame(wait);
        return;
      }
      loading.hidden = true;
      document.documentElement.dataset.ready = "true";
      parent.postMessage({ type: "zork-story-ready", id: current }, location.origin);
    } else requestAnimationFrame(wait);
  };
  requestAnimationFrame(wait);
  window.addEventListener("message", (e) => {
    if (e.origin !== location.origin || e.source !== parent) return;
    if (e.data?.type === "zork-select-story") {
      current = e.data.id;
      const selected = current;
      // Allow iframe resize and the GPUI geometry recorder to finish before replaying input.
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          if (selected !== current || failed) return;
          if (selected.startsWith("family-")) wasm.select_family(selected.slice(7));
          else wasm.select_story(selected);
          parent.postMessage({ type: "zork-story-selected", id: selected }, location.origin);
        }),
      );
    }
  });
} catch (error) {
  reportFailure(error);
}

setTimeout(() => {
  if (!failed && !ready()) {
    loading.hidden = false;
    loading.textContent = "GPUI 尚未就绪，请查看浏览器图形后端支持。";
    parent.postMessage({ type: "zork-story-error", message: "图形后端未就绪" }, location.origin);
  }
}, 30000);
