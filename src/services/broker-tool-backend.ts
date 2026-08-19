import type { IsolatedMcpService } from "./codex/isolated-mcp-service.js";
import type { BrokerToolBackend, CallIntegrationArgs, CoauthorConfigureArgs, CoauthorStatusArgs, ListIntegrationToolsArgs, PostFileArgs, PostMessageArgs, PostStateArgs, RegisterJobArgs, ThreadCoordinates, ThreadHistoryArgs } from "./codex/dynamic-tools.js";
import type { JobManager } from "./job-manager.js";
import type { SlackAgentBridge } from "./slack/slack-agent-bridge.js";

export function createBrokerToolBackend(options: { readonly bridge: SlackAgentBridge; readonly jobManager: JobManager; readonly isolatedMcp: IsolatedMcpService }): BrokerToolBackend {
  return new WorkerBrokerToolBackend(options);
}

class WorkerBrokerToolBackend implements BrokerToolBackend {
  readonly #bridge: SlackAgentBridge;
  readonly #jobManager: JobManager;
  readonly #isolatedMcp: IsolatedMcpService;

  constructor(options: { readonly bridge: SlackAgentBridge; readonly jobManager: JobManager; readonly isolatedMcp: IsolatedMcpService }) {
    this.#bridge = options.bridge;
    this.#jobManager = options.jobManager;
    this.#isolatedMcp = options.isolatedMcp;
  }

  async postMessage(args: PostMessageArgs, coords: ThreadCoordinates): Promise<unknown> {
    requireReasonForStopKind(args.kind, args.reason);
    const target = chatTarget(coords);
    if (isFeishu(coords)) {
      await this.#bridge.postChatMessage({
        platform: "feishu",
        conversationId: target.conversationId,
        rootMessageId: target.rootMessageId,
        text: args.text,
        kind: args.kind,
        reason: args.reason,
      });
      return { ok: true };
    }

    await this.#bridge.postSlackMessage({
      channelId: requireSlackChannelId(coords),
      rootThreadTs: requireSlackRootThreadTs(coords),
      text: args.text,
      kind: args.kind,
      reason: args.reason,
    });
    return { ok: true };
  }

  async postState(args: PostStateArgs, coords: ThreadCoordinates): Promise<unknown> {
    requireReasonForStopKind(args.kind, args.reason);
    const target = chatTarget(coords);
    if (isFeishu(coords)) {
      await this.#bridge.postChatState({
        platform: "feishu",
        conversationId: target.conversationId,
        rootMessageId: target.rootMessageId,
        kind: args.kind,
        reason: args.reason,
      });
      return { ok: true };
    }

    await this.#bridge.postSlackState({
      channelId: requireSlackChannelId(coords),
      rootThreadTs: requireSlackRootThreadTs(coords),
      kind: args.kind,
      reason: args.reason,
    });
    return { ok: true };
  }

  async postFile(args: PostFileArgs, coords: ThreadCoordinates): Promise<unknown> {
    const target = chatTarget(coords);
    if (isFeishu(coords)) {
      const file = await this.#bridge.postChatFile({
        platform: "feishu",
        conversationId: target.conversationId,
        rootMessageId: target.rootMessageId,
        filePath: args.filePath,
        initialComment: args.initialComment,
      });
      return { ok: true, file };
    }

    const file = await this.#bridge.postSlackFile({
      channelId: requireSlackChannelId(coords),
      rootThreadTs: requireSlackRootThreadTs(coords),
      filePath: args.filePath,
      initialComment: args.initialComment,
    });
    return { ok: true, file };
  }

  async threadHistory(args: ThreadHistoryArgs, coords: ThreadCoordinates): Promise<unknown> {
    const target = chatTarget(coords);
    if (isFeishu(coords)) {
      const result = await this.#bridge.readChatThreadHistory({
        platform: "feishu",
        conversationId: target.conversationId,
        rootMessageId: target.rootMessageId,
        beforeMessageId: args.beforeMessageId,
        beforeCursor: args.beforeCursor,
        limit: args.limit,
      });
      return {
        ok: true,
        platform: "feishu",
        conversationId: target.conversationId,
        rootMessageId: target.rootMessageId,
        returnedCount: result.messages.length,
        hasMore: result.hasMore,
        nextCursor: result.nextCursor,
        formattedText: result.formattedText,
        messages: result.messages,
      };
    }

    const result = await this.#bridge.readThreadHistory({
      channelId: requireSlackChannelId(coords),
      rootThreadTs: requireSlackRootThreadTs(coords),
      beforeMessageTs: args.beforeMessageId ?? args.beforeCursor,
      limit: args.limit,
    });
    return {
      ok: true,
      platform: "slack",
      channelId: coords.channelId,
      rootThreadTs: coords.rootThreadTs,
      returnedCount: result.messages.length,
      hasMore: result.hasMore,
      formattedText: result.formattedText,
      messages: result.messages,
    };
  }

  async coauthorStatus(_args: CoauthorStatusArgs, coords: ThreadCoordinates): Promise<unknown> {
    const status = await this.#bridge.getCommitCoauthorStatus(requireWorkspacePath(coords));
    if (!status) {
      throw new Error("session_not_found");
    }
    return { ok: true, status };
  }

  async coauthorConfigure(args: CoauthorConfigureArgs, coords: ThreadCoordinates): Promise<unknown> {
    const status = await this.#bridge.configureSessionCoauthors({
      cwd: requireWorkspacePath(coords),
      coauthors: args.coauthors,
      ignoreMissing: args.ignoreMissing,
    });
    if (!status) {
      throw new Error("session_not_found");
    }
    return { ok: true, status };
  }

  async registerJob(args: RegisterJobArgs, coords: ThreadCoordinates): Promise<unknown> {
    const target = chatTarget(coords);
    const job = await this.#jobManager.registerJob({
      platform: isFeishu(coords) ? "feishu" : "slack",
      conversationId: target.conversationId,
      rootMessageId: target.rootMessageId,
      channelId: coords.channelId || undefined,
      rootThreadTs: coords.rootThreadTs || undefined,
      kind: args.kind,
      script: args.script,
      cwd: args.cwd,
      restartOnBoot: args.restartOnBoot,
    });
    return {
      ok: true,
      job: {
        id: job.id,
        token: job.token,
        status: job.status,
        kind: job.kind,
        cwd: job.cwd,
        shell: job.shell,
        scriptPath: job.scriptPath,
        restartOnBoot: job.restartOnBoot,
        platform: job.platform,
        conversationId: job.conversationId,
        rootMessageId: job.rootMessageId,
        channelId: job.channelId,
        rootThreadTs: job.rootThreadTs,
        createdAt: job.createdAt,
      },
    };
  }

  async listIntegrationTools(args: ListIntegrationToolsArgs, _coords: ThreadCoordinates): Promise<unknown> {
    const tools = await this.#isolatedMcp.listTools(args.server);
    return {
      ok: true,
      server: args.server,
      tools,
    };
  }

  async callIntegration(args: CallIntegrationArgs, _coords: ThreadCoordinates): Promise<unknown> {
    const result = await this.#isolatedMcp.callTool({
      server: args.server,
      name: args.name,
      arguments: args.arguments ?? {},
    });
    return {
      ok: true,
      server: args.server,
      name: args.name,
      result,
    };
  }
}

