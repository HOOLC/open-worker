#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";

const XAI_AUTH_KEY = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";
const destRoot = path.resolve(process.argv[2] || ".data");
const grokAuthPath = process.env.GROK_AUTH_JSON || path.join(os.homedir(), ".grok", "auth.json");

const raw = JSON.parse(await fs.readFile(grokAuthPath, "utf8"));
const entry = raw[XAI_AUTH_KEY];
if (!entry || typeof entry !== "object") {
  throw new Error(`no xAI session in ${grokAuthPath}`);
}
const access = typeof entry.key === "string" ? entry.key.trim() : "";
const refresh = typeof entry.refresh_token === "string" ? entry.refresh_token.trim() : "";
if (!access || !refresh) {
  throw new Error("xAI session is missing access/refresh tokens");
}

const expiresRaw = entry.expires_at;
const expires = typeof expiresRaw === "number" && Number.isFinite(expiresRaw) ? (expiresRaw < 1e12 ? expiresRaw * 1000 : expiresRaw) : Date.parse(String(expiresRaw ?? ""));

const document = {
  provider: "xai",
  billing: "subscription",
  base_url: "https://cli-chat-proxy.grok.com/v1",
  headers: {
    "user-agent": "xai-grok-cli",
    "x-xai-token-auth": "xai-grok-cli",
  },
  models: [
    {
      id: "grok-4.6",
      api: "openai-completions",
      streaming: true,
      thinking: ["low", "medium", "high", "xhigh"],
      default_thinking: "xhigh",
      capabilities: { input: ["text", "image"] },
      default: true,
    },
  ],
  auth: {
    type: "oauth",
    access,
    refresh,
    expires: Number.isFinite(expires) ? expires : Date.now() + 60 * 60 * 1000,
    email: typeof entry.email === "string" ? entry.email : undefined,
    team_id: typeof entry.team_id === "string" ? entry.team_id : undefined,
  },
};

const dest = path.join(destRoot, "profiles", "grok.json");
await fs.mkdir(path.dirname(dest), { recursive: true });
await fs.writeFile(dest, `${JSON.stringify(document, null, 2)}\n`, { mode: 0o600 });
process.stdout.write(`seeded grok profile at ${dest}\n`);
