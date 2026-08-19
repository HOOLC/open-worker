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

export async function removeLegacyLaunchAgent(label, plistPath) {
  if (!(await fileExists(plistPath))) {
    return;
  }

  try {
    runCommand("launchctl", ["bootout", `gui/${process.getuid()}`, plistPath]);
  } catch {
    // The old GUI launchd domain may be absent; removing the stale plist is the important part.
  }
  await fs.rm(plistPath, { force: true });
  console.error(`Removed legacy LaunchAgent for ${label}: ${plistPath}`);
}

export async function writeLaunchdFiles(paths, options, seedBrokerEnv) {
  const adminLauncherPath = path.join(paths.currentAdminReleasePath, "scripts", "ops", "macos-launchd-launcher.mjs");
  const workerLauncherPath = path.join(paths.currentWorkerReleasePath, "scripts", "ops", "macos-launchd-launcher.mjs");
  const adminPlist = renderPlist({
    label: options.adminLabel,
    nodePath: options.nodePath,
    launcherPath: adminLauncherPath,
    repoRootPath: paths.currentAdminReleasePath,
    envFilePath: paths.adminEnvFile,
    entryPoint: "dist/src/admin-index.js",
    stdoutPath: paths.adminStdoutPath,
    stderrPath: paths.adminStderrPath,
    runUser: options.runUser,
    homeDir: os.homedir(),
  });
  const workerPlist = renderPlist({
    label: options.workerLabel,
    nodePath: options.nodePath,
    launcherPath: workerLauncherPath,
    repoRootPath: paths.currentWorkerReleasePath,
    envFilePath: paths.workerEnvFile,
    entryPoint: "dist/src/worker-index.js",
    stdoutPath: paths.workerStdoutPath,
    stderrPath: paths.workerStderrPath,
    runUser: options.runUser,
    homeDir: os.homedir(),
  });
  const cloudflaredPlist = seedBrokerEnv.CLOUDFLARED_TUNNEL_TOKEN
    ? renderCloudflaredPlist({
        label: options.cloudflaredLabel,
        cloudflaredPath: options.cloudflaredPath,
        token: seedBrokerEnv.CLOUDFLARED_TUNNEL_TOKEN,
        serviceRoot: paths.serviceRoot,
        stdoutPath: paths.cloudflaredStdoutPath,
        stderrPath: paths.cloudflaredStderrPath,
        runUser: options.runUser,
        homeDir: os.homedir(),
      })
    : null;

  await ensureDir(paths.envDir);
  await writeLaunchDaemonPlist(paths.adminPlistPath, adminPlist);
  await writeLaunchDaemonPlist(paths.workerPlistPath, workerPlist);
  if (cloudflaredPlist) {
    await writeLaunchDaemonPlist(paths.cloudflaredPlistPath, cloudflaredPlist);
  }
  await removeLegacyLaunchAgent(options.adminLabel, paths.legacyAdminAgentPath);
  await removeLegacyLaunchAgent(options.workerLabel, paths.legacyWorkerAgentPath);
  await removeLegacyLaunchAgent(options.cloudflaredLabel, paths.legacyCloudflaredAgentPath);
  await fs.writeFile(paths.adminEnvFile, renderEnvFile(buildAdminEnv(paths, options, seedBrokerEnv)), "utf8");
  await fs.writeFile(paths.workerEnvFile, renderEnvFile(buildWorkerEnv(paths, options, seedBrokerEnv)), "utf8");
  return Boolean(cloudflaredPlist);
}

export function launchdDomain() {
  return "system";
}

export function useSudoForLaunchctl(plistPath) {
  return shouldInstallLaunchDaemonWithSudo(plistPath);
}

export function runLaunchctl(args, plistPath) {
  if (useSudoForLaunchctl(plistPath)) {
    runCommand("sudo", ["launchctl", ...args]);
    return;
  }
  runCommand("launchctl", args);
}

export function bootout(plistPath) {
  try {
    runLaunchctl(["bootout", launchdDomain(), plistPath], plistPath);
  } catch {
    // ignore missing services
  }
}

export function bootstrap(plistPath) {
  runLaunchctl(["bootstrap", launchdDomain(), plistPath], plistPath);
}

export function kickstart(label, plistPath) {
  runLaunchctl(["kickstart", "-k", `${launchdDomain()}/${label}`], plistPath);
}
