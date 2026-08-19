import { type ChildProcessByStdio } from "node:child_process";

import type { Readable } from "node:stream";

import { expect } from "vitest";

import { CURRENT_STATE_SCHEMA_VERSION, STATE_DATABASE_FILENAME, STATE_STORE_BUSY_TIMEOUT_MS, StateStore } from "../src/store/state-store.js";

export const LOCK_DATABASE_SCRIPT = `
const { DatabaseSync } = require("node:sqlite");

const database = new DatabaseSync(process.env.DB_PATH);
database.exec("PRAGMA journal_mode = WAL; CREATE TABLE IF NOT EXISTS lock_probe (id INTEGER); BEGIN IMMEDIATE; INSERT INTO lock_probe (id) VALUES (1);");
process.stdout.write("locked\\n");
setTimeout(() => {
  try {
    database.exec("ROLLBACK");
  } finally {
    database.close();
  }
}, 350);
`;

export function waitForOutput(child: ChildProcessByStdio<null, Readable, Readable>, marker: string): Promise<void> {
  return new Promise((resolve, reject) => {
    let stdout = "";
    let stderr = "";
    const timeout = setTimeout(() => {
      cleanup();
      reject(new Error(`Timed out waiting for child output ${marker}. stdout=${stdout} stderr=${stderr}`));
    }, 2_000);
    const onStdout = (chunk: Buffer) => {
      stdout += chunk.toString("utf8");
      if (stdout.includes(marker)) {
        cleanup();
        resolve();
      }
    };
    const onStderr = (chunk: Buffer) => {
      stderr += chunk.toString("utf8");
    };
    const onExit = (code: number | null, signal: NodeJS.Signals | null) => {
      cleanup();
      reject(new Error(`Child exited before ${marker}: code=${code} signal=${signal} stdout=${stdout} stderr=${stderr}`));
    };
    const cleanup = () => {
      clearTimeout(timeout);
      child.stdout.off("data", onStdout);
      child.stderr.off("data", onStderr);
      child.off("exit", onExit);
    };
    child.stdout.on("data", onStdout);
    child.stderr.on("data", onStderr);
    child.once("exit", onExit);
  });
}

export function extractMethodBody(source: string, marker: string): string {
  const markerIndex = source.indexOf(marker);
  expect(markerIndex).toBeGreaterThanOrEqual(0);
  const bodyStart = source.indexOf("{", markerIndex);
  expect(bodyStart).toBeGreaterThanOrEqual(0);
  let depth = 0;
  for (let index = bodyStart; index < source.length; index += 1) {
    const character = source[index];
    if (character === "{") {
      depth += 1;
    } else if (character === "}") {
      depth -= 1;
      if (depth === 0) {
        return source.slice(bodyStart + 1, index);
      }
    }
  }
  throw new Error(`Could not extract method body for ${marker}`);
}
