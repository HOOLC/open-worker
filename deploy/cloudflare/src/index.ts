import { Container } from "@cloudflare/containers";
import { DurableObject } from "cloudflare:workers";
import { allowedKeys, decodeKey, hexKey, MAX_AGE_MS, readPayload, verifyPayload } from "./pkarr";

export class Relay extends Container<Env> {
  defaultPort = 8080;
  sleepAfter = "10m";
  envVars = { ALLOWED_KEYS: [...allowedKeys(this.env.ALLOWED_KEYS)].join(",") };
}

type RecordValue = { payload: Uint8Array; timestamp: string; expires: number };

// One strongly consistent object per public key; no global discovery bottleneck.
export class DiscoveryRecord extends DurableObject<Env> {
  publish(payload: Uint8Array, timestamp: string): number {
    return this.ctx.storage.transactionSync(() => {
      const old = this.ctx.storage.kv.get<RecordValue>("record");
      if (old && BigInt(timestamp) < BigInt(old.timestamp)) return 409;
      if (old && timestamp === old.timestamp) {
        // An identical retry is idempotent and does not renew an expired record.
        return old.payload.length === payload.length && old.payload.every((b, i) => b === payload[i]) ? 204 : 409;
      }
      this.ctx.storage.kv.put("record", {
        payload,
        timestamp,
        expires: Number(BigInt(timestamp) / 1000n) + MAX_AGE_MS,
      });
      return 204;
    });
  }

  resolve(): Uint8Array | null {
    const record = this.ctx.storage.kv.get<RecordValue>("record");
    // Retain the timestamp after expiry so an old signed record cannot be replayed.
    return record && record.expires > Date.now() ? record.payload : null;
  }
}

const cors = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, PUT, OPTIONS",
  "access-control-allow-headers": "Content-Type",
  "cache-control": "no-store",
};

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const path = new URL(request.url).pathname;
    if ((path === "/healthz" || path === "/ping") && request.method === "GET") {
      return Response.json({ service: "zork-network", relay: "iroh-relay-1.1.0" });
    }
    if (path === "/relay" && request.method === "GET") {
      if (request.headers.get("upgrade")?.toLowerCase() !== "websocket") {
        return new Response("WebSocket required", { status: 426 });
      }
      // Every connection on this relay URL must reach the same relay process.
      // Random per-request routing would split peers into disconnected relays.
      return env.RELAY.getByName("primary").fetch(request);
    }
    const match = /^\/pkarr\/([a-z0-9]{52})$/.exec(path);
    if (!match) return new Response("Not found", { status: 404 });
    if (request.method === "OPTIONS") return new Response(null, { status: 204, headers: cors });
    if (request.method !== "GET" && request.method !== "PUT") {
      return new Response("Method not allowed", {
        status: 405,
        headers: { ...cors, allow: "GET, PUT, OPTIONS" },
      });
    }
    let key: Uint8Array<ArrayBuffer>;
    try {
      key = decodeKey(match[1]);
    } catch {
      return new Response("Invalid public key", { status: 400, headers: cors });
    }
    if (!allowedKeys(env.ALLOWED_KEYS).has(hexKey(key))) {
      return new Response("Endpoint not allowed", { status: 403, headers: cors });
    }
    const record = env.RECORDS.getByName(match[1]);
    if (request.method === "GET") {
      const payload = await record.resolve();
      return new Response(payload ? new Uint8Array(payload) : null, {
        status: payload ? 200 : 404,
        headers: { ...cors, "content-type": "application/octet-stream" },
      });
    }
    let payload: Uint8Array<ArrayBuffer>;
    let timestamp: bigint;
    try {
      payload = await readPayload(request);
      timestamp = await verifyPayload(key, payload);
    } catch {
      return new Response("Invalid signed packet", { status: 400, headers: cors });
    }
    const status = await record.publish(payload, timestamp.toString());
    return new Response(null, { status, headers: cors });
  },
} satisfies ExportedHandler<Env>;