function isFeishu(coords: ThreadCoordinates): boolean {
  return coords.platform === "feishu";
}

function chatTarget(coords: ThreadCoordinates): { readonly conversationId: string; readonly rootMessageId: string } {
  const conversationId = coords.conversationId ?? coords.channelId;
  const rootMessageId = coords.rootMessageId ?? coords.rootThreadTs;
  if (!conversationId || !rootMessageId) {
    throw new Error("missing_thread_coordinates");
  }
  return { conversationId, rootMessageId };
}

function requireSlackChannelId(coords: ThreadCoordinates): string {
  const channelId = coords.channelId.trim();
  if (!channelId) {
    throw new Error("missing_thread_coordinates");
  }
  return channelId;
}

function requireSlackRootThreadTs(coords: ThreadCoordinates): string {
  const rootThreadTs = coords.rootThreadTs.trim();
  if (!rootThreadTs) {
    throw new Error("missing_thread_coordinates");
  }
  return rootThreadTs;
}

function requireWorkspacePath(coords: ThreadCoordinates): string {
  const workspacePath = coords.workspacePath.trim();
  if (!workspacePath) {
    throw new Error("missing_workspace_path");
  }
  return workspacePath;
}

function requireReasonForStopKind(kind: string, reason: string | undefined): void {
  if ((kind === "block" || kind === "wait") && !reason?.trim()) {
    throw new Error("missing_reason");
  }
}
