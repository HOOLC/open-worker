import { asset } from "../content/assets";
import { Link } from "react-router-dom";
export function Product() {
  return (
    <article>
      <div className="page-intro">
        <p className="eyebrow">ZORK / PRODUCT</p>
        <h1>设备 → Leader → Task</h1>
        <p>长期协作与群聊式任务。层级通过缩进表达，信息和配置始终属于明确的设备。</p>
      </div>
      <div className="rule-strip">
        {[
          ["32 px", "导航行高"],
          ["44 px", "会话头"],
          ["200–420 px", "可调侧栏"],
          ["32–160 px", "输入框高度"],
        ].map(([value, label]) => (
          <div key={label}>
            <b>{value}</b>
            <span>{label}</span>
          </div>
        ))}
      </div>
      <section className="prototype-stage">
        <div className="frame-caption">原始交互样稿 · 隔离示例数据</div>
        <iframe title="Zork 产品交互样稿" src={asset("reference-prototype/gui.html")} />
      </section>
      <p>
        <Link to="/pc/connection/list/compact">打开当前完整页面设计 →</Link>　
        <Link to="/materials/docs/14-controls-revision">2026.09 控件规范 →</Link>　
        <Link to="/materials/docs/01-concepts">产品概念 →</Link>
      </p>
    </article>
  );
}
