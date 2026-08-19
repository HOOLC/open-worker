import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import { collectRfc0001LocalAudit, createRfc0001AuditCliReport, parseRfc0001AuditArgs, RFC0001_REQUIRED_LOCAL_EVIDENCE_PATTERNS, RFC0001_REQUIRED_LOCAL_FIXTURE_FILES, RFC0001_REQUIRED_LOCAL_IMPLEMENTATION_FILES, RFC0001_REQUIRED_LOCAL_TEST_FILES } from "./manual/run-rfc-0001-local-audit.js";

const requiredScripts = {
  test: "vitest run",
  "test:e2e:feishu-mock": "vitest run test/feishu-codex-bridge.test.ts test/feishu-platform-adapter.test.ts test/feishu-fixture-replay.test.ts test/dual-platform-runtime.test.ts",
  "manual:feishu-smoke": "tsx test/manual/run-real-feishu-smoke.ts --",
  "manual:self-regression": "tsx test/manual/run-self-regression.ts --",
  "rfc:feishu-audit": "tsx test/manual/run-rfc-0001-local-audit.ts",
  "rfc:feishu-audit:local": "tsx test/manual/run-rfc-0001-local-audit.ts -- --local-only",
  "rfc:feishu-completion-audit": "tsx test/manual/run-rfc-0001-completion-audit.ts",
  "rfc:feishu-test-plan": "tsx test/manual/run-rfc-0001-test-plan.ts",
  "ops:auth:profiles": "node scripts/ops/auth-profiles.mjs",
};

