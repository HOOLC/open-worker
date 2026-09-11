import { BrandMotion } from "./BrandMotion";
import { Link } from "react-router-dom";
import { asset, avatars, providerNames } from "../content/assets";
export function Brand() {
  return (
    <article className="brand-page">
      <div className="page-intro">
        <p className="eyebrow">ZORK / IDENTITY</p>
        <h1>折角伙伴，安静地接着做。</h1>
        <p>品牌、人物、工作状态各有独立的表达。这里保留当前方向、可编辑素材与待定提案。</p>
      </div>
      <section className="identity-grid">
        <div className="mark-stage">
          <img src={asset("assets/brand/mark-reverse.svg")} alt="折角伙伴" />
        </div>
        <div className="wordmark-stage">
          <img src={asset("assets/brand/zork-wordmark-draft.svg")} alt="Zork 字标" />
          <span className="status-note">字标提案 · 待评审</span>
          <p>名称统一为 Zork。图标与字标采用同一组折角轮廓。</p>
          <Link to="/materials/docs/02-principles">品牌规范 →</Link>
        </div>
      </section>
      <section className="color-strip">
        {[
          ["炭墨", "#24272B"],
          ["暖纸", "#F6F3EA"],
          ["柿橙", "#E9643B"],
        ].map(([name, color]) => (
          <div key={name}>
            <i style={{ background: color }} />
            <b>{name}</b>
            <code>{color}</code>
          </div>
        ))}
      </section>
      <section className="document-section">
        <h2>角色有职责，个体有样子。</h2>
        <p>12 个稳定的动物头像。Leader、Worker、在线和未读通过独立信息表达。</p>
        <div className="avatar-strip">
          {avatars.map((name) => (
            <img key={name} src={asset(`assets/avatars/${name}.svg`)} alt={name} />
          ))}
        </div>
        <Link to="/materials/docs/11-avatars">头像规范 →</Link>
      </section>
      <section className="document-section">
        <h2>品牌动效</h2>
        <p>悬停查看图标、字标与组合动效。动效遵循减少动态效果设置。</p>
        <div className="motion-grid">
          {[
            ["linked", "图标与字标"],
            ["wordmark", "字标"],
            ["icon", "图标"],
            ["icon-to-wordmark", "标志变形"],
          ].map(([name, label]) => (
            <BrandMotion key={name} name={name} label={label} />
          ))}
        </div>
      </section>
      <section className="document-section">
        <h2>供应商身份</h2>
        <div className="provider-strip">
          {providerNames.map((name) => (
            <img key={name} src={asset(`assets/providers/${name}.svg`)} alt={name} />
          ))}
        </div>
        <p className="muted">外部品牌符号用于识别服务，不代替 Zork 的交互与状态语义。</p>
      </section>
    </article>
  );
}
