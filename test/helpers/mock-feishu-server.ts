import http from "node:http";
import { once } from "node:events";

import { WebSocketServer, type WebSocket } from "ws";

export interface PostedFeishuMessage {
  readonly method: "create" | "reply";
  readonly receiveId?: string | undefined;
  readonly replyToMessageId?: string | undefined;
  readonly msgType: string;
  readonly content: unknown;
  readonly uuid?: string | undefined;
  readonly replyInThread?: boolean | undefined;
  readonly messageId: string;
}

export interface PatchedFeishuMessage {
  readonly messageId: string;
  readonly content: unknown;
}

export interface UploadedFeishuFile {
  readonly kind: "image" | "file";
  readonly key: string;
}

export interface MockFeishuHistoryItem {
  readonly message_id: string;
  readonly root_id?: string | undefined;
  readonly parent_id?: string | undefined;
  readonly thread_id?: string | undefined;
  readonly msg_type?: string | undefined;
  readonly create_time?: string | undefined;
  readonly chat_id?: string | undefined;
  readonly body?:
    | {
        readonly content?: string | undefined;
      }
    | undefined;
  readonly raw?:
    | {
        readonly sender?: {
          readonly id?: string | undefined;
          readonly id_type?: string | undefined;
          readonly sender_type?: string | undefined;
        };
      }
    | undefined;
}

export interface FeishuReceiveMessageEvent {
  readonly sender: {
    readonly sender_id: {
      readonly open_id: string;
    };
    readonly sender_type: string;
  };
  readonly message: {
    readonly chat_id: string;
    readonly chat_type: string;
    readonly message_id: string;
    readonly message_type: string;
    readonly content: string;
    readonly root_id?: string | undefined;
    readonly parent_id?: string | undefined;
    readonly thread_id?: string | undefined;
    readonly create_time?: string | undefined;
    readonly mentions?: readonly {
      readonly key: string;
      readonly id: {
        readonly open_id: string;
      };
      readonly name: string;
    }[];
  };
}

export class MockFeishuServer {
  readonly #server: http.Server;
  readonly #wsServer: WebSocketServer;
  #socket: WebSocket | undefined;
  #nextMessageId = 1;
  #nextImageId = 1;
  #nextFileId = 1;
  readonly postedMessages: PostedFeishuMessage[] = [];
  readonly patchedMessages: PatchedFeishuMessage[] = [];
  readonly uploadedFiles: UploadedFeishuFile[] = [];
  readonly listRequests: Array<{ readonly url: string; readonly params: Record<string, string> }> = [];
  readonly resourceRequests: Array<{ readonly messageId: string; readonly fileKey: string; readonly type?: string | undefined }> = [];
  historyItems: MockFeishuHistoryItem[] = [];
  historyHasMore = false;
  historyPageToken: string | undefined;
  replyError: { readonly code: number; readonly msg: string } | undefined;
  resourceBytes = Buffer.from("hello");
  resourceContentType = "image/png";

