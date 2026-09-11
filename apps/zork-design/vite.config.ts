import { defineConfig } from "vite-plus";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import { hostname } from "node:os";

export default defineConfig({
  base: "/design/",
  plugins: [
    react(),
    {
      name: "gpui-reload",
      configureServer(server) {
        const build = resolve(import.meta.dirname, "components/web/build.json");
        server.watcher.add(build);
        server.watcher.on("change", (file) => {
          if (file === build) server.ws.send({ type: "full-reload" });
        });
      },
    },
  ],
  server: {
    host: "0.0.0.0",
    port: 49186,
    strictPort: true,
    allowedHosts: [hostname(), hostname().replace(/\.local$/, "") + ".local"],
  },
  build: {
    rollupOptions: {
      input: {
        app: resolve(import.meta.dirname, "index.html"),
        referenceApp: resolve(import.meta.dirname, "reference.html"),
        reference: resolve(import.meta.dirname, "src/bridge/reference-entry.ts"),
      },
      output: {
        entryFileNames: (chunk) => (chunk.name === "reference" ? "reference-entry.js" : "assets/[name]-[hash].js"),
      },
    },
  },
  lint: {
    plugins: ["typescript", "react"],
    ignorePatterns: [
      "public/**",
      "dist/**",
      "components/**",
      "archive/**",
      "assets/**",
      "mobile/**",
      "motion/**",
      "prototype/**",
      "wordmark/**",
      "scripts/**",
      "docs/**",
      "implementation/**",
      "previews/**",
    ],
    rules: { "no-unused-vars": "off" },
  },
  fmt: {
    printWidth: 120,
    ignorePatterns: [
      "public/**",
      "dist/**",
      "components/**",
      "archive/**",
      "assets/**",
      "mobile/**",
      "motion/**",
      "prototype/**",
      "wordmark/**",
      "scripts/**",
      "docs/**",
      "implementation/**",
      "previews/**",
      "pnpm-lock.yaml",
    ],
  },
  test: { include: ["src/**/*.test.ts"], environment: "node" },
});
