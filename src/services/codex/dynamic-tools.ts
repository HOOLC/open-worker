import path from "node:path";

import type { JsonLike } from "../../types.js";

// dynamicTools declarations sent on thread/start. Declarations persist across
// thread/resume, so the client sends them only on start and never on resume.
// Namespace names must not collide with reserved app-server namespaces.
export const RESERVED_DYNAMIC_TOOL_NAMESPACES: readonly string[] = ["functions", "multi_tool_use", "file_search", "web", "browser", "image_gen", "computer", "container", "terminal", "python", "python_user_visible", "api_tool", "tool_search", "submodel_delegator"];

export interface DynamicToolFunction {
  readonly type: "function";
  readonly name: string;
  readonly description: string;
  readonly inputSchema: {
    readonly type: "object";
    readonly properties?: Readonly<Record<string, unknown>>;
    readonly required?: readonly string[];
    readonly additionalProperties?: boolean;
  };
}

export interface DynamicToolNamespace {
  readonly type: "namespace";
  readonly name: string;
  readonly description: string;
  readonly tools: readonly DynamicToolFunction[];
}

export type DynamicToolDeclaration = DynamicToolNamespace;

export function isReservedDynamicToolNamespace(name: string): boolean {
  return RESERVED_DYNAMIC_TOOL_NAMESPACES.includes(name);
}

// item/tool/call request the app-server sends to the client when the model
// invokes a declared tool.
export interface DynamicToolCallRequest {
  readonly threadId: string;
  readonly turnId: string;
  readonly callId: string;
  readonly namespace: string;
  readonly tool: string;
  readonly arguments: Record<string, unknown>;
}

export interface DynamicToolContentItem {
  readonly type: "inputText";
  readonly text: string;
}

export interface DynamicToolCallResult {
  readonly contentItems: readonly DynamicToolContentItem[];
  readonly success: boolean;
  readonly reason?: string | undefined;
}

// Platform tool backends are injected from outside the Codex adapter; this
// module must not import Slack or Feishu services.
export interface DynamicToolCallContext {
  readonly platform: "slack" | "feishu" | undefined;
  readonly channelId: string;
  readonly conversationId: string | undefined;
  readonly conversationKind: string | undefined;
  readonly rootThreadTs: string;
  readonly rootMessageId: string | undefined;
  readonly platformThreadId: string | undefined;
  readonly workspacePath: string;
  readonly sessionKey: string | undefined;
}

// Coordinates recorded for every Codex thread on both thread/start and
// thread/resume, used to route reverse item/tool/call requests back to the
// originating platform session.
export interface ThreadCoordinates extends DynamicToolCallContext {
  readonly threadId: string;
}

export interface DynamicToolBackend {
  readonly listDeclarations: () => readonly DynamicToolDeclaration[];
  readonly handleCall: (call: DynamicToolCallRequest, context: DynamicToolCallContext | undefined) => Promise<DynamicToolCallResult>;
}

export function toDynamicToolCallRequest(params: Record<string, unknown>): DynamicToolCallRequest | undefined {
  const threadId = normalizeOptionalString(params.threadId) ?? normalizeOptionalString(params.thread_id);
  const turnId = normalizeOptionalString(params.turnId) ?? normalizeOptionalString(params.turn_id);
  const callId = normalizeOptionalString(params.callId) ?? normalizeOptionalString(params.call_id);
  const namespace = normalizeOptionalString(params.namespace);
  const tool = normalizeOptionalString(params.tool) ?? normalizeOptionalString(params.toolName) ?? normalizeOptionalString(params.tool_name);
  if (!threadId || !turnId || !callId || !namespace || !tool) {
    return undefined;
  }

  return {
    threadId,
    turnId,
    callId,
    namespace,
    tool,
    arguments: isRecord(params.arguments) ? params.arguments : {},
  };
}

export function toDynamicToolCallResult(value: unknown, fallbackReason: string): DynamicToolCallResult {
  if (isRecord(value)) {
    const success = value.success === undefined ? true : Boolean(value.success);
    const contentItems = Array.isArray(value.contentItems)
      ? value.contentItems
          .map((item) => (isRecord(item) ? normalizeOptionalString(item.text) : undefined))
          .filter((text): text is string => text !== undefined)
          .map((text) => ({ type: "inputText" as const, text }))
      : [];
    const reason = normalizeOptionalString(value.reason);
    if (success) {
      return { contentItems, success: true };
    }
    return reason ? { contentItems, success: false, reason } : { contentItems, success: false, reason: fallbackReason };
  }

  return { contentItems: [], success: false, reason: fallbackReason };
}

