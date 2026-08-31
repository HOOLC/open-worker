import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vite-plus/test";

const workspaceRoot = fileURLToPath(new URL("../", import.meta.url));
const cratesRoot = path.join(workspaceRoot, "crates");
const workspaceDeclarations = {
  "futures-util": 'futures-util = { version = "0.3.31", features = ["channel"] }',
  reqwest: 'reqwest = { version = "0.12.28", default-features = false, features = ["blocking", "json", "rustls-tls", "stream"] }',
  serde: 'serde = { version = "1.0.228", features = ["alloc", "derive", "rc"] }',
  tokio: 'tokio = { version = "1.48.0", features = ["full", "test-util"] }',
  "tokio-tungstenite": 'tokio-tungstenite = { version = "0.26", features = ["rustls-tls-webpki-roots"] }',
} as const;

async function memberManifests(): Promise<Array<{ readonly name: string; readonly source: string }>> {
  const entries = await fs.readdir(cratesRoot, { withFileTypes: true });
  return Promise.all(
    entries
      .filter((entry) => entry.isDirectory())
      .map(async (entry) => ({
        name: entry.name,
        source: await fs.readFile(path.join(cratesRoot, entry.name, "Cargo.toml"), "utf8"),
      })),
  );
}

function dependencyDeclarations(source: string, dependency: string): string[] {
  const escaped = dependency.replaceAll("-", "\\-");
  return [...source.matchAll(new RegExp(`^${escaped}(?:\\.workspace)?\\s*=.*$`, "gm"))].map(([line]) => line);
}

describe("Rust workspace dependency features", () => {
  // Contract: docs/rust-build-cache.md#约束
  it("inherits one declaration for every unified direct dependency", async () => {
    const rootManifest = await fs.readFile(path.join(workspaceRoot, "Cargo.toml"), "utf8");
    const manifests = await memberManifests();

    for (const [dependency, workspaceDeclaration] of Object.entries(workspaceDeclarations)) {
      expect(dependencyDeclarations(rootManifest, dependency)).toEqual([workspaceDeclaration]);
      const users = manifests.flatMap((manifest) =>
        dependencyDeclarations(manifest.source, dependency).map((declaration) => ({
          crate: manifest.name,
          declaration,
        })),
      );
      expect(users.length).toBeGreaterThan(0);
      expect(users).toEqual(
        users.map(({ crate }) => ({
          crate,
          declaration: `${dependency}.workspace = true`,
        })),
      );
    }
  });
});
