import { WebSocket } from "ws";

export * from "@larksuiteoapi/node-sdk";

export class WSClient {
  #socket;
  #dispatcher;

  constructor() {
    this.#socket = undefined;
    this.#dispatcher = undefined;
  }

  async start(params) {
    const eventDispatcher = params?.eventDispatcher;
    if (!eventDispatcher) {
      throw new Error("Mock Feishu WSClient requires an eventDispatcher");
    }

    const wsUrl = process.env.FEISHU_MOCK_WS_URL;
    if (!wsUrl) {
      throw new Error("FEISHU_MOCK_WS_URL is required for the Feishu e2e SDK intercept");
    }

    this.#dispatcher = eventDispatcher;
    this.#socket = new WebSocket(wsUrl);
    await new Promise((resolve, reject) => {
      this.#socket.once("open", resolve);
      this.#socket.once("error", reject);
    });
    this.#socket.on("message", (data) => {
      void this.#dispatch(data);
    });
  }

  close() {
    this.#socket?.removeAllListeners();
    this.#socket?.close();
    this.#socket = undefined;
  }

  async #dispatch(data) {
    let parsed;
    try {
      parsed = JSON.parse(data.toString());
    } catch {
      return;
    }

    const eventType = typeof parsed.eventType === "string" ? parsed.eventType : undefined;
    if (!eventType || !this.#dispatcher) {
      return;
    }

    await this.#dispatcher.invoke({
      schema: "2.0",
      header: {
        event_id: typeof parsed.eventId === "string" ? parsed.eventId : `evt-${Date.now()}`,
        event_type: eventType,
      },
      event: parsed.event,
    });
  }
}
