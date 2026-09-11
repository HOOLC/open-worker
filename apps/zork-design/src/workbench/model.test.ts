import { describe, expect, it } from "vite-plus/test";
import { routeFor, stateRoute } from "./model";
import type { Story } from "./types";
const stories = [
  { id: "button-primary", family: "button", state: "primary" },
  { id: "button-disabled", family: "button", state: "disabled" },
  { id: "model-detail-compact", family: "model", state: "detail-compact" },
  { id: "model-create-wide", family: "model", state: "create-wide" },
] as Story[];
describe("component and page routes", () => {
  it("keeps component states inside one family route", () => {
    const route = routeFor(stories, "/pc/button-disabled");
    expect(route.family).toBe("button");
    expect(route.items).toHaveLength(2);
    expect(route.basic).toBe(true);
  });
  it("restores a page scene and viewport from its link", () => {
    const path = stateRoute("model", "create", "wide");
    expect(routeFor(stories, path).selected.id).toBe("model-create-wide");
  });
  it("falls back without an invalid empty story", () => {
    expect(routeFor(stories, "/pc/unknown").selected.id).toBe("button-primary");
  });
});