export function toDynamicToolDeclarationsJson(declarations: readonly DynamicToolDeclaration[] | undefined): JsonLike[] | undefined {
  if (!declarations || declarations.length === 0) {
    return undefined;
  }

  return declarations.map(
    (namespace): JsonLike => ({
      type: "namespace",
      name: namespace.name,
      description: namespace.description,
      tools: namespace.tools.map(
        (tool): JsonLike => ({
          type: "function",
          name: tool.name,
          description: tool.description,
          inputSchema: toJsonValue(tool.inputSchema),
        }),
      ),
    }),
  );
}

function toJsonValue(value: unknown): JsonLike {
  if (value === null || typeof value === "boolean" || typeof value === "number" || typeof value === "string") {
    return value;
  }
  if (Array.isArray(value)) {
    return value.map((entry) => toJsonValue(entry));
  }
  if (isRecord(value)) {
    const result: Record<string, JsonLike> = {};
    for (const [key, entry] of Object.entries(value)) {
      if (entry === undefined) {
        continue;
      }
      result[key] = toJsonValue(entry);
    }
    return result;
  }
  return null;
}

function normalizeOptionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const TOOL_CALL_TIMEOUT_MS = 30_000;
const TOOL_RESULT_TEXT_LIMIT = 100_000;

export type ChatMessageKind = "progress" | "final" | "block" | "wait";
export type ChatStateKind = "wait" | "block" | "final";
export type IntegrationServerName = "linear" | "notion";

export interface PostMessageArgs {
  readonly text: string;
  readonly kind: ChatMessageKind;
  readonly reason?: string | undefined;
}

export interface PostStateArgs {
  readonly kind: ChatStateKind;
  readonly reason?: string | undefined;
}

export interface PostFileArgs {
  readonly filePath: string;
  readonly initialComment?: string | undefined;
}

export interface ThreadHistoryArgs {
  readonly beforeMessageId?: string | undefined;
  readonly beforeCursor?: string | undefined;
  readonly limit?: number | undefined;
}

export type CoauthorStatusArgs = Record<string, never>;

export interface CoauthorConfigureArgs {
  readonly coauthors: readonly string[];
  readonly ignoreMissing?: boolean | undefined;
}

export interface RegisterJobArgs {
  readonly kind: string;
  readonly script: string;
  readonly cwd?: string | undefined;
  readonly restartOnBoot?: boolean | undefined;
}

export interface ListIntegrationToolsArgs {
  readonly server: IntegrationServerName;
}

export interface CallIntegrationArgs {
  readonly server: IntegrationServerName;
  readonly name: string;
  readonly arguments?: Record<string, unknown> | undefined;
}

