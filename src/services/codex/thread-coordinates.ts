export interface ThreadCoordinates {
  readonly threadId: string;
  readonly platform: "slack" | "feishu" | undefined;
  readonly channelId: string;
  readonly conversationId: string | undefined;
  readonly conversationKind: string | undefined;
  readonly rootThreadTs: string;
  readonly rootMessageId: string | undefined;
  readonly platformThreadId: string | undefined;
  readonly workspacePath: string;
  readonly sessionKey: string | undefined;
}
