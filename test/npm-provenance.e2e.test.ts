import fs from "node:fs/promises";

import { describe, expect, it } from "vite-plus/test";

const CANONICAL_REPOSITORY = "HOOLC/open-worker";
const CANONICAL_REPOSITORY_URL = `https://github.com/${CANONICAL_REPOSITORY}`;

type PackageMetadata = {
  readonly version?: string;
  readonly homepage?: string;
  readonly bugs?: { readonly url?: string };
  readonly repository?: { readonly url?: string };
};

describe("npm provenance repository identity", () => {
  it("keeps every release manifest aligned with the canonical GitHub repository", async () => {
    const manifests = await Promise.all(
      ["../package.json", "../packages/zork/package.json"].map(async (relativePath) => {
        return JSON.parse(await fs.readFile(new URL(relativePath, import.meta.url), "utf8")) as PackageMetadata;
      }),
    );

    for (const manifest of manifests) {
      expect(normalizeRepositoryUrl(manifest.repository?.url)).toBe(CANONICAL_REPOSITORY_URL);
      expect(manifest.homepage).toBe(`${CANONICAL_REPOSITORY_URL}#readme`);
      expect(manifest.bugs?.url).toBe(`${CANONICAL_REPOSITORY_URL}/issues`);
    }
    expect(manifests.map((manifest) => manifest.version)).toEqual([manifests[0]?.version, manifests[0]?.version]);
    expect(manifests[0]?.version).toMatch(/^\d+\.\d+\.\d+$/);
  });

  it("publishes zork and zork-gateway from one package", async () => {
    const manifest = JSON.parse(await fs.readFile(new URL("../packages/zork/package.json", import.meta.url), "utf8")) as {
      readonly name?: string;
      readonly bin?: Record<string, string>;
    };
    expect(manifest.name).toBe("@agent-session-broker/zork");
    expect(manifest.bin).toMatchObject({
      zork: "./bin/zork.mjs",
      "zork-gateway": "./bin/zork-gateway.mjs",
    });
    expect(Object.keys(manifest.bin ?? {})).not.toContain("zork-control");
  });
});

function normalizeRepositoryUrl(value: string | undefined): string | undefined {
  return value?.replace(/^git\+/, "").replace(/\.git$/, "");
}
