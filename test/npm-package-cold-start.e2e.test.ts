import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { execFile, spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { getFreePort } from "./e2e-broker-helpers.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";

const execFileAsync = promisify(execFile);
const repoRoot = path.dirname(fileURLToPath(new URL("../package.json", import.meta.url)));
const packageTargets = ["admin", "worker"] as const;
const commandTimeoutMs = 30_000;
const installTimeoutMs = 60_000;
const setupTimeoutMs = 180_000;
const testTimeoutMs = 30_000;

let fixtureRoot = "";
const installRoots = new Map<(typeof packageTargets)[number], string>();

describe.sequential("published npm package cold starts", () => {
  beforeAll(async () => {
    fixtureRoot = await fs.mkdtemp(path.join(os.tmpdir(), "broker-npm-cold-start-"));
    const archiveRoot = path.join(fixtureRoot, "archives");

    await runCommand("pnpm", ["build"], { cwd: repoRoot });
    await runCommand(process.execPath, [path.join(repoRoot, "scripts/build/stage-npm-packages.mjs")], { cwd: repoRoot });
    await fs.mkdir(archiveRoot, { recursive: true });

    const results = await Promise.allSettled(packageTargets.map((target) => prepareTargetInstall(target, archiveRoot)));
    const failures = results.filter((result): result is PromiseRejectedResult => result.status === "rejected").map((result) => result.reason);
    if (failures.length > 0) {
      throw new AggregateError(failures, "Failed to prepare clean package installs");
    }
  }, setupTimeoutMs);

  afterAll(async () => {
    if (fixtureRoot) {
      await fs.rm(fixtureRoot, { force: true, recursive: true });
    }
  });

  it(
    "runs the admin bootstrap CLI from a clean tarball install",
    async () => {
      const installRoot = requireInstallRoot("admin");
      const binPath = path.join(installRoot, "node_modules", ".bin", "agent-session-broker-macos-bootstrap");
      const { stdout } = await runCommand(binPath, ["--help"], {
        cwd: fixtureRoot,
        env: {
          HOME: path.join(fixtureRoot, "bootstrap-home"),
          PATH: process.env.PATH ?? "",
          TMPDIR: os.tmpdir(),
          LANG: "C.UTF-8",
          NODE_OPTIONS: "",
          NODE_PATH: "",
        },
      });

      expect(stdout).toContain("Usage:");
      expect(stdout).toContain("--package-version");
    },
    testTimeoutMs,
  );

  it(
    "boots the admin entry point from a clean tarball install",
    async () => {
      const service = await startPackedService("admin");
      try {
        await expect(waitForHttpOk(`${service.baseUrl}/healthz`, service)).resolves.toMatchObject({ ok: true });
        await waitForLog(service, "Admin service booted");
        await expectServiceToStayAlive(service);
      } finally {
        await service.stop();
      }
    },
    testTimeoutMs,
  );

  it(
    "boots the worker entry point from a clean tarball install",
    async () => {
      const mockSlack = new MockSlackServer("UBOT", {
        appId: "AAPP",
        botId: "BBOT",
      });
      let mockStarted = false;
      let service: Awaited<ReturnType<typeof startPackedService>> | undefined;
      try {
        const slackPort = await mockSlack.start();
        mockStarted = true;
        service = await startPackedService("worker", slackPort);
        await expect(waitForHttpOk(`${service.baseUrl}/healthz`, service)).resolves.toMatchObject({ ok: true });
        await expect(waitForHttpOk(`${service.baseUrl}/readyz`, service)).resolves.toMatchObject({ ok: true });
        await withTimeout(mockSlack.waitForSocket(), 15_000, "mock Slack socket connection");
        await waitForLog(service, "Worker service booted");
        await expectServiceToStayAlive(service);
      } finally {
        const cleanups: Promise<unknown>[] = [];
        if (service) {
          cleanups.push(service.stop());
        }
        if (mockStarted) {
          cleanups.push(withTimeout(mockSlack.stop(), 5_000, "mock Slack cleanup"));
        }
        await settleAll(cleanups, "Failed to clean up worker cold-start resources");
      }
    },
    testTimeoutMs,
  );
});

async function prepareTargetInstall(target: (typeof packageTargets)[number], archiveRoot: string): Promise<void> {
  const { stdout } = await runCommand("npm", ["pack", path.join(repoRoot, "artifacts/npm-packages", target), "--pack-destination", archiveRoot, "--json"], {
    cwd: repoRoot,
  });
  const packResult = JSON.parse(stdout) as Array<{ readonly filename?: string }>;
  const filename = packResult[0]?.filename;
  if (!filename) {
    throw new Error(`npm pack did not report an archive for ${target}: ${stdout}`);
  }

  const archivePath = path.join(archiveRoot, filename);
  const installRoot = path.join(fixtureRoot, `install-${target}`);
  installRoots.set(target, installRoot);
  await fs.mkdir(installRoot, { recursive: true });
  await fs.writeFile(path.join(installRoot, "package.json"), `${JSON.stringify({ private: true }, null, 2)}\n`, "utf8");
  await runCommand("npm", ["install", "--ignore-scripts", "--omit=dev", "--no-audit", "--no-fund", "--package-lock=false", "--install-strategy=nested", archivePath], {
    cwd: installRoot,
    timeoutMs: installTimeoutMs,
  });
  await runCommand("npm", ["ls", "--omit=dev", "--all"], { cwd: installRoot });
}

async function runCommand(
  command: string,
  args: readonly string[],
  options: {
    readonly cwd: string;
    readonly env?: NodeJS.ProcessEnv;
    readonly timeoutMs?: number;
  },
): Promise<{ readonly stdout: string; readonly stderr: string }> {
  return await execFileAsync(command, args, {
    cwd: options.cwd,
    env: options.env,
    killSignal: "SIGKILL",
    maxBuffer: 10 * 1024 * 1024,
    timeout: options.timeoutMs ?? commandTimeoutMs,
  });
}

async function startPackedService(
  target: (typeof packageTargets)[number],
  slackPort?: number,
): Promise<{
  readonly baseUrl: string;
  readonly child: ChildProcess;
  readonly logs: string[];
  readonly stop: () => Promise<void>;
}> {
  const runtimeRoot = path.join(fixtureRoot, `runtime-${target}`);
  const port = await getFreePort();
  await fs.mkdir(runtimeRoot, { recursive: true });

  const logs: string[] = [];
  const packageName = `@agent-session-broker/${target}`;
  const installRoot = requireInstallRoot(target);
  const entryPath = path.join(installRoot, "node_modules", packageName, "dist/src", `${target}-index.js`);
  const slackBaseUrl = slackPort ? `http://127.0.0.1:${slackPort}/api` : "http://127.0.0.1:1/api";
  const child = spawn(process.execPath, [entryPath], {
    cwd: runtimeRoot,
    env: {
      HOME: path.join(runtimeRoot, "home"),
      PATH: process.env.PATH ?? "",
      TMPDIR: os.tmpdir(),
      LANG: "C.UTF-8",
      NODE_ENV: "test",
      NODE_OPTIONS: "",
      NODE_PATH: "",
      SERVICE_NAME: `npm-cold-start-${target}`,
      SLACK_APP_TOKEN: "xapp-test",
      SLACK_BOT_TOKEN: "xoxb-test",
      SLACK_API_BASE_URL: slackBaseUrl,
      SLACK_SOCKET_OPEN_URL: "apps.connections.open",
      DATA_ROOT: path.join(runtimeRoot, "data"),
      STATE_DIR: path.join(runtimeRoot, "state"),
      SESSIONS_ROOT: path.join(runtimeRoot, "sessions"),
      REPOS_ROOT: path.join(runtimeRoot, "repos"),
      JOBS_ROOT: path.join(runtimeRoot, "jobs"),
      LOG_DIR: path.join(runtimeRoot, "logs"),
      CODEX_HOME: path.join(runtimeRoot, "codex-home"),
      PORT: String(port),
      WORKER_PORT: String(port),
      WORKER_BIND_HOST: "127.0.0.1",
      BROKER_HTTP_BASE_URL: `http://127.0.0.1:${port}`,
      ADMIN_BASE_URL: `http://127.0.0.1:${port}`,
      WORKER_BASE_URL: `http://127.0.0.1:${port}`,
      FEISHU_ENABLED: "false",
      DISK_CLEANUP_ENABLED: "false",
      LOG_RAW_SLACK_EVENTS: "false",
      LOG_RAW_CODEX_RPC: "false",
      LOG_RAW_HTTP_REQUESTS: "false",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });

  child.stdout?.on("data", (chunk) => logs.push(chunk.toString()));
  child.stderr?.on("data", (chunk) => logs.push(chunk.toString()));

  return {
    baseUrl: `http://127.0.0.1:${port}`,
    child,
    logs,
    stop: () => stopChild(child),
  };
}

function requireInstallRoot(target: (typeof packageTargets)[number]): string {
  const installRoot = installRoots.get(target);
  if (!installRoot) {
    throw new Error(`Missing clean install root for ${target}`);
  }
  return installRoot;
}

async function waitForHttpOk(
  url: string,
  service: {
    readonly child: ChildProcess;
    readonly logs: readonly string[];
  },
): Promise<Record<string, unknown>> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (service.child.exitCode !== null || service.child.signalCode !== null) {
      throw new Error(`Packed service exited before ${url} became healthy:\n${service.logs.join("")}`);
    }
    try {
      const response = await fetch(url);
      if (response.ok) {
        return (await response.json()) as Record<string, unknown>;
      }
    } catch {
      // The process may still be starting.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out waiting for ${url}:\n${service.logs.join("")}`);
}

async function stopChild(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) {
    return;
  }
  child.kill("SIGTERM");
  await Promise.race([once(child, "exit"), new Promise((resolve) => setTimeout(resolve, 5_000))]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL");
    await once(child, "exit");
  }
}

async function waitForLog(
  service: {
    readonly child: ChildProcess;
    readonly logs: readonly string[];
  },
  expected: string,
): Promise<void> {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (service.logs.join("").includes(expected)) {
      return;
    }
    if (service.child.exitCode !== null || service.child.signalCode !== null) {
      break;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Packed service did not log ${JSON.stringify(expected)}:\n${service.logs.join("")}`);
}

async function expectServiceToStayAlive(service: { readonly child: ChildProcess; readonly logs: readonly string[] }): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 500));
  if (service.child.exitCode !== null || service.child.signalCode !== null) {
    throw new Error(`Packed service exited after reporting healthy:\n${service.logs.join("")}`);
  }
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, label: string): Promise<T> {
  let timeout: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timeout = setTimeout(() => reject(new Error(`Timed out waiting for ${label}`)), timeoutMs);
      }),
    ]);
  } finally {
    if (timeout) {
      clearTimeout(timeout);
    }
  }
}

async function settleAll(promises: readonly Promise<unknown>[], message: string): Promise<void> {
  const results = await Promise.allSettled(promises);
  const failures = results.filter((result): result is PromiseRejectedResult => result.status === "rejected").map((result) => result.reason);
  if (failures.length > 0) {
    throw new AggregateError(failures, message);
  }
}
