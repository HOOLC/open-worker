import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import type { SlackUserIdentity } from "../../types.js";

const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const templatePath = path.resolve(moduleDir, "prompts", "slack-thread-base-instructions.md");

let templateCache: Promise<string> | undefined;

export interface BuildSlackThreadBaseInstructionsOptions {
  readonly platform?: "slack" | "feishu" | undefined;
  readonly channelId: string;
  readonly rootThreadTs: string;
  readonly conversationId?: string | undefined;
  readonly conversationKind?: string | undefined;
  readonly rootMessageId?: string | undefined;
  readonly platformThreadId?: string | undefined;
  readonly workspacePath: string;
  readonly reposRoot: string;
  readonly codexGeneratedImagesRoot: string;
  readonly slackBotIdentity: SlackUserIdentity | null;
  readonly personalMemory?: string | undefined;
}

// Platform guidance is taught as the zork-call CLI. Native Codex tools are not
// injected. Commands are identical on Slack and Feishu.
export async function buildSlackThreadBaseInstructions(options: BuildSlackThreadBaseInstructionsOptions): Promise<string> {
  const template = await loadTemplate();
  const platform = options.platform === "feishu" ? "feishu" : "slack";
  const chatSurfaceName = platform === "feishu" ? "Feishu" : "Slack";
  const conversationId = options.conversationId ?? options.channelId;
  const rootMessageId = options.rootMessageId ?? options.rootThreadTs;
  const variant = buildDynamicToolsVariant(options, chatSurfaceName, platform);

  return renderTemplate(template, {
    chat_surface_name: chatSurfaceName,
    execution_environment_section: await buildExecutionEnvironmentSection(),
    session_workspace: options.workspacePath,
    shared_repos_root: options.reposRoot,
    thread_coordinates_section: formatThreadCoordinatesSection({
      platform,
      channelId: options.channelId,
      rootThreadTs: options.rootThreadTs,
      conversationId,
      conversationKind: options.conversationKind,
      rootMessageId,
      platformThreadId: options.platformThreadId,
    }),
    thread_model_note:
      platform === "slack"
        ? "Slack message model: this session is anchored to one Slack thread. Treat each forwarded message in this thread as a possible follow-up in the same product session."
        : "Feishu message model: this session is anchored to one Feishu topic. Treat `root_message_id` and `platform_thread_id` as the Feishu equivalent of a Slack thread; every forwarded message in this topic is a possible follow-up in the same product session.",
    ...variant,
    chat_bot_identity_section: platform === "slack" ? formatSlackBotIdentitySection(options.slackBotIdentity) : "Feishu bot identity: when a Feishu message mentions the broker bot in this session, that mention refers to you.",
    personal_memory_section: formatPersonalMemorySection(options.personalMemory),
  });
}

function buildDynamicToolsVariant(options: BuildSlackThreadBaseInstructionsOptions, chatSurfaceName: string, platform: "slack" | "feishu"): PromptVariant {
  return {
    dynamic_tools_section: buildCliSection(options, chatSurfaceName, platform),
    turn_stopping_contract: turnStoppingContract(chatSurfaceName, "`zork-call chat post-state`"),
    coauthor_contract: ["- Use `zork-call coauthor status` and `zork-call coauthor configure` to inspect or update session co-author state when needed.", ...coauthorContractTail("`zork-call coauthor configure`")].join("\n"),
  };
}

interface PromptVariant {
  readonly dynamic_tools_section: string;
  readonly turn_stopping_contract: string;
  readonly coauthor_contract: string;
}

