import type { AppConfig } from "../config.js";
import { configureLogger } from "../logger.js";
import { StateStore } from "../store/state-store.js";
import type { AgentRuntime } from "./agent-runtime/types.js";
import { CodexAppServerRuntime } from "./agent-runtime/codex-app-server-runtime.js";
import { SessionAuthProfileRuntime } from "./agent-runtime/session-auth-profile-runtime.js";
import type { AuthProfileService } from "./auth-profile-service.js";
import { createBrokerToolBackend } from "./broker-tool-backend.js";
import type { BrokerToolBackend } from "./codex/dynamic-tools.js";
import { CodexBroker } from "./codex/codex-broker.js";
import { IsolatedMcpService } from "./codex/isolated-mcp-service.js";
import { DiskPressureCleanupService } from "./disk-pressure-cleanup-service.js";
import { FeishuCodexBridge } from "./feishu/feishu-codex-bridge.js";
import { FeishuPlatformAdapter } from "./feishu/feishu-platform-adapter.js";
import { GitHubAuthorMappingService } from "./github-author-mapping-service.js";
import { GitHubPrIdentityService } from "./github-pr-identity-service.js";
import { JobManager } from "./job-manager.js";
import { SessionManager } from "./session-manager.js";
import { SlackApi } from "./slack/slack-api.js";
import { SlackAgentBridge } from "./slack/slack-agent-bridge.js";

export function configureServiceLogger(config: AppConfig): void {
  configureLogger({
    logDir: config.logDir,
    level: config.logLevel,
    rawSlackEvents: config.logRawSlackEvents,
    rawFeishuEvents: config.logRawFeishuEvents,
    rawCodexRpc: config.logRawCodexRpc,
    rawHttpRequests: config.logRawHttpRequests,
    rawMaxBytes: config.logRawMaxBytes,
  });
}

export function createSessionServices(config: AppConfig): {
  readonly stateStore: StateStore;
  readonly sessions: SessionManager;
} {
  const stateStore = new StateStore(config.stateDir, config.sessionsRoot);
  const sessions = new SessionManager({
    stateStore,
    sessionsRoot: config.sessionsRoot,
  });

  return {
    stateStore,
    sessions,
  };
}

export function createSlackApi(config: AppConfig): SlackApi {
  return new SlackApi({
    baseUrl: config.slackApiBaseUrl,
    appToken: config.slackAppToken,
    botToken: config.slackBotToken,
  });
}

export async function createGitHubAuthorMappings(config: AppConfig): Promise<GitHubAuthorMappingService> {
  const mappings = new GitHubAuthorMappingService({
    stateDir: config.stateDir,
  });
  await mappings.load();
  return mappings;
}

export async function createGitHubPrIdentity(config: AppConfig): Promise<GitHubPrIdentityService> {
  const identities = new GitHubPrIdentityService({
    stateDir: config.stateDir,
    defaultGitHubLogin: config.defaultGitHubLogin,
    defaultGitHubToken: config.defaultGitHubToken,
    githubApiBaseUrl: config.githubApiBaseUrl,
    githubOAuthScopes: config.githubOAuthScopes,
  });
  await identities.load();
  return identities;
}

export function createCodexBroker(config: AppConfig): CodexBroker {
  return new CodexBroker({
    serviceName: config.serviceName,
    brokerHttpBaseUrl: config.brokerHttpBaseUrl,
    codexHome: config.codexHome,
    teamCodexHomePath: config.codexTeamHomePath,
    reposRoot: config.reposRoot,
    hostCodexHomePath: config.codexHostHomePath,
    codexAppServerPort: config.codexAppServerPort,
    codexAppServerUrl: config.codexAppServerUrl,
    codexAuthJsonPath: config.codexAuthJsonPath,
    codexDisabledMcpServers: config.codexDisabledMcpServers,
    tempadLinkServiceUrl: config.tempadLinkServiceUrl,
    openAiApiKey: config.codexOpenAiApiKey,
  });
}

