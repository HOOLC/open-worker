import { createHighlighterCore } from "shiki/core";
import rust from "shiki/langs/rust.mjs";
import typescript from "shiki/langs/typescript.mjs";
import json from "shiki/langs/json.mjs";
import yaml from "shiki/langs/yaml.mjs";
import python from "shiki/langs/python.mjs";
import githubLight from "shiki/themes/github-light.mjs";
export const version = "4.4.3";
export function create(engine) {
  return createHighlighterCore({
    engine,
    themes: [githubLight],
    langs: [rust, typescript, json, yaml, python],
  });
}