  constructor() {
    this.#server = http.createServer((request, response) => {
      void this.#handleHttp(request, response);
    });
    this.#wsServer = new WebSocketServer({ noServer: true });
    this.#server.on("upgrade", (request, socket, head) => {
      const pathname = request.url?.split("?")[0];
      if (pathname !== "/socket") {
        socket.destroy();
        return;
      }

      this.#wsServer.handleUpgrade(request, socket, head, (websocket) => {
        this.#socket = websocket;
        websocket.on("close", () => {
          if (this.#socket === websocket) {
            this.#socket = undefined;
          }
        });
        this.#wsServer.emit("connection", websocket, request);
      });
    });
  }

  async start(): Promise<number> {
    this.#server.listen(0, "127.0.0.1");
    await once(this.#server, "listening");
    const address = this.#server.address();
    if (!address || typeof address === "string") {
      throw new Error("Mock Feishu server did not bind to a TCP port");
    }

    return address.port;
  }

  async stop(): Promise<void> {
    this.#socket?.close();
    this.#wsServer.close();
    await new Promise<void>((resolve, reject) => {
      this.#server.close((error) => {
        if (error) {
          reject(error);
          return;
        }

        resolve();
      });
    });
  }

  get origin(): string {
    const address = this.#server.address();
    if (!address || typeof address === "string") {
      throw new Error("Mock Feishu server is not listening");
    }

    return `http://127.0.0.1:${address.port}`;
  }

  get wsUrl(): string {
    return `${this.origin.replace("http://", "ws://")}/socket`;
  }

  async waitForSocket(): Promise<void> {
    if (this.#socket && this.#socket.readyState === 1) {
      return;
    }

    await once(this.#wsServer, "connection");
  }

  async sendEvent(eventType: string, event: unknown, eventId = `evt-${Date.now()}`): Promise<void> {
    await this.waitForSocket();
    await new Promise<void>((resolve, reject) => {
      this.#socket?.send(
        JSON.stringify({
          eventType,
          eventId,
          event,
        }),
        (error) => {
          if (error) {
            reject(error);
            return;
          }

          resolve();
        },
      );
    });
  }

  async sendReceiveMessage(event: FeishuReceiveMessageEvent, eventId?: string): Promise<void> {
    await this.sendEvent("im.message.receive_v1", event, eventId ?? `evt-${event.message.message_id}`);
  }

  async waitForPostedMessage(predicate: (message: PostedFeishuMessage) => boolean, timeoutMs = 30_000): Promise<PostedFeishuMessage> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const match = this.postedMessages.find(predicate);
      if (match) {
        return match;
      }

      await delay(50);
    }

    throw new Error("Timed out waiting for Feishu outbound message");
  }

  async #handleHttp(request: http.IncomingMessage, response: http.ServerResponse): Promise<void> {
    const url = new URL(request.url ?? "/", "http://127.0.0.1");
    const pathname = url.pathname;
    const body = await readRequestBody(request);

    if (request.method === "POST" && pathname === "/open-apis/auth/v3/tenant_access_token/internal") {
      writeJson(response, {
        code: 0,
        tenant_access_token: "t-feishu-e2e",
        expire: 7200,
      });
      return;
    }

    if (request.method === "POST" && pathname === "/open-apis/auth/v3/app_access_token/internal") {
      writeJson(response, {
        code: 0,
        app_access_token: "a-feishu-e2e",
        expire: 7200,
      });
      return;
    }

    if (request.method === "POST" && pathname === "/open-apis/im/v1/messages") {
      const posted = this.#recordOutbound("create", body);
      writeJson(response, {
        code: 0,
        msg: "ok",
        data: posted,
      });
      return;
    }

    const replyMatch = pathname.match(/^\/open-apis\/im\/v1\/messages\/([^/]+)\/reply$/u);
    if (request.method === "POST" && replyMatch?.[1]) {
      if (this.replyError) {
        writeJson(response, this.replyError);
        return;
      }

      const posted = this.#recordOutbound("reply", body, decodeURIComponent(replyMatch[1]));
      writeJson(response, {
        code: 0,
        msg: "ok",
        data: posted,
      });
      return;
    }

    const patchMatch = pathname.match(/^\/open-apis\/im\/v1\/messages\/([^/]+)$/u);
    if (request.method === "PATCH" && patchMatch?.[1]) {
      const messageId = decodeURIComponent(patchMatch[1]);
      this.patchedMessages.push({
        messageId,
        content: parseJsonContent(body.content),
      });
      writeJson(response, {
        code: 0,
        msg: "ok",
        data: {
          message_id: messageId,
        },
      });
      return;
    }

    if (request.method === "GET" && pathname === "/open-apis/im/v1/messages") {
      const params = Object.fromEntries(url.searchParams.entries());
      this.listRequests.push({
        url: request.url ?? pathname,
        params,
      });
      writeJson(response, {
        code: 0,
        msg: "ok",
        data: {
          has_more: this.historyHasMore,
          page_token: this.historyPageToken,
          items: this.historyItems,
        },
      });
      return;
    }

    if (request.method === "POST" && pathname === "/open-apis/im/v1/images") {
      const key = `img_uploaded_${this.#nextImageId++}`;
      this.uploadedFiles.push({
        kind: "image",
        key,
      });
      writeJson(response, {
        code: 0,
        data: {
          image_key: key,
        },
      });
      return;
    }

    if (request.method === "POST" && pathname === "/open-apis/im/v1/files") {
      const key = `file_uploaded_${this.#nextFileId++}`;
      this.uploadedFiles.push({
        kind: "file",
        key,
      });
      writeJson(response, {
        code: 0,
        data: {
          file_key: key,
        },
      });
      return;
    }

    const resourceMatch = pathname.match(/^\/open-apis\/im\/v1\/messages\/([^/]+)\/resources\/([^/]+)$/u);
    if (request.method === "GET" && resourceMatch?.[1] && resourceMatch[2]) {
      this.resourceRequests.push({
        messageId: decodeURIComponent(resourceMatch[1]),
        fileKey: decodeURIComponent(resourceMatch[2]),
        type: url.searchParams.get("type") ?? undefined,
      });
      response.writeHead(200, {
        "content-type": this.resourceContentType,
        "content-length": String(this.resourceBytes.byteLength),
      });
      response.end(this.resourceBytes);
      return;
    }

    writeJson(
      response,
      {
        code: 404,
        msg: `unhandled ${request.method} ${pathname}`,
      },
      404,
    );
  }

  #recordOutbound(
    method: "create" | "reply",
    body: Record<string, unknown>,
    replyToMessageId?: string,
  ): {
    readonly message_id: string;
    readonly root_id: string;
    readonly parent_id?: string;
    readonly chat_id?: string;
    readonly msg_type: string;
    readonly create_time: string;
  } {
    const messageId = `om_reply_${this.#nextMessageId++}`;
    const msgType = typeof body.msg_type === "string" ? body.msg_type : "text";
    const receiveId = typeof body.receive_id === "string" ? body.receive_id : undefined;
    const posted: PostedFeishuMessage = {
      method,
      receiveId,
      replyToMessageId,
      msgType,
      content: parseJsonContent(body.content),
      uuid: typeof body.uuid === "string" ? body.uuid : undefined,
      replyInThread: typeof body.reply_in_thread === "boolean" ? body.reply_in_thread : undefined,
      messageId,
    };
    this.postedMessages.push(posted);
    const rootId = replyToMessageId ?? messageId;
    return {
      message_id: messageId,
      root_id: rootId,
      ...(replyToMessageId ? { parent_id: replyToMessageId } : {}),
      ...(receiveId ? { chat_id: receiveId } : {}),
      msg_type: msgType,
      create_time: String(Date.now()),
    };
  }
}

