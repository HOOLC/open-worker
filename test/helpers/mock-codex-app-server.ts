import http from "node:http";
import { randomUUID } from "node:crypto";

import { WebSocketServer, type WebSocket } from "ws";

import type { CodexInputItem } from "../../src/services/codex/app-server-client.js";
import { toDynamicToolCallResult, type DynamicToolCallResult } from "../../src/services/codex/dynamic-tools.js";

interface MockTurnRecord {
  readonly threadId: string;
  readonly turnId: string;
  readonly cwd: string;
  readonly input: readonly CodexInputItem[];
  status: "inProgress" | "completed" | "interrupted" | "failed";
  finalMessage: string;
  errorMessage?: string | undefined;
  usage?: unknown;
}

interface MockThreadRecord {
  readonly id: string;
  cwd: string;
  baseInstructions?: string | null | undefined;
  readonly turns: MockTurnRecord[];
  activeTurnId?: string | undefined;
}

export interface MockTurnContext {
  readonly threadId: string;
  readonly turnId: string;
  readonly cwd: string;
  readonly input: readonly CodexInputItem[];
  readonly thread: MockThreadRecord;
  notify: (method: string, params?: Record<string, unknown>) => void;
  complete: (message?: string, usage?: unknown) => void;
  fail: (message: string) => void;
  interrupt: (message?: string) => void;
  emitToolStart: (name: string, callId?: string) => void;
  emitToolEnd: (callId?: string) => void;
}

export interface MockTurnSteerRequest {
  readonly threadId: string;
  readonly expectedTurnId: string;
  readonly input: readonly CodexInputItem[];
}

const INVOKE_TOOL_TIMEOUT_MS = 35_000;

interface PendingServerRequest {
  readonly resolve: (value: Record<string, unknown>) => void;
  readonly reject: (error: Error) => void;
  readonly timer: ReturnType<typeof setTimeout>;
}

interface IncomingRpcMessage {
  readonly id?: string | number;
  readonly method?: string;
  readonly params?: Record<string, unknown>;
  readonly result?: unknown;
  readonly error?: { readonly message?: string; readonly code?: number };
}

export class MockCodexAppServer {
  readonly #server = http.createServer();
  readonly #wsServer = new WebSocketServer({ server: this.#server });
  readonly #connections = new Set<WebSocket>();
  readonly #threads = new Map<string, MockThreadRecord>();
  readonly #threadSockets = new Map<string, WebSocket>();
  readonly #pendingServerRequests = new Map<string, PendingServerRequest>();
  #lastSocket: WebSocket | undefined;
  #serverRequestSeq = 0;
  readonly turnsStarted: MockTurnRecord[] = [];
  readonly threadStarts: Array<{
    readonly threadId: string;
    readonly cwd: string;
    readonly baseInstructions: string | null;
    readonly experimentalRawEvents: unknown;
    readonly params: Record<string, unknown>;
  }> = [];
  readonly threadResumes: Array<{
    readonly threadId: string;
    readonly cwd: string;
    readonly baseInstructions: unknown;
    readonly params: Record<string, unknown>;
  }> = [];
  readonly steers: Array<{
    readonly threadId: string;
    readonly turnId: string;
    readonly input: readonly CodexInputItem[];
  }> = [];
  readonly interrupts: Array<{
    readonly threadId: string;
    readonly turnId: string;
  }> = [];
  readonly onTurnStart: ((context: MockTurnContext) => Promise<void> | void) | undefined;
  readonly onTurnSteer: ((context: MockTurnContext) => Promise<void> | void) | undefined;
  readonly onTurnSteerRequest: ((request: MockTurnSteerRequest) => string | undefined) | undefined;
  readonly #emitThreadTokenUsage: boolean;
  delayThreadReadMs: number;