function buildCliSection(options: BuildSlackThreadBaseInstructionsOptions, chatSurfaceName: string, platform: "slack" | "feishu"): string {
  return [
    `${chatSurfaceName} CLI for this session. \`zork-call\` is on PATH. Thread coordinates are already bound; do not pass channel, conversation, or thread ids.`,
    "",
    "- `zork-call chat post-message --text TEXT --kind progress|final|block|wait`: send a visible update. For block/wait, include `--reason`.",
    `- ${markdownNote(chatSurfaceName, "`zork-call chat post-message`")}`,
    `- ${postFileNote(chatSurfaceName, "`zork-call chat post-file`")}`,
    `- When sending a terminal ${chatSurfaceName} state, set kind to final, block, or wait. For block/wait, include a short reason.`,
    "- `zork-call chat post-state --kind wait|block|final`: record a silent final, wait, or block state without posting another message.",
    "- `zork-call chat post-file --file-path ABS`: upload a local image or file. Prefer an absolute path.",
    `- Built-in Codex image-generation outputs are saved under \`${options.codexGeneratedImagesRoot}/<thread-id>/...\`. When you want to share one in ${chatSurfaceName}, upload it yourself with \`zork-call chat post-file\`.`,
    "- `zork-call chat thread-history`: read earlier thread context when you need to backfill messages that were not forwarded into this turn. Paginate with `--before-message-id` or `--before-cursor`.",
    `- \`zork-call job register --kind KIND --script SCRIPT\`: register a broker-managed background job. It runs for at most 12 hours, then stops and wakes this session. Register again if it should keep running. Only tell ${chatSurfaceName} you will keep monitoring after the job registration succeeds.`,
    "- `zork-call notify --text TEXT`: wake this session from a running job script. Do not use it to post to the thread.",
    "- `zork-call coauthor status`: inspect the current session's co-author status.",
    "- `zork-call coauthor configure --coauthor NAME`: configure the current session's co-authors. Accepts current-session contributors by Slack user id, @mention, display name, real name, username, email, GitHub login, or GitHub email. GitHub author identity comes only from the user's GitHub OAuth binding.",
    "- Prefer absolute `--file-path` values when uploading local artifacts.",
    `- Registered background jobs receive environment variables including ${registeredJobEnvVars(platform)}. \`zork-call\` is on PATH. When the script exits, this session is woken automatically with the exit code and stderr. Mid-run, use \`zork-call notify\`.`,
    "",
    "Isolated Linear/Notion access for this session:",
    "",
    `- The main Codex runtime for this ${chatSurfaceName} broker does not load the linear or notion MCPs directly.`,
    "- To use Linear or Notion, first run `zork-call integration list-tools`, then `zork-call integration call` for the specific tool you need.",
    '- `zork-call integration list-tools --server linear|notion`: list isolated tools.',
    "- `zork-call integration call --server linear|notion --name NAME [--json '{...}']`: call a listed Linear or Notion tool.",
    `- If the isolated integration call fails, tell ${chatSurfaceName} that the specific integration is unavailable right now. Do not assume the whole runtime is broken.`,
  ].join("\n");
}

function markdownNote(chatSurfaceName: string, api: string): string {
  return chatSurfaceName === "Feishu"
    ? `Write Feishu-facing text in the \`text\` field of ${api}. Prefer readable Markdown-style text; the broker maps it onto Feishu-visible formatting.`
    : `Write normal Markdown in the \`text\` field of ${api}. Do not handcraft Slack \`mrkdwn\`; the broker converts markdownish output to \`mrkdwn\` before posting.`;
}

function postFileNote(chatSurfaceName: string, api: string): string {
  return chatSurfaceName === "Feishu" ? `For ${api}, \`initialComment\` is posted as the Feishu file caption.` : `For ${api}, \`initialComment\` also accepts normal Markdown and is converted before posting.`;
}

function turnStoppingContract(chatSurfaceName: string, silentStateApi: string): string {
  return [
    `- If the work is done, send a ${chatSurfaceName} update with kind=final.`,
    `- If the thread already has a clear completion update from you and you only need to settle broker state, record a silent final state through ${silentStateApi} instead of posting another completion message.`,
    `- If you are blocked and need user input, approval, credentials, or any other human/external intervention, send a ${chatSurfaceName} update with kind=block and include a concrete reason.`,
    `- If your visible ${chatSurfaceName} reply already explains the blocker in human language, record a silent block state through ${silentStateApi} instead of sending a second '[block]' line.`,
    `- If you are intentionally waiting because a broker-managed async job is already running and will wake this session later, either send a visible ${chatSurfaceName} update with kind=wait or record a silent wait state with ${silentStateApi}.`,
    "- Prefer the silent wait-state API when humans do not need an immediate user-visible update. Use a visible kind=wait message only when entering wait is itself worth telling the thread about.",
    `- Do not send one plain ${chatSurfaceName} reply and then a second state-only reply just to attach final/block/wait. Either send a single visible message with the appropriate kind attached, or send the human-facing reply once and record the state silently through ${silentStateApi}.`,
    `- When you do send a visible kind=final/block/wait message, write normal human-facing text. Do not prefix the message body with tags like [final], [block], or [wait].`,
    "- Do not emit repeated wait updates for routine watcher ticks, unchanged CI polls, or other low-signal monitoring noise.",
    "- Do not end a run silently when you intend to stop. If you stop without an explicit final/block/wait explanation, the broker will treat it as an unexpected stop and wake you again.",
  ].join("\n");
}

function coauthorContractTail(missingBindingAction: string): readonly string[] {
  return [
    "- Do not bypass git hooks, disable the configured hooks path, or use `--no-verify` to dodge the gate.",
    "- Commits from this Slack session should remain non-blocking: if selected co-authors already have GitHub OAuth bindings, commit directly without an extra registration step.",
    `- If co-author GitHub OAuth binding is missing and the commit would benefit from it, proactively ask in Slack or call ${missingBindingAction} yourself before committing.`,
    "- If the user explicitly authorizes proceeding without unresolved co-authors, set the session to ignore missing co-authors and continue; unresolved co-authors may be skipped for that commit.",
    "- The broker may append `Co-authored-by:` trailers automatically from selected Slack users' GitHub OAuth bindings.",
  ];
}

