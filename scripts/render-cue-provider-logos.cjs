// Render Cue’s original vector brand assets with its empty-state CSS.
const path = require("path");
const cue = path.resolve(process.argv[2] || "../Cue");
const { createRequire } = require("module");
const fs = require("fs");
const r = createRequire(path.join(cue, "clients/packages/ui/package.json"));
const React = r("react");
const { renderToStaticMarkup } = r("react-dom/server");
const { transformSync } = r("esbuild");
const src = path.join(cue, "clients/packages/ui/src/components/provider-brand-logos/ProviderBrandLogos.tsx");
const mod = { exports: {} };
new Function("require", "module", "exports", transformSync(fs.readFileSync(src, "utf8"), { loader: "tsx", format: "cjs", jsx: "automatic" }).code)(r, mod, mod.exports);
const logos = ["Linear", "Slack", "Github", "GoogleDrive"].map((n) => renderToStaticMarkup(React.createElement(mod.exports[n + "ProviderLogo"]))).join("");
(async () => {
  const browser = await r("playwright").chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 100, height: 24 }, deviceScaleFactor: 2 });
  await page.setContent(
    `<style>body{margin:0;background:transparent}.logos{display:flex;align-items:center;padding:2px 8px;width:max-content;mask-image:linear-gradient(to right,transparent 0%,#000 30%,#000 70%,transparent 100%)}svg{box-sizing:border-box;width:20px;height:20px;border-radius:100%;background:#fdfdfd;box-shadow:inset 0 0 0 .5px #dfe0e2,0 1px 2px #1018280d;padding:2px;color:#191919}svg+svg{margin-left:-2px}</style><div class="logos">${logos}</div>`,
  );
  await page.locator(".logos").screenshot({ path: path.resolve(__dirname, "../crates/zork-gui/assets/cue/provider-logos.png"), omitBackground: true });
  await browser.close();
})();
