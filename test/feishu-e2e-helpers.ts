import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

import { createChatSessionKey } from "../src/services/chat/chat-session-key.js";
import { MockCodexAppServer, type MockTurnContext } from "./helpers/mock-codex-app-server.js";
import { MockFeishuServer } from "./helpers/mock-feishu-server.js";
import { MockSlackServer } from "./manual/mock-slack-server.js";
import { createFeishuE2eEnv, feishuE2eSdkRegisterUrl, getFreePort, removeTempRoot, startBrokerProcess } from "./e2e-broker-helpers.js";

export interface FeishuE2eRuntime {
  readonly tempRoot: string;
  readonly baseUrl: string;
  readonly mockFeishu: MockFeishuServer;
  readonly mockSlack: MockSlackServer;
  readonly mockCodex: MockCodexAppServer;
  readonly logs: readonly string[];
  readonly stop: () => Promise<void>;
}

export function feishuSessionKey(conversationId: string, rootMessageId: string): string {
  return createChatSessionKey({
    platform: "feishu",
    conversationId,
    rootMessageId,
  });
}

export async function startFeishuE2eRuntime(options?: { readonly extraEnv?: Record<string, string>; readonly onTurnStart?: ((context: MockTurnContext) => Promise<void> | void) | undefined }): Promise<FeishuE2eRuntime> {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "feishu-broker-e2e-"));
  const mockFeishu = new MockFeishuServer();
  const mockSlack = new MockSlackServer("UBOT", {
    botId: "BBOT",
    appId: "AAPP",
  });
  const mockCodex = new MockCodexAppServer(
    options?.onTurnStart
      ? {
          onTurnStart: options.onTurnStart,
        }
      : undefined,
  );
  const feishuPort = await mockFeishu.start();
  const slackPort = await mockSlack.start();
  const codexUrl = await mockCodex.start();
  let broker:
    | {
        readonly baseUrl: string;
        readonly logs: readonly string[];
        readonly stop: () => Promise<void>;
      }
    | undefined;

  const stop = async (): Promise<void> => {
    await broker?.stop();
    await mockCodex.stop();
    await mockSlack.stop();
    await mockFeishu.stop();
    await removeTempRoot(tempRoot);
  };

  try {
    broker = await startBrokerProcess({
      port: await getFreePort(),
      slackPort,
      codexUrl,
      tempRoot,
      extraEnv: createFeishuE2eEnv(feishuPort, options?.extraEnv),
      nodeImports: [feishuE2eSdkRegisterUrl()],
    });
    await mockFeishu.waitForSocket();
    await mockSlack.waitForSocket();
  } catch (error) {
    await stop();
    throw error;
  }

  return {
    tempRoot,
    baseUrl: broker.baseUrl,
    mockFeishu,
    mockSlack,
    mockCodex,
    logs: broker.logs,
    stop,
  };
}

export async function readBrokerJsonl(tempRoot: string): Promise<Array<Record<string, unknown>>> {
  const logPath = path.join(tempRoot, "logs", "broker.jsonl");
  try {
    const raw = await fs.readFile(logPath, "utf8");
    return raw
      .split("\n")
      .filter((line) => line.trim().length > 0)
      .map((line) => JSON.parse(line) as Record<string, unknown>);
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT") {
      return [];
    }

    throw error;
  }
}

export async function postChatJson(baseUrl: string, pathname: string, payload: Record<string, unknown>): Promise<Response> {
  return await fetch(`${baseUrl}${pathname}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
    },
    body: JSON.stringify(payload),
  });
}
