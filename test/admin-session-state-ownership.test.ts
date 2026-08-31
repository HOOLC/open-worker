import fs from "node:fs/promises";

import { describe, expect, it } from "vite-plus/test";

import { sessionOperationalState } from "../apps/admin-ui/session-row-display.js";
import { summarizeSessionLead } from "../apps/admin-ui/session-selection.js";
import { sessionFilters } from "../apps/admin-ui/session-types.js";
import { defaultUiState, normalizeUiState } from "../apps/admin-ui/session-view-state.js";

describe("Admin session state ownership", () => {
  it("does not infer Agent execution state from Gateway records", () => {
    expect(sessionOperationalState({}).label).toBe("Agent 状态未知");
    expect(sessionOperationalState({ lastUserMessage: { text: "accepted" } })).toMatchObject({
      label: "已投递",
      detail: "已有 Agent mailbox receipt",
    });
    expect(summarizeSessionLead({})).toBe("暂无输入记录");
  });

  it("defaults to all routing sessions instead of a synthetic ongoing view", () => {
    expect(sessionFilters).toEqual(["all", "jobs", "issues"]);
    expect(defaultUiState().sessionFilter).toBe("all");
    expect(normalizeUiState({ sessionFilter: "ongoing" }).sessionFilter).toBe("all");
  });

  it("does not retain an Agent runtime event to Slack status projection", async () => {
    const source = await fs.readFile(new URL("../crates/slack/src/status.rs", import.meta.url), "utf8");
    for (const obsoleteProjection of ["apply_event", "status_for_event", "turn_start", "message_start", "tool_execution_start"]) {
      expect(source).not.toContain(obsoleteProjection);
    }
  });

  it("has one Profile contract and leaves automatic selection to Gateway", async () => {
    const profileRoot = new URL("../crates/profile/src/", import.meta.url);
    const files = ["lib.rs", "storage.rs", "execution.rs"];
    const source = (await Promise.all(files.map((file) => fs.readFile(new URL(file, profileRoot), "utf8")))).join("\n");
    for (const obsoleteConcept of ["mod pool;", "mod request;", "pub use request", "SelectOptions", "select_for_new_session", "managed_profiles_root", "load_execution", "list_app_profiles", "OpenAICompatProfile::full()"]) {
      expect(source).not.toContain(obsoleteConcept);
    }
  });

  it("recomputes Profile choices when public account or quota status changes", async () => {
    const source = await fs.readFile(new URL("../apps/admin-ui/session-timeline.tsx", import.meta.url), "utf8");
    expect(source).not.toContain("useMemo(() => profileOptions(");
  });

  it("stores canonical IM binding identity once and has no old runtime job aliases", async () => {
    const database = await fs.readFile(new URL("../crates/gateway/src/db.rs", import.meta.url), "utf8");
    const normalBindingSchema = database.slice(database.indexOf("CREATE TABLE IF NOT EXISTS sessions"), database.indexOf("CREATE TABLE IF NOT EXISTS inbound_messages"));
    expect(normalBindingSchema).toContain("connection_id TEXT NOT NULL");
    expect(normalBindingSchema).toContain("platform TEXT NOT NULL");
    for (const duplicateColumn of ["conversation_id TEXT", "conversation_kind TEXT", "root_message_id TEXT", "platform_thread_id TEXT"]) {
      expect(normalBindingSchema).not.toContain(duplicateColumn);
    }

    const jobSchema = database.slice(database.indexOf("CREATE TABLE IF NOT EXISTS background_jobs"), database.indexOf("CREATE TABLE IF NOT EXISTS admin_events"));
    for (const duplicatedBindingField of ["connection_id TEXT", "platform TEXT", "channel_id TEXT", "root_thread_ts TEXT"]) {
      expect(jobSchema).not.toContain(duplicatedBindingField);
    }

    const http = await fs.readFile(new URL("../crates/gateway/src/http.rs", import.meta.url), "utf8");
    expect(http).not.toContain('"legacyAliases"');
    expect(http).not.toContain('["conversation_id", "conversationId", "channel_id"]');
    expect(http).not.toContain('["root_message_id", "rootMessageId", "thread_ts"]');
  });
});
