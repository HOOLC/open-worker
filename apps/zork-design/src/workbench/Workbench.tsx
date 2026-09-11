import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { asset } from "../content/assets";
import { pageFamilies, pageTitles, routeFor, sceneOf, sceneTitles, stateRoute } from "./model";
import { GpuiCanvas } from "./GpuiCanvas";
import { HtmlReference } from "./HtmlReference";
import { SnapshotComparison } from "./SnapshotComparison";
import type { Catalog, Mode, Rect, WasmState } from "./types";
import "../styles/workbench.css";
const equalRect = (a: Rect | null, b: Rect) =>
  a !== null &&
  ["x", "y", "width", "height"].every((key) => Math.abs(a[key as keyof Rect] - b[key as keyof Rect]) < 0.1);
export function Workbench({ active }: { active: boolean }) {
  const location = useLocation(),
    navigate = useNavigate(),
    lastPath = useRef("/pc/button");
  if (active) lastPath.current = location.pathname;
  const [catalog, setCatalog] = useState<Catalog | null>(null),
    [error, setError] = useState(""),
    [query, setQuery] = useState(""),
    [mode, setMode] = useState<Mode>("live"),
    [zoom, setZoom] = useState("fit"),
    [resetToken, setResetToken] = useState(0);
  const [gpuiBounds, setGpuiBounds] = useState<Rect | null>(null),
    [referenceBounds, setReferenceBounds] = useState<Rect | null>(null),
    [engineState, setEngineState] = useState<WasmState | null>(null),
    [gpuiStatus, setGpuiStatus] = useState<"loading" | "ready" | "error">("loading");
  const pane = useRef<HTMLDivElement>(null),
    root = useRef<HTMLDivElement>(null),
    [space, setSpace] = useState({ width: 900, height: 720 });
  useEffect(() => {
    fetch(asset("catalog.json"), { cache: "no-store" })
      .then((response) => {
        if (!response.ok) throw Error("组件目录加载失败");
        return response.json();
      })
      .then(setCatalog)
      .catch((cause) => setError(String(cause)));
  }, []);
  useEffect(() => {
    if (!pane.current || !root.current) return;
    const measure = () => {
      const style = getComputedStyle(pane.current!);
      setSpace({
        width: Math.max(
          320,
          pane.current!.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight),
        ),
        height: Math.max(440, root.current!.clientHeight - 180),
      });
    };
    const observer = new ResizeObserver(measure);
    observer.observe(pane.current);
    observer.observe(root.current);
    measure();
    return () => observer.disconnect();
  }, [catalog, active]);
  const route = catalog ? routeFor(catalog.stories, lastPath.current) : null;
  const selectedId = route?.selected.id;
  useEffect(() => {
    setGpuiBounds(null);
    setReferenceBounds(null);
    setEngineState(null);
  }, [selectedId, route?.family]);
  const onFrame = useCallback((rect: Rect, state: WasmState) => {
    setGpuiBounds((old) => (equalRect(old, rect) ? old : rect));
    setEngineState((old) => (JSON.stringify(old) === JSON.stringify(state) ? old : state));
  }, []);
  const onReference = useCallback((rect: Rect) => setReferenceBounds((old) => (equalRect(old, rect) ? old : rect)), []);
  if (error)
    return (
      <div className="workbench-error" role="alert">
        {error}
      </div>
    );
  if (!catalog || !route) return <div className="workbench-loading">正在读取组件目录…</div>;
  const { family, items, selected, basic } = route,
    families = [...new Set(catalog.stories.map((s) => s.family))],
    actualMode = basic ? "live" : mode;
  const scenes = [...new Set(items.map(sceneOf))],
    size = selected.state.endsWith("-wide") ? "wide" : "compact";
  const gpui = gpuiBounds ?? selected.native.bounds,
    reference = referenceBounds ?? selected.design.bounds ?? gpui;
  const scale = zoom === "fit" ? Math.min(1, space.width / Math.max(gpui.width, reference.width)) : Number(zoom);
  const desired = basic ? "family-" + family : selected.id,
    nativeOnly = family === "conversation";
  const dimensions = basic
    ? { width: space.width, height: space.height }
    : { width: selected.width, height: selected.height };
  const title = pageTitles[family] ?? selected.title;
  function keyboardTab(event: React.KeyboardEvent<HTMLAnchorElement>, index: number, visible: string[]) {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? visible.length - 1
          : (index + (event.key === "ArrowDown" ? 1 : -1) + visible.length) % visible.length;
    navigate("/pc/" + visible[next]);
    requestAnimationFrame(() => document.getElementById("component-tab-" + visible[next])?.focus());
  }
  return (
    <div className="workbench" ref={root} data-family={family} data-testid="workbench">
      <aside className="component-navigation">
        <label className="component-search">
          <span className="sr-only">搜索组件或页面</span>
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="搜索组件或页面" />
        </label>
        <p className="component-count">
          {families.filter((f) => !pageFamilies.has(f)).length} 个基础组件 ·{" "}
          {families.filter((f) => pageFamilies.has(f)).length} 个页面示例
        </p>
        <nav role="tablist" aria-orientation="vertical" aria-label="组件与页面">
          {[true, false].map((primitive) => {
            const visible = families
              .filter((f) => !pageFamilies.has(f) === primitive)
              .filter((f) => {
                const item = catalog.stories.find((s) => s.family === f)!;
                return (f + " " + item.title).toLowerCase().includes(query.toLowerCase());
              });
            return (
              <div key={String(primitive)}>
                <h2>{primitive ? "基础组件" : "页面示例"}</h2>
                {visible.map((f, index) => (
                  <Link
                    role="tab"
                    aria-selected={f === family}
                    tabIndex={f === family ? 0 : -1}
                    id={"component-tab-" + f}
                    aria-controls="component-panel"
                    data-id={f}
                    key={f}
                    to={"/pc/" + f}
                    onKeyDown={(event) => keyboardTab(event, index, visible)}
                  >
                    {pageTitles[f] ?? catalog.stories.find((s) => s.family === f)!.title}
                  </Link>
                ))}
              </div>
            );
          })}
        </nav>
      </aside>
      <section
        className="component-content"
        id="component-panel"
        role="tabpanel"
        aria-labelledby={"component-tab-" + family}
      >
        <header className="component-heading">
          <div>
            <p className="eyebrow">
              {basic ? "基础组件" : "页面示例"} · <a href={asset("examples/control-style.html")}>2026.09 控件设计</a>
            </p>
            <h1>{title}</h1>
            <p>{basic ? `${items.length} 个状态 · 同一个 GPUI 画布` : "统一示例参数，对照实际页面"}</p>
          </div>
          <button className="quiet-button" onClick={() => setResetToken((value) => value + 1)}>
            {basic ? "重置" : "重置两侧"}
          </button>
        </header>
        {!basic && (
          <>
            <div className="page-controls">
              <label>
                页面状态
                <select
                  value={sceneOf(selected)}
                  onChange={(event) => navigate(stateRoute(family, event.target.value, size))}
                >
                  {scenes.map((scene) => (
                    <option key={scene} value={scene}>
                      {sceneTitles[scene] ?? scene}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                窗口尺寸
                <select
                  value={size}
                  onChange={(event) => navigate(stateRoute(family, sceneOf(selected), event.target.value))}
                >
                  <option value="compact">900 × 600</option>
                  <option value="wide">1280 × 800</option>
                </select>
              </label>
              <span className="fixture-summary">
                {catalog.fixture.device.name} · {catalog.fixture.profile.profile_id} /{" "}
                {String(catalog.fixture.profile.models[0]?.id)}
              </span>
            </div>
            <div className="comparison-toolbar">
              <div className="mode-buttons" role="group" aria-label="对比方式">
                {(
                  [
                    ["live", "实时对照"],
                    ["side", "并排快照"],
                    ["overlay", "叠加"],
                    ["diff", "像素差异"],
                  ] as const
                ).map(([value, label]) => (
                  <button key={value} aria-pressed={mode === value} onClick={() => setMode(value)}>
                    {label}
                  </button>
                ))}
              </div>
              <label>
                缩放
                <select value={zoom} onChange={(event) => setZoom(event.target.value)}>
                  <option value="fit">适应宽度</option>
                  <option value="1">100%</option>
                  <option value=".75">75%</option>
                  <option value=".5">50%</option>
                </select>
              </label>
            </div>
          </>
        )}
        <div className={"live-panes " + (basic ? "primitive" : "pages")} hidden={actualMode !== "live"}>
          {!basic && (
            <section className="preview-pane reference-pane">
              <div className="pane-heading">
                <b>HTML 设计</b>
                <span>
                  {selected.width} × {selected.height}
                </span>
              </div>
              <div className="canvas-body">
                <HtmlReference
                  key={selected.id + "-" + resetToken}
                  story={selected}
                  catalog={catalog}
                  bounds={referenceBounds}
                  scale={scale}
                  onBounds={onReference}
                />
              </div>
            </section>
          )}
          <section className="preview-pane gpui-pane">
            <div className="pane-heading">
              <b>{basic ? "GPUI 组件" : "GPUI 实现"}</b>
              <span>
                {nativeOnly
                  ? "原生快照"
                  : gpuiStatus === "error"
                    ? "加载失败"
                    : gpuiStatus === "ready" && engineState?.id === desired && !engineState.pending_actions
                      ? "可交互"
                      : "加载中"}
              </span>
            </div>
            <div className="canvas-body" ref={pane}>
              <GpuiCanvas
                desired={desired}
                story={selected}
                width={dimensions.width}
                height={dimensions.height}
                basic={basic}
                hidden={!active || actualMode !== "live" || nativeOnly}
                scale={scale}
                bounds={gpuiBounds}
                resetToken={resetToken}
                onFrame={onFrame}
                onStatus={setGpuiStatus}
              />
              {nativeOnly && (
                <img
                  className="native-page-image"
                  src={asset("components/" + selected.native.image)}
                  style={{ width: selected.native.bounds.width * scale }}
                  alt="原生 GPUI 页面快照"
                />
              )}
            </div>
          </section>
        </div>
        {!basic && actualMode !== "live" && <SnapshotComparison story={selected} mode={actualMode} scale={scale} />}
        {!basic && (
          <details className="comparison-parameters">
            <summary>示例参数与来源</summary>
            <dl>
              <dt>窗口</dt>
              <dd>
                {selected.width} × {selected.height}
              </dd>
              <dt>字体</dt>
              <dd>
                {catalog.fixture.fonts.latin} / {catalog.fixture.fonts.cjk}
              </dd>
              <dt>输入数据</dt>
              <dd>
                <code>
                  {JSON.stringify({
                    profile: catalog.fixture.profile.profile_id,
                    agent: catalog.fixture.agents[0]?.name,
                    model: catalog.fixture.profile.models[0]?.id,
                  })}
                </code>
              </dd>
              <dt>GPUI</dt>
              <dd>{selected.source}</dd>
            </dl>
            <p>字体、窗口与初始数据统一。交互后可通过“重置两侧”恢复同一状态；快照保留用于检查结构与像素差异。</p>
            {nativeOnly && <p>会话整页暂由原生渲染，当前在此显示其实际快照。</p>}
            <p>参考页与原生页面使用同一组初始数据和基础尺寸；完整表单、菜单及新增设置均已纳入当前设计。</p>
          </details>
        )}
      </section>
    </div>
  );
}
