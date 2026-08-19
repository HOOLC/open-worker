#!/usr/bin/env node

import { readFileSync } from "node:fs";

import fs from "node:fs/promises";

import os from "node:os";

import path from "node:path";

import { repoRoot, runCommand } from "./lib.mjs";
import {
  DEFAULT_SERVICE_ROOT,
  DEFAULT_ADMIN_LABEL,
  DEFAULT_WORKER_LABEL,
  DEFAULT_CLOUDFLARED_LABEL,
  DEFAULT_NODE_PATH,
  DEFAULT_CLOUDFLARED_PATH,
  DEFAULT_LAUNCHD_DAEMON_DIR,
  DEFAULT_CODEX_VERSION,
  DEFAULT_PACKAGE_INFO,
  RELEASE_METADATA_FILENAME,
  CODEX_HOME_FILE_ENTRIES,
  CODEX_HOME_DIRECTORY_ENTRIES,
  BROKER_ENV_PASSTHROUGH_KEYS,
  readDefaultPackageInfo,
  parseArgs,
  printHelp,
  shellQuote,
  parseEnvFile,
  fileExists,
  readExistingBrokerEnv,
  buildSeedBrokerEnv,
  assertRequiredBrokerEnv,
  ensureDir,
  copyFileResolved,
  copyDirectoryResolved,
  writeTextFile,
  buildPortableCodexHome,
} from "./macos-bootstrap-helpers-1.mjs";
import { removeLegacyLaunchAgent, writeLaunchdFiles, launchdDomain, useSudoForLaunchctl, runLaunchctl, bootout, bootstrap, kickstart } from "./macos-bootstrap-helpers-3.mjs";

export async function buildPortableGhConfigHome(sourceGhConfigHome, targetRuntimeHome) {
  if (!(await fileExists(sourceGhConfigHome))) {
    return;
  }

  const targetGhConfigHome = path.join(targetRuntimeHome, ".config", "gh");
  await ensureDir(targetGhConfigHome);
  await copyFileResolved(path.join(sourceGhConfigHome, "config.yml"), path.join(targetGhConfigHome, "config.yml"));
  await copyFileResolved(path.join(sourceGhConfigHome, "hosts.yml"), path.join(targetGhConfigHome, "hosts.yml"));
}

export async function initializeRuntimeData(dataRoot) {
  await ensureDir(path.join(dataRoot, "state"));
  await ensureDir(path.join(dataRoot, "jobs"));
  await ensureDir(path.join(dataRoot, "sessions"));
  await ensureDir(path.join(dataRoot, "logs", "raw"));
  await ensureDir(path.join(dataRoot, "logs", "sessions"));
  await ensureDir(path.join(dataRoot, "logs", "jobs"));
  await ensureDir(path.join(dataRoot, "repos"));
  await ensureDir(path.join(dataRoot, "runtime-home"));
  await ensureDir(path.join(dataRoot, "auth-profiles", "profiles"));
}

export function renderEnvFile(env) {
  return (
    Object.entries(env)
      .filter(([, value]) => value !== undefined && value !== null && String(value).length > 0)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, value]) => `${key}=${JSON.stringify(String(value))}`)
      .join("\n") + "\n"
  );
}

export function renderEnvironmentVariables(environment) {
  const entries = Object.entries(environment)
    .filter(([, value]) => value !== undefined && value !== null && String(value).length > 0)
    .flatMap(([key, value]) => [`    <key>${key}</key>`, `    <string>${value}</string>`]);
  return ["  <key>EnvironmentVariables</key>", "  <dict>", ...entries, "  </dict>"];
}