export interface BrokerToolBackend {
  readonly postMessage: (args: PostMessageArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly postState: (args: PostStateArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly postFile: (args: PostFileArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly threadHistory: (args: ThreadHistoryArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly coauthorStatus: (args: CoauthorStatusArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly coauthorConfigure: (args: CoauthorConfigureArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly registerJob: (args: RegisterJobArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly listIntegrationTools: (args: ListIntegrationToolsArgs, coords: ThreadCoordinates) => Promise<unknown>;
  readonly callIntegration: (args: CallIntegrationArgs, coords: ThreadCoordinates) => Promise<unknown>;
}

function asToolArgs<T>(args: Record<string, unknown>): T {
  return args as unknown as T;
}

interface CatalogFunction extends DynamicToolFunction {
  readonly invoke: (backend: BrokerToolBackend, args: Record<string, unknown>, coords: ThreadCoordinates) => Promise<unknown>;
}

interface CatalogNamespace {
  readonly type: "namespace";
  readonly name: string;
  readonly description: string;
  readonly tools: readonly CatalogFunction[];
}

const DYNAMIC_TOOL_CATALOG: readonly CatalogNamespace[] = [
  {
    type: "namespace",
    name: "chat",
    description: "Post messages, files, and silent state to this thread, and read earlier history.",
    tools: [
      {
        type: "function",
        name: "post_message",
        description: "Post a visible progress/final/block/wait message. Include reason when kind is block or wait.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            text: { type: "string", description: "Human-facing message body.", minLength: 1 },
            kind: { type: "string", description: "progress | final | block | wait", enum: ["progress", "final", "block", "wait"] },
            reason: { type: "string", description: "Short reason. Required by the broker for block/wait.", minLength: 1 },
          },
          required: ["text", "kind"],
        },
        invoke: (backend, args, coords) => backend.postMessage(asToolArgs<PostMessageArgs>(args), coords),
      },
      {
        type: "function",
        name: "post_state",
        description: "Record a silent wait/block/final state without posting another message. Include reason when kind is block or wait.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            kind: { type: "string", description: "wait | block | final", enum: ["wait", "block", "final"] },
            reason: { type: "string", description: "Short reason. Required by the broker for block/wait.", minLength: 1 },
          },
          required: ["kind"],
        },
        invoke: (backend, args, coords) => backend.postState(asToolArgs<PostStateArgs>(args), coords),
      },
      {
        type: "function",
        name: "post_file",
        description: "Upload a local file to this thread. filePath must be absolute.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            filePath: { type: "string", description: "Absolute path of the local file to upload.", minLength: 1, format: "absolute-path" },
            initialComment: { type: "string", description: "Optional caption posted with the file.", minLength: 1 },
          },
          required: ["filePath"],
        },
        invoke: (backend, args, coords) => backend.postFile(asToolArgs<PostFileArgs>(args), coords),
      },
      {
        type: "function",
        name: "thread_history",
        description: "Read earlier messages in this thread. Paginate with beforeMessageId or beforeCursor, not both.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            beforeMessageId: { type: "string", description: "Return messages before this message id.", minLength: 1 },
            beforeCursor: { type: "string", description: "Return messages before this opaque cursor.", minLength: 1 },
            limit: { type: "integer", description: "Positive maximum number of messages to return.", minimum: 1 },
          },
        },
        invoke: (backend, args, coords) => backend.threadHistory(asToolArgs<ThreadHistoryArgs>(args), coords),
      },
    ],
  },
  {
    type: "namespace",
    name: "coauthor",
    description: "Inspect and configure git commit co-authors for this session.",
    tools: [
      {
        type: "function",
        name: "status",
        description: "Return git co-author status for this session.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {},
        },
        invoke: (backend, args, coords) => backend.coauthorStatus(asToolArgs<CoauthorStatusArgs>(args), coords),
      },
      {
        type: "function",
        name: "configure",
        description: "Set session co-authors by Slack user id, @mention, display name, real name, username, email, or GitHub login/email.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            coauthors: {
              type: "array",
              description: "People to select as co-authors for this session.",
              items: { type: "string" },
              minItems: 1,
            },
            ignoreMissing: { type: "boolean", description: "If true, proceed even when some co-authors cannot be resolved." },
          },
          required: ["coauthors"],
        },
        invoke: (backend, args, coords) => backend.coauthorConfigure(asToolArgs<CoauthorConfigureArgs>(args), coords),
      },
    ],
  },
  {
    type: "namespace",
    name: "job",
    description: "Register broker-managed background jobs for this session.",
    tools: [
      {
        type: "function",
        name: "register",
        description: "Register a background job with kind and script. cwd and restartOnBoot are optional.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            kind: { type: "string", description: "Job kind label, for example watch_ci.", minLength: 1 },
            script: { type: "string", description: "Job script body to run.", minLength: 1 },
            cwd: { type: "string", description: "Working directory relative to the session workspace, or absolute.", minLength: 1 },
            restartOnBoot: { type: "boolean", description: "If true, restart the job when the broker boots. Defaults to true." },
          },
          required: ["kind", "script"],
        },
        invoke: (backend, args, coords) => backend.registerJob(asToolArgs<RegisterJobArgs>(args), coords),
      },
    ],
  },
  {
    type: "namespace",
    name: "integration",
    description: "List and call isolated Linear and Notion tools.",
    tools: [
      {
        type: "function",
        name: "list_tools",
        description: "List isolated MCP tools for server linear or notion.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            server: { type: "string", description: "linear or notion", enum: ["linear", "notion"] },
          },
          required: ["server"],
        },
        invoke: (backend, args, coords) => backend.listIntegrationTools(asToolArgs<ListIntegrationToolsArgs>(args), coords),
      },
      {
        type: "function",
        name: "call",
        description: "Call an isolated MCP tool. arguments is a JSON object.",
        inputSchema: {
          type: "object",
          additionalProperties: false,
          properties: {
            server: { type: "string", description: "linear or notion", enum: ["linear", "notion"] },
            name: { type: "string", description: "Tool name from list_tools.", minLength: 1 },
            arguments: { type: "object", description: "JSON object passed to the MCP tool.", additionalProperties: true },
          },
          required: ["server", "name"],
        },
        invoke: (backend, args, coords) => backend.callIntegration(asToolArgs<CallIntegrationArgs>(args), coords),
      },
    ],
  },
];

