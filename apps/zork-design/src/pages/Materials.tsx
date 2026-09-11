import { useState } from "react";
import { Link, useLocation } from "react-router-dom";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeRaw from "rehype-raw";
import manifest from "../../assets/manifest.json";
import { documents } from "../content/documents";
import { asset } from "../content/assets";
function destination(href: string) {
  const doc = href.match(/(?:\.\.\/)?(?:docs\/)?([^/]+)\.(?:md|html)(?:#.*)?$/)?.[1];
  if (doc && documents.some((d) => d.id === doc)) return "/materials/docs/" + doc;
  if (href.includes("components/")) return "/pc/button";
  if (href.includes("mobile/")) return "/mobile";
  if (href.includes("wordmark/") || href.includes("motion/")) return "/brand";
  if (href.includes("prototype/")) return "/product";
  if (href.includes("assets/") && href.endsWith(".html")) return "/materials";
  return null;
}
export function Materials() {
  const path = useLocation().pathname.split("/"),
    selected = documents.find((d) => d.id === path[3]);
  const [query, setQuery] = useState("");
  const assets = manifest.items.filter(
    (item) =>
      item.path.endsWith(".svg") && (item.path + " " + item.category).toLowerCase().includes(query.toLowerCase()),
  );
  return (
    <div className="materials-layout">
      <aside className="document-nav">
        <Link to="/materials" aria-current={!selected ? "page" : undefined}>
          素材库
        </Link>
        {documents.map((doc) => (
          <Link
            key={doc.id}
            to={"/materials/docs/" + doc.id}
            aria-current={selected?.id === doc.id ? "page" : undefined}
          >
            {doc.title}
          </Link>
        ))}
      </aside>
      {selected ? (
        <article className="markdown-document">
          <Markdown
            remarkPlugins={[remarkGfm]}
            rehypePlugins={[rehypeRaw]}
            components={{
              a: ({ href = "", children }) => {
                const to = destination(href);
                return to ? (
                  <Link to={to}>{children}</Link>
                ) : (
                  <a href={href.startsWith("http") ? href : asset(href.replace(/^\.\.\//, ""))}>{children}</a>
                );
              },
              img: ({ src = "", alt }) => (
                <img src={src.startsWith("http") ? src : asset(src.replace(/^\.\.\//, ""))} alt={alt} />
              ),
              iframe: ({ src = "", title, ...props }) => {
                if (src.includes("components/web/")) {
                  const story = new URL(src, location.href).searchParams.get("story") ?? "button";
                  return (
                    <p>
                      <Link to={"/pc/" + story}>{title || "打开交互组件"} →</Link>
                    </p>
                  );
                }
                return <iframe {...props} src={asset(src.replace(/^\.\.\//, ""))} title={title || "交互示例"} />;
              },
            }}
          >
            {selected.markdown}
          </Markdown>
        </article>
      ) : (
        <article className="asset-library">
          <div className="page-intro">
            <p className="eyebrow">ZORK / MATERIALS</p>
            <h1>可编辑，也可继续维护。</h1>
            <p>素材与来源状态保持可追溯。提案、当前方向和外部品牌资源分别标明。</p>
          </div>
          <label className="search-label">
            搜索素材
            <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="头像、图标、品牌…" />
          </label>
          <div className="asset-grid">
            {assets.map((item) => (
              <a key={item.path} className="asset-card" href={asset(item.path)} target="_blank" rel="noreferrer">
                <div className="asset-stage">
                  <img src={asset(item.path)} alt={item.path.split("/").at(-1)} />
                </div>
                <strong>{item.path.split("/").at(-1)?.replace(".svg", "")}</strong>
                <small>
                  {item.category} · {item.status}
                </small>
              </a>
            ))}
          </div>
          <p className="muted">
            完整元数据见 <a href={asset("assets/manifest.json")}>素材清单</a>，视觉参数见{" "}
            <a href={asset("tokens/design-tokens.json")}>设计 tokens</a>。
          </p>
        </article>
      )}
    </div>
  );
}