export function renderDaemonCommon({ label, runUser, homeDir, workingDirectory, stdoutPath, stderrPath }) {
  return [
    "  <key>Label</key>",
    `  <string>${label}</string>`,
    "  <key>UserName</key>",
    `  <string>${runUser}</string>`,
    ...renderEnvironmentVariables({
      HOME: homeDir,
      PATH: "/opt/homebrew/opt/node@24/bin:/opt/homebrew/bin:/usr/bin:/bin",
    }),
    "  <key>WorkingDirectory</key>",
    `  <string>${workingDirectory}</string>`,
    "  <key>RunAtLoad</key>",
    "  <true/>",
    "  <key>KeepAlive</key>",
    "  <true/>",
    "  <key>ProcessType</key>",
    "  <string>Background</string>",
    "  <key>StandardOutPath</key>",
    `  <string>${stdoutPath}</string>`,
    "  <key>StandardErrorPath</key>",
    `  <string>${stderrPath}</string>`,
  ];
}

export function renderPlist({ label, nodePath, launcherPath, repoRootPath, envFilePath, entryPoint, stdoutPath, stderrPath, runUser, homeDir }) {
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">',
    '<plist version="1.0">',
    "<dict>",
    ...renderDaemonCommon({
      label,
      runUser,
      homeDir,
      workingDirectory: repoRootPath,
      stdoutPath,
      stderrPath,
    }),
    "  <key>ProgramArguments</key>",
    "  <array>",
    `    <string>${nodePath}</string>`,
    `    <string>${launcherPath}</string>`,
    "    <string>--repo-root</string>",
    `    <string>${repoRootPath}</string>`,
    "    <string>--env-file</string>",
    `    <string>${envFilePath}</string>`,
    "    <string>--entry-point</string>",
    `    <string>${entryPoint}</string>`,
    "  </array>",
    "</dict>",
    "</plist>",
    "",
  ].join("\n");
}

export function renderCloudflaredPlist({ label, cloudflaredPath, token, serviceRoot, stdoutPath, stderrPath, runUser, homeDir }) {
  return [
    '<?xml version="1.0" encoding="UTF-8"?>',
    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">',
    '<plist version="1.0">',
    "<dict>",
    ...renderDaemonCommon({
      label,
      runUser,
      homeDir,
      workingDirectory: serviceRoot,
      stdoutPath,
      stderrPath,
    }),
    "  <key>ProgramArguments</key>",
    "  <array>",
    `    <string>${cloudflaredPath}</string>`,
    "    <string>tunnel</string>",
    "    <string>--no-autoupdate</string>",
    "    <string>--url</string>",
    "    <string>http://127.0.0.1:3000</string>",
    "    <string>run</string>",
    "    <string>--token</string>",
    `    <string>${token}</string>`,
    "  </array>",
    "</dict>",
    "</plist>",
    "",
  ].join("\n");
}

export function buildPaths(serviceRoot, options) {
  const remoteHome = os.homedir();
  const launchdDaemonDir = options.launchdDaemonDir;
  return {
    serviceRoot,
    repoRoot: serviceRoot,
    releasesRoot: path.join(serviceRoot, "releases"),
    currentAdminReleasePath: path.join(serviceRoot, "current-admin"),
    previousAdminReleasePath: path.join(serviceRoot, "previous-admin"),
    failedAdminReleasePath: path.join(serviceRoot, "failed-admin"),
    currentWorkerReleasePath: path.join(serviceRoot, "current-worker"),
    previousWorkerReleasePath: path.join(serviceRoot, "previous-worker"),
    failedWorkerReleasePath: path.join(serviceRoot, "failed-worker"),
    dataRoot: path.join(remoteHome, ".zork"),
    teamCodexHome: path.join(remoteHome, ".zork", "team-codex-home"),
    runtimeSupportRoot: path.join(serviceRoot, "runtime-support"),
    codexSupportHome: path.join(serviceRoot, "runtime-support", "codex"),
    agentsSupportHome: path.join(serviceRoot, "runtime-support", ".agents"),
    envDir: path.join(serviceRoot, "config"),
    adminEnvFile: path.join(serviceRoot, "config", "admin.env"),
    workerEnvFile: path.join(serviceRoot, "config", "worker.env"),
    logsDir: path.join(serviceRoot, "logs"),
    launchdDaemonDir,
    adminPlistPath: path.join(launchdDaemonDir, `${options.adminLabel}.plist`),
    workerPlistPath: path.join(launchdDaemonDir, `${options.workerLabel}.plist`),
    cloudflaredPlistPath: path.join(launchdDaemonDir, `${options.cloudflaredLabel}.plist`),
    legacyAdminAgentPath: path.join(remoteHome, "Library", "LaunchAgents", `${options.adminLabel}.plist`),
    legacyWorkerAgentPath: path.join(remoteHome, "Library", "LaunchAgents", `${options.workerLabel}.plist`),
    legacyCloudflaredAgentPath: path.join(remoteHome, "Library", "LaunchAgents", `${options.cloudflaredLabel}.plist`),
    adminStdoutPath: path.join(serviceRoot, "logs", "admin.launchd.out.log"),
    adminStderrPath: path.join(serviceRoot, "logs", "admin.launchd.err.log"),
    workerStdoutPath: path.join(serviceRoot, "logs", "worker.launchd.out.log"),
    workerStderrPath: path.join(serviceRoot, "logs", "worker.launchd.err.log"),
    cloudflaredStdoutPath: path.join(serviceRoot, "logs", "cloudflared.out.log"),
    cloudflaredStderrPath: path.join(serviceRoot, "logs", "cloudflared.err.log"),
  };
}

