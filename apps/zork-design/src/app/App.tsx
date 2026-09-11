import { lazy, Suspense, useEffect, useState } from "react";
import { NavLink, Navigate, Route, Routes, useLocation } from "react-router-dom";
import { Brand } from "../pages/Brand";
import { Product } from "../pages/Product";
import { Mobile } from "../pages/Mobile";
const Materials = lazy(() => import("../pages/Materials").then((module) => ({ default: module.Materials })));
const Workbench = lazy(() => import("../workbench/Workbench").then((module) => ({ default: module.Workbench })));
import { asset } from "../content/assets";
const tabs = [
  ["/brand", "品牌"],
  ["/product", "产品"],
  ["/pc", "PC 组件库"],
  ["/mobile", "移动端"],
  ["/materials", "规范与素材"],
];
export function App() {
  const location = useLocation(),
    pc = location.pathname.startsWith("/pc");
  const [visitedPc, setVisitedPc] = useState(pc);
  useEffect(() => {
    if (pc) setVisitedPc(true);
  }, [pc]);
  return (
    <div className="design-app">
      <header className="design-header">
        <NavLink className="design-brand" to="/brand">
          <img src={asset("assets/brand/mark.svg")} alt="" />
          <img className="wordmark" src={asset("assets/brand/zork-wordmark-draft.svg")} alt="Zork" />
          <span>Design</span>
        </NavLink>
        <nav aria-label="设计手册">
          <div role="tablist" className="design-tabs">
            {tabs.map(([to, label]) => (
              <NavLink role="tab" key={to} to={to} aria-selected={location.pathname.startsWith(to)}>
                {label}
              </NavLink>
            ))}
          </div>
        </nav>
      </header>
      <main className={pc ? "design-main pc-active" : "design-main"}>
        <section hidden={pc} className="handbook-page">
          <Suspense fallback={<p>正在读取设计资料…</p>}>
            <Routes>
              <Route path="/brand/*" element={<Brand />} />
              <Route path="/product/*" element={<Product />} />
              <Route path="/mobile/*" element={<Mobile />} />
              <Route path="/materials/*" element={<Materials />} />
              <Route path="/pc/*" element={null} />
              <Route path="*" element={<Navigate to="/brand" replace />} />
            </Routes>
          </Suspense>
        </section>
        {visitedPc && (
          <section className="pc-page" hidden={!pc}>
            <Suspense fallback={<p>正在读取组件库…</p>}>
              <Workbench active={pc} />
            </Suspense>
          </section>
        )}
      </main>
    </div>
  );
}
