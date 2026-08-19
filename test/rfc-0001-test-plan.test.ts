import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { collectRfc0001TestPlanVerification, parseRfc0001TestPlanArgs } from "./manual/run-rfc-0001-test-plan.js";

const repoRoot = process.cwd();

const requiredScripts = {
  "format:check": "oxfmt --check .",
  lint: "oxlint . --deny-warnings",
  build: "pnpm build:admin-ui && tsc -p tsconfig.json && node scripts/build/copy-static-assets.mjs",
  test: "vitest run --no-file-parallelism",
  "test:e2e:feishu-mock": "vitest run test/feishu-e2e.test.ts test/chat-routes.e2e.test.ts",
  "rfc:feishu-audit": "tsx test/manual/run-rfc-0001-local-audit.ts",
  "rfc:feishu-completion-audit": "tsx test/manual/run-rfc-0001-completion-audit.ts",
  "rfc:feishu-test-plan": "tsx test/manual/run-rfc-0001-test-plan.ts",
  "manual:codex-coding-smoke": "tsx test/manual/run-real-codex-coding-smoke.ts",
  "manual:feishu-smoke": "tsx test/manual/run-real-feishu-smoke.ts --",
};

const verifierEnv = {
  SLACK_APP_TOKEN: "xapp-test",
  SLACK_BOT_TOKEN: "xoxb-test",
  FEISHU_ENABLED: "true",
  FEISHU_APP_ID: "cli_test",
  FEISHU_APP_SECRET: "test-secret",
  FEISHU_BOT_OPEN_ID: "ou_test",
  FEISHU_DOMAIN: "feishu",
  FEISHU_API_BASE_URL: "https://open.feishu.cn/open-apis",
  FEISHU_GROUP_MESSAGE_MODE: "all",
  FEISHU_STARTUP_REQUIRED: "true",
  LOG_RAW_FEISHU_EVENTS: "false",
} satisfies Record<string, string>;

describe("RFC 0001 test plan verifier", () => {
  it("accepts pnpm's optional argument separator before verifier flags", () => {
    expect(parseRfc0001TestPlanArgs(["--", "--json", "--evidence-dir", "evidence/feishu-smoke"])).toMatchObject({
      json: true,
      help: false,
      evidenceDir: "evidence/feishu-smoke",
    });
  });

  it("passes against the checked-in RFC 0001 test plan", async () => {
    const report = await collectRfc0001TestPlanVerification({ env: verifierEnv });

    expect(report.ok).toBe(true);
    expect(report.nextActions).toEqual([]);
    expect(report.checks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ id: "testplan.default_commands", status: "pass" }),
        expect.objectContaining({ id: "testplan.layer2_commands", status: "pass" }),
        expect.objectContaining({ id: "testplan.capability_matrix", status: "pass" }),
        expect.objectContaining({ id: "testplan.saved_smoke_report", status: "pass" }),
        expect.objectContaining({ id: "testplan.rfc_audit", status: "pass" }),
      ]),
    );
  });

  it("fails when the default acceptance command set drifts", async () => {
    const repo = await createFixtureRepo({
      testPlanContent: (await readCheckedInTestPlan()).replace("pnpm lint\n", ""),
    });

    const report = await collectRfc0001TestPlanVerification({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.ok).toBe(false);
    expect(report.checks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "testplan.default_commands",
          status: "missing",
          evidence: expect.arrayContaining(["missing=pnpm lint"]),
        }),
      ]),
    );
  });

  it("fails when a documented proof file is missing", async () => {
    const repo = await createFixtureRepo();

    const report = await collectRfc0001TestPlanVerification({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.ok).toBe(false);
    expect(report.checks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "testplan.capability_matrix",
          status: "missing",
          evidence: expect.arrayContaining(["Admin dashboard:missing_proof=test/admin-service.e2e.test.ts"]),
        }),
      ]),
    );
  });
});

async function createFixtureRepo(options: { readonly testPlanContent?: string | undefined } = {}): Promise<string> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "rfc-0001-test-plan-"));
  await writeFile(
    path.join(root, "package.json"),
    JSON.stringify(
      {
        scripts: requiredScripts,
      },
      null,
      2,
    ),
  );
  await writeFile(path.join(root, "docs", "rfcs", "0001-slack-feishu-dual-platform", "test-plan.md"), options.testPlanContent ?? (await readCheckedInTestPlan()));
  return root;
}

async function readCheckedInTestPlan(): Promise<string> {
  return await fs.readFile(path.join(repoRoot, "docs", "rfcs", "0001-slack-feishu-dual-platform", "test-plan.md"), "utf8");
}

async function writeFile(filePath: string, content: string): Promise<void> {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, content);
}
