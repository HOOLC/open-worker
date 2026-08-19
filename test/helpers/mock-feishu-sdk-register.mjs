import http from "node:http";
import https from "node:https";
import { register } from "node:module";

installFeishuHttpsRedirect();
register(new URL("./mock-feishu-sdk-loader.mjs", import.meta.url));

function installFeishuHttpsRedirect() {
  const originalRequest = https.request.bind(https);
  const originalGet = https.get.bind(https);

  https.request = function patchedHttpsRequest(...args) {
    const rewritten = rewriteOpenFeishuRequest(args);
    if (rewritten) {
      return http.request(...rewritten);
    }

    return originalRequest(...args);
  };

  https.get = function patchedHttpsGet(...args) {
    const rewritten = rewriteOpenFeishuRequest(args);
    if (rewritten) {
      return http.get(...rewritten);
    }

    return originalGet(...args);
  };
}

function rewriteOpenFeishuRequest(args) {
  const mockOrigin = process.env.FEISHU_MOCK_ORIGIN;
  if (!mockOrigin) {
    return undefined;
  }

  const parsed = parseRequestArgs(args);
  if (!parsed || parsed.url.hostname !== "open.feishu.cn") {
    return undefined;
  }

  const mock = new URL(mockOrigin);
  const rewritten = {
    ...parsed.options,
    protocol: "http:",
    hostname: mock.hostname,
    host: mock.host,
    port: mock.port,
    path: `${parsed.url.pathname}${parsed.url.search}`,
    href: `${mock.origin}${parsed.url.pathname}${parsed.url.search}`,
    agent: undefined,
    createConnection: undefined,
    defaultPort: undefined,
  };

  return parsed.callback ? [rewritten, parsed.callback] : [rewritten];
}

function parseRequestArgs(args) {
  const first = args[0];
  const second = args[1];
  const third = args[2];

  if (typeof first === "string" || first instanceof URL) {
    const url = new URL(first);
    if (typeof second === "function") {
      return {
        url,
        options: {},
        callback: second,
      };
    }

    return {
      url,
      options: second ?? {},
      callback: typeof third === "function" ? third : undefined,
    };
  }

  if (!first || typeof first !== "object") {
    return undefined;
  }

  const options = first;
  const hostname = String(options.hostname ?? String(options.host ?? "").split(":")[0] ?? "");
  const protocol = String(options.protocol ?? "https:");
  const port = options.port ?? (protocol === "http:" ? 80 : 443);
  const path = String(options.path ?? "/");
  return {
    url: new URL(`${protocol}//${hostname}:${port}${path}`),
    options,
    callback: typeof second === "function" ? second : undefined,
  };
}
