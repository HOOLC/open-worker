import { constants as fsConstants, existsSync } from "node:fs";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { ensureDir } from "./fs.js";
import { resolveRuntimeToolPath } from "./runtime-paths.js";

const GH_SHIM = ["#!/usr/bin/env bash", "set -euo pipefail", 'if [ -z "${BROKER_GH_HELPER:-}" ]; then', "  printf '%s\\n' 'BROKER_GH_HELPER is required for broker gh wrapper.' >&2", "  exit 1", "fi", 'exec node "$BROKER_GH_HELPER" "$@"'].join("\n");

export function resolveZorkBinDir(dataRoot?: string): string {
  const root = dataRoot?.trim() || process.env.DATA_ROOT?.trim() || path.join(process.env.HOME?.trim() || os.homedir(), ".zork");
  return path.join(path.resolve(root), "bin");
}

export async function ensureZorkCallBin(binDir: string): Promise<{
  readonly binDir: string;
  readonly zorkCallPath: string;
  readonly realGhPath?: string | undefined;
}> {
  await ensureDir(binDir);
  await writeExecutable(path.join(binDir, "gh"), GH_SHIM);
  const zorkCallPath = resolveZorkCallToolPath();
  await writeExecutable(path.join(binDir, "zork-call"), renderShim(zorkCallPath));
  const realGhPath = process.env.BROKER_REAL_GH_PATH?.trim() || (await findExecutableOnPath("gh", process.env.PATH, [binDir]));
  return realGhPath
    ? {
        binDir,
        zorkCallPath,
        realGhPath,
      }
    : {
        binDir,
        zorkCallPath,
      };
}

function resolveZorkCallToolPath(): string {
  const jsPath = resolveRuntimeToolPath("zork-call.js");
  const tsPath = resolveRuntimeToolPath("zork-call.ts");
  if (existsSync(jsPath)) {
    return jsPath;
  }
  if (existsSync(tsPath)) {
    return tsPath;
  }
  return jsPath;
}

function renderShim(toolPath: string): string {
  if (toolPath.endsWith(".ts")) {
    const tsxPath = resolveTsxBin(path.dirname(toolPath));
    return ["#!/usr/bin/env bash", "set -euo pipefail", `exec ${posixSingleQuote(tsxPath)} ${posixSingleQuote(toolPath)} "$@"`].join("\n");
  }

  return ["#!/usr/bin/env bash", "set -euo pipefail", `exec ${posixSingleQuote(process.execPath)} ${posixSingleQuote(toolPath)} "$@"`].join("\n");
}

function resolveTsxBin(fromDir: string): string {
  let current = fromDir;
  while (true) {
    const candidate = path.join(current, "node_modules", ".bin", "tsx");
    if (existsSync(candidate)) {
      return candidate;
    }

    const parent = path.dirname(current);
    if (parent === current) {
      throw new Error(`tsx not found for zork-call from ${fromDir}`);
    }
    current = parent;
  }
}

async function writeExecutable(filePath: string, contents: string): Promise<void> {
  const body = contents.endsWith("\n") ? contents : `${contents}\n`;
  await fs.writeFile(filePath, body, { mode: 0o755 });
  await fs.chmod(filePath, 0o755);
}

async function findExecutableOnPath(command: string, pathValue: string | undefined, skipDirs: readonly string[]): Promise<string | undefined> {
  const skipped = new Set(skipDirs.map((dir) => path.resolve(dir)));
  for (const dir of (pathValue ?? "").split(path.delimiter).filter(Boolean)) {
    if (skipped.has(path.resolve(dir))) {
      continue;
    }
    const candidate = path.join(dir, command);
    try {
      await fs.access(candidate, fsConstants.X_OK);
      return candidate;
    } catch {
      // Keep searching PATH.
    }
  }
  return undefined;
}

function posixSingleQuote(value: string): string {
  return `'${value.replaceAll("'", `'\\''`)}'`;
}
