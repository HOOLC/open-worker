#!/usr/bin/env node

import { readFileSync } from "node:fs";

import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { repoRoot, runCommand } from "./lib.mjs";
import {
  buildPortableGhConfigHome,
  initializeRuntimeData,
  renderEnvFile,
  renderEnvironmentVariables,
  renderDaemonCommon,
  renderPlist,
  renderCloudflaredPlist,
  buildPaths,
  buildReleaseMetadata,
  packageSpec,
  packageRootForInstallRoot,
  normalizePackageVersion,
  buildAdminEnv,
  buildWorkerEnv,
  installTooling,
  prepareSharedHomes,
  ensureInitialReleases,
  ensureInitialReleaseTarget,
  shouldInstallLaunchDaemonWithSudo,
  writeLaunchDaemonPlist,
} from "./macos-bootstrap-helpers-2.mjs";
import { removeLegacyLaunchAgent, writeLaunchdFiles, launchdDomain, useSudoForLaunchctl, runLaunchctl, bootout, bootstrap, kickstart } from "./macos-bootstrap-helpers-3.mjs";

export const DEFAULT_SERVICE_ROOT = repoRoot;

export const DEFAULT_ADMIN_LABEL = "io.github.hoolc.agent-session-broker";

export const DEFAULT_WORKER_LABEL = "io.github.hoolc.agent-session-broker.worker";

export const DEFAULT_CLOUDFLARED_LABEL = "io.github.hoolc.agent-session-broker.cloudflared";

export const DEFAULT_NODE_PATH = "/opt/homebrew/opt/node@24/bin/node";

export const DEFAULT_CLOUDFLARED_PATH = "/opt/homebrew/bin/cloudflared";

export const DEFAULT_LAUNCHD_DAEMON_DIR = "/Library/LaunchDaemons";

export const DEFAULT_CODEX_VERSION = "0.114.0";

export const DEFAULT_PACKAGE_INFO = readDefaultPackageInfo();

export const RELEASE_METADATA_FILENAME = ".broker-release.json";

export const CODEX_HOME_FILE_ENTRIES = [".credentials.json", ".personality_migration", "AGENT.md", "AGENTS.md", "config.toml", "memory.md", "models_cache.json"];

export const CODEX_HOME_DIRECTORY_ENTRIES = ["memories", "rules", "skills", "superpowers", "vendor_imports"];

export const BROKER_ENV_PASSTHROUGH_KEYS = [
  "SLACK_APP_TOKEN",
  "SLACK_BOT_TOKEN",
  "SLACK_API_BASE_URL",
  "SLACK_SOCKET_OPEN_URL",
  "SLACK_INITIAL_THREAD_HISTORY_COUNT",
  "SLACK_HISTORY_API_MAX_LIMIT",
  "SLACK_ACTIVE_TURN_RECONCILE_INTERVAL_MS",
  "SLACK_MISSED_THREAD_RECOVERY_INTERVAL_MS",
  "LOG_LEVEL",
  "LOG_RAW_SLACK_EVENTS",
  "LOG_RAW_CODEX_RPC",
  "LOG_RAW_HTTP_REQUESTS",
  "LOG_RAW_MAX_BYTES",
  "DISK_CLEANUP_ENABLED",
  "DISK_CLEANUP_CHECK_INTERVAL_MS",
  "DISK_CLEANUP_MIN_FREE_BYTES",
  "DISK_CLEANUP_TARGET_FREE_BYTES",
  "DISK_CLEANUP_INACTIVE_SESSION_MS",
  "DISK_CLEANUP_JOB_PROTECTION_MS",
  "DISK_CLEANUP_OLD_LOG_MS",
  "ISOLATED_MCP_SERVERS",
  "CODEX_DISABLED_MCP_SERVERS",
  "CODEX_APP_SERVER_URL",
  "OPENAI_API_KEY",
  "TEMPAD_LINK_SERVICE_URL",
  "BROKER_ADMIN_TOKEN",
  "ADMIN_BASE_URL",
  "GITHUB_API_BASE_URL",
  "GITHUB_OAUTH_SCOPES",
  "BROKER_DEFAULT_GITHUB_LOGIN",
  "BROKER_DEFAULT_GITHUB_TOKEN",
  "GH_TOKEN",
  "GITHUB_TOKEN",
  "CLOUDFLARED_TUNNEL_TOKEN",
];

