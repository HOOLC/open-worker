#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const DEFAULT_PROFILE_NAME = "primary";

function usage() {
  console.error(
    [
      "Usage:",
      "  DATA_ROOT=<path> node scripts/ops/auth-profiles.mjs status",
      "  DATA_ROOT=<path> node scripts/ops/auth-profiles.mjs bootstrap [--profile <name>] [--refresh-host]",
      "  DATA_ROOT=<path> node scripts/ops/auth-profiles.mjs list",
      "  DATA_ROOT=<path> node scripts/ops/auth-profiles.mjs import --name <profile> --from <path>",
      "  DATA_ROOT=<path> node scripts/ops/auth-profiles.mjs import-host --name <profile>",
    ].join("\n"),
  );
}

function requireOption(value, name) {
  if (!value) {
    throw new Error(`Missing required option: ${name}`);
  }
  return value;
}

function parseArgs(argv) {
  const args = [...argv];
  const command = args.shift();
  const positional = [];
  const options = {
    profileName: undefined,
    sourcePath: undefined,
    refreshHost: false,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case "--profile":
      case "--name":
        options.profileName = requireOption(args.shift(), arg);
        break;
      case "--from":
        options.sourcePath = requireOption(args.shift(), "--from");
        break;
      case "--refresh-host":
        options.refreshHost = true;
        break;
      default:
        positional.push(arg);
        break;
    }
  }

  return { command, positional, options };
}

async function ensureDir(dirPath) {
  await fs.mkdir(dirPath, { recursive: true });
}

function sanitizeProfileName(name) {
  const trimmed = name.trim();
  if (!trimmed) {
    throw new Error("profile name must not be empty");
  }

  const normalized = trimmed.replace(/[^a-zA-Z0-9._-]+/g, "-");
  if (!normalized) {
    throw new Error(`invalid profile name: ${name}`);
  }

  return normalized;
}

async function pathInfo(filePath) {
  try {
    const stat = await fs.lstat(filePath);
    const base = {
      path: filePath,
      exists: true,
      isSymlink: stat.isSymbolicLink(),
    };
    if (stat.isSymbolicLink()) {
      const linkTarget = await fs.readlink(filePath);
      const resolvedTarget = path.resolve(path.dirname(filePath), linkTarget);
      const targetStat = await fs.stat(filePath);
      return {
        ...base,
        linkTarget,
        resolvedTarget,
        size: targetStat.size,
        mtime: targetStat.mtime.toISOString(),
      };
    }

    return {
      ...base,
      size: stat.size,
      mtime: stat.mtime.toISOString(),
    };
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return {
        path: filePath,
        exists: false,
      };
    }

    throw error;
  }
}

async function backupFileIfNeeded(filePath, backupDir, backupName) {
  try {
    await fs.lstat(filePath);
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return null;
    }
    throw error;
  }

  await ensureDir(backupDir);
  const backupPath = path.join(backupDir, backupName ?? path.basename(filePath));
  await fs.cp(filePath, backupPath, { dereference: false, force: true, recursive: true });
  return backupPath;
}

async function ensureManagedSymlink({ linkPath, targetPath, backupDir, backupName }) {
  const relativeTarget = path.relative(path.dirname(linkPath), targetPath);
  let current;
  try {
    current = await fs.lstat(linkPath);
  } catch (error) {
    if (!(error && typeof error === "object" && "code" in error && error.code === "ENOENT")) {
      throw error;
    }
  }

  if (current?.isSymbolicLink()) {
    const linkTarget = await fs.readlink(linkPath);
    const resolvedTarget = path.resolve(path.dirname(linkPath), linkTarget);
    if (resolvedTarget === path.resolve(targetPath)) {
      return null;
    }
  }

  const backupPath = await backupFileIfNeeded(linkPath, backupDir, backupName);
  await fs.rm(linkPath, { force: true, recursive: true });
  await fs.symlink(relativeTarget, linkPath, "file");
  return backupPath;
}

async function fileExists(filePath) {
  try {
    await fs.access(filePath);
    return true;
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return false;
    }

    throw error;
  }
}

