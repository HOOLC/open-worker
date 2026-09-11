let data,
  selected,
  selectedFamily,
  mode = "live";
const $ = (id) => document.getElementById(id),
  text = (id, value) => ($(id).textContent = value);
const pageFamilies = new Set(["connection", "model", "agent", "conversation"]);
const pageTitles = { connection: "模型连接", model: "模型管理", agent: "Agent 设置", conversation: "对话页面" };
const sceneTitles = { list: "列表", create: "添加", provider: "供应商选择", detail: "详情", protocol: "协议选择", edit: "编辑", dropdown: "模型选择", messages: "消息", composer: "输入与评论", history: "历史记录" };
const size = (b) => (b ? `${Math.round(b.width)} × ${Math.round(b.height)} px` : "");
const isBasic = () => !pageFamilies.has(selectedFamily);
const sceneOf = (s) => s.state.replace(/-(compact|wide)$/, "");
const remembered = new Map();
function families() {
  return [...new Set(data.stories.map((s) => s.family))];
}
function list() {
  const query = $("search").value.toLowerCase();
  $("stories").replaceChildren();
  for (const basic of [true, false]) {
    const items = families()
      .filter((f) => !pageFamilies.has(f) === basic)
      .filter((f) => {
        const s = data.stories.find((s) => s.family === f);
        return (f + " " + s.title).toLowerCase().includes(query);
      });
    if (!items.length) continue;
    const heading = document.createElement("div");
    heading.className = "group-label";
    heading.textContent = basic ? "基础组件" : "页面示例";
    $("stories").append(heading);
    for (const family of items) {
      const stories = data.stories.filter((s) => s.family === family),
        b = document.createElement("button");
      b.className = "story";
      b.dataset.id = family;
      b.id = "tab-" + family;
      b.setAttribute("role", "tab");
      b.setAttribute("aria-controls", "component-panel");
      b.tabIndex = family === selectedFamily ? 0 : -1;
      b.setAttribute("aria-selected", String(family === selectedFamily));
      b.setAttribute("aria-current", String(family === selectedFamily));
      b.textContent = pageTitles[family] || stories[0].title;
      b.onclick = () => {
        select(family);
        $("tab-" + family)?.focus({ preventScroll: true });
      };
      b.onkeydown = (e) => {
        if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(e.key)) return;
        e.preventDefault();
        const tabs = [...$("stories").querySelectorAll("[role=tab]")],
          i = tabs.indexOf(b),
          next = e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : (i + (e.key === "ArrowDown" ? 1 : -1) + tabs.length) % tabs.length;
        tabs[next].click();
      };
      $("stories").append(b);
    }
  }
}
function select(id) {
  const parts = id.split("/"),
    legacy = data.stories.find((s) => s.id === id);
  const family = families().includes(parts[0]) ? parts[0] : legacy?.family || "button";
  selectedFamily = family;
  $("component-panel").setAttribute("aria-labelledby", "tab-" + family);
  const stories = data.stories.filter((s) => s.family === family);
  selected = legacy || remembered.get(family) || stories[0];
  if (pageFamilies.has(family)) {
    const scenes = [...new Set(stories.map(sceneOf))];
    $("scene").replaceChildren(...scenes.map((s) => new Option(sceneTitles[s] || s, s)));
    const scene = parts[1] || sceneOf(selected),
      dimension = parts[2] || (selected.state.endsWith("-wide") ? "wide" : "compact");
    selected = stories.find((s) => sceneOf(s) === scene && s.state.endsWith("-" + dimension)) || selected;
    $("scene").value = sceneOf(selected);
    $("page-size").value = selected.state.endsWith("-wide") ? "wide" : "compact";
    remembered.set(family, selected);
  }
  history.replaceState(null, "", "#" + family + (isBasic() ? "" : "/" + sceneOf(selected) + "/" + $("page-size").value));
  text("family", isBasic() ? "基础组件 / ZORK UI" : "页面示例 / ZORK UI");
  text("title", pageTitles[family] || selected.title);
  text("state", isBasic() ? `${stories.length} 个状态 · 直接使用共享 GPUI 组件` : "同一页面的状态与窗口尺寸，在这里切换");
  $("page-controls").hidden = isBasic();
  $("toolbar").hidden = isBasic();
  $("evidence").hidden = isBasic();
  if (isBasic()) mode = "live";
  document.querySelectorAll("[data-mode]").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.mode === mode)));
  text("reference-size", size(selected.design?.bounds));
  text("native-size", size(selected.native.bounds));
  text("source", selected.source);
  text("design-source", selected.design?.source || selected.design?.reason || "待补设计");
  text("checks", (selected.checks || []).join(" · "));
  $("full-native").href = selected.native.window;
  $("full-reference").hidden = selected.design?.status !== "captured";
  $("full-reference").href = selected.design?.window || "#";
  $("notice").hidden = isBasic() || selected.design?.status === "captured";
  text("notice", selected.design?.reason || "该页面状态尚未定义 HTML 参考。");
  list();
  draw();
}
function selectScene() {
  select(selectedFamily + "/" + $("scene").value + "/" + $("page-size").value);
}
function scale() {
  if ($("zoom").value !== "fit") return Number($("zoom").value);
  const w = Math.max(selected.native.bounds.width, selected.design?.bounds?.width || 0);
  const available = mode === "side" ? Math.max(240, ($("comparison").clientWidth - 18) / 2 - 28) : $("overlay-pane").clientWidth - 28;
  return Math.min(1, available / w);
}
function img(src, width) {
  const i = document.createElement("img");
  i.src = src;
  i.alt = "页面快照";
  i.style.width = width + "px";
  return i;
}
async function draw() {
  const s = selected;
  if (!s) return;
  const ref = s.design?.status === "captured";
  $("comparison").hidden = mode !== "side";
  $("overlay-pane").hidden = mode === "side";
  $("opacity-control").hidden = mode !== "overlay";
  const z = scale();
  $("reference-pane").replaceChildren();
  $("native-pane").replaceChildren(img(s.native.image, s.native.bounds.width * z));
  if (ref) $("reference-pane").append(img(s.design.image, s.design.bounds.width * z));
  else $("reference-pane").textContent = s.design?.reason || "待补设计";
  $("overlay-pane").replaceChildren();
  if (mode === "side") return;
  if (!ref) {
    $("overlay-pane").textContent = "此状态尚无 HTML 设计，暂不进行像素比较。";
    return;
  }
  const width = Math.ceil(Math.max(s.native.bounds.width, s.design.bounds.width)),
    height = Math.ceil(Math.max(s.native.bounds.height, s.design.bounds.height));
  if (mode === "overlay") {
    const stage = document.createElement("div");
    stage.className = "overlay-stage";
    stage.style.width = width * z + "px";
    stage.style.height = height * z + "px";
    const a = img(s.design.image, s.design.bounds.width * z),
      b = img(s.native.image, s.native.bounds.width * z);
    b.style.opacity = $("opacity").value;
    stage.append(a, b);
    $("overlay-pane").append(stage);
  } else {
    const [a, b] = await Promise.all(
      [s.design.image, s.native.image].map(
        (src) =>
          new Promise((resolve, reject) => {
            const i = new Image();
            i.onload = () => resolve(i);
            i.onerror = reject;
            i.src = src;
          }),
      ),
    );
    if (selected !== s || mode !== "diff") return;
    const make = () => {
      const c = document.createElement("canvas");
      c.width = width;
      c.height = height;
      const x = c.getContext("2d");
      x.fillStyle = "white";
      x.fillRect(0, 0, width, height);
      return [c, x];
    };
    const [ca, xa] = make(),
      [cb, xb] = make();
    xa.drawImage(a, 0, 0, s.design.bounds.width, s.design.bounds.height);
    xb.drawImage(b, 0, 0, s.native.bounds.width, s.native.bounds.height);
    const pa = xa.getImageData(0, 0, width, height),
      pb = xb.getImageData(0, 0, width, height);
    for (let i = 0; i < pa.data.length; i += 4) {
      const d = Math.max(...[0, 1, 2].map((j) => Math.abs(pa.data[i + j] - pb.data[i + j])));
      pa.data.set([...(d > 30 ? [200, 58, 50] : [245, 245, 242]), 255], i);
    }
    xa.putImageData(pa, 0, 0);
    ca.style.width = width * z + "px";
    ca.style.height = height * z + "px";
    $("overlay-pane").append(ca);
  }
}
fetch("manifest.json", { cache: "no-store" })
  .then((r) => r.json())
  .then((d) => {
    data = d;
    text("summary", `${families().filter((f) => !pageFamilies.has(f)).length} 个基础组件 · ${pageFamilies.size} 个页面示例`);
    select(location.hash.slice(1));
  });
$("search").oninput = list;
$("scene").onchange = selectScene;
$("page-size").onchange = selectScene;
$("zoom").onchange = () => draw();
$("opacity").oninput = () => draw();
document.querySelectorAll("[data-mode]").forEach(
  (b) =>
    (b.onclick = () => {
      mode = b.dataset.mode;
      document.querySelectorAll("[data-mode]").forEach((x) => x.setAttribute("aria-pressed", String(x === b)));
      draw();
    }),
);
window.onresize = () => draw();
