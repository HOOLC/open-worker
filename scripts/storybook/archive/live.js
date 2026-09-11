/* Interactive GPUI/WASM view, using the same independent Rust zork-ui package. */
const liveSection = document.createElement("div");
liveSection.id = "live-comparison";
liveSection.hidden = true;
liveSection.innerHTML =
  '<section><div class="pane-head"><b>设计参考</b><span>原始 HTML · 可交互</span></div><div class="live-viewport" id="live-reference"></div></section><section><div class="pane-head"><b>zork-ui / GPUI Web</b><span id="live-status">可交互 · WASM</span><button id="live-reset">重置</button></div><div class="live-viewport" id="live-native"></div></section>';
$("comparison").after(liveSection);
const liveFrame = document.createElement("iframe");
liveFrame.title = "同源 Rust 组件 · GPUI Web";
liveFrame.allow = "clipboard-read; clipboard-write";
const referenceFrame = document.createElement("iframe");
referenceFrame.title = "原始 HTML 设计组件";
let referenceLoaded = null,
  referenceBounds = null,
  referenceZoom = 1;
function positionReference() {
  if (!referenceBounds) return;
  const b = referenceBounds,
    z = referenceZoom;
  referenceFrame.style.transform = `translate(${(24 - b.x) * z}px,${(24 - b.y) * z}px) scale(${z})`;
  const panel = $("live-reference");
  panel.style.height = Math.max(parseFloat(panel.style.height) || 0, (b.height + 48) * z) + "px";
}
window.addEventListener("message", (e) => {
  if (e.origin === location.origin && e.source === referenceFrame.contentWindow && e.data?.type === "zork-reference-bounds" && e.data.id === selected.id) {
    referenceBounds = e.data.bounds;
    positionReference();
  }
});
function renderReference(s, z) {
  const panel = $("live-reference");
  referenceZoom = z;
  if (s.design?.status !== "captured") {
    panel.replaceChildren();
    referenceLoaded = null;
    const p = document.createElement("p");
    p.className = "missing";
    p.textContent = s.design?.reason || "此状态尚无 HTML 设计";
    panel.append(p);
    return;
  }
  if (referenceFrame.parentNode !== panel) panel.replaceChildren(referenceFrame);
  referenceFrame.width = s.width >= 900 ? s.width : 1280;
  referenceFrame.height = s.height >= 600 ? s.height : 800;
  referenceFrame.style.width = referenceFrame.width + "px";
  referenceFrame.style.height = referenceFrame.height + "px";
  if (referenceLoaded !== s.id) {
    referenceLoaded = s.id;
    referenceBounds = s.design.bounds;
    positionReference();
    referenceFrame.onload = () => {
      const doc = referenceFrame.contentDocument;
      const script = doc.createElement("script");
      script.src = new URL("reference.js", location.href).href;
      script.onload = () =>
        referenceFrame.contentWindow.prepareZorkReference(s).catch((error) => {
          text("notice", "HTML 设计加载失败：" + error.message);
          $("notice").hidden = false;
        });
      doc.head.append(script);
    };
    referenceFrame.src = "/gui.html?component=" + encodeURIComponent(s.id);
  } else positionReference();
}
let webReady = false,
  requested = null,
  loaded = null;
const originalDraw = draw;
function liveScale(w) {
  if ($("zoom").value !== "fit") return Number($("zoom").value);
  return Math.min(1, Math.max(240, $("live-native").clientWidth) / (w + 48));
}
function requestedId() {
  return isBasic() ? "family-" + selectedFamily : selected.id;
}
function requestStory() {
  const id = requestedId();
  if (webReady && selected.family !== "conversation" && loaded !== id) {
    loaded = id;
    liveFrame.contentWindow.postMessage({ type: "zork-select-story", id }, location.origin);
  }
}
window.addEventListener("message", (e) => {
  if (e.origin !== location.origin || e.source !== liveFrame.contentWindow) return;
  if (e.data?.type === "zork-story-ready") {
    webReady = true;
    loaded = e.data.id;
    text("live-status", "可交互 · WASM");
    requestStory();
  }
  if (e.data?.type === "zork-story-error") text("live-status", "加载失败：" + e.data.message);
});
$("live-reset").onclick = () => {
  loaded = null;
  requestStory();
};
draw = function () {
  if (mode !== "live") {
    liveSection.hidden = true;
    originalDraw();
    return;
  }
  $("comparison").hidden = true;
  $("overlay-pane").hidden = true;
  $("opacity-control").hidden = true;
  liveSection.hidden = false;
  const s = selected,
    ref = $("live-reference"),
    native = $("live-native");
  liveSection.classList.toggle("primitive", isBasic());
  if (isBasic()) {
    referenceFrame.remove();
    referenceLoaded = null;
    referenceBounds = null;
    ref.replaceChildren();
    liveFrame.hidden = false;
    [...native.children].filter((e) => e !== liveFrame).forEach((e) => e.remove());
    const width = Math.max(320, native.clientWidth),
      height = Math.max(520, window.innerHeight - 280);
    native.style.height = height + "px";
    liveFrame.width = width;
    liveFrame.height = height;
    liveFrame.style.width = width + "px";
    liveFrame.style.height = height + "px";
    liveFrame.style.transform = "none";
    if (liveFrame.parentNode !== native) native.replaceChildren(liveFrame);
    if (!requested) {
      requested = requestedId();
      liveFrame.src = "web/index.html?story=" + encodeURIComponent(requested);
      text("live-status", "加载 GPUI…");
    } else {
      requestStory();
      if (webReady) text("live-status", "全部状态 · 可交互");
    }
    return;
  }

  if (s.family === "conversation") {
    liveFrame.hidden = true;
    [...native.children].filter((e) => e !== liveFrame).forEach((e) => e.remove());
    native.style.height = "auto";
    ref.style.height = "auto";
    native.append(img(s.native.image, s.native.bounds.width * liveScale(s.native.bounds.width)));
    const note = document.createElement("p");
    note.className = "missing";
    note.textContent = "整页会话仍由桌面业务容器装配；这里保留原生快照。基础控件和设置组件可切换到 Web 交互。";
    native.prepend(note);
    renderReference(s, liveScale(s.design?.bounds?.width || s.native.bounds.width));
    text("live-status", "整页原生对照");
    return;
  }
  const b = s.native.bounds,
    d = s.design?.bounds,
    w = Math.max(b.width, d?.width || 0),
    h = Math.max(b.height, d?.height || 0, s.family === "dropdown" ? 220 : 50);
  const z = liveScale(w);
  liveFrame.hidden = false;
  [...native.children].filter((e) => e !== liveFrame).forEach((e) => e.remove());
  for (const panel of [ref, native]) {
    panel.style.height = (h + 48) * z + "px";
    panel.style.minHeight = "180px";
  }
  renderReference(s, z);
  liveFrame.width = s.width;
  liveFrame.height = s.height;
  liveFrame.style.width = s.width + "px";
  liveFrame.style.height = s.height + "px";
  liveFrame.style.transform = `translate(${(24 - b.x) * z}px,${(24 - b.y) * z}px) scale(${z})`;
  if (!liveFrame.parentNode || liveFrame.parentNode !== native) {
    native.replaceChildren(liveFrame);
  }
  if (!requested) {
    requested = s.id;
    liveFrame.src = "web/index.html?story=" + encodeURIComponent(s.id);
    text("live-status", "加载 GPUI…");
  } else {
    requestStory();
    if (webReady) text("live-status", "可交互 · WASM");
  }
};
