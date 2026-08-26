import fs from "node:fs/promises";

import { describe, expect, it } from "vite-plus/test";

import { sessionOperationalState } from "../apps/admin-ui/session-row-display.js";
import { sessionFilters } from "../apps/admin-ui/session-view-helpers-1.js";
import { summarizeSessionLead } from "../apps/admin-ui/session-view-helpers-4.js";
import { defaultUiState, normalizeUiState } from "../apps/admin-ui/session-view-helpers-5.js";

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

  it("keeps profile resolution in-process without Zode replica or callback abstractions", async () => {
    const agentRoot = new URL("../crates/agent/src/", import.meta.url);
    const files = ["lib.rs", "main.rs", "session/runtime/mod.rs", "session/runtime/ports/mod.rs", "session/state/mod.rs"];
    const source = (await Promise.all(files.map((file) => fs.readFile(new URL(file, agentRoot), "utf8")))).join("\n");
    for (const obsoleteConcept of ["ReplicaPort", "SecretLease", "auth_revision", "provider_execution", "ExternalCallback", "profile_replicas", "pub mod protocol", "pub mod control"]) {
      expect(source).not.toContain(obsoleteConcept);
    }
  });

  it("lets Agent create every session identity without a hidden create replay protocol", async () => {
    const agentRoot = new URL("../crates/agent/src/", import.meta.url);
    const files = ["http/mod.rs", "session/runtime/commands/mod.rs", "session/runtime/ports/store.rs", "session/store.rs"];
    const source = (await Promise.all(files.map((file) => fs.readFile(new URL(file, agentRoot), "utf8")))).join("\n");
    for (const obsoleteConcept of ["SessionCreateCommand", "lookup_session_create", "replay_only", "IdempotencyReceiptNotFound"]) {
      expect(source).not.toContain(obsoleteConcept);
    }
  });

  it("identifies local Agent sessions only by session_id", async () => {
    const agentRoot = new URL("../crates/agent/src/", import.meta.url);
    const files = ["http/mod.rs", "session/state/mod.rs", "session/store.rs", "session/runtime/mod.rs", "session/runtime/model.rs", "session/runtime/transition.rs", "session/runtime/commands/mod.rs", "session/runtime/append.rs", "session/runtime/ports/store.rs", "session/runtime/ports/timer.rs"];
    const source = (await Promise.all(files.map((file) => fs.readFile(new URL(file, agentRoot), "utf8")))).join("\n");
    for (const obsoleteOwner of ["SessionOwner", "authority_id", "subject", "session-owner"]) {
      expect(source).not.toContain(obsoleteOwner);
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

  it("has one mailbox input shape and keeps message identity inside the runtime", async () => {
    const agentRoot = new URL("../crates/agent/src/", import.meta.url);
    const files = ["http/mod.rs", "session/runtime/commands/mod.rs", "session/state/mod.rs", "session/state/reducer.rs"];
    const source = (await Promise.all(files.map((file) => fs.readFile(new URL(file, agentRoot), "utf8")))).join("\n");
    for (const obsoleteConcept of ["MailboxMessageKind", "RuntimeNotification", "kind: MailboxMessageKind"]) {
      expect(source).not.toContain(obsoleteConcept);
    }
  });

  it("recomputes Profile choices when public account or quota status changes", async () => {
    const source = await fs.readFile(new URL("../apps/admin-ui/session-view-helpers-3.tsx", import.meta.url), "utf8");
    expect(source).not.toContain("useMemo(() => profileOptions(");
  });

  it("stores Slack routing coordinates once and has no old runtime job aliases", async () => {
    const database = await fs.readFile(new URL("../crates/gateway/src/db.rs", import.meta.url), "utf8");
    const schema = database.slice(database.indexOf("CREATE TABLE IF NOT EXISTS sessions"), database.indexOf("CREATE TABLE IF NOT EXISTS admin_events"));
    for (const duplicateColumn of ["platform TEXT", "conversation_id TEXT", "conversation_kind TEXT", "root_message_id TEXT", "platform_thread_id TEXT"]) {
      expect(schema).not.toContain(duplicateColumn);
    }

    const http = await fs.readFile(new URL("../crates/gateway/src/http.rs", import.meta.url), "utf8");
    expect(http).not.toContain('"legacyAliases"');
    expect(http).not.toContain('["conversation_id", "conversationId", "channel_id"]');
    expect(http).not.toContain('["root_message_id", "rootMessageId", "thread_ts"]');
  });
});
