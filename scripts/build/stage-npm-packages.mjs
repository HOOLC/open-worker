#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const destination = process.env.ZORK_STAGE_DESTINATION || path.join(repoRoot, "artifacts", "npm-packages", "zork");
await fs.rm(destination, { force: true, recursive: true });
await fs.mkdir(destination, { recursive: true });
await copyFile(path.join(repoRoot, "packages", "zork", "package.json"), path.join(destination, "package.json"));
await copyFile(path.join(repoRoot, "README.md"), path.join(destination, "README.md"));
await copyFile(path.join(repoRoot, "LICENSE"), path.join(destination, "LICENSE"));
await copyFile(path.join(repoRoot, "crates", "zork-mesh", "LICENSE.synchronicity"), path.join(destination, "licenses", "Synchronicity.txt"));
await copyFile(path.join(repoRoot, "bin", "zork.mjs"), path.join(destination, "bin", "zork.mjs"));
await copyFile(path.join(repoRoot, "bin", "zork-station.mjs"), path.join(destination, "bin", "zork-station.mjs"));
await stageNativeBinary("zork", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-station", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-agent", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-gh", path.join(destination, "bin", "native"));

async function stageNativeBinary(name, nativeDir) {
  const candidates = process.env.ZORK_STAGE_BIN_DIR ? [path.join(process.env.ZORK_STAGE_BIN_DIR, name)] : [path.join(repoRoot, "target", "release", name), path.join(repoRoot, "target", "debug", name)];
  for (const source of candidates) {
    try {
      await fs.access(source);
    } catch {
      continue;
    }
    await fs.mkdir(nativeDir, { recursive: true });
    const platformName = `${name}-${os.platform()}-${os.arch()}`;
    await copyFile(source, path.join(nativeDir, platformName));
    await fs.chmod(path.join(nativeDir, platformName), 0o755);
    return;
  }
  throw new Error(`Cannot package incomplete Station installation: missing ${name}`);
}

async function copyFile(source, destinationPath) {
  await fs.mkdir(path.dirname(destinationPath), { recursive: true });
  await fs.copyFile(source, destinationPath);
}