export function readDefaultPackageInfo() {
  try {
    const packageJson = JSON.parse(readFileSync(path.join(repoRoot, "package.json"), "utf8"));
    return {
      adminName: "@agent-session-broker/admin",
      workerName: "@agent-session-broker/worker",
      version: packageJson.version || "latest",
    };
  } catch {
    return {
      adminName: "@agent-session-broker/admin",
      workerName: "@agent-session-broker/worker",
      version: "latest",
    };
  }
}

export function parseArgs(argv) {
  const options = {
    serviceRoot: DEFAULT_SERVICE_ROOT,
    adminLabel: DEFAULT_ADMIN_LABEL,
    workerLabel: DEFAULT_WORKER_LABEL,
    cloudflaredLabel: DEFAULT_CLOUDFLARED_LABEL,
    nodePath: DEFAULT_NODE_PATH,
    cloudflaredPath: DEFAULT_CLOUDFLARED_PATH,
    npmPath: undefined,
    launchdDaemonDir: DEFAULT_LAUNCHD_DAEMON_DIR,
    runUser: os.userInfo().username,
    adminPackageName: DEFAULT_PACKAGE_INFO.adminName,
    workerPackageName: DEFAULT_PACKAGE_INFO.workerName,
    packageVersion: DEFAULT_PACKAGE_INFO.version,
    npmRegistryUrl: undefined,
    codexVersion: DEFAULT_CODEX_VERSION,
    startWorker: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    switch (argument) {
      case "--service-root":
        options.serviceRoot = path.resolve(argv[index + 1]);
        index += 1;
        break;
      case "--label":
        options.adminLabel = argv[index + 1];
        index += 1;
        break;
      case "--worker-label":
        options.workerLabel = argv[index + 1];
        index += 1;
        break;
      case "--cloudflared-label":
        options.cloudflaredLabel = argv[index + 1];
        index += 1;
        break;
      case "--node-path":
        options.nodePath = argv[index + 1];
        index += 1;
        break;
      case "--cloudflared-path":
        options.cloudflaredPath = argv[index + 1];
        index += 1;
        break;
      case "--npm-path":
        options.npmPath = argv[index + 1];
        index += 1;
        break;
      case "--launchd-daemon-dir":
        options.launchdDaemonDir = path.resolve(argv[index + 1]);
        index += 1;
        break;
      case "--run-user":
        options.runUser = argv[index + 1];
        index += 1;
        break;
      case "--admin-package-name":
        options.adminPackageName = argv[index + 1];
        index += 1;
        break;
      case "--worker-package-name":
        options.workerPackageName = argv[index + 1];
        index += 1;
        break;
      case "--package-version":
        options.packageVersion = argv[index + 1];
        index += 1;
        break;
      case "--npm-registry-url":
        options.npmRegistryUrl = argv[index + 1];
        index += 1;
        break;
      case "--codex-version":
        options.codexVersion = argv[index + 1];
        index += 1;
        break;
      case "--start-worker":
        options.startWorker = true;
        break;
      case "--help":
      case "-h":
        printHelp();
        process.exit(0);
      default:
        throw new Error(`Unknown argument: ${argument}`);
    }
  }

  return options;
}

export function printHelp() {
  console.log(
    [
      "Usage:",
      "  node scripts/ops/macos-bootstrap.mjs [options]",
      "",
      "What it does:",
      "  - prepares shared runtime directories under the service root",
      "  - installs built admin and worker npm packages under releases/<target>/npm-<version>",
      "  - writes admin/worker launchd plists and env files",
      "  - starts admin immediately; worker is optional",
      "",
      "Notes:",
      "  - preferred flow: install the admin package, then run this script with --package-version",
      "  - auth.json is not copied by this script; import auth profiles later through /admin",
      "  - Slack tokens come from the current shell env or an existing config/broker.env",
      "",
      "Options:",
      `  --service-root <path>                Service root, default ${DEFAULT_SERVICE_ROOT}`,
      `  --label <label>                     Admin launchd label, default ${DEFAULT_ADMIN_LABEL}`,
      `  --worker-label <label>              Worker launchd label, default ${DEFAULT_WORKER_LABEL}`,
      `  --cloudflared-label <label>         Cloudflared launchd label, default ${DEFAULT_CLOUDFLARED_LABEL}`,
      "  --start-worker                      Also start the worker after bootstrap",
      "  --node-path <path>                  Node binary for launchd",
      `  --cloudflared-path <path>           Cloudflared binary, default ${DEFAULT_CLOUDFLARED_PATH}`,
      "  --npm-path <path>                   npm binary, default next to --node-path",
      `  --launchd-daemon-dir <path>         LaunchDaemon plist directory, default ${DEFAULT_LAUNCHD_DAEMON_DIR}`,
      `  --run-user <user>                   UserName for LaunchDaemons, default ${os.userInfo().username}`,
      "  --admin-package-name <name>         Admin npm package name",
      "  --worker-package-name <name>        Worker npm package name",
      "  --package-version <version>         Broker npm package version",
      "  --npm-registry-url <url>            Optional npm registry URL",
      "  --codex-version <version>           codex CLI version to install globally",
    ].join("\n"),
  );
}

