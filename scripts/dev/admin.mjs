#!/usr/bin/env node

import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const adminUiPort = process.env.ADMIN_UI_DEV_PORT || "5173";
const controlPort = process.env.PORT || "3001";
const adminApiOrigin = process.env.ADMIN_API_PROXY_ORIGIN || `http://127.0.0.1:${controlPort}`;

const children = [
  spawn("vp", ["dev", "--host", "127.0.0.1", "--port", adminUiPort, "--strictPort"], {
    cwd: path.join(repoRoot, "apps", "admin-ui"),
    env: {
      ...process.env,
      ADMIN_API_PROXY_ORIGIN: adminApiOrigin,
      ADMIN_UI_DEV_PORT: adminUiPort,
    },
    stdio: "inherit",
  }),
  spawn("cargo", ["run", "-p", "zork-gateway", "--", "--ui-dir", path.join(repoRoot, "apps", "admin-ui", "dist")], {
    cwd: repoRoot,
    stdio: "inherit",
  }),
];

let exiting = false;

function stopAll(signal = "SIGTERM") {
  if (exiting) {
    return;
  }
  exiting = true;
  for (const child of children) {
    if (!child.killed) {
      child.kill(signal);
    }
  }
}

for (const child of children) {
  child.on("exit", (code, signal) => {
    if (!exiting && code !== 0) {
      stopAll();
      process.exitCode = code ?? (signal ? 1 : 0);
    }
  });
}

process.on("SIGINT", () => {
  stopAll("SIGINT");
});
process.on("SIGTERM", () => {
  stopAll("SIGTERM");
});