export function buildDynamicToolsDeclaration(): readonly DynamicToolDeclaration[] {
  return DYNAMIC_TOOL_CATALOG.map((namespace) => {
    if (isReservedDynamicToolNamespace(namespace.name)) {
      throw new Error(`dynamic tool namespace collides with reserved name: ${namespace.name}`);
    }

    return {
      type: "namespace",
      name: namespace.name,
      description: namespace.description,
      tools: namespace.tools.map((tool) => ({
        type: "function",
        name: tool.name,
        description: tool.description,
        inputSchema: tool.inputSchema,
      })),
    };
  });
}

export async function handleToolCall(params: DynamicToolCallRequest | Record<string, unknown>, resolveCoords: (threadId: string) => ThreadCoordinates | undefined, backend: BrokerToolBackend): Promise<DynamicToolCallResult> {
  const request = toDynamicToolCallRequest(isRecord(params) ? params : {});
  if (!request) {
    return failedToolResult("invalid tool call request");
  }

  const catalogTool = findCatalogTool(request.namespace, request.tool);
  if (!catalogTool) {
    return failedToolResult(`unknown tool: ${request.namespace}.${request.tool}`);
  }

  const validated = validateAndNormalizeArguments(catalogTool.inputSchema, request.arguments);
  if (!validated.ok) {
    return failedToolResult(validated.error);
  }

  if (request.namespace === "chat" && request.tool === "thread_history") {
    const hasBeforeMessageId = hasOwn(validated.value, "beforeMessageId");
    const hasBeforeCursor = hasOwn(validated.value, "beforeCursor");
    if (hasBeforeMessageId && hasBeforeCursor) {
      return failedToolResult("provide only one of beforeMessageId or beforeCursor");
    }
  }

  const coords = resolveThreadCoordinates(request.threadId, resolveCoords);
  if (!coords) {
    return failedToolResult(`unknown thread: ${request.threadId}`);
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  const work = startCatalogToolWork(catalogTool, backend, validated.value, coords);
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error("tool call timed out"));
    }, TOOL_CALL_TIMEOUT_MS);
  });

  try {
    const value = await Promise.race([work, timeout]);
    return succeededToolResult(value);
  } catch (error) {
    void work.catch(() => undefined);
    return failedToolResult(error instanceof Error ? error.message : String(error));
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  }
}

function findCatalogTool(namespace: string, tool: string): CatalogFunction | undefined {
  const catalogNamespace = DYNAMIC_TOOL_CATALOG.find((entry) => entry.name === namespace);
  return catalogNamespace?.tools.find((entry) => entry.name === tool);
}

// A backend invoke function is untrusted at this boundary: async wrapping
// turns any synchronous throw into a rejection so the timeout/finally handling
// always runs and the caller only ever observes a DynamicToolCallResult.
async function startCatalogToolWork(catalogTool: CatalogFunction, backend: BrokerToolBackend, args: Record<string, unknown>, coords: ThreadCoordinates): Promise<unknown> {
  return await catalogTool.invoke(backend, args, coords);
}

// The coordinates lookup comes from the adapter; never let a throwing lookup
// surface as a JSON-RPC protocol error instead of a success:false tool result.
function resolveThreadCoordinates(threadId: string, resolveCoords: (threadId: string) => ThreadCoordinates | undefined): ThreadCoordinates | undefined {
  try {
    return resolveCoords(threadId);
  } catch {
    return undefined;
  }
}

function validateAndNormalizeArguments(schema: DynamicToolFunction["inputSchema"], args: Record<string, unknown>): { readonly ok: true; readonly value: Record<string, unknown> } | { readonly ok: false; readonly error: string } {
  const properties = schema.properties ?? {};
  const required = new Set(schema.required ?? []);
  const rejectUnknownKeys = schema.additionalProperties !== true;
  const normalized: Record<string, unknown> = {};

  for (const key of Object.keys(args)) {
    if (!Object.prototype.hasOwnProperty.call(properties, key)) {
      if (rejectUnknownKeys) {
        return { ok: false, error: `unknown argument: ${key}` };
      }
      normalized[key] = args[key];
      continue;
    }

    const property = properties[key];
    const result = normalizeProperty(key, property, args[key], required.has(key));
    if (!result.ok) {
      return result;
    }
    if (result.omit) {
      continue;
    }
    normalized[key] = result.value;
  }

  for (const key of required) {
    if (!hasOwn(normalized, key)) {
      return { ok: false, error: `missing required argument: ${key}` };
    }
  }

  return { ok: true, value: normalized };
}

