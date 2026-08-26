import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { afterEach, describe, expect, it } from "vite-plus/test";

import { brokerRoot, getFreePort, removeTempRoot, spawnBinary, stopChild, waitForReady, writeConfig } from "./helpers.js";

describe.sequential("admin plane (in-process)", () => {
  const cleanups: Array<() => Promise<void>> = [];

  afterEach(async () => {
    while (cleanups.length > 0) {
      await cleanups.pop()?.();
    }
  });

  it("serves admin APIs and realtime session data from the same process", async () => {
    const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "admin-e2e-"));
    cleanups.push(async () => removeTempRoot(tempRoot));
    const dataRoot = tempRoot;

    const [runtimePort, controlPort] = await Promise.all([getFreePort(), getFreePort()]);
    await writeConfig(dataRoot, {
      bind: {
        runtime: `127.0.0.1:${runtimePort}`,
        control: `127.0.0.1:${controlPort}`,
      },
    });
    const child = spawnBinary("zork-gateway", {
      cwd: brokerRoot,
      args: ["--data", dataRoot, "--fake-agent", "--ui-dir", path.join(tempRoot, "missing-ui")],
      env: { RUST_LOG: "info" },
    });
    cleanups.push(async () => stopChild(child));
    await waitForReady(`http://127.0.0.1:${controlPort}/readyz`);
    await waitForReady(`http://127.0.0.1:${runtimePort}/readyz`);

    const ready = await fetch(`http://127.0.0.1:${controlPort}/readyz`);
    expect(ready.status).toBe(200);
    await expect(ready.json()).resolves.toMatchObject({ ok: true, service: "zork-gateway" });

    // Realtime data flows through the admin plane without an HTTP hop.
    const sessions = await fetch(`http://127.0.0.1:${controlPort}/admin/api/sessions`);
    expect(sessions.status).toBe(200);
    await expect(sessions.json()).resolves.toMatchObject({ ok: true });

    const overview = await fetch(`http://127.0.0.1:${controlPort}/admin/api/overview`);
    expect(overview.status).toBe(200);
    await expect(overview.json()).resolves.toMatchObject({
      ok: true,
      service: { name: "zork-gateway", mode: "single" },
    });

    // Settings round-trip: write Slack tokens through the admin API.
    const saved = await fetch(`http://127.0.0.1:${controlPort}/admin/api/settings`, {
      method: "PUT",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ slack: { appToken: "xapp-test", botToken: "xoxb-test" } }),
    });
    expect(saved.status).toBe(200);
    const file = JSON.parse(await fs.readFile(path.join(dataRoot, "config.json"), "utf8")) as {
      slack?: { app_token?: string; bot_token?: string };
    };
    expect(file.slack?.app_token).toBe("xapp-test");
    expect(file.slack?.bot_token).toBe("xoxb-test");

    // Reload without a supervisor answers an error, not a crash.
    const reload = await fetch(`http://127.0.0.1:${controlPort}/admin/api/reload`, {
      method: "POST",
    });
    expect([200, 500, 502]).toContain(reload.status);
    // Process stays alive after a failed reload attempt.
    const still = await fetch(`http://127.0.0.1:${controlPort}/readyz`);
    expect(still.status).toBe(200);
  }, 60_000);
});
