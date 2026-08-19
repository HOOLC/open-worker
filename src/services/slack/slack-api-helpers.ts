import type { JsonLike, SlackImageAttachment, SlackSenderKind } from "../../types.js";

export function parseRetryAfterMs(header: string | null): number | undefined {
  if (!header) {
    return undefined;
  }
  const seconds = Number(header);
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return undefined;
  }
  return Math.ceil(seconds * 1_000);
}

export function normalizeSlackFileAttachments(files: unknown): SlackImageAttachment[] {
  if (!Array.isArray(files)) {
    return [];
  }

  return files.flatMap((entry) => {
    if (!entry || typeof entry !== "object") {
      return [];
    }

    const file = entry as Record<string, unknown>;
    const fileId = normalizeSlackField(file.id);
    const mimetype = normalizeSlackField(file.mimetype);

    if (!fileId) {
      return [];
    }

    const url = pickSlackFileUrl(file);
    if (!url) {
      return [];
    }

    return [
      {
        fileId,
        name: normalizeSlackField(file.name),
        title: normalizeSlackField(file.title),
        mimetype,
        filetype: normalizeSlackField(file.filetype),
        size: normalizeSlackNumber(file.size),
        width: normalizeSlackNumber(file.original_w ?? file.thumb_1024_w ?? file.thumb_960_w ?? file.thumb_720_w ?? file.thumb_480_w ?? file.thumb_360_w),
        height: normalizeSlackNumber(file.original_h ?? file.thumb_1024_h ?? file.thumb_960_h ?? file.thumb_720_h ?? file.thumb_480_h ?? file.thumb_360_h),
        url,
      },
    ];
  });
}

export const normalizeSlackImageAttachments = normalizeSlackFileAttachments;

export function normalizeSlackJson(value: unknown): JsonLike | undefined {
  if (value === null) {
    return null;
  }

  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return value;
  }

  if (Array.isArray(value)) {
    return value.map((entry) => normalizeSlackJson(entry)).filter((entry): entry is JsonLike => entry !== undefined);
  }

  if (typeof value !== "object") {
    return undefined;
  }

  const normalizedEntries = Object.entries(value)
    .map(([key, entry]) => [key, normalizeSlackJson(entry)] as const)
    .filter(([, entry]) => entry !== undefined);

  return Object.fromEntries(normalizedEntries) as { [key: string]: JsonLike };
}

export function resolveSlackMessageAuthor(message: Record<string, unknown>): {
  readonly userId?: string | undefined;
  readonly senderKind: SlackSenderKind;
  readonly botId?: string | undefined;
  readonly appId?: string | undefined;
  readonly senderUsername?: string | undefined;
} {
  const userId = normalizeSlackField(message.user);
  if (userId) {
    return {
      userId,
      senderKind: "user",
    };
  }

  const botId = normalizeSlackField(message.bot_id);
  const appId = normalizeSlackField(message.app_id);
  const senderUsername = normalizeSlackField(message.username);

  if (botId) {
    return {
      userId: `bot:${botId}`,
      senderKind: "bot",
      botId,
      appId,
      senderUsername,
    };
  }

  if (appId) {
    return {
      userId: `app:${appId}`,
      senderKind: "app",
      appId,
      senderUsername,
    };
  }

  if (senderUsername) {
    return {
      userId: `username:${senderUsername}`,
      senderKind: "unknown",
      senderUsername,
    };
  }

  return {
    senderKind: "unknown",
  };
}

export function isSupportedSlackMessageSubtype(value: unknown): boolean {
  const subtype = normalizeSlackField(value);
  if (!subtype) {
    return true;
  }

  return !["message_changed", "message_deleted", "channel_join", "channel_leave", "channel_topic", "channel_purpose", "channel_name", "channel_archive", "channel_unarchive", "thread_broadcast"].includes(subtype);
}

export function pickSlackFileUrl(file: Record<string, unknown>): string | undefined {
  const candidates = [file.thumb_1024, file.thumb_960, file.thumb_720, file.thumb_480, file.thumb_360, file.url_private_download, file.url_private];

  for (const candidate of candidates) {
    const normalized = normalizeSlackField(candidate);
    if (normalized) {
      return normalized;
    }
  }

  return undefined;
}

export function normalizeSlackField(value: unknown): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }

  const trimmed = value.trim();
  return trimmed ? trimmed : undefined;
}

export function conversationType(channel: { readonly is_im?: boolean; readonly is_mpim?: boolean; readonly is_group?: boolean; readonly is_channel?: boolean }): string | undefined {
  if (channel.is_im) {
    return "im";
  }
  if (channel.is_mpim) {
    return "mpim";
  }
  if (channel.is_group) {
    return "group";
  }
  if (channel.is_channel) {
    return "channel";
  }
  return undefined;
}

export function normalizeSlackNumber(value: unknown): number | undefined {
  if (typeof value === "number" && Number.isFinite(value)) {
    return value;
  }

  if (typeof value === "string") {
    const parsed = Number(value);
    if (Number.isFinite(parsed)) {
      return parsed;
    }
  }

  return undefined;
}
