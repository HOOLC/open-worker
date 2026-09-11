import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import { create } from "./common.mjs";
export { version } from "./common.mjs";
export function init() {
  return create(createOnigurumaEngine(import("shiki/wasm")));
}