function resolvePaths() {
  const dataRootSource = path.resolve(process.env.DATA_ROOT?.trim() || path.join(os.homedir(), ".zork"));
  const managedRoot = path.join(dataRootSource, "auth-profiles");
  const profilesRoot = path.join(managedRoot, "profiles");
  return {
    dataRootSource,
    managedRoot,
    profilesRoot,
    hostManagedAuthPath: path.join(managedRoot, "host", "auth.json"),
    legacyManagedAuthPath: path.join(managedRoot, "legacy", "auth.json"),
    hostAuthPath: path.join(os.homedir(), ".codex", "auth.json"),
    activeAuthPath: path.join(dataRootSource, "codex-home", "auth.json"),
  };
}

function profilePath(paths, profileName) {
  return path.join(paths.profilesRoot, `${sanitizeProfileName(profileName)}.json`);
}

async function ensureHostManagedCopy(paths, refreshHost) {
  await ensureDir(path.dirname(paths.hostManagedAuthPath));
  if (!refreshHost && (await fileExists(paths.hostManagedAuthPath))) {
    return false;
  }

  await fs.copyFile(paths.hostAuthPath, paths.hostManagedAuthPath);
  return true;
}

async function seedInitialProfile(paths, initialProfileName) {
  const initialProfilePath = profilePath(paths, initialProfileName);
  await ensureDir(paths.profilesRoot);

  if (await fileExists(initialProfilePath)) {
    return initialProfilePath;
  }

  if (await fileExists(paths.legacyManagedAuthPath)) {
    await fs.copyFile(paths.legacyManagedAuthPath, initialProfilePath);
    return initialProfilePath;
  }

  await fs.copyFile(paths.activeAuthPath, initialProfilePath);
  return initialProfilePath;
}

async function bootstrapProfiles(options) {
  const paths = resolvePaths();
  const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  const backupDir = path.join(paths.managedRoot, "backups", stamp);
  const initialProfileName = sanitizeProfileName(options.profileName || DEFAULT_PROFILE_NAME);

  const copiedHost = await ensureHostManagedCopy(paths, options.refreshHost);
  const initialProfilePath = await seedInitialProfile(paths, initialProfileName);

  const hostBackup = await ensureManagedSymlink({
    linkPath: paths.hostAuthPath,
    targetPath: paths.hostManagedAuthPath,
    backupDir,
    backupName: "host-auth.json",
  });
  return {
    ok: true,
    paths,
    copiedHost,
    initialProfileName,
    initialProfilePath,
    hostBackup,
  };
}

async function listProfiles(options) {
  const paths = resolvePaths();
  await ensureDir(paths.profilesRoot);
  const entries = await fs.readdir(paths.profilesRoot);
  const profiles = [];
  for (const entry of entries.sort()) {
    if (!entry.endsWith(".json")) {
      continue;
    }
    profiles.push(await pathInfo(path.join(paths.profilesRoot, entry)));
  }

  return {
    managedRoot: paths.managedRoot,
    profiles,
  };
}

async function importProfile(options) {
  const profileName = sanitizeProfileName(requireOption(options.profileName, "--name"));
  const sourcePath = requireOption(options.sourcePath, "--from");
  const paths = resolvePaths();
  const targetPath = profilePath(paths, profileName);
  await ensureDir(paths.profilesRoot);
  await fs.copyFile(sourcePath, targetPath);

  return {
    ok: true,
    profileName,
    sourcePath,
    targetPath,
  };
}

async function importHostProfile(options) {
  return await importProfile({
    ...options,
    sourcePath: path.join(os.homedir(), ".codex", "auth.json"),
  });
}

async function getStatus(options) {
  const paths = resolvePaths();

  return {
    dataRootSource: paths.dataRootSource,
    managedRoot: paths.managedRoot,
    hostAuth: await pathInfo(paths.hostAuthPath),
    activeAuth: await pathInfo(paths.activeAuthPath),
    hostManagedAuth: await pathInfo(paths.hostManagedAuthPath),
    profiles: await listProfiles(options),
  };
}

async function main() {
  const { command, positional, options } = parseArgs(process.argv.slice(2));
  if (!command) {
    usage();
    process.exitCode = 1;
    return;
  }

  let result;
  switch (command) {
    case "status":
      result = await getStatus(options);
      break;
    case "bootstrap":
      result = await bootstrapProfiles(options);
      break;
    case "list":
      result = await listProfiles(options);
      break;
    case "import":
      result = await importProfile(options);
      break;
    case "import-host":
      result = await importHostProfile(options);
      break;
    default:
      usage();
      process.exitCode = 1;
      return;
  }

  console.log(JSON.stringify(result, null, 2));
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack || error.message : String(error));
  process.exitCode = 1;
});
