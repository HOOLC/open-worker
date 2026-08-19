#!/usr/bin/env node

import { execFile } from "node:child_process";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";

import { AppServerClient } from "../../src/services/codex/app-server-client.js";
import { AppServerProcess } from "../../src/services/codex/app-server-process.js";

const execFileAsync = promisify(execFile);
const EXPECTED_CONTENT = "REAL_CODEX_CODING_SMOKE_OK";

interface CliOptions {
  readonly json: boolean;
  readonly keepWorkspace: boolean;
  readonly outputDir?: string | undefined;
  readonly timeoutMs: number;
}

interface CodingSmokeReport {
  readonly ok: boolean;
  readonly workspacePath: string;
  readonly workspaceRetained: boolean;
  readonly expectedContent: string;
  readonly actualContent: string | null;
  readonly finalMessage: string;
  readonly checkStdout: string;
  readonly error?: string | undefined;
}

interface PathSnapshot {
  readonly exists: boolean;
  readonly kind?: "file" | "symlink" | "other" | undefined;
  readonly content?: Buffer | undefined;
  readonly linkTarget?: string | undefined;
  readonly mode?: number | undefined;
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2));
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "codex-coding-smoke-"));
  const workspacePath = path.join(root, "workspace");
  const codexHome = path.join(root, "codex-home");
  const runtimeAgentPath = path.join(os.homedir(), ".codex", "AGENT.md");
  const runtimeAgentSnapshot = await snapshotPath(runtimeAgentPath);
  let report: CodingSmokeReport | undefined;

  await prepareWorkspace(workspacePath);

  const processManager = new AppServerProcess({
    brokerHttpBaseUrl: "http://127.0.0.1:3300",
    zorkBinDir: path.join(codexHome, "bin"),
    codexHome,
    port: 4601,
    openAiApiKey: process.env.OPENAI_API_KEY,
    authJsonPath: path.join(os.homedir(), ".codex", "auth.json"),
  });
  const client = new AppServerClient({
    url: processManager.url,
    serviceName: "codex-coding-smoke",
    brokerHttpBaseUrl: "http://127.0.0.1:3300",
    reposRoot: path.join(codexHome, "repos"),
  });

  try {
    await processManager.start();
    await client.connect();
    await client.ensureAuthenticated();

    const threadId = await client.ensureThread({
      channelId: "C-CODING-SMOKE",
      rootThreadTs: "thread-coding-smoke",
      workspacePath,
    });

    const started = await client.startTurn(threadId, workspacePath, [
      {
        type: "text",
        text: ["This is a real Codex coding smoke test.", `Change target.txt so its entire contents are ${EXPECTED_CONTENT} followed by one newline.`, "Do not edit check.mjs.", "Run `node check.mjs` in this workspace.", "Reply with CODING_SMOKE_DONE only after the check passes."].join("\n"),
        text_elements: [],
      },
    ]);

    const result = await withTimeout(started.completion, options.timeoutMs);
    const actualContent = await readTarget(workspacePath);
    const check = await runCheck(workspacePath);
    const ok = actualContent.trim() === EXPECTED_CONTENT && check.ok;
    report = {
      ok,
      workspacePath: formatCodingSmokeWorkspacePath(workspacePath),
      workspaceRetained: !ok || options.keepWorkspace,
      expectedContent: EXPECTED_CONTENT,
      actualContent,
      finalMessage: result.finalMessage,
      checkStdout: check.stdout,
      error: check.error ? sanitizeCodingSmokeReportText(check.error) : undefined,
    };
  } catch (error) {
    report = {
      ok: false,
      workspacePath: formatCodingSmokeWorkspacePath(workspacePath),
      workspaceRetained: true,
      expectedContent: EXPECTED_CONTENT,
      actualContent: await readTarget(workspacePath).catch(() => null),
      finalMessage: "",
      checkStdout: "",
      error: sanitizeCodingSmokeReportText(formatError(error)),
    };
  } finally {
    await client.close();
    await processManager.stop();
    await restorePath(runtimeAgentPath, runtimeAgentSnapshot);
  }

  printReport(report, options.json);
  if (options.outputDir) {
    const reportFile = await writeCodingSmokeBundle(options.outputDir, report);
    if (!options.json) {
      console.log(`wrote coding smoke bundle: ${path.basename(reportFile)}`);
    }
  }
  if (report.ok && !options.keepWorkspace) {
    await fs.rm(root, { force: true, recursive: true });
  }
  if (!report.ok) {
    process.exitCode = 1;
  }
}

async function snapshotPath(filePath: string): Promise<PathSnapshot> {
  const stat = await fs.lstat(filePath).catch(() => null);
  if (!stat) {
    return { exists: false };
  }

  if (stat.isSymbolicLink()) {
    return {
      exists: true,
      kind: "symlink",
      linkTarget: await fs.readlink(filePath),
    };
  }

  if (stat.isFile()) {
    return {
      exists: true,
      kind: "file",
      content: await fs.readFile(filePath),
      mode: stat.mode,
    };
  }

  return {
    exists: true,
    kind: "other",
  };
}

async function restorePath(filePath: string, snapshot: PathSnapshot): Promise<void> {
  if (snapshot.kind === "other") {
    return;
  }

  await fs.rm(filePath, {
    force: true,
    recursive: true,
  });

  if (!snapshot.exists) {
    return;
  }

  await fs.mkdir(path.dirname(filePath), { recursive: true });
  if (snapshot.kind === "symlink" && snapshot.linkTarget) {
    await fs.symlink(snapshot.linkTarget, filePath);
    return;
  }

  if (snapshot.kind === "file" && snapshot.content) {
    await fs.writeFile(filePath, snapshot.content, {
      mode: snapshot.mode,
    });
  }
}

