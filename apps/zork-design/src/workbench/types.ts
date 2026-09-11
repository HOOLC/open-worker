export type Rect = { x: number; y: number; width: number; height: number };
export type ElementInfo = {
  id: string;
  label: string;
  role: string;
  visible: boolean;
  enabled: boolean;
  bounds: Rect;
  visible_bounds: Rect;
  center: { x: number; y: number };
};
export type Snapshot = { revision: number; elements: ElementInfo[]; viewport: { width: number; height: number } };
export type WasmState = {
  id: string;
  pending_actions?: number;
  action_error?: string | null;
  states?: Array<{ id: string; text?: string; clicks?: number; selected?: number; quote?: string | null }>;
  [key: string]: unknown;
};
export type WasmApi = {
  snapshot(): string;
  story_state(): string;
  catalog(): string;
  select_story(id: string): void;
  select_family(family: string): void;
  action(action: string): void;
};
export interface Story {
  id: string;
  family: string;
  title: string;
  state: string;
  source: string;
  reference: string;
  width: number;
  height: number;
  target: string;
  actions: unknown[];
  native: { image: string; window: string; geometry: string; bounds: Rect };
  design: { status: string; source?: string; image?: string; window?: string; bounds?: Rect; reason?: string };
  checks?: string[];
}
export interface PageFixture {
  device: { id: string; name: string; sidebar_width: number };
  profile: { profile_id: string; provider: string; billing: string; models: Array<Record<string, unknown>> };
  agents: Array<{
    id: string;
    prototype_id: string;
    name: string;
    role: string;
    avatar: string;
    profile_id: string;
    model: string;
    allowed_leaders: string[];
  }>;
  model_form: {
    id: string;
    api: string;
    context_window: number;
    max_output_tokens: number;
    thinking: string;
    default_thinking: string;
  };
  conversation: {
    id: string;
    placeholder: string;
    messages: Array<{ id: string; role: string; who: string; content: string; time: string; created_at: string }>;
  };
  history: { now: number; records: Array<{ event_id: string; event: Record<string, unknown> }> };
  account: { name: string; email: string; device_name: string; identity: string };
  mesh: {
    peers: Array<{ id: string; name: string; online: boolean; can_manage: boolean }>;
    invitation: string;
    expires: string;
  };
  fonts: { latin: string; cjk: string; weights: number[] };
}
export interface Catalog {
  stories: Story[];
  fixture: PageFixture;
  providers: Array<{ id: string; label: string; billing: Array<{ id: string; label: string }> }>;
  buildId: string;
  tokens: { colors: Record<string, string>; sizes: Record<string, number> };
}
export type Mode = "live" | "side" | "overlay" | "diff";
export type GpuiWindow = Window & { zorkStory?: WasmApi };
