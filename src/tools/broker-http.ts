const FETCH_TIMEOUT_MS = 30_000;

export interface ChatCoordinates {
  readonly platform: "slack" | "feishu";
  readonly conversationId: string;
  readonly rootMessageId: string;
}

export interface BrokerFetchResult {
  readonly status: number;
  readonly ok: boolean;
  readonly text: string;
}

export function readEnv(key: string): string | undefined {
  const value = process.env[key]?.trim();
  return value ? value : undefined;
}

export function requireEnv(key: string): string {
  const value = readEnv(key);
  if (!value) {
    throw new Error(`missing environment variable ${key}`);
  }
  return value;
}

export function resolveApiBase(): string {
  return requireEnv("BROKER_API_BASE").replace(/\/+$/u, "");
}

export async function resolveChatCoordinates(): Promise<ChatCoordinates> {
  const platform = readEnv("CHAT_PLATFORM");
  const conversationId = readEnv("CHAT_CONVERSATION_ID");
  const rootMessageId = readEnv("CHAT_ROOT_MESSAGE_ID");
  if (platform && conversationId && rootMessageId) {
    if (platform !== "slack" && platform !== "feishu") {
      throw new Error(`invalid CHAT_PLATFORM: ${platform}`);
    }
    return { platform, conversationId, rootMessageId };
  }

  const threadId = readEnv("CODEX_THREAD_ID");
  if (threadId) {
    return await lookupThreadCoordinates(threadId);
  }

  throw new Error("missing session identity");
}

export function resolveCoauthorCwd(): string {
  return readEnv("SESSION_WORKSPACE") ?? process.cwd();
}

export async function requestBroker(options: { readonly method: "GET" | "POST"; readonly path: string; readonly query?: Record<string, string | undefined>; readonly body?: Record<string, unknown> }): Promise<void> {
  const result = await fetchBroker(options);
  if (!result.ok) {
    throw new Error(`broker request failed (${result.status}): ${result.text}`);
  }

  writeStdout(result.text);
}

export async function fetchBroker(options: { readonly method: "GET" | "POST"; readonly path: string; readonly query?: Record<string, string | undefined>; readonly body?: Record<string, unknown> }): Promise<BrokerFetchResult> {
  const url = new URL(`${resolveApiBase()}${options.path}`);
  for (const [key, value] of Object.entries(options.query ?? {})) {
    if (value) {
      url.searchParams.set(key, value);
    }
  }

  const response = await fetch(url, {
    method: options.method,
    ...(options.body
      ? {
          headers: { "content-type": "application/json" },
          body: JSON.stringify(options.body),
        }
      : {}),
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  });
  const text = await response.text();
  return {
    status: response.status,
    ok: response.status >= 200 && response.status < 300,
    text,
  };
}

async function lookupThreadCoordinates(threadId: string): Promise<ChatCoordinates> {
  const result = await fetchBroker({
    method: "GET",
    path: "/cli/context",
    query: { threadId },
  });
  if (!result.ok) {
    throw new Error(`cli context lookup failed (${result.status}): ${result.text}`);
  }

  const payload = parseJsonObject(result.text);
  const conversationId = readOptionalString(payload.conversationId) ?? readOptionalString(payload.conversation_id) ?? readOptionalString(payload.channelId);
  const rootMessageId = readOptionalString(payload.rootMessageId) ?? readOptionalString(payload.root_message_id) ?? readOptionalString(payload.rootThreadTs);
  if (!conversationId || !rootMessageId) {
    throw new Error("cli context is missing conversation coordinates");
  }

  const platformValue = readOptionalString(payload.platform);
  const platform = platformValue === "feishu" ? "feishu" : "slack";
  return { platform, conversationId, rootMessageId };
}

function writeStdout(text: string): void {
  if (!text.trim()) {
    return;
  }

  process.stdout.write(text);
  if (!text.endsWith("\n")) {
    process.stdout.write("\n");
  }
}

function parseJsonObject(text: string): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(text);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
      throw new Error("cli context response is not an object");
    }
    return parsed as Record<string, unknown>;
  } catch (error) {
    throw new Error(error instanceof Error ? error.message : "cli context response is not JSON");
  }
}

function readOptionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}
