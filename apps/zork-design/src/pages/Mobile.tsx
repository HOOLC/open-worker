import { Link } from "react-router-dom";
import { asset } from "../content/assets";
export function Mobile() {
  return (
    <article>
      <div className="page-intro">
        <p className="eyebrow">ZORK / MOBILE</p>
        <h1>同一套关系，适合触摸的界面。</h1>
        <p>移动端采用单屏导航与 48 px 触摸行，不把桌面侧栏缩小后照搬。</p>
      </div>
      <div className="mobile-layout">
        <iframe title="移动端交互样稿" src={asset("mobile/prototype/index.html#nav")} />
        <div>
          <h2>保留任务关系，调整交互方式。</h2>
          <p>设备、Leader 与 Task 的归属保持一致。返回、选择、输入与底部操作按单手操作设计。</p>
          <Link to="/materials/docs/10-mobile">移动端规范 →</Link>
          <p className="muted">移动端仍是明确的平台变体，不能用浏览器宽度推定原生端已完成。</p>
        </div>
      </div>
    </article>
  );
}
