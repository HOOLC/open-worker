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
  removeLegacyLaunchAgent,
  writeLaunchdFiles,
  launchdDomain,
  useSudoForLaunchctl,
  runLaunchctl,
  bootout,
  bootstrap,
  kickstart,
} from "./macos-bootstrap-helpers.mjs";

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const paths = buildPaths(options.serviceRoot, options);
  const existingBrokerEnv = await readExistingBrokerEnv(paths.serviceRoot);
  const seedBrokerEnv = buildSeedBrokerEnv(existingBrokerEnv);
  assertRequiredBrokerEnv(seedBrokerEnv);

  await prepareSharedHomes(paths);
  await installTooling(options);
  const initialReleases = await ensureInitialReleases(paths, options);
  const cloudflaredConfigured = await writeLaunchdFiles(paths, options, seedBrokerEnv);

  bootout(paths.adminPlistPath);
  bootstrap(paths.adminPlistPath);
  kickstart(options.adminLabel, paths.adminPlistPath);

  if (options.startWorker) {
    bootout(paths.workerPlistPath);
    bootstrap(paths.workerPlistPath);
    kickstart(options.workerLabel, paths.workerPlistPath);
  }

  if (cloudflaredConfigured) {
    bootout(paths.cloudflaredPlistPath);
    bootstrap(paths.cloudflaredPlistPath);
    kickstart(options.cloudflaredLabel, paths.cloudflaredPlistPath);
  }

  console.log(
    JSON.stringify(
      {
        ok: true,
        serviceRoot: paths.serviceRoot,
        adminPlistPath: paths.adminPlistPath,
        workerPlistPath: paths.workerPlistPath,
        cloudflaredPlistPath: cloudflaredConfigured ? paths.cloudflaredPlistPath : null,
        currentAdminReleasePath: paths.currentAdminReleasePath,
        currentWorkerReleasePath: paths.currentWorkerReleasePath,
        initialReleases,
        workerStarted: options.startWorker,
        cloudflaredStarted: cloudflaredConfigured,
      },
      null,
      2,
    ),
  );
}

await main();