export function shellQuote(value) {
  return `'${String(value).replaceAll("'", `'"'"'`)}'`;
}

export function parseEnvFile(text) {
  const env = {};
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) {
      continue;
    }

    const separatorIndex = line.indexOf("=");
    if (separatorIndex <= 0) {
      continue;
    }

    const key = line.slice(0, separatorIndex).trim();
    const rawValue = line.slice(separatorIndex + 1).trim();
    if (!key) {
      continue;
    }

    let value = rawValue;
    if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
      try {
        value = JSON.parse(value);
      } catch {
        value = value.slice(1, -1);
      }
    }

    env[key] = String(value);
  }

  return env;
}

export async function fileExists(filePath) {
  try {
    await fs.lstat(filePath);
    return true;
  } catch {
    return false;
  }
}

export async function readExistingBrokerEnv(serviceRoot) {
  const envFilePath = path.join(serviceRoot, "config", "broker.env");
  if (!(await fileExists(envFilePath))) {
    return {};
  }

  return parseEnvFile(await fs.readFile(envFilePath, "utf8"));
}

export function buildSeedBrokerEnv(existingBrokerEnv) {
  const merged = {};
  for (const key of BROKER_ENV_PASSTHROUGH_KEYS) {
    const fromProcess = process.env[key];
    if (fromProcess !== undefined && fromProcess !== null && String(fromProcess).length > 0) {
      merged[key] = String(fromProcess);
      continue;
    }

    const fromExisting = existingBrokerEnv[key];
    if (fromExisting !== undefined && fromExisting !== null && String(fromExisting).length > 0) {
      merged[key] = String(fromExisting);
    }
  }

  return merged;
}

export function assertRequiredBrokerEnv(seedBrokerEnv) {
  const missing = ["SLACK_APP_TOKEN", "SLACK_BOT_TOKEN"].filter((key) => !seedBrokerEnv[key]);
  if (missing.length > 0) {
    throw new Error(`Missing required broker environment values: ${missing.join(", ")}. ` + "Provide them in the current shell env or the existing config/broker.env before bootstrap.");
  }
}

export async function ensureDir(dirPath) {
  await fs.mkdir(dirPath, { recursive: true });
}

export async function copyFileResolved(sourcePath, targetPath) {
  if (!(await fileExists(sourcePath))) {
    return;
  }

  await ensureDir(path.dirname(targetPath));
  await fs.copyFile(sourcePath, targetPath);
}

export async function copyDirectoryResolved(sourcePath, targetPath) {
  if (!(await fileExists(sourcePath))) {
    return;
  }

  await ensureDir(path.dirname(targetPath));
  await fs.cp(sourcePath, targetPath, {
    recursive: true,
    dereference: true,
    force: true,
  });
}

export async function writeTextFile(sourcePath, targetPath, fallback = "") {
  await ensureDir(path.dirname(targetPath));
  if (!(await fileExists(sourcePath))) {
    await fs.writeFile(targetPath, fallback, "utf8");
    return;
  }

  const content = await fs.readFile(sourcePath, "utf8");
  await fs.writeFile(targetPath, content, "utf8");
}

export async function buildPortableCodexHome(sourceCodexHome, targetCodexHome) {
  await ensureDir(targetCodexHome);

  for (const entry of CODEX_HOME_FILE_ENTRIES) {
    if (entry === "memory.md") {
      await writeTextFile(path.join(sourceCodexHome, entry), path.join(targetCodexHome, entry), "");
      continue;
    }

    if (entry === ".credentials.json") {
      continue;
    }

    await copyFileResolved(path.join(sourceCodexHome, entry), path.join(targetCodexHome, entry));
  }

  for (const entry of CODEX_HOME_DIRECTORY_ENTRIES) {
    await copyDirectoryResolved(path.join(sourceCodexHome, entry), path.join(targetCodexHome, entry));
  }
}