describe("RFC 0001 local audit", () => {
  it("accepts pnpm's optional argument separator before audit flags", () => {
    expect(parseRfc0001AuditArgs(["--", "--json", "--local-only", "--evidence-dir", "evidence/feishu-smoke"])).toMatchObject({
      json: true,
      help: false,
      localOnly: true,
      evidenceDir: "evidence/feishu-smoke",
    });
  });

  it("passes local progressive-disclosure assets while keeping real tenant gates open", async () => {
    const repo = await createFixtureRepo();
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.ok).toBe(false);
    expect(report.localOk).toBe(true);
    expect(report.realTenantOk).toBe(false);
    expect(report.localChecks.every((check) => check.status === "pass")).toBe(true);
    expect(report.realTenantChecks).toEqual(expect.arrayContaining([expect.objectContaining({ id: "real.preflight", status: "missing" }), expect.objectContaining({ id: "real.setup_evidence", status: "missing" }), expect.objectContaining({ id: "real.saved_smoke", status: "missing" })]));
    expect(report.nextActions.join("\n")).toContain("real.preflight");
    expect(report.nextActions.join("\n")).toContain("real.setup_evidence");
    expect(report.nextActions.join("\n")).toContain("real.saved_smoke");
  });

  it("keeps completion ok strict while allowing explicit local-only CLI success", async () => {
    const repo = await createFixtureRepo();
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(createRfc0001AuditCliReport(report, { localOnly: false })).toMatchObject({
      mode: "full",
      ok: false,
      localOk: true,
      realTenantOk: false,
      exitOk: false,
    });
    expect(createRfc0001AuditCliReport(report, { localOnly: true })).toMatchObject({
      mode: "local",
      ok: false,
      localOk: true,
      realTenantOk: false,
      exitOk: true,
    });
  });

  it("fails local readiness when RFC files or package scripts drift", async () => {
    const repo = await createFixtureRepo({
      omitDeepDive: "observability.md",
      scripts: {
        ...requiredScripts,
        "rfc:feishu-audit": "tsx test/manual/something-else.ts",
      },
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.rfc_deep_dives",
          status: "missing",
          evidence: expect.arrayContaining(["missing=observability.md"]),
        }),
        expect.objectContaining({
          id: "local.package_scripts",
          status: "missing",
          evidence: expect.arrayContaining(["rfc:feishu-audit=missing_or_unexpected"]),
        }),
      ]),
    );
  });

  it("fails local readiness when the Feishu permission request packet is missing", async () => {
    const repo = await createFixtureRepo({
      omitFiles: ["docs/feishu-permission-request.md"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.permission_request_packet",
          status: "missing",
          evidence: ["missing=docs/feishu-permission-request.md"],
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining(["docs.permission_request_packet:docs/feishu-permission-request.md:missing_file"]),
        }),
      ]),
    );
  });

  it("fails local readiness when the Feishu setup runbook loses rollout steps", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "docs.feishu_setup_runbook",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.feishu_setup_doc",
          status: "pass",
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("docs.feishu_setup_runbook:docs/feishu-setup.md:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when the README loses the user-facing Feishu surface", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "docs.readme_user_surface",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("docs.readme_user_surface:README.md:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when the Feishu permission request packet loses approval rationale", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "docs.permission_request_packet",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.permission_request_packet",
          status: "pass",
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("docs.permission_request_packet:docs/feishu-permission-request.md:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when the setup evidence template stops being a safe placeholder", async () => {
    const repo = await createFixtureRepo({
      setupEvidenceTemplateContent: "{}\n",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.setup_evidence_template",
          status: "missing",
          evidence: expect.arrayContaining(["template_evaluator_status=fail", "target=missing", "consoleLabels=0/9", "missing labels: appType, botCapability, eventDelivery, receiveMessageEvent, groupMessagePermission, sendMessagePermission, cardCallback, resourcePermission, botIdentitySource"]),
        }),
      ]),
    );
  });

  it("fails local readiness when implementation or test slices drift", async () => {
    const repo = await createFixtureRepo({
      omitFiles: ["src/services/feishu/feishu-event-parser.ts", "test/dual-platform-runtime.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=src/services/feishu/feishu-event-parser.ts"]),
        }),
        expect.objectContaining({
          id: "local.test_slices",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/dual-platform-runtime.test.ts"]),
        }),
      ]),
    );
  });

  it("fails local readiness when Feishu config or startup assets drift", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "config.feishu_flags",
      omitFiles: [".env.example", "test/config.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=.env.example"]),
        }),
        expect.objectContaining({
          id: "local.test_slices",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/config.test.ts"]),
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining(["env.feishu_rollout_flags:.env.example:missing_file", expect.stringContaining("config.feishu_flags:src/config.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when Phase 1 Slack compatibility slices drift", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "http.slack_compat_wrappers",
      omitFiles: ["src/http/slack-routes.ts", "test/e2e-broker.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=src/http/slack-routes.ts"]),
        }),
        expect.objectContaining({
          id: "local.test_slices",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/e2e-broker.test.ts"]),
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining(["http.slack_compat_wrappers:src/http/slack-routes.ts:missing_file"]),
        }),
      ]),
    );
  });

  it("fails local readiness when RFC Feishu replay fixtures drift", async () => {
    const repo = await createFixtureRepo({
      omitFiles: ["test/fixtures/feishu/group-followup-parent-only.json", "test/fixtures/feishu/history-page.json"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.feishu_fixtures",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/fixtures/feishu/group-followup-parent-only.json", "missing=test/fixtures/feishu/history-page.json"]),
        }),
      ]),
    );
  });

  it("fails local readiness when Phase 4 HTTP or integration contract slices drift", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "api.feishu_resource_transfer",
      omitFiles: ["src/http/integration-routes.ts", "test/integration-routes.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=src/http/integration-routes.ts"]),
        }),
        expect.objectContaining({
          id: "local.test_slices",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/integration-routes.test.ts"]),
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("api.feishu_resource_transfer:test/feishu-api.test.ts:missing_snippet="), "http.integration_mcp_arguments:test/integration-routes.test.ts:missing_file"]),
        }),
      ]),
    );
  });

  it("fails local readiness when Feishu prompt instructions drift back to Slack-only routing", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "prompt.feishu_platform_runtime_instructions",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("prompt.feishu_platform_runtime_instructions:src/services/codex/slack-thread-base-instructions.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when Phase 5 admin, co-author, or job slices drift", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "admin.platform_health",
      omitFiles: ["src/services/github-author-mapping-service.ts", "test/job-routes.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=src/services/github-author-mapping-service.ts"]),
        }),
        expect.objectContaining({
          id: "local.test_slices",
          status: "missing",
          evidence: expect.arrayContaining(["missing=test/job-routes.test.ts"]),
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("admin.platform_health:test/admin-service.test.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when ops auth path-safety slices drift", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "ops.auth_profiles_path_summarization",
      omitFiles: ["scripts/ops/auth-profiles.mjs"],
      scripts: {
        ...requiredScripts,
        "ops:auth:profiles": "node scripts/ops/auth-profiles-legacy.mjs",
      },
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.implementation_surfaces",
          status: "missing",
          evidence: expect.arrayContaining(["missing=scripts/ops/auth-profiles.mjs"]),
        }),
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining(["ops.auth_profiles_path_summarization:scripts/ops/auth-profiles.mjs:missing_file"]),
        }),
        expect.objectContaining({
          id: "local.package_scripts",
          status: "missing",
          evidence: expect.arrayContaining(["ops:auth:profiles=missing_or_unexpected"]),
        }),
      ]),
    );
  });

  it("fails local readiness when ops status/check redaction evidence drifts", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "ops.status_check_real_sanitization",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("ops.status_check_real_sanitization:test/ops-feishu-preflight.test.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local readiness when required behavior evidence content drifts", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "bridge.non_at_followup",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("bridge.non_at_followup:test/feishu-codex-bridge.test.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("fails local behavior evidence when a probed file is missing", async () => {
    const repo = await createFixtureRepo({
      omitFiles: ["test/feishu-real-smoke.test.ts"],
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining(["smoke.final_evidence_gates:test/feishu-real-smoke.test.ts:missing_file"]),
        }),
      ]),
    );
  });

  it("fails local readiness when saved smoke replay no longer requires setup evidence", async () => {
    const repo = await createFixtureRepo({
      omitEvidenceProbe: "smoke.saved_evidence_requires_setup",
    });
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.localOk).toBe(false);
    expect(report.localChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "local.behavior_evidence",
          status: "missing",
          evidence: expect.arrayContaining([expect.stringContaining("smoke.saved_evidence_requires_setup:test/feishu-real-smoke.test.ts:missing_snippet=")]),
        }),
      ]),
    );
  });

  it("marks preflight ready from supplied env while still requiring tenant evidence files", async () => {
    const repo = await createFixtureRepo();
    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {
        SLACK_APP_TOKEN: "xapp-test",
        SLACK_BOT_TOKEN: "xoxb-test",
        FEISHU_ENABLED: "true",
        FEISHU_APP_ID: "cli-test",
        FEISHU_APP_SECRET: "secret-test",
        FEISHU_BOT_OPEN_ID: "ou_bot",
        FEISHU_DOMAIN: "feishu",
        FEISHU_API_BASE_URL: "https://open.feishu.cn/open-apis",
        FEISHU_GROUP_MESSAGE_MODE: "all",
        FEISHU_STARTUP_REQUIRED: "true",
        FEISHU_ALL_MESSAGE_DELIVERY_VERIFIED: "false",
        LOG_RAW_FEISHU_EVENTS: "false",
        BROKER_ADMIN_TOKEN: "admin-test",
      },
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.realTenantOk).toBe(false);
    expect(report.realTenantChecks).toEqual(expect.arrayContaining([expect.objectContaining({ id: "real.preflight", status: "pass" }), expect.objectContaining({ id: "real.setup_evidence", status: "missing" }), expect.objectContaining({ id: "real.saved_smoke", status: "missing" })]));
  });

  it("marks preflight ready from saved evidence without requiring live rollout secrets", async () => {
    const repo = await createFixtureRepo();
    await writeJson(path.join(repo, "evidence", "feishu-smoke", "feishu-preflight-report.json"), fixturePreflightReport());

    const report = await collectRfc0001LocalAudit({
      cwd: repo,
      env: {},
      evidenceDir: path.join(repo, "evidence", "feishu-smoke"),
    });

    expect(report.realTenantOk).toBe(false);
    expect(report.realTenantChecks).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: "real.preflight",
          status: "pass",
          evidence: expect.arrayContaining(["source=saved_preflight_report", "present=feishu-preflight-report.json", "preflight.slack_credentials_present=pass"]),
        }),
        expect.objectContaining({ id: "real.setup_evidence", status: "missing" }),
        expect.objectContaining({ id: "real.saved_smoke", status: "missing" }),
      ]),
    );
    expect(report.nextActions.join("\n")).not.toContain("real.preflight");
  });
});

