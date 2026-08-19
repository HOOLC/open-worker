import http from "node:http";

import { afterEach, describe, expect, it } from "vitest";

import { WebSocketServer, type WebSocket } from "ws";

export interface TestServer {
  readonly url: string;
  readonly close: () => Promise<void>;
}

export async function createServer(onMessage: (socket: WebSocket, message: { id?: string; method?: string; params?: Record<string, unknown> }) => void): Promise<TestServer> {
  const server = http.createServer();
  const wsServer = new WebSocketServer({ server });
  const connections = new Set<WebSocket>();

  wsServer.on("connection", (socket) => {
    connections.add(socket);
    socket.on("close", () => {
      connections.delete(socket);
    });
    socket.on("message", (data) => {
      onMessage(socket, JSON.parse(data.toString()) as { id?: string; method?: string; params?: Record<string, unknown> });
    });
  });

  await new Promise<void>((resolve) => {
    server.listen(0, "127.0.0.1", () => resolve());
  });

  const address = server.address();
  if (!address || typeof address === "string") {
    throw new Error("failed to bind test websocket server");
  }

  return {
    url: `ws://127.0.0.1:${address.port}`,
    close: async () => {
      for (const connection of connections) {
        connection.close();
      }

      await new Promise<void>((done) => {
        wsServer.close(() => {
          server.close(() => done());
        });
      });
    },
  };
}