export function buildReleaseMetadata(target, packageName, packageVersion) {
  return {
    revision: null,
    shortRevision: null,
    branch: null,
    target,
    packageName,
    packageVersion,
    packageSpec: packageSpec(packageName, packageVersion),
    requestedVersion: packageVersion,
    installedAt: new Date().toISOString(),
    installedBy: os.userInfo().username,
    installedFromHost: os.hostname(),
    stateSchemaVersion: 3,
  };
}

export function packageSpec(packageName, version) {
  return `${packageName}@${version}`;
}

export function packageRootForInstallRoot(installRoot, packageName) {
  return path.join(installRoot, "node_modules", ...packageName.split("/"));
}

export function normalizePackageVersion(version) {
  const normalized = String(version || "").trim();
  if (!normalized || !/^[0-9A-Za-z][0-9A-Za-z._+-]*$/.test(normalized)) {
    throw new Error(`Invalid package version: ${version}`);
  }
  return normalized;
}

export function buildAdminEnv(paths, options, seedBrokerEnv) {
  const adminBaseUrl = seedBrokerEnv.ADMIN_BASE_URL || "http://127.0.0.1:3000";
  return {
    ...seedBrokerEnv,
    NODE_ENV: "production",
    PATH: "/opt/homebrew/opt/node@24/bin:/opt/homebrew/bin:/usr/bin:/bin",
    PORT: "3000",
    ADMIN_BASE_URL: adminBaseUrl,
    WORKER_PORT: "3001",
    WORKER_BIND_HOST: "127.0.0.1",
    WORKER_BASE_URL: "http://127.0.0.1:3001",
    BROKER_HTTP_BASE_URL: "http://127.0.0.1:3001",
    SERVICE_NAME: "slack-codex-broker-admin",
    SERVICE_ROOT: paths.serviceRoot,
    DATA_ROOT: paths.dataRoot,
    STATE_DIR: path.join(paths.dataRoot, "state"),
    JOBS_ROOT: path.join(paths.dataRoot, "jobs"),
    SESSIONS_ROOT: path.join(paths.dataRoot, "sessions"),
    REPOS_ROOT: path.join(paths.dataRoot, "repos"),
    LOG_DIR: path.join(paths.dataRoot, "logs"),
    CODEX_HOME: path.join(paths.dataRoot, "codex-home"),
    CODEX_TEAM_HOME: paths.teamCodexHome,
    CODEX_HOST_HOME_PATH: paths.codexSupportHome,
    CODEX_AUTH_JSON_PATH: path.join(paths.dataRoot, "codex-home", "auth.json"),
    CODEX_APP_SERVER_PORT: "4590",
    ADMIN_LAUNCHD_LABEL: options.adminLabel,
    WORKER_LAUNCHD_LABEL: options.workerLabel,
    ADMIN_PLIST_PATH: paths.adminPlistPath,
    RELEASE_ADMIN_PACKAGE_NAME: options.adminPackageName,
    RELEASE_WORKER_PACKAGE_NAME: options.workerPackageName,
    ...(options.npmRegistryUrl ? { RELEASE_NPM_REGISTRY_URL: options.npmRegistryUrl } : {}),
    RELEASES_ROOT: paths.releasesRoot,
    CURRENT_ADMIN_RELEASE_PATH: paths.currentAdminReleasePath,
    PREVIOUS_ADMIN_RELEASE_PATH: paths.previousAdminReleasePath,
    FAILED_ADMIN_RELEASE_PATH: paths.failedAdminReleasePath,
    CURRENT_WORKER_RELEASE_PATH: paths.currentWorkerReleasePath,
    PREVIOUS_WORKER_RELEASE_PATH: paths.previousWorkerReleasePath,
    FAILED_WORKER_RELEASE_PATH: paths.failedWorkerReleasePath,
    WORKER_PLIST_PATH: paths.workerPlistPath,
  };
}