async function createFixtureRepo(
  options: {
    readonly omitDeepDive?: string | undefined;
    readonly omitEvidenceProbe?: string | undefined;
    readonly omitFiles?: readonly string[] | undefined;
    readonly scripts?: Record<string, string> | undefined;
    readonly setupEvidenceTemplateContent?: string | undefined;
  } = {},
): Promise<string> {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), "rfc-0001-audit-"));
  const rfcRoot = path.join(root, "docs", "rfcs");
  const deepDiveRoot = path.join(rfcRoot, "0001-slack-feishu-dual-platform");
  await fs.mkdir(deepDiveRoot, { recursive: true });
  await fs.mkdir(path.join(root, "test", "manual"), { recursive: true });

  await writeFile(
    path.join(root, "package.json"),
    JSON.stringify(
      {
        scripts: options.scripts ?? requiredScripts,
      },
      null,
      2,
    ),
  );
  await writeFile(path.join(rfcRoot, "0001-slack-feishu-dual-platform.md"), "# RFC 0001\n\nThis is the 2-minute entry point.\n");
  if (!options.omitFiles?.includes("README.md")) {
    await writeFile(path.join(root, "README.md"), fixtureContentForFile("README.md", options.omitEvidenceProbe));
  }
  for (const file of ["architecture.md", "implementation.md", "test-plan.md", "observability.md", "permissions.md", "review-gates.md"]) {
    if (file !== options.omitDeepDive) {
      await writeFile(path.join(deepDiveRoot, file), `# ${file}\n`);
    }
  }
  if (!options.omitFiles?.includes("docs/feishu-setup.md")) {
    await writeFile(path.join(root, "docs", "feishu-setup.md"), fixtureContentForFile("docs/feishu-setup.md", options.omitEvidenceProbe));
  }
  if (!options.omitFiles?.includes("docs/feishu-permission-request.md")) {
    await writeFile(path.join(root, "docs", "feishu-permission-request.md"), fixtureContentForFile("docs/feishu-permission-request.md", options.omitEvidenceProbe));
  }
  if (!options.omitFiles?.includes("docs/feishu-setup-evidence.example.json")) {
    await writeFile(path.join(root, "docs", "feishu-setup-evidence.example.json"), options.setupEvidenceTemplateContent ?? fixtureSetupEvidenceTemplateContent());
  }
  await writeFile(path.join(root, "test", "manual", "run-real-feishu-smoke.ts"), "export {};\n");
  for (const file of [...RFC0001_REQUIRED_LOCAL_IMPLEMENTATION_FILES, ...RFC0001_REQUIRED_LOCAL_TEST_FILES, ...RFC0001_REQUIRED_LOCAL_FIXTURE_FILES]) {
    if (!options.omitFiles?.includes(file)) {
      await writeFile(path.join(root, file), fixtureContentForFile(file, options.omitEvidenceProbe));
    }
  }

  return root;
}

