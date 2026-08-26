import { TimelineEvent } from "./timeline-display";

export type UiState = {
  readonly adminView: string;
  readonly sessionFilter: string;
  readonly selectedSessionKey: string | null;
};

export type SessionRecord = Record<string, any>;

export type TimelinePayload =
  | {
      readonly events?: TimelineEvent[];
      readonly trace?: Record<string, any>;
      readonly session?: SessionRecord;
      readonly page?: {
        readonly limit?: number;
        readonly hasMore?: boolean;
        readonly nextBeforeSequence?: number | null;
      };
    }
  | TimelineEvent[];

export function timelinePayloadSession(payload: TimelinePayload | null): SessionRecord | null {
  return payload && !Array.isArray(payload) && payload.session ? payload.session : null;
}

export function mergeSessionRecords(base: SessionRecord | null | undefined, detail: SessionRecord | null | undefined): SessionRecord | null {
  if (!base) return detail || null;
  if (!detail) return base;
  return {
    ...detail,
    ...base,
    backgroundJobs: Array.isArray(detail.backgroundJobs) ? detail.backgroundJobs : base.backgroundJobs,
    failedBackgroundJobs: Array.isArray(detail.failedBackgroundJobs) ? detail.failedBackgroundJobs : base.failedBackgroundJobs,
    workspacePath: detail.workspacePath ?? base.workspacePath,
    id: detail.id ?? base.id,
    sessionPageLinkPostedAt: detail.sessionPageLinkPostedAt ?? base.sessionPageLinkPostedAt,
    lastObservedMessageTs: detail.lastObservedMessageTs ?? base.lastObservedMessageTs,
    lastDeliveredMessageTs: detail.lastDeliveredMessageTs ?? base.lastDeliveredMessageTs,
  };
}

export const sessionFilters = ["all", "jobs", "issues"];

export const TIMELINE_PAGE_SIZE = 30;

export const TIMELINE_AUTO_LOAD_THRESHOLD = 32;