export function buildWorkerEnv(paths, options, seedBrokerEnv) {
  const adminBaseUrl = seedBrokerEnv.ADMIN_BASE_URL || "http://127.0.0.1:3000";
  return {
    ...seedBrokerEnv,
    NODE_ENV: "production",
    PATH: "/opt/homebrew/opt/node@24/bin:/opt/homebrew/bin:/usr/bin:/bin",
    PORT: "3001",
    ADMIN_BASE_URL: adminBaseUrl,
    WORKER_PORT: "3001",
    WORKER_BIND_HOST: "127.0.0.1",
    WORKER_BASE_URL: "http://127.0.0.1:3001",
    BROKER_HTTP_BASE_URL: "http://127.0.0.1:3001",
    SERVICE_NAME: "slack-codex-broker-worker",
    SERVICE_ROOT: paths.serviceRoot,
    DATA_ROOT: paths.dataRoot,
    STATE_DIR: path.join(paths.dataRoot, "state"),
    JOBS_ROOT: path.join(paths.dataRoot, "jobs"),
    SESSIONS_ROOT: path.join(paths.dataRoot, "sessions"),
    REPOS_ROOT: path.join(paths.dataRoot, "repos"),
    LOG_DIR: path.join(paths.dataRoot, "logs"),
    CODEX_HOME: path.join(paths.dataRoot, "codex-home"),
    CODEX_TEAM_HOME: paths.teamCodexHome,
    CODEX_HOST_HOME_PATH: paths.codexSupportHome,
    CODEX_AUTH_JSON_PATH: path.join(paths.dataRoot, "codex-home", "auth.json"),
    CODEX_APP_SERVER_PORT: "4590",
    ADMIN_LAUNCHD_LABEL: options.adminLabel,
    WORKER_LAUNCHD_LABEL: options.workerLabel,
    ADMIN_PLIST_PATH: paths.adminPlistPath,
    WORKER_PLIST_PATH: paths.workerPlistPath,
    RELEASE_ADMIN_PACKAGE_NAME: options.adminPackageName,
    RELEASE_WORKER_PACKAGE_NAME: options.workerPackageName,
    ...(options.npmRegistryUrl ? { RELEASE_NPM_REGISTRY_URL: options.npmRegistryUrl } : {}),
    RELEASES_ROOT: paths.releasesRoot,
    CURRENT_ADMIN_RELEASE_PATH: paths.currentAdminReleasePath,
    PREVIOUS_ADMIN_RELEASE_PATH: paths.previousAdminReleasePath,
    FAILED_ADMIN_RELEASE_PATH: paths.failedAdminReleasePath,
    CURRENT_WORKER_RELEASE_PATH: paths.currentWorkerReleasePath,
    PREVIOUS_WORKER_RELEASE_PATH: paths.previousWorkerReleasePath,
    FAILED_WORKER_RELEASE_PATH: paths.failedWorkerReleasePath,
  };
}

export async function installTooling(options) {
  const npmPath = options.npmPath || path.join(path.dirname(options.nodePath), "npm");
  runCommand(npmPath, ["install", "-g", "--force", `@openai/codex@${options.codexVersion}`]);
}