function normalizeProperty(key: string, property: unknown, value: unknown, required: boolean): { readonly ok: true; readonly omit: true } | { readonly ok: true; readonly omit: false; readonly value: unknown } | { readonly ok: false; readonly error: string } {
  if (value === undefined) {
    return required ? { ok: false, error: `missing required argument: ${key}` } : { ok: true, omit: true };
  }

  const schema = isRecord(property) ? property : {};
  const expectedType = typeof schema.type === "string" ? schema.type : undefined;

  if (expectedType === "string") {
    if (typeof value !== "string") {
      return { ok: false, error: `invalid argument: ${key} (expected string)` };
    }
    const trimmed = value.trim();
    if (!trimmed) {
      return required ? { ok: false, error: `missing required argument: ${key}` } : { ok: true, omit: true };
    }
    const minLength = typeof schema.minLength === "number" ? schema.minLength : undefined;
    if (minLength !== undefined && trimmed.length < minLength) {
      return { ok: false, error: `invalid argument: ${key} (expected string)` };
    }
    const enumValues = readStringEnum(schema.enum);
    if (enumValues && !enumValues.includes(trimmed)) {
      return { ok: false, error: `invalid argument: ${key} (expected one of ${enumValues.join("|")})` };
    }
    if (schema.format === "absolute-path" && !path.isAbsolute(trimmed)) {
      return { ok: false, error: `invalid argument: ${key} (expected absolute path)` };
    }
    return { ok: true, omit: false, value: trimmed };
  }

  if (expectedType === "boolean") {
    if (typeof value !== "boolean") {
      return { ok: false, error: `invalid argument: ${key} (expected boolean)` };
    }
    return { ok: true, omit: false, value };
  }

  if (expectedType === "integer") {
    if (typeof value !== "number" || !Number.isInteger(value)) {
      return { ok: false, error: `invalid argument: ${key} (expected integer)` };
    }
    const minimum = typeof schema.minimum === "number" ? schema.minimum : undefined;
    if (minimum !== undefined && value < minimum) {
      return { ok: false, error: `invalid argument: ${key} (expected integer >= ${minimum})` };
    }
    return { ok: true, omit: false, value };
  }

  if (expectedType === "array") {
    if (!Array.isArray(value)) {
      return { ok: false, error: `invalid argument: ${key} (expected array)` };
    }
    const items: string[] = [];
    for (const [index, entry] of value.entries()) {
      if (typeof entry !== "string") {
        return { ok: false, error: `invalid argument: ${key}[${index}] (expected string)` };
      }
      const trimmed = entry.trim();
      if (trimmed) {
        items.push(trimmed);
      }
    }
    const minItems = typeof schema.minItems === "number" ? schema.minItems : undefined;
    if (minItems !== undefined && items.length < minItems) {
      return { ok: false, error: `invalid argument: ${key} (expected non-empty array of strings)` };
    }
    if (items.length === 0 && required) {
      return { ok: false, error: `missing required argument: ${key}` };
    }
    if (items.length === 0) {
      return { ok: true, omit: true };
    }
    return { ok: true, omit: false, value: items };
  }

  if (expectedType === "object") {
    if (!isRecord(value)) {
      return { ok: false, error: `invalid argument: ${key} (expected object)` };
    }
    return { ok: true, omit: false, value };
  }

  return { ok: false, error: `invalid argument: ${key}` };
}

function readStringEnum(value: unknown): readonly string[] | undefined {
  if (!Array.isArray(value) || value.length === 0 || value.some((entry) => typeof entry !== "string")) {
    return undefined;
  }
  return value;
}

function succeededToolResult(value: unknown): DynamicToolCallResult {
  return {
    contentItems: [{ type: "inputText", text: capResultText(toCompactJson(value)) }],
    success: true,
  };
}

function failedToolResult(reason: string): DynamicToolCallResult {
  return {
    contentItems: [{ type: "inputText", text: reason }],
    success: false,
    reason,
  };
}

function toCompactJson(value: unknown): string {
  try {
    return JSON.stringify(value ?? null) ?? "null";
  } catch (error) {
    throw new Error(error instanceof Error ? error.message : "tool result is not JSON-serializable");
  }
}

function capResultText(text: string): string {
  if (Buffer.byteLength(text, "utf8") <= TOOL_RESULT_TEXT_LIMIT) {
    return text;
  }

  const note = "\n…[truncated]";
  const budget = TOOL_RESULT_TEXT_LIMIT - Buffer.byteLength(note, "utf8");
  let kept = 0;
  let keptBytes = 0;
  // Walk whole code points so the truncation boundary never splits a character.
  for (const char of text) {
    const charBytes = Buffer.byteLength(char, "utf8");
    if (keptBytes + charBytes > budget) {
      break;
    }
    keptBytes += charBytes;
    kept += char.length;
  }

  return `${text.slice(0, kept)}${note}`;
}

function hasOwn(record: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(record, key) && record[key] !== undefined;
}