function registeredJobEnvVars(platform: "slack" | "feishu"): string {
  return platform === "slack"
    ? "BROKER_JOB_ID, BROKER_API_BASE, CHAT_PLATFORM, CHAT_CONVERSATION_ID, CHAT_ROOT_MESSAGE_ID, SLACK_CHANNEL_ID, SLACK_THREAD_TS, SESSION_KEY, SESSION_WORKSPACE, and REPOS_ROOT"
    : "BROKER_JOB_ID, BROKER_API_BASE, CHAT_PLATFORM, CHAT_CONVERSATION_ID, CHAT_ROOT_MESSAGE_ID, SESSION_KEY, SESSION_WORKSPACE, and REPOS_ROOT";
}

function formatThreadCoordinatesSection(options: {
  readonly platform: "slack" | "feishu";
  readonly channelId: string;
  readonly rootThreadTs: string;
  readonly conversationId: string;
  readonly conversationKind?: string | undefined;
  readonly rootMessageId: string;
  readonly platformThreadId?: string | undefined;
}): string {
  if (options.platform === "slack") {
    return [`- channel_id: ${options.channelId}`, `- thread_ts: ${options.rootThreadTs}`].join("\n");
  }

  return ["- platform: feishu", `- conversation_id: ${options.conversationId}`, options.conversationKind ? `- conversation_kind: ${options.conversationKind}` : undefined, `- root_message_id: ${options.rootMessageId}`, options.platformThreadId ? `- platform_thread_id: ${options.platformThreadId}` : undefined]
    .filter((line): line is string => Boolean(line))
    .join("\n");
}

async function loadTemplate(): Promise<string> {
  if (!templateCache) {
    templateCache = fs.readFile(templatePath, "utf8");
  }

  return await templateCache;
}

function renderTemplate(template: string, variables: Record<string, string>): string {
  const rendered = template.replace(/{{\s*([a-z0-9_]+)\s*}}/gi, (_match, key: string) => {
    const value = variables[key];
    if (value === undefined) {
      throw new Error(`Missing prompt template variable: ${key}`);
    }

    return value;
  });

  return rendered.replace(/\n{3,}/g, "\n\n").trim();
}

async function buildExecutionEnvironmentSection(): Promise<string> {
  const runtimePlatform = process.platform;
  const runtimeHostname = os.hostname();
  const runtimeContainerized = await isContainerizedRuntime();

  return [
    "Current execution environment:",
    `- runtime_platform: ${runtimePlatform}`,
    `- runtime_hostname: ${runtimeHostname}`,
    `- runtime_containerized: ${runtimeContainerized}`,
    "- Shell commands, file edits, git, gh, clone, and worktree operations happen in this runtime.",
    "- Verify platform-specific app/runtime behavior from the runtime you can actually observe. Do not assume a different host environment unless the user explicitly gives you one.",
  ].join("\n");
}

function formatSlackBotIdentitySection(identity: SlackUserIdentity | null): string {
  if (!identity) {
    return "Slack bot identity: when a Slack message mentions the bot user for this broker, that mention refers to you.";
  }

  const lines = ["Slack bot identity in this workspace:", `- bot_user_id: ${identity.userId}`, `- bot_mention: ${identity.mention}`];

  if (identity.displayName) {
    lines.push(`- bot_display_name: ${identity.displayName}`);
  }

  if (identity.realName && identity.realName !== identity.displayName) {
    lines.push(`- bot_real_name: ${identity.realName}`);
  }

  if (identity.username && identity.username !== identity.displayName) {
    lines.push(`- bot_username: ${identity.username}`);
  }

  lines.push("- If a Slack message mentions this bot identity, that mention refers to you.");
  return lines.join("\n");
}

function formatPersonalMemorySection(personalMemory?: string): string {
  const normalized = personalMemory?.trim();
  if (!normalized) {
    return "";
  }

  return `Personal long-lived memory from ~/.codex/AGENT.md:\n${normalized}`;
}

async function isContainerizedRuntime(): Promise<boolean> {
  if (process.env.CONTAINER?.trim() || process.env.KUBERNETES_SERVICE_HOST?.trim()) {
    return true;
  }

  if (await pathExists("/.dockerenv")) {
    return true;
  }

  if (await pathExists("/run/.containerenv")) {
    return true;
  }

  if (process.platform === "linux") {
    const cgroupText = await fs.readFile("/proc/1/cgroup", "utf8").catch(() => "");
    if (/(docker|containerd|kubepods|podman|lxc)/i.test(cgroupText)) {
      return true;
    }
  }

  return false;
}

async function pathExists(targetPath: string): Promise<boolean> {
  try {
    await fs.access(targetPath);
    return true;
  } catch {
    return false;
  }
}
