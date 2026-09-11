import { build } from "esbuild";
import { fileURLToPath } from "node:url";
const absWorkingDir = fileURLToPath(new URL(".", import.meta.url));
await build({ absWorkingDir, entryPoints: ["oniguruma.mjs", "javascript.mjs"], bundle: true, format: "esm", platform: "browser", target: "chrome149", minify: true, outdir: "dist" });
