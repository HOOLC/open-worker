#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..", "..");
const destination = path.join(repoRoot, "artifacts", "npm-packages", "zork");
await fs.rm(path.dirname(destination), { force: true, recursive: true });
await fs.mkdir(destination, { recursive: true });
await copyFile(path.join(repoRoot, "packages", "zork", "package.json"), path.join(destination, "package.json"));
await copyFile(path.join(repoRoot, "README.md"), path.join(destination, "README.md"));
await copyFile(path.join(repoRoot, "LICENSE"), path.join(destination, "LICENSE"));
await copyDirectory(path.join(repoRoot, "apps", "admin-ui", "dist"), path.join(destination, "dist", "admin-ui"));
await copyFile(path.join(repoRoot, "bin", "zork.mjs"), path.join(destination, "bin", "zork.mjs"));
await copyFile(path.join(repoRoot, "bin", "zork-gateway.mjs"), path.join(destination, "bin", "zork-gateway.mjs"));
await stageNativeBinary("zork", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-gateway", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-agent", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-call", path.join(destination, "bin", "native"));
await stageNativeBinary("zork-gh", path.join(destination, "bin", "native"));

async function stageNativeBinary(name, nativeDir) {
  const candidates = [path.join(repoRoot, "target", "release", name), path.join(repoRoot, "target", "debug", name)];
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
}

async function copyFile(source, destinationPath) {
  await fs.mkdir(path.dirname(destinationPath), { recursive: true });
  await fs.copyFile(source, destinationPath);
}

async function copyDirectory(source, destinationPath) {
  await fs.mkdir(path.dirname(destinationPath), { recursive: true });
  await fs.cp(source, destinationPath, { recursive: true });
}
