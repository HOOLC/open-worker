import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import { create } from "./common.mjs";
export { version } from "./common.mjs";
export function init() {
  return create(createJavaScriptRegexEngine());
}