export async function prepareSharedHomes(paths) {
  const sourceCodexHome = path.join(os.homedir(), ".codex");
  const sourceGhConfigHome = path.join(os.homedir(), ".config", "gh");
  const sourceAgentsHome = path.join(os.homedir(), ".agents");

  await ensureDir(paths.runtimeSupportRoot);
  await ensureDir(paths.dataRoot);
  await initializeRuntimeData(paths.dataRoot);
  await buildPortableCodexHome(sourceCodexHome, path.join(paths.dataRoot, "codex-home"));
  await buildPortableGhConfigHome(sourceGhConfigHome, path.join(paths.dataRoot, "runtime-home"));
  await copyDirectoryResolved(sourceAgentsHome, paths.agentsSupportHome);
}

export async function ensureInitialReleases(paths, options) {
  const version = normalizePackageVersion(options.packageVersion);
  const admin = await ensureInitialReleaseTarget(
    paths,
    options,
    {
      target: "admin",
      packageName: options.adminPackageName,
      currentReleasePath: paths.currentAdminReleasePath,
    },
    version,
  );
  const worker = await ensureInitialReleaseTarget(
    paths,
    options,
    {
      target: "worker",
      packageName: options.workerPackageName,
      currentReleasePath: paths.currentWorkerReleasePath,
    },
    version,
  );
  return {
    admin,
    worker,
  };
}

export async function ensureInitialReleaseTarget(paths, options, targetOptions, version) {
  const installRoot = path.join(paths.releasesRoot, targetOptions.target, `npm-${version}`);
  const releaseRoot = packageRootForInstallRoot(installRoot, targetOptions.packageName);
  const npmPath = options.npmPath || path.join(path.dirname(options.nodePath), "npm");
  await ensureDir(path.dirname(installRoot));
  if (!(await fileExists(releaseRoot))) {
    await fs.rm(installRoot, { recursive: true, force: true });
    runCommand(npmPath, ["install", "--prefix", installRoot, "--omit=dev", "--ignore-scripts", "--no-audit", "--no-fund", ...(options.npmRegistryUrl ? ["--registry", options.npmRegistryUrl] : []), packageSpec(targetOptions.packageName, version)]);
  }
  await fs.writeFile(path.join(releaseRoot, RELEASE_METADATA_FILENAME), `${JSON.stringify(buildReleaseMetadata(targetOptions.target, targetOptions.packageName, version), null, 2)}\n`, "utf8");

  await fs.rm(targetOptions.currentReleasePath, { recursive: true, force: true });
  await fs.symlink(path.relative(path.dirname(targetOptions.currentReleasePath), releaseRoot), targetOptions.currentReleasePath, "dir");
  return {
    packageName: targetOptions.packageName,
    packageVersion: version,
    releaseRoot,
  };
}

export function shouldInstallLaunchDaemonWithSudo(plistPath) {
  return path.resolve(plistPath).startsWith(`${DEFAULT_LAUNCHD_DAEMON_DIR}${path.sep}`) && typeof process.getuid === "function" && process.getuid() !== 0;
}

export async function writeLaunchDaemonPlist(plistPath, plist) {
  if (!shouldInstallLaunchDaemonWithSudo(plistPath)) {
    await ensureDir(path.dirname(plistPath));
    await fs.writeFile(plistPath, plist, "utf8");
    await fs.chmod(plistPath, 0o644);
    if (path.resolve(plistPath).startsWith(`${DEFAULT_LAUNCHD_DAEMON_DIR}${path.sep}`)) {
      try {
        runCommand("chown", ["root:wheel", plistPath]);
      } catch {
        // Ownership correction is best effort when already running as root in tests.
      }
    }
    return;
  }

  const tempPath = path.join(os.tmpdir(), `agent-session-broker-${path.basename(plistPath)}.${process.pid}.tmp`);
  await fs.writeFile(tempPath, plist, "utf8");
  try {
    runCommand("sudo", ["install", "-o", "root", "-g", "wheel", "-m", "0644", tempPath, plistPath]);
  } finally {
    await fs.rm(tempPath, { force: true });
  }
}
