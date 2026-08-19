// -nocheck
import http from "node:http";

import fs from "node:fs/promises";

import path from "node:path";

import { fileURLToPath, URL } from "node:url";

import type { AppConfig } from "../config.js";

import type { AdminService } from "../services/admin-service.js";

import { readJsonBody, respondJson } from "./common.js";

import { renderAdminPage } from "./admin-page.js";

export function matchSessionJobCancelPath(pathname: string): {
  readonly sessionKey: string;
  readonly jobId: string;
} | null {
  const prefix = "/admin/api/sessions/";
  const suffix = "/cancel";
  if (!pathname.startsWith(prefix) || !pathname.endsWith(suffix)) {
    return null;
  }
  const middle = pathname.slice(prefix.length, -suffix.length);
  const marker = "/jobs/";
  const markerIndex = middle.indexOf(marker);
  if (markerIndex <= 0) {
    return null;
  }
  const encodedSessionKey = middle.slice(0, markerIndex);
  const encodedJobId = middle.slice(markerIndex + marker.length);
  if (!encodedSessionKey || !encodedJobId || encodedSessionKey.includes("/") || encodedJobId.includes("/")) {
    return null;
  }

  return {
    sessionKey: decodeURIComponent(encodedSessionKey),
    jobId: decodeURIComponent(encodedJobId),
  };
}

export function matchSessionTimelineEventPath(pathname: string): {
  readonly sessionKey: string;
  readonly eventId: string;
} | null {
  const prefix = "/admin/api/sessions/";
  const marker = "/timeline-events/";
  if (!pathname.startsWith(prefix)) {
    return null;
  }
  const middle = pathname.slice(prefix.length);
  const markerIndex = middle.indexOf(marker);
  if (markerIndex <= 0) {
    return null;
  }
  const encodedSessionKey = middle.slice(0, markerIndex);
  const encodedEventId = middle.slice(markerIndex + marker.length);
  if (!encodedSessionKey || !encodedEventId || encodedSessionKey.includes("/") || encodedEventId.includes("/")) {
    return null;
  }
  return {
    sessionKey: decodeURIComponent(encodedSessionKey),
    eventId: decodeURIComponent(encodedEventId),
  };
}

export async function serveAdminSpaIndex(response: http.ServerResponse, config: AppConfig): Promise<boolean> {
  if (process.env.ADMIN_UI_DEV_ORIGIN) {
    response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
    response.end(
      renderAdminPage({
        serviceName: config.serviceName,
      }),
    );
    return true;
  }

  const indexPath = await findAdminSpaIndex();
  if (indexPath) {
    const html = await fs.readFile(indexPath, "utf8");
    response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
    response.end(html);
    return true;
  }

  response.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
  response.end(
    renderAdminPage({
      serviceName: config.serviceName,
    }),
  );
  return true;
}

export async function findAdminSpaIndex(): Promise<string | null> {
  const moduleDir = path.dirname(fileURLToPath(import.meta.url));
  const candidates = [path.resolve(moduleDir, "..", "..", "admin-ui", "index.html"), path.resolve(moduleDir, "..", "..", "dist", "admin-ui", "index.html")];

  for (const candidate of candidates) {
    try {
      await fs.access(candidate);
      return candidate;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
        throw error;
      }
    }
  }

  return null;
}

export async function serveAdminAsset(url: URL, response: http.ServerResponse): Promise<boolean> {
  const assetName = decodeURIComponent(url.pathname.slice("/admin/assets/".length));
  if (!assetName || assetName.includes("\0")) {
    return false;
  }

  const moduleDir = path.dirname(fileURLToPath(import.meta.url));
  const assetRoots = [path.resolve(moduleDir, "..", "..", "admin-ui", "assets"), path.resolve(moduleDir, "..", "..", "dist", "admin-ui", "assets")];

  for (const assetRoot of assetRoots) {
    const assetPath = path.resolve(assetRoot, assetName);
    if (!assetPath.startsWith(`${assetRoot}${path.sep}`)) {
      continue;
    }

    try {
      const content = await fs.readFile(assetPath);
      response.writeHead(200, {
        "content-type": contentTypeForAsset(assetPath),
        "cache-control": "no-store",
      });
      response.end(content);
      return true;
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") {
        throw error;
      }
    }
  }

  response.writeHead(404, { "content-type": "application/json" });
  response.end(JSON.stringify({ ok: false, error: "admin_asset_not_found" }));
  return true;
}

export function isAdminSpaRoute(pathname: string): boolean {
  return pathname === "/admin" || pathname === "/admin/" || pathname.startsWith("/admin/sessions/");
}

