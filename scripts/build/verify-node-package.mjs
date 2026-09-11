#!/usr/bin/env node
import fs from "node:fs/promises";
import path from "node:path";

const root = process.argv[2];
if (!root) throw new Error("Usage: verify-node-package.mjs PACKAGE_DIRECTORY");
const platforms = ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"];
const names = ["zork", "zork-gateway", "zork-agent", "zork-gh"];
for (const platform of platforms) {
  for (const name of names) {
    const file = path.join(root, "bin", "native", `${name}-${platform}`);
    const info = await fs.stat(file);
    if (!info.isFile() || info.size < 1024) throw new Error(`Incomplete node package: ${file}`);
    // GitHub artifact downloads do not preserve executable mode.
    await fs.chmod(file, 0o755);
  }
}
console.log("Verified complete node packages for macOS and Linux, ARM64 and x64.");
