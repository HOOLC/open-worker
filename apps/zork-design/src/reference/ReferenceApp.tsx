import { useEffect, useRef, useState } from "react";
import type { Catalog, Rect, Story } from "../workbench/types";
import { asset } from "../content/assets";
import { ProfilesPage } from "./Profiles";
import { AgentsPage } from "./Agents";
import { ConversationPage } from "./Conversation";
import { AddDevicePage, ClientPage, DevicePage, MeshPage } from "./Settings";
import { Glyph } from "./controls";
export interface ReferenceSnapshot {
  id: string;
  bounds: Rect;
  surface: "dialog" | "page";
  regions?: Rect[];
  surfaceBounds?: Rect;
}
declare global {
  interface Window {
    zorkReference?: { snapshot: () => ReferenceSnapshot | null };
  }
}
export function ReferenceApp() {
  const [catalog, setCatalog] = useState<Catalog | null>(null),
    [error, setError] = useState(""),
    [selected, setSelected] = useState(new URLSearchParams(location.search).get("story") ?? "connection-list-compact");
  const product = new URLSearchParams(location.search).get("mode") === "product";
  const parts = selected.split("-"),
    family = parts[0],
    size = parts.at(-1) === "wide" ? "wide" : "compact";
  const story =
    catalog?.stories.find((s) => s.id === selected) ??
    ({
      id: selected,
      family,
      state: parts.slice(1).join("-"),
      width: size === "wide" ? 1280 : 900,
      height: size === "wide" ? 800 : 600,
    } as Story);
  const current = useRef(story);
  current.current = story;
  useEffect(() => {
    fetch(asset("catalog.json"), { cache: "no-store" })
      .then((r) => r.json())
      .then(setCatalog)
      .catch((e) => setError(String(e)));
  }, []);
  useEffect(() => {
    if (!catalog) return;
    let cancelled = false;
    const snapshot = (): ReferenceSnapshot | null => {
      const target =
        document.querySelector<HTMLElement>("dialog[open][data-reference-target]") ??
        document.querySelector<HTMLElement>("[data-reference-target]");
      if (!target) return null;
      const r = target.getBoundingClientRect();
      if (!r.width || !r.height) return null;
      let bounds = { x: r.x, y: r.y, width: r.width, height: r.height };
      const regions: Rect[] = [];
      for (const menu of document.querySelectorAll<HTMLElement>(".ref-menu")) {
        const m = menu.getBoundingClientRect();
        regions.push({ x: m.x, y: m.y, width: m.width, height: m.height });
        const right = Math.max(bounds.x + bounds.width, m.right),
          bottom = Math.max(bounds.y + bounds.height, m.bottom);
        bounds.x = Math.min(bounds.x, m.x);
        bounds.y = Math.min(bounds.y, m.y);
        bounds.width = right - bounds.x;
        bounds.height = bottom - bounds.y;
      }
      return {
        id: current.current.id,
        bounds,
        surface: target.tagName === "DIALOG" ? "dialog" : "page",
        surfaceBounds: { x: r.x, y: r.y, width: r.width, height: r.height },
        regions,
      };
    };
    window.zorkReference = { snapshot };
    void (async () => {
      await document.fonts.ready;
      await Promise.all([...document.images].map((image) => image.decode().catch(() => undefined)));
      await new Promise<void>((resolve) => requestAnimationFrame(() => requestAnimationFrame(() => resolve())));
      if (cancelled) return;
      document.documentElement.dataset.referenceReady = story.id;
      document.documentElement.dataset.fixture = JSON.stringify({
        device: catalog.fixture.device.id,
        profile: catalog.fixture.profile.profile_id,
        model: catalog.fixture.profile.models[0]?.id,
        agent: catalog.fixture.agents[0]?.name,
        width: innerWidth,
        height: innerHeight,
      });
      const state = snapshot();
      if (state) parent.postMessage({ type: "zork-reference-bounds", ...state }, location.origin);
    })();
    return () => {
      cancelled = true;
      delete document.documentElement.dataset.referenceReady;
      delete window.zorkReference;
    };
  }, [catalog, story.id]);
  useEffect(() => {
    if (!catalog?.tokens) return;
    const aliases: Record<string, string> = {
      text: "text",
      muted: "muted",
      border: "border",
      border_strong: "strong",
      sidebar: "sidebar",
      sidebar_hover: "hover",
      selected: "selected",
      prompt: "prompt",
      success: "success",
      danger: "danger",
      subtle: "subtle",
    };
    for (const [source, name] of Object.entries(aliases))
      document.documentElement.style.setProperty("--ref-" + name, catalog.tokens.colors[source]);
    for (const [source, name] of [
      ["CONTROL_HEIGHT", "control-height"],
      ["BUTTON_HEIGHT", "button-height"],
      ["FIELD_HEIGHT", "field-height"],
      ["DROPDOWN_HEIGHT", "dropdown-height"],
      ["DIALOG_WIDTH", "modal-width"],
    ])
      document.documentElement.style.setProperty("--ref-" + name, catalog.tokens.sizes[source] + "px");
  }, [catalog]);
  if (error) return <p role="alert">设计参考加载失败：{error}</p>;
  if (!catalog) return <p className="ref-note">正在读取设计数据…</p>;
  let page;
  switch (family) {
    case "connection":
    case "model":
      page = <ProfilesPage key={selected} catalog={catalog} story={story} />;
      break;
    case "agent":
      page = <AgentsPage key={selected} catalog={catalog} story={story} />;
      break;
    case "conversation":
      page = (
        <ConversationPage
          key={selected}
          catalog={catalog}
          story={story}
          onNavigate={product ? setSelected : undefined}
        />
      );
      break;
    case "client":
      page = <ClientPage key={selected} catalog={catalog} story={story} />;
      break;
    case "device":
      page = <DevicePage key={selected} catalog={catalog} story={story} />;
      break;
    case "mesh":
      page = <MeshPage key={selected} catalog={catalog} story={story} />;
      break;
    case "enrollment":
      page = <AddDevicePage key={selected} catalog={catalog} story={story} />;
      break;
    default:
      page = <p role="alert">未注册的设计页面：{family}</p>;
  }
  if (family === "conversation") return page;
  return (
    <div className="ref-app">
      <div className="ref-window">
        <aside className="ref-sidebar">
          {product ? (
            <>
              <button className="ref-nav" onClick={() => setSelected("conversation-messages-wide")}>
                <Glyph name="arrow-left" />
                返回对话
              </button>
              {[
                ["client-signed-out", "客户端设置"],
                ["device-running", "mini1"],
                ["agent-list", "队员"],
                ["connection-list", "大模型"],
                ["mesh-connected", "设备连接"],
                ["enrollment-start", "添加设备"],
              ].map(([id, label]) => (
                <button
                  className={"ref-nav " + (selected.startsWith(id.split("-")[0]) ? "selected" : "")}
                  key={id}
                  onClick={() => setSelected(id + "-wide")}
                >
                  {label}
                </button>
              ))}
            </>
          ) : null}
        </aside>
        <main className="ref-main">{page}</main>
      </div>
    </div>
  );
}