  constructor(options?: {
    readonly onTurnStart?: ((context: MockTurnContext) => Promise<void> | void) | undefined;
    readonly onTurnSteer?: ((context: MockTurnContext) => Promise<void> | void) | undefined;
    readonly onTurnSteerRequest?: ((request: MockTurnSteerRequest) => string | undefined) | undefined;
    readonly emitThreadTokenUsage?: boolean | undefined;
    readonly delayThreadReadMs?: number | undefined;
  }) {
    this.onTurnStart = options?.onTurnStart;
    this.onTurnSteer = options?.onTurnSteer;
    this.onTurnSteerRequest = options?.onTurnSteerRequest;
    this.#emitThreadTokenUsage = options?.emitThreadTokenUsage ?? false;
    this.delayThreadReadMs = options?.delayThreadReadMs ?? 0;

    this.#wsServer.on("connection", (socket) => {
      this.#connections.add(socket);
      this.#lastSocket = socket;
      socket.on("close", () => {
        this.#connections.delete(socket);
        if (this.#lastSocket === socket) {
          this.#lastSocket = [...this.#connections].at(-1);
        }
        for (const [threadId, threadSocket] of this.#threadSockets) {
          if (threadSocket === socket) {
            this.#threadSockets.delete(threadId);
          }
        }
        this.#rejectPendingServerRequests(new Error("client disconnected during item/tool/call"));
      });
      socket.on("message", (data) => {
        void this.#handleMessage(socket, JSON.parse(data.toString()) as IncomingRpcMessage).catch((error) => {
          process.stderr.write(`[mock-codex] handleMessage failed: ${error instanceof Error ? error.message : String(error)}\n`);
        });
      });
    });
  }

  get lastThreadStartParams(): Record<string, unknown> | undefined {
    return this.threadStarts.at(-1)?.params;
  }

  get dynamicTools(): unknown {
    return this.lastThreadStartParams?.dynamicTools;
  }

  async start(): Promise<string> {
    await new Promise<void>((resolve) => {
      this.#server.listen(0, "127.0.0.1", () => resolve());
    });

    const address = this.#server.address();
    if (!address || typeof address === "string") {
      throw new Error("Mock Codex app-server failed to bind");
    }

    return `ws://127.0.0.1:${address.port}`;
  }

  async stop(): Promise<void> {
    this.#rejectPendingServerRequests(new Error("mock Codex app-server stopped"));
    for (const connection of this.#connections) {
      connection.close();
    }

    await new Promise<void>((resolve) => {
      this.#wsServer.close(() => {
        this.#server.close(() => resolve());
      });
    });
  }

  findLatestTurn(predicate: (turn: MockTurnRecord) => boolean): MockTurnRecord | undefined {
    return [...this.turnsStarted].reverse().find(predicate);
  }

  getThread(threadId: string): MockThreadRecord | undefined {
    return this.#threads.get(threadId);
  }

  async invokeTool(threadId: string, namespace: string, tool: string, args: Record<string, unknown> = {}): Promise<DynamicToolCallResult> {
    const socket = this.#socketFor(threadId);
    if (!socket || socket.readyState !== 1) {
      throw new Error("no connected Codex client for invokeTool");
    }

    const thread = this.#threads.get(threadId);
    const turnId = thread?.activeTurnId ?? thread?.turns.at(-1)?.turnId ?? randomUUID();
    const callId = randomUUID();
    const toolName = `${namespace}.${tool}`;
    const startedItem = {
      type: "dynamicToolCall",
      id: callId,
      tool: toolName,
      arguments: args,
    };

    this.#notify(socket, "item/started", {
      threadId,
      turnId,
      item: startedItem,
    });

    const requestId = `mock-tool-${++this.#serverRequestSeq}`;
    let result: Record<string, unknown>;
    try {
      result = await this.#requestFromClient(
        socket,
        requestId,
        "item/tool/call",
        {
          threadId,
          turnId,
          callId,
          namespace,
          tool,
          arguments: args,
        },
        INVOKE_TOOL_TIMEOUT_MS,
      );
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      this.#notify(socket, "item/completed", {
        threadId,
        turnId,
        item: {
          ...startedItem,
          contentItems: [{ type: "inputText", text: reason }],
          success: false,
        },
      });
      throw error;
    }

    const normalized = toDynamicToolCallResult(result, "item/tool/call failed");
    this.#notify(socket, "item/completed", {
      threadId,
      turnId,
      item: {
        ...startedItem,
        contentItems: normalized.contentItems,
        success: normalized.success,
        ...(normalized.reason ? { reason: normalized.reason } : {}),
      },
    });
    return normalized;
  }

  async #handleMessage(socket: WebSocket, message: IncomingRpcMessage): Promise<void> {
    if (message.id !== undefined && message.method === undefined) {
      this.#settleServerRequest(message);
      return;
    }

    const method = message.method;
    const params = message.params ?? {};

    try {
      switch (method) {
        case "initialize":
          this.#respond(socket, message.id, { ok: true });
          return;
        case "account/read":
          this.#respond(socket, message.id, {
            account: { type: "apiKey" },
            requiresOpenaiAuth: false,
          });
          return;
        case "account/rateLimits/read":
          this.#respond(socket, message.id, {
            rateLimits: {
              limitId: "codex",
              limitName: "Codex",
              primary: {
                usedPercent: 12,
                windowDurationMins: 300,
                resetsAt: 1_777_777_777,
              },
              secondary: {
                usedPercent: 3,
                windowDurationMins: 10_080,
                resetsAt: 1_778_888_888,
              },
              credits: {
                hasCredits: true,
                unlimited: false,
                balance: "42.5",
              },
              planType: "team",
            },
            rateLimitsByLimitId: {
              codex: {
                limitId: "codex",
                limitName: "Codex",
                primary: {
                  usedPercent: 12,
                  windowDurationMins: 300,
                  resetsAt: 1_777_777_777,
                },
                secondary: {
                  usedPercent: 3,
                  windowDurationMins: 10_080,
                  resetsAt: 1_778_888_888,
                },
                credits: {
                  hasCredits: true,
                  unlimited: false,
                  balance: "42.5",
                },
                planType: "team",
              },
            },
          });
          return;
        case "thread/start": {
          const threadId = randomUUID();
          const cwd = String(params.cwd ?? "");
          const baseInstructions = typeof params.baseInstructions === "string" ? params.baseInstructions : null;
          this.#threads.set(threadId, {
            id: threadId,
            cwd,
            baseInstructions,
            turns: [],
          });
          this.threadStarts.push({
            threadId,
            cwd,
            baseInstructions,
            experimentalRawEvents: params.experimentalRawEvents,
            params,
          });
          this.#threadSockets.set(threadId, socket);
          this.#lastSocket = socket;
          this.#respond(socket, message.id, {
            thread: { id: threadId },
          });
          return;
        }
        case "thread/resume": {
          const threadId = String(params.threadId ?? "");
          const thread = this.#threads.get(threadId);
          this.#threadSockets.set(threadId, socket);
          this.#lastSocket = socket;
          this.threadResumes.push({
            threadId,
            cwd: String(params.cwd ?? thread?.cwd ?? ""),
            baseInstructions: params.baseInstructions,
            params,
          });
          if (!thread) {
            this.#error(socket, message.id, `no rollout found for thread id ${threadId}`);
            return;
          }

          thread.cwd = String(params.cwd ?? thread.cwd);
          this.#respond(socket, message.id, {
            thread: { id: threadId },
          });
          return;
        }
        case "turn/start": {
          const threadId = String(params.threadId ?? "");
          const thread = this.#requireThread(threadId);
          const turnId = randomUUID();
          const turn: MockTurnRecord = {
            threadId,
            turnId,
            cwd: String(params.cwd ?? thread.cwd),
            input: normalizeInput(params.input),
            status: "inProgress",
            finalMessage: "",
          };
          thread.turns.push(turn);
          thread.activeTurnId = turnId;
          this.turnsStarted.push(turn);
          this.#respond(socket, message.id, {
            turn: { id: turnId },
          });

          const context = this.#createTurnContext(socket, thread, turn);
          setTimeout(() => {
            void this.#runTurnStart(context, turn);
          }, 10);
          return;
        }
        case "turn/steer": {
          const threadId = String(params.threadId ?? "");
          const expectedTurnId = String(params.expectedTurnId ?? "");
          const thread = this.#requireThread(threadId);

          if (!thread.activeTurnId) {
            this.#error(socket, message.id, "no active turn to steer");
            return;
          }

          if (thread.activeTurnId !== expectedTurnId) {
            this.#error(socket, message.id, `expected active turn id \`${expectedTurnId}\` but found \`${thread.activeTurnId}\``);
            return;
          }

          const turn = this.#requireTurn(thread, expectedTurnId);
          const input = normalizeInput(params.input);
          const requestError = this.onTurnSteerRequest?.({
            threadId,
            expectedTurnId,
            input,
          });
          if (requestError) {
            this.#error(socket, message.id, requestError);
            return;
          }

          this.steers.push({
            threadId,
            turnId: expectedTurnId,
            input,
          });
          this.#respond(socket, message.id, { ok: true });

          const context = this.#createTurnContext(socket, thread, turn);
          setTimeout(() => {
            void this.onTurnSteer?.(context);
          }, 10);
          return;
        }
        case "turn/interrupt": {
          const threadId = String(params.threadId ?? "");
          const turnId = String(params.turnId ?? "");
          const thread = this.#requireThread(threadId);
          const turn = this.#requireTurn(thread, turnId);
          this.interrupts.push({
            threadId,
            turnId,
          });
          this.#respond(socket, message.id, { ok: true });
          this.#interruptTurn(socket, thread, turn, "interrupted");
          return;
        }
        case "thread/read": {
          if (this.delayThreadReadMs > 0) {
            await new Promise((resolve) => setTimeout(resolve, this.delayThreadReadMs));
          }
          const threadId = String(params.threadId ?? "");
          const thread = this.#requireThread(threadId);
          this.#respond(socket, message.id, {
            thread: {
              turns: thread.turns.map((turn) => ({
                id: turn.turnId,
                status: turn.status,
                error: turn.errorMessage ? { message: turn.errorMessage } : null,
                usage: turn.usage,
                items: turn.finalMessage
                  ? [
                      {
                        type: "agentMessage",
                        text: turn.finalMessage,
                      },
                    ]
                  : [],
              })),
            },
          });
          return;
        }
        default:
          this.#error(socket, message.id, `unsupported method: ${method ?? "unknown"}`);
      }
    } catch (error) {
      this.#error(socket, message.id, error instanceof Error ? error.message : String(error));
    }
  }

  #createTurnContext(socket: WebSocket, thread: MockThreadRecord, turn: MockTurnRecord): MockTurnContext {
    return {
      threadId: thread.id,
      turnId: turn.turnId,
      cwd: turn.cwd,
      input: turn.input,
      thread,
      notify: (method, params = {}) => {
        socket.send(
          JSON.stringify({
            method,
            params,
          }),
        );
      },
      complete: (message = "", usage?: unknown) => {
        if (turn.status !== "inProgress") {
          return;
        }

        turn.status = "completed";
        turn.finalMessage = message;
        turn.usage = this.#emitThreadTokenUsage ? undefined : usage;
        thread.activeTurnId = undefined;
        if (usage && this.#emitThreadTokenUsage) {
          socket.send(
            JSON.stringify({
              method: "thread/tokenUsage/updated",
              params: {
                threadId: thread.id,
                turnId: turn.turnId,
                tokenUsage: toThreadTokenUsage(usage),
              },
            }),
          );
        }
        if (message) {
          socket.send(
            JSON.stringify({
              method: "item/agentMessage/delta",
              params: {
                turnId: turn.turnId,
                delta: message,
              },
            }),
          );
        }
        socket.send(
          JSON.stringify({
            method: "turn/completed",
            params: {
              turn: {
                id: turn.turnId,
                usage: this.#emitThreadTokenUsage ? undefined : usage,
              },
              usage: this.#emitThreadTokenUsage ? undefined : usage,
            },
          }),
        );
      },
      fail: (message) => {
        if (turn.status !== "inProgress") {
          return;
        }

        turn.status = "failed";
        turn.errorMessage = message;
        thread.activeTurnId = undefined;
      },
      interrupt: (message = "") => {
        this.#interruptTurn(socket, thread, turn, message);
      },
      emitToolStart: (name, callId = "call-1") => {
        socket.send(
          JSON.stringify({
            method: "codex/event/tool_start",
            params: {
              threadId: thread.id,
              turnId: turn.turnId,
              callId,
              name,
            },
          }),
        );
      },
      emitToolEnd: (callId = "call-1") => {
        socket.send(
          JSON.stringify({
            method: "codex/event/tool_end",
            params: {
              threadId: thread.id,
              turnId: turn.turnId,
              callId,
            },
          }),
        );
      },
    };
  }

  async #runTurnStart(context: MockTurnContext, turn: MockTurnRecord): Promise<void> {
    try {
      await this.onTurnStart?.(context);
    } catch (error) {
      context.fail(error instanceof Error ? error.message : String(error));
    } finally {
      if (turn.status === "inProgress") {
        context.complete("");
      }
    }
  }

  #interruptTurn(socket: WebSocket, thread: MockThreadRecord, turn: MockTurnRecord, message: string): void {
    if (turn.status !== "inProgress") {
      return;
    }

    turn.status = "interrupted";
    turn.finalMessage = message;
    thread.activeTurnId = undefined;
    socket.send(
      JSON.stringify({
        method: "codex/event/turn_aborted",
        params: {
          msg: {
            turn_id: turn.turnId,
          },
        },
      }),
    );
  }

  #requireThread(threadId: string): MockThreadRecord {
    const thread = this.#threads.get(threadId);
    if (!thread) {
      throw new Error(`Unknown thread ${threadId}`);
    }
    return thread;
  }

  #requireTurn(thread: MockThreadRecord, turnId: string): MockTurnRecord {
    const turn = thread.turns.find((entry) => entry.turnId === turnId);
    if (!turn) {
      throw new Error(`Unknown turn ${turnId}`);
    }
    return turn;
  }

  #socketFor(threadId: string): WebSocket | undefined {
    return this.#threadSockets.get(threadId) ?? this.#lastSocket;
  }

  #notify(socket: WebSocket, method: string, params: Record<string, unknown>): void {
    socket.send(
      JSON.stringify({
        method,
        params,
      }),
    );
  }

  #requestFromClient(socket: WebSocket, requestId: string, method: string, params: Record<string, unknown>, timeoutMs: number): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.#pendingServerRequests.delete(requestId);
        reject(new Error("item/tool/call timed out"));
      }, timeoutMs);
      this.#pendingServerRequests.set(requestId, { resolve, reject, timer });
      socket.send(
        JSON.stringify({
          id: requestId,
          method,
          params,
        }),
        (error) => {
          if (!error) {
            return;
          }
          this.#pendingServerRequests.delete(requestId);
          clearTimeout(timer);
          reject(error);
        },
      );
    });
  }

  #settleServerRequest(message: IncomingRpcMessage): void {
    if (message.id === undefined) {
      return;
    }

    const requestId = String(message.id);
    const pending = this.#pendingServerRequests.get(requestId);
    if (!pending) {
      return;
    }

    this.#pendingServerRequests.delete(requestId);
    clearTimeout(pending.timer);
    if (message.error) {
      pending.reject(new Error(message.error.message?.trim() || "item/tool/call error"));
      return;
    }

    pending.resolve(isRecord(message.result) ? message.result : {});
  }

  #rejectPendingServerRequests(error: Error): void {
    for (const [requestId, pending] of this.#pendingServerRequests) {
      this.#pendingServerRequests.delete(requestId);
      clearTimeout(pending.timer);
      pending.reject(error);
    }
  }

  #respond(socket: WebSocket, id: string | number | undefined, result: Record<string, unknown>): void {
    socket.send(
      JSON.stringify({
        id,
        result,
      }),
    );
  }

  #error(socket: WebSocket, id: string | number | undefined, message: string): void {
    socket.send(
      JSON.stringify({
        id,
        error: {
          message,
        },
      }),
    );
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function normalizeInput(value: unknown): readonly CodexInputItem[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value as readonly CodexInputItem[];
}

function toThreadTokenUsage(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    return {
      last: {},
      total: {},
    };
  }

  const record = value as Record<string, unknown>;
  const usage = {
    inputTokens: record.input_tokens ?? record.inputTokens ?? 0,
    cachedInputTokens: record.cached_input_tokens ?? record.cachedInputTokens ?? 0,
    outputTokens: record.output_tokens ?? record.outputTokens ?? 0,
    reasoningOutputTokens: record.reasoning_tokens ?? record.reasoningTokens ?? record.reasoning_output_tokens ?? record.reasoningOutputTokens ?? 0,
    totalTokens: record.total_tokens ?? record.totalTokens ?? 0,
    model: record.model,
    effort: record.effort,
  };
  return {
    last: usage,
    total: usage,
  };
}