export function createAgentRuntime(options: { readonly config: AppConfig; readonly codex: CodexBroker; readonly sessions: SessionManager; readonly authProfiles: AuthProfileService; readonly toolBackend?: BrokerToolBackend | undefined }): AgentRuntime {
  const legacyRuntime = options.config.codexAppServerUrl
    ? new CodexAppServerRuntime({
        codex: options.codex,
        sessions: options.sessions,
      })
    : undefined;
  return new SessionAuthProfileRuntime({
    config: options.config,
    sessions: options.sessions,
    authProfiles: options.authProfiles,
    legacyRuntime,
    toolBackend: options.toolBackend,
  });
}

export { createBrokerToolBackend };

export function bindBrokerToolBackend(options: { readonly codex: CodexBroker; readonly agentRuntime: AgentRuntime; readonly backend: BrokerToolBackend }): void {
  options.codex.setToolBackend(options.backend);
  if (options.agentRuntime instanceof SessionAuthProfileRuntime) {
    options.agentRuntime.setToolBackend(options.backend);
  }
}

export function createSlackBridge(options: {
  readonly config: AppConfig;
  readonly sessions: SessionManager;
  readonly codex?: CodexBroker | undefined;
  readonly agentRuntime: AgentRuntime;
  readonly githubAuthorMappings?: GitHubAuthorMappingService | undefined;
  readonly githubPrIdentity: GitHubPrIdentityService;
}): SlackAgentBridge {
  return new SlackAgentBridge({
    config: options.config,
    sessions: options.sessions,
    agentRuntime: options.agentRuntime,
    githubPrIdentity: options.githubPrIdentity,
    feishuBridge: createFeishuBridge({
      config: options.config,
      sessions: options.sessions,
      codex: options.codex,
      mappings: options.githubAuthorMappings,
      githubPrIdentity: options.githubPrIdentity,
    }),
  });
}

function createFeishuBridge(options: { readonly config: AppConfig; readonly sessions: SessionManager; readonly codex?: CodexBroker | undefined; readonly mappings?: GitHubAuthorMappingService | undefined; readonly githubPrIdentity: GitHubPrIdentityService }): FeishuCodexBridge | undefined {
  if (!options.config.feishuEnabled) {
    return undefined;
  }
  if (!options.codex) {
    throw new Error("Feishu bridge requires the legacy Codex broker adapter");
  }
  if (!options.config.feishuAppId || !options.config.feishuAppSecret) {
    throw new Error("Feishu bridge requires FEISHU_APP_ID and FEISHU_APP_SECRET");
  }

  return new FeishuCodexBridge({
    sessions: options.sessions,
    codex: options.codex,
    groupMessageMode: options.config.feishuGroupMessageMode,
    initialThreadHistoryCount: options.config.feishuInitialThreadHistoryCount,
    historyApiMaxLimit: options.config.feishuHistoryApiMaxLimit,
    mappings: options.mappings,
    adminBaseUrl: options.config.adminBaseUrl,
    githubPrIdentity: options.githubPrIdentity,
    adapter: new FeishuPlatformAdapter({
      appId: options.config.feishuAppId,
      appSecret: options.config.feishuAppSecret,
      apiBaseUrl: options.config.feishuApiBaseUrl,
      botIdentity: {
        openId: options.config.feishuBotOpenId,
        userId: options.config.feishuBotUserId,
        unionId: options.config.feishuBotUnionId,
      },
      groupMessageMode: options.config.feishuGroupMessageMode,
      startupRequired: options.config.feishuStartupRequired,
    }),
  });
}

export function createIsolatedMcpService(config: AppConfig): IsolatedMcpService {
  return new IsolatedMcpService({
    codexHome: config.codexHome,
    isolatedMcpServers: config.isolatedMcpServers,
  });
}

export function createJobManager(options: { readonly config: AppConfig; readonly sessions: SessionManager; readonly bridge: SlackAgentBridge }): JobManager {
  return new JobManager({
    sessions: options.sessions,
    jobsRoot: options.config.jobsRoot,
    reposRoot: options.config.reposRoot,
    brokerHttpBaseUrl: options.config.brokerHttpBaseUrl,
    onEvent: async (event) => {
      await options.bridge.acceptBackgroundJobEvent(event);
    },
  });
}

export function createDiskPressureCleanup(options: { readonly config: AppConfig; readonly sessions: SessionManager; readonly jobManager: JobManager }): DiskPressureCleanupService {
  return new DiskPressureCleanupService({
    config: options.config,
    sessions: options.sessions,
    jobTerminator: options.jobManager,
  });
}