function parseArgs(argv: readonly string[]): CliOptions {
  const args = [...argv];
  const options: {
    json: boolean;
    keepWorkspace: boolean;
    outputDir?: string | undefined;
    timeoutMs: number;
  } = {
    json: false,
    keepWorkspace: false,
    timeoutMs: 180_000,
  };

  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg || arg === "--") continue;
    if (arg === "--json") {
      options.json = true;
      continue;
    }
    if (arg === "--keep-workspace") {
      options.keepWorkspace = true;
      continue;
    }
    if (arg === "--output-dir") {
      const value = args[index + 1];
      if (!value) throw new Error("Missing value for --output-dir");
      options.outputDir = value;
      index += 1;
      continue;
    }
    if (arg.startsWith("--output-dir=")) {
      options.outputDir = arg.slice("--output-dir=".length);
      continue;
    }
    if (arg === "--timeout-ms") {
      const value = args[index + 1];
      if (!value) throw new Error("Missing value for --timeout-ms");
      options.timeoutMs = Number.parseInt(value, 10);
      index += 1;
      continue;
    }
    if (arg.startsWith("--timeout-ms=")) {
      options.timeoutMs = Number.parseInt(arg.slice("--timeout-ms=".length), 10);
      continue;
    }
    throw new Error(`Unknown option: ${arg}`);
  }

  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    throw new Error("--timeout-ms must be a positive integer");
  }
  return options;
}

async function prepareWorkspace(workspacePath: string): Promise<void> {
  await fs.mkdir(workspacePath, { recursive: true });
  await fs.writeFile(path.join(workspacePath, "target.txt"), "BROKEN\n", "utf8");
  await fs.writeFile(
    path.join(workspacePath, "check.mjs"),
    ['import fs from "node:fs";', "", 'const actual = fs.readFileSync("target.txt", "utf8").trim();', `if (actual !== "${EXPECTED_CONTENT}") {`, "  console.error(`target.txt=${actual}`);", "  process.exit(1);", "}", 'console.log("coding smoke passed");', ""].join("\n"),
    "utf8",
  );
}

async function readTarget(workspacePath: string): Promise<string> {
  return await fs.readFile(path.join(workspacePath, "target.txt"), "utf8");
}

async function runCheck(workspacePath: string): Promise<{ readonly ok: boolean; readonly stdout: string; readonly error?: string | undefined }> {
  try {
    const result = await execFileAsync(process.execPath, ["check.mjs"], { cwd: workspacePath });
    return {
      ok: true,
      stdout: result.stdout.trim(),
    };
  } catch (error) {
    return {
      ok: false,
      stdout: "",
      error: formatError(error),
    };
  }
}

async function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`Timed out after ${timeoutMs}ms`)), timeoutMs);
  });
  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

function printReport(report: CodingSmokeReport, json: boolean): void {
  const sanitized = sanitizeCodingSmokeReport(report);
  if (json) {
    console.log(JSON.stringify(sanitized, null, 2));
    return;
  }

  console.log(`Real Codex coding smoke: ${sanitized.ok ? "ok" : "failed"}`);
  console.log(`workspace: ${sanitized.workspacePath}`);
  if (sanitized.workspaceRetained) console.log("workspace retained: true");
  console.log(`expected: ${sanitized.expectedContent}`);
  console.log(`actual: ${sanitized.actualContent ?? "missing"}`);
  if (sanitized.checkStdout) console.log(`check: ${sanitized.checkStdout}`);
  if (sanitized.error) console.log(`error: ${sanitized.error}`);
}

async function writeCodingSmokeBundle(outputDir: string, report: CodingSmokeReport): Promise<string> {
  await fs.mkdir(outputDir, { recursive: true });
  const reportFile = path.join(outputDir, "codex-coding-smoke-report.json");
  await fs.writeFile(reportFile, `${JSON.stringify(sanitizeCodingSmokeReport(report), null, 2)}\n`);
  return reportFile;
}

function sanitizeCodingSmokeReport(report: CodingSmokeReport): CodingSmokeReport {
  return {
    ...report,
    workspacePath: sanitizeCodingSmokeReportText(report.workspacePath),
    actualContent: report.actualContent ? sanitizeCodingSmokeReportText(report.actualContent) : report.actualContent,
    finalMessage: sanitizeCodingSmokeReportText(report.finalMessage),
    checkStdout: sanitizeCodingSmokeReportText(report.checkStdout),
    error: report.error ? sanitizeCodingSmokeReportText(report.error) : undefined,
  };
}

function formatError(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function formatCodingSmokeWorkspacePath(workspacePath: string): string {
  return `${path.basename(path.dirname(workspacePath))}/${path.basename(workspacePath)}`;
}

export function sanitizeCodingSmokeReportText(value: string): string {
  return value.replace(/(["'])(\/[^"']+)\1/gu, (_match, quote: string, filePath: string) => `${quote}${path.basename(filePath)}${quote}`).replace(/(^|[\s=])\/(?!\/)([^\s'"]+)/gu, (_match, prefix: string, filePathTail: string) => `${prefix}${path.basename(`/${filePathTail}`)}`);
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  void main().catch((error) => {
    console.error(sanitizeCodingSmokeReportText(formatError(error)));
    process.exitCode = 1;
  });
}
