import type { AppConfig } from "../../config.js";
import { SessionManager } from "../session-manager.js";
import type { SlackInputMessage, SlackSessionRecord } from "../../types.js";
import type { AgentRuntime, AgentRuntimeEvent } from "../agent-runtime/types.js";
import { isAuthProfileUnavailableError, type AuthProfileUnavailableError } from "../agent-runtime/session-auth-profile-runtime.js";
import { AgentTraceRecorder } from "../agent-runtime/agent-trace-recorder.js";
import { SlackApi } from "./slack-api.js";
import { SlackAssistantStatusController } from "./slack-assistant-status.js";
import { createSlackInputFromThreadMessage, isSlackMessageEffectivelyEmpty, parseSlackTextMetadata } from "./slack-event-parser.js";
import {
  chunkSlackMessage,
  clampHistoryLimit,
  compareIsoTimestamp,
  createSyntheticMessageTs,
  createSlackFailureFingerprint,
  formatSlackRunFailureMessage,
  isBeforeSlackTs,
  isMissingAgentSessionError,
  isRecoverableAgentTurnFailure,
  parseActiveTurnMismatch,
  isMissingActiveTurnInputError,
  isSlackMessageAfterCursor,
  shouldResetConflictingActiveTurnMismatch,
  shouldForceResetStaleIdleRuntime,
  shouldPostSlackRunFailure,
  shouldNotifySlackFailure,
  shouldAutoRecoverSession,
} from "./slack-conversation-utils.js";
import { SlackInboundStore } from "./slack-inbound-store.js";
import { SlackSelfMessageFilter } from "./slack-self-filter.js";
import { SlackCoauthorService } from "./slack-coauthor-service.js";
import type { GitHubPrIdentityService } from "../github-pr-identity-service.js";
import { SlackTurnReconciler } from "./slack-turn-reconciler.js";
import { SlackTurnRunner } from "./slack-turn-runner.js";

interface RuntimeSessionState {
  readonly queue: PendingDispatchRequest[];
  processing: boolean;
  generation: number;
  autoResumeTimer?: NodeJS.Timeout | undefined;
  blockedUntilMs?: number | undefined;
  blockedFailureFingerprint?: string | undefined;
  lastFailureNotificationFingerprint?: string | undefined;
  lastFailureNotificationAtMs?: number | undefined;
}

interface PendingDispatchRequest {
  readonly kind: "dispatch_pending";
  readonly recoveryKind?: "missed_thread_messages" | undefined;
}

const AUTO_RESUME_AFTER_FAILURE_MS = 5_000;
const NONRECOVERABLE_DISPATCH_RETRY_COOLDOWN_MS = 5 * 60 * 1_000;
const MISSED_THREAD_RECOVERY_RATE_LIMIT_MIN_BACKOFF_MS = 60_000;
const MISSED_THREAD_RECOVERY_RATE_LIMIT_MAX_BACKOFF_MS = 10 * 60_000;
export interface SlackConversationServiceBase {
  [key: string]: any;
}

export class SlackConversationServiceBase {
  readonly privateConfig: AppConfig;

  readonly privateSessions: SessionManager;

  readonly privateAgentRuntime: AgentRuntime;

  readonly privateTraceRecorder: AgentTraceRecorder;

  readonly privateSlackApi: SlackApi;

  readonly privateSelfMessageFilter: SlackSelfMessageFilter;

  readonly privateCoauthors: {
    readonly noteIncomingSlackInput: (session: SlackSessionRecord, item: SlackInputMessage) => Promise<SlackSessionRecord>;
  };

  readonly privateGithubPrIdentity: GitHubPrIdentityService | undefined;

  readonly privateRuntimeSessions = new Map<string, RuntimeSessionState>();

  readonly privateStatusControllers = new Map<string, SlackAssistantStatusController>();

  readonly privateInboundStore: SlackInboundStore;

  readonly privateTurnRunner: SlackTurnRunner;

  readonly privateTurnReconciler: SlackTurnReconciler;

  readonly privateAgentRuntimeEventHandler: (event: AgentRuntimeEvent) => void;

  readonly privateSessionPageLinkPosts = new Map<string, Promise<SlackSessionRecord>>();

  privateBotUserId = "";

  privateActiveTurnReconcileTimer: NodeJS.Timeout | undefined;

  privateActiveTurnReconcilePromise: Promise<void> | undefined;

  privateStartupRecoveryPromise: Promise<void> | undefined;

  privateStopped = true;

  privateCatchUpPromise: Promise<void> | undefined;

  privateLastMissedThreadRecoveryAtMs = 0;

  privateMissedThreadRecoveryRateLimitBackoffMs = 0;

  privateMissedThreadRecoveryRateLimitUntilMs = 0;

  constructor(options: {
    readonly config: AppConfig;
    readonly sessions: SessionManager;
    readonly agentRuntime: AgentRuntime;
    readonly slackApi: SlackApi;
    readonly selfMessageFilter: SlackSelfMessageFilter;
    readonly coauthors?: SlackCoauthorService | undefined;
    readonly githubPrIdentity?: GitHubPrIdentityService | undefined;
  }) {
    this.privateConfig = options.config;
    this.privateSessions = options.sessions;
    this.privateAgentRuntime = options.agentRuntime;
    this.privateSlackApi = options.slackApi;
    this.privateSelfMessageFilter = options.selfMessageFilter;
    this.privateCoauthors = options.coauthors ?? {
      noteIncomingSlackInput: async (session) => session,
    };
    this.privateGithubPrIdentity = options.githubPrIdentity;
    this.privateInboundStore = new SlackInboundStore({
      sessions: this.privateSessions,
      slackApi: this.privateSlackApi,
    });
    this.privateTraceRecorder = new AgentTraceRecorder({
      sessions: this.privateSessions,
    });
    this.privateTurnRunner = new SlackTurnRunner({
      agentRuntime: this.privateAgentRuntime,
      slackApi: this.privateSlackApi,
      sessions: this.privateSessions,
      inboundStore: this.privateInboundStore,
    });
    this.privateTurnReconciler = new SlackTurnReconciler({
      sessions: this.privateSessions,
      turnRunner: this.privateTurnRunner,
      inboundStore: this.privateInboundStore,
    });
    this.privateAgentRuntimeEventHandler = (event) => {
      this.privateHandleAgentRuntimeEvent(event);
    };
    this.privateAgentRuntime.on("event", this.privateAgentRuntimeEventHandler);
  }
}
