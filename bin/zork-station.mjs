#!/usr/bin/env node

import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const binDir = path.dirname(fileURLToPath(import.meta.url));
const platformName = `zork-station-${process.platform}-${process.arch}`;
const candidates = [path.join(binDir, "native", platformName), path.join(binDir, "native", "zork-station")];
const binary = candidates.find((candidate) => fs.existsSync(candidate));

if (!binary) {
  console.error(`zork-station native binary not found for ${process.platform}-${process.arch}. Build with cargo build --release -p zork-station and re-pack.`);
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), {
  stdio: "inherit",
});
child.on("error", (error) => {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }
  process.exit(code ?? 1);
});
