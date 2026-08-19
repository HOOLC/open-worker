/**
 * One-shot protocol probe for the dynamicTools experimental API.
 * Verifies against the locally installed Codex app-server:
 *   1. thread/start accepts a dynamicTools declaration
 *   2. the server sends item/tool/call as a JSON-RPC request when the model
 *      invokes a declared tool, and completes the turn after our response
 *   3. thread/resume without dynamicTools keeps the declaration
 *
 * Usage: npx tsx test/manual/probe-dynamic-tools.ts
 * Exit 0 = all three verified; non-zero with a FAIL line otherwise.
 */
import { spawn } from "node:child_process";
import { WebSocket } from "ws";

const PORT = 4719;
const URL = `ws://127.0.0.1:${PORT}`;

interface JsonRpcMessage {
  id?: number | string;
  method?: string;
  params?: Record<string, unknown>;
  result?: unknown;
  error?: { code: number; message: string };
}

function rpc(id: number | string, method: string, params?: Record<string, unknown>): string {
  return JSON.stringify({ id, method, params });
}

async function main(): Promise<void> {
  const child = spawn("codex", ["app-server", "--disable", "apps", "--listen", `ws://127.0.0.1:${PORT}`], {
    stdio: ["ignore", "pipe", "pipe"],
  });
  const stderr: string[] = [];
  child.stderr?.on("data", (c) => stderr.push(String(c)));

  const ws = await new Promise<WebSocket>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("app-server did not listen")), 20_000);
    const attempt = (tries: number): void => {
      const socket = new WebSocket(URL);
      socket.once("open", () => {
        clearTimeout(timer);
        resolve(socket);
      });
      socket.once("error", (error) => {
        if (tries > 30) {
          clearTimeout(timer);
          reject(error);
          return;
        }
        setTimeout(() => attempt(tries + 1), 500);
      });
    };
    attempt(0);
  });

  const pending = new Map<string | number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>();
  let serverRequest: JsonRpcMessage | undefined;
  const notifications: JsonRpcMessage[] = [];
  let nextId = 1;

  ws.on("message", (data) => {
    const message = JSON.parse(String(data)) as JsonRpcMessage;
    if (message.id !== undefined && message.method !== undefined) {
      serverRequest = message;
      return;
    }
    if (message.id !== undefined && (message.result !== undefined || message.error !== undefined)) {
      const entry = pending.get(message.id);
      pending.delete(message.id);
      if (message.error) {
        entry?.reject(new Error(`${message.error.code}: ${message.error.message}`));
      } else {
        entry?.resolve(message.result);
      }
      return;
    }
    if (message.method) {
      notifications.push(message);
    }
  });

  const request = async (method: string, params?: Record<string, unknown>): Promise<unknown> => {
    const id = nextId++;
    ws.send(rpc(id, method, params));
    return await new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      setTimeout(() => {
        if (pending.has(id)) {
          pending.delete(id);
          reject(new Error(`timeout waiting for ${method}`));
        }
      }, 30_000);
    });
  };

  const failures: string[] = [];
  const check = (label: string, ok: boolean, detail?: unknown): void => {
    if (ok) {
      console.log(`PASS ${label}`);
    } else {
      console.log(`FAIL ${label}${detail !== undefined ? ` :: ${JSON.stringify(detail)?.slice(0, 300)}` : ""}`);
      failures.push(label);
    }
  };

  try {
    await request("initialize", {
      clientInfo: { name: "dynamic-tools-probe", version: "0.0.1" },
      capabilities: { experimentalApi: true },
    });

    const dynamicTools = [
      {
        type: "namespace",
        name: "probe",
        description: "Probe namespace",
        tools: [
          {
            type: "function",
            name: "echo",
            description: "Echo the given text back. Always call this tool at least once before finishing.",
            inputSchema: {
              type: "object",
              properties: { text: { type: "string" } },
              required: ["text"],
            },
          },
        ],
      },
    ];

    // 1. thread/start accepts dynamicTools
    let threadId: string | undefined;
    try {
      const start = (await request("thread/start", {
        cwd: process.cwd(),
        approvalPolicy: "never",
        sandbox: "danger-full-access",
        serviceName: "dynamic-tools-probe",
        baseInstructions: "You are a probe assistant. Call the probe.echo tool with text 'hello-from-probe' before replying.",
        developerInstructions: null,
        personality: null,
        ephemeral: false,
        experimentalRawEvents: true,
        dynamicTools,
      })) as { thread?: { id?: string } };
      threadId = start?.thread?.id;
      check("thread/start accepts dynamicTools", Boolean(threadId));
    } catch (error) {
      check("thread/start accepts dynamicTools", false, error);
      throw new Error("probe cannot continue");
    }

    // 2. item/tool/call reverse request when the model invokes the tool
    let turnDone: (() => void) | undefined;
    const turnCompleted = new Promise<void>((resolve) => {
      turnDone = resolve;
    });
    const watch = setInterval(() => {
      if (notifications.some((n) => n.method === "turn/completed")) {
        turnDone?.();
      }
    }, 100);

    const turn = (await request("turn/start", {
      threadId,
      input: [{ type: "text", text: "Call probe.echo with text 'hello-from-probe' and then reply done.", text_elements: [] }],
      cwd: process.cwd(),
      approvalPolicy: "never",
      sandboxPolicy: { type: "dangerFullAccess" },
      collaborationMode: null,
      outputSchema: null,
      model: null,
      effort: null,
      summary: "auto",
      personality: null,
    })) as { turn?: { id?: string } };
    check("turn/start with declared tool", Boolean(turn?.turn?.id));

    const toolCallDeadline = Date.now() + 90_000;
    while (!serverRequest && Date.now() < toolCallDeadline) {
      await new Promise((r) => setTimeout(r, 200));
    }
    check("server sent item/tool/call request", serverRequest?.method === "item/tool/call", serverRequest);
    if (!serverRequest) {
      console.log(
        "DEBUG notifications:",
        JSON.stringify(
          notifications.map((n) => ({ method: n.method, turnStatus: (n.params as { turn?: { status?: string } } | undefined)?.turn?.status })),
          null,
          1,
        ).slice(0, 2000),
      );
    }

    if (serverRequest?.id !== undefined) {
      const args = (serverRequest.params as { arguments?: Record<string, unknown> } | undefined)?.arguments;
      ws.send(
        JSON.stringify({
          id: serverRequest.id,
          result: {
            contentItems: [{ type: "inputText", text: `echo:${String(args?.text ?? "")}` }],
            success: true,
          },
        }),
      );
    }

    await Promise.race([turnCompleted, new Promise((r) => setTimeout(r, 120_000))]);
    clearInterval(watch);
    check(
      "turn completed after tool response",
      notifications.some((n) => n.method === "turn/completed"),
    );

    // 3. thread/resume without dynamicTools keeps the declaration
    try {
      const resumed = (await request("thread/resume", {
        threadId,
        cwd: process.cwd(),
        approvalPolicy: "never",
        sandbox: "danger-full-access",
        model: null,
        modelProvider: null,
        config: null,
        baseInstructions: null,
        developerInstructions: null,
        personality: null,
        persistExtendedHistory: true,
      })) as { thread?: { id?: string } };
      check("thread/resume succeeds without dynamicTools", resumed?.thread?.id === threadId);
      // Probe whether the declaration survived: run one more turn and see if a tool call still arrives.
      serverRequest = undefined as JsonRpcMessage | undefined;
      notifications.length = 0;
      let resumeDone: (() => void) | undefined;
      const resumeCompleted = new Promise<void>((resolve) => {
        resumeDone = resolve;
      });
      const watch2 = setInterval(() => {
        if (notifications.some((n) => n.method === "turn/completed")) {
          resumeDone?.();
        }
      }, 100);
      await request("turn/start", {
        threadId,
        input: [{ type: "text", text: "Call probe.echo with text 'resume-check' and then reply done.", text_elements: [] }],
        cwd: process.cwd(),
        approvalPolicy: "never",
        sandboxPolicy: { type: "dangerFullAccess" },
        collaborationMode: null,
        outputSchema: null,
        model: null,
        effort: null,
        summary: "auto",
        personality: null,
      });
      const resumeDeadline = Date.now() + 90_000;
      while (!serverRequest && Date.now() < resumeDeadline) {
        await new Promise((r) => setTimeout(r, 200));
      }
      clearInterval(watch2);
      check("dynamicTools declaration survives thread/resume", serverRequest?.method === "item/tool/call", serverRequest?.method);
      if (serverRequest?.id !== undefined) {
        ws.send(
          JSON.stringify({
            id: serverRequest.id,
            result: { contentItems: [{ type: "inputText", text: "ok" }], success: true },
          }),
        );
      }
      await Promise.race([resumeCompleted, new Promise((r) => setTimeout(r, 60_000))]);
    } catch (error) {
      check("dynamicTools declaration survives thread/resume", false, error);
    }
  } finally {
    ws.close();
    child.kill("SIGTERM");
    if (failures.length > 0) {
      console.error(`\n${failures.length} check(s) failed. Server stderr:\n${stderr.join("").slice(-1500)}`);
      process.exit(1);
    }
    console.log("\nall dynamicTools probes passed");
    process.exit(0);
  }
}

void main().catch((error) => {
  console.error("probe crashed:", error);
  process.exit(1);
});
