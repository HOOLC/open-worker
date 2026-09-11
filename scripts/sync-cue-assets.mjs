// Run after Cue's frozen pnpm install: node scripts/sync-cue-assets.mjs ../Cue
// Export Cue's actual Central Icons, including its optical stroke normalization.
import { createRequire } from "node:module";
import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
const cue = resolve(process.argv[2] ?? "../Cue");
const ui = resolve(cue, "clients/packages/ui");
const require = createRequire(resolve(ui, "package.json"));
const React = require("react");
const { renderToStaticMarkup } = require("react-dom/server");
const index = readFileSync(resolve(ui, "src/components/icons/index.tsx"), "utf8");
const out = resolve(dirname(fileURLToPath(import.meta.url)), "../crates/zork-gui/assets/cue");
mkdirSync(resolve(out, "fonts"), { recursive: true });
const icons = [
  "Home",
  "ListChecks",
  "Folder1",
  "ArrowUp",
  "ChevronDown",
  "Sparkles",
  "ShapesPlusXSquareCircle",
  "PanelLeft",
  "Plus",
  "Search",
  "SettingsSliderHorizontal",
  "Inbox",
  "Folder2",
  "Puzzle",
  "Settings",
  "ArrowLeft",
  "ArrowRight",
  "Clock",
  "PanelRight",
  "X",
  "Reload",
  "SettingsSliderThree",
  "MicrophoneFilled",
  "Paperclip",
  "Check",
  "Filter2",
  "LayoutColumn",
  "BarsThree",
  "CircleDashed",
  "CircleX",
  "Loader",
  "File",
  "Download",
];
const manifest = [];
for (const name of icons) {
  const match = index.match(new RegExp("import \\{ (\\w+) as Central" + name + 'Icon \\} from "([^"]+)"'));
  if (!match) throw new Error(`Missing Cue mapping: ${name}`);
  const [, centralName, module] = match;
  const Component = require(module)[centralName];
  const svg = renderToStaticMarkup(React.createElement(Component, { mode: "raw" })).replaceAll('stroke-width="2"', 'stroke-width="1.5"');
  const filename = name.replace(/[A-Z]/g, (c, i) => (i ? "-" : "") + c.toLowerCase()) + ".svg";
  writeFileSync(resolve(out, filename), svg + "\n");
  manifest.push({ file: filename, cueExport: name + "Icon", centralName, module, strokeNormalization: "2 → 1.5, matching Cue styles.css" });
}
for (const file of ["InterVariable.woff2", "InterVariable-Italic.woff2", "LICENSE.txt"]) {
  copyFileSync(resolve(ui, "src/fonts", file), resolve(out, "fonts", file));
}
writeFileSync(resolve(out, "icons.json"), JSON.stringify(manifest, null, 2) + "\n");