export function createFeishuGroupTextEvent(options: {
  readonly messageId: string;
  readonly text: string;
  readonly chatId?: string | undefined;
  readonly chatType?: "group" | "p2p" | undefined;
  readonly mentionBot?: boolean | undefined;
  readonly parentId?: string | undefined;
  readonly rootId?: string | undefined;
  readonly threadId?: string | undefined;
  readonly createTime?: string | undefined;
  readonly senderId?: string | undefined;
  readonly senderType?: "user" | "app" | "bot" | undefined;
}): FeishuReceiveMessageEvent {
  const mentionBot = options.mentionBot ?? false;
  const event: FeishuReceiveMessageEvent = {
    sender: {
      sender_id: {
        open_id: options.senderId ?? "ou_user",
      },
      sender_type: options.senderType ?? "user",
    },
    message: {
      chat_id: options.chatId ?? "oc_group",
      chat_type: options.chatType ?? "group",
      message_id: options.messageId,
      message_type: "text",
      content: JSON.stringify({
        text: options.text,
      }),
      create_time: options.createTime ?? "1710000000000",
      ...(options.rootId ? { root_id: options.rootId } : {}),
      ...(options.parentId ? { parent_id: options.parentId } : {}),
      ...(options.threadId ? { thread_id: options.threadId } : {}),
      ...(mentionBot
        ? {
            mentions: [
              {
                key: "@_user_1",
                id: {
                  open_id: "ou_bot",
                },
                name: "Codex",
              },
            ],
          }
        : {}),
    },
  };
  return event;
}

export function createFeishuImageEvent(options: { readonly messageId: string; readonly imageKey: string; readonly chatId?: string | undefined; readonly mentionBot?: boolean | undefined }): FeishuReceiveMessageEvent {
  const mentionBot = options.mentionBot ?? true;
  return {
    sender: {
      sender_id: {
        open_id: "ou_user",
      },
      sender_type: "user",
    },
    message: {
      chat_id: options.chatId ?? "oc_group",
      chat_type: "group",
      message_id: options.messageId,
      message_type: "image",
      content: JSON.stringify({
        image_key: options.imageKey,
      }),
      create_time: "1710000001000",
      ...(mentionBot
        ? {
            mentions: [
              {
                key: "@_user_1",
                id: {
                  open_id: "ou_bot",
                },
                name: "Codex",
              },
            ],
          }
        : {}),
    },
  };
}

async function readRequestBody(request: http.IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of request) {
    chunks.push(Buffer.from(chunk));
  }

  if (chunks.length === 0) {
    return {};
  }

  const rawBody = Buffer.concat(chunks).toString("utf8");
  const contentType = request.headers["content-type"] ?? "";
  if (contentType.includes("application/json") || rawBody.startsWith("{") || rawBody.startsWith("[")) {
    try {
      return JSON.parse(rawBody) as Record<string, unknown>;
    } catch {
      return {
        raw: rawBody,
      };
    }
  }

  return {
    raw: rawBody,
  };
}

function parseJsonContent(value: unknown): unknown {
  if (typeof value !== "string") {
    return value;
  }

  try {
    return JSON.parse(value);
  } catch {
    return value;
  }
}

function writeJson(response: http.ServerResponse, body: unknown, status = 200): void {
  response.writeHead(status, {
    "content-type": "application/json",
  });
  response.end(JSON.stringify(body));
}

async function delay(timeoutMs: number): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, timeoutMs));
}