export function contentTypeForAsset(assetPath: string): string {
  const extension = path.extname(assetPath).toLowerCase();
  if (extension === ".css") return "text/css; charset=utf-8";
  if (extension === ".js") return "text/javascript; charset=utf-8";
  if (extension === ".map") return "application/json; charset=utf-8";
  if (extension === ".svg") return "image/svg+xml";
  return "application/octet-stream";
}

export async function readAdminBody(request: http.IncomingMessage, response: http.ServerResponse): Promise<Record<string, unknown> | null> {
  try {
    return await readJsonBody(request);
  } catch (error) {
    respondJson(response, 400, {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
    return null;
  }
}

export function readPositiveNumber(value: unknown): number | undefined {
  const numeric = typeof value === "number" ? value : typeof value === "string" ? Number.parseFloat(value) : Number.NaN;
  return Number.isFinite(numeric) && numeric > 0 ? numeric : undefined;
}

export function readReleaseTarget(value: unknown): "admin" | "worker" | undefined {
  return value === "admin" || value === "worker" ? value : undefined;
}

export async function runAdminOperation(
  response: http.ServerResponse,
  operation: () => Promise<Record<string, unknown>>,
  options?: {
    readonly errorStatus?: ((error: unknown) => number) | undefined;
  },
): Promise<void> {
  try {
    respondJson(response, 200, await operation());
  } catch (error) {
    respondJson(response, options?.errorStatus?.(error) ?? 500, {
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}

export function isSessionNotFoundError(error: unknown): boolean {
  const message = error instanceof Error ? error.message : String(error);
  return message.startsWith("Session not found:");
}

export function streamAdminEvents(request: http.IncomingMessage, response: http.ServerResponse, adminService: AdminService, url: URL): void {
  let cursor = readEventCursor(url, request);
  if (cursor <= 0) {
    cursor = adminService.getRealtimeCursor();
  }
  let closed = false;
  let draining = false;

  response.writeHead(200, {
    "content-type": "text/event-stream; charset=utf-8",
    "cache-control": "no-store, no-transform",
    connection: "keep-alive",
    "x-accel-buffering": "no",
  });
  response.flushHeaders?.();

  const interval = setInterval(() => {
    void drain();
  }, 500);

  request.on("close", () => {
    closed = true;
    clearInterval(interval);
  });

  void drain();

  async function drain(): Promise<void> {
    if (closed || draining) {
      return;
    }
    draining = true;
    try {
      const payload = await adminService.listRealtimeEvents({
        afterSequence: cursor,
        limit: 100,
      });
      const events = Array.isArray(payload.events) ? (payload.events as Array<Record<string, unknown>>) : [];
      for (const event of events) {
        const sequence = Number(event.sequence);
        if (!Number.isFinite(sequence)) {
          continue;
        }
        cursor = Math.max(cursor, sequence);
        response.write(`id: ${sequence}\n`);
        response.write("event: admin-event\n");
        response.write(`data: ${JSON.stringify({ ok: true, event })}\n\n`);
      }
    } catch (error) {
      response.write("event: admin-error\n");
      response.write(
        `data: ${JSON.stringify({
          ok: false,
          error: error instanceof Error ? error.message : String(error),
        })}\n\n`,
      );
    } finally {
      draining = false;
    }
  }
}

export function readEventCursor(url: URL, request: http.IncomingMessage): number {
  const fromHeader = request.headers["last-event-id"];
  const value = Array.isArray(fromHeader) ? fromHeader.at(-1) : fromHeader;
  const parsed = value == null || value === "" ? NaN : Number(value);
  if (Number.isFinite(parsed) && parsed >= 0) {
    return Math.floor(parsed);
  }

  const fromQuery = Number(url.searchParams.get("after") ?? "");
  return Number.isFinite(fromQuery) && fromQuery >= 0 ? Math.floor(fromQuery) : 0;
}

export async function respondTracedAdminJson(response: http.ServerResponse, label: string, load: () => Promise<Record<string, unknown>>): Promise<void> {
  const startedAt = Date.now();
  const body = await load();
  const durationMs = Math.max(0, Date.now() - startedAt);
  response.setHeader("server-timing", `admin;desc="${label}";dur=${durationMs}`);
  response.setHeader("x-admin-duration-ms", String(durationMs));
  respondJson(response, 200, body);
}

export function isAuthorizedAdminRequest(request: http.IncomingMessage, config: AppConfig): boolean {
  if (!config.brokerAdminToken) {
    return true;
  }

  const fromHeader = request.headers["x-admin-token"];
  if (typeof fromHeader === "string" && fromHeader === config.brokerAdminToken) {
    return true;
  }

  const authorization = request.headers.authorization;
  if (typeof authorization === "string" && authorization.startsWith("Bearer ")) {
    return authorization.slice("Bearer ".length) === config.brokerAdminToken;
  }

  return false;
}