function fixtureContentForFile(file: string, omitEvidenceProbe: string | undefined): string {
  const snippets = RFC0001_REQUIRED_LOCAL_EVIDENCE_PATTERNS.filter((probe) => probe.file === file && probe.id !== omitEvidenceProbe).flatMap((probe) => probe.snippets);
  return ["export {};", ...snippets].join("\n");
}

async function writeFile(filePath: string, content: string): Promise<void> {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  await fs.writeFile(filePath, content);
}

async function writeJson(filePath: string, content: unknown): Promise<void> {
  await writeFile(filePath, `${JSON.stringify(content, null, 2)}\n`);
}

function fixturePreflightReport(): unknown {
  return {
    ok: true,
    checkedAt: "2026-05-30T07:47:46.534Z",
    checks: [
      "preflight.slack_credentials_present",
      "preflight.feishu_enabled",
      "preflight.feishu_credentials_present",
      "preflight.feishu_bot_identity_present",
      "scope.china_feishu",
      "preflight.feishu_api_base_china",
      "preflight.group_message_mode_all",
      "preflight.startup_required",
      "preflight.raw_feishu_events_disabled",
    ].map((id) => ({
      id,
      label: id,
      required: true,
      status: "pass",
      evidence: [`${id}=pass`],
    })),
    nextActions: [],
  };
}

function fixtureSetupEvidenceTemplateContent(): string {
  return `${JSON.stringify(
    {
      target: "china_feishu",
      consoleLabels: {
        appType: "China Feishu self-built/custom app",
        botCapability: "机器人能力",
        eventDelivery: "使用长连接接收事件",
        receiveMessageEvent: "接收消息",
        groupMessagePermission: "获取群组中所有消息",
        sendMessagePermission: "以应用的身份发消息",
        cardCallback: "卡片回传交互",
        resourcePermission: "获取与上传图片或文件资源",
        botIdentitySource: "Bot/app information page showing open_id/user_id/union_id",
      },
      permissions: {
        imMessageGroupMsg: {
          apiName: "im:message.group_msg",
          status: "pending",
          approvalEvidence: "replace with a redacted real approval ticket or console screenshot reference",
        },
        sendMessage: {
          apiName: "im:message:send_as_bot",
          status: "pending",
          evidence: "replace with redacted send/reply permission evidence",
        },
        cardCallback: {
          eventName: "card.action.trigger",
          status: "pending",
          evidence: "replace with redacted card callback subscription evidence",
        },
        resourceTransfer: {
          scopeName: "获取与上传图片或文件资源",
          status: "pending",
          evidence: "replace with redacted image/file resource permission evidence",
        },
      },
      notes: [
        "Copy this file into the rollout evidence directory and replace labels with the exact text shown in the real tenant.",
        "Do not paste raw App Secret, access tokens, message bodies, user emails, or raw bot IDs into PR evidence.",
        "Record bot identity and credential posture as set/missing only; keep ticket and screenshot references redacted.",
      ],
    },
    null,
    2,
  )}\n`;
}
