import type { Story } from "./types";
export const pageFamilies = new Set([
  "connection",
  "model",
  "agent",
  "conversation",
  "client",
  "device",
  "mesh",
  "enrollment",
]);
export const pageTitles: Record<string, string> = {
  connection: "大模型",
  model: "模型管理",
  agent: "队员",
  conversation: "对话页面",
  client: "客户端设置",
  device: "设备设置",
  mesh: "设备连接",
  enrollment: "添加设备",
};
export const sceneTitles: Record<string, string> = {
  list: "列表",
  create: "添加",
  provider: "供应商选择",
  detail: "详情",
  protocol: "协议选择",
  edit: "编辑",
  dropdown: "模型选择",
  messages: "消息",
  composer: "输入与评论",
  history: "历史记录",
  "signed-out": "未登录",
  "signed-in": "已登录",
  loading: "加载中",
  error: "错误",
  running: "运行中",
  stopped: "已停止",
  connected: "已连接",
  empty: "空状态",
  manual: "手动连接",
  start: "开始",
  command: "加入命令",
  expired: "已过期",
};
export const sceneOf = (story: Story) => story.state.replace(/-(compact|wide)$/, "");
export function routeFor(stories: Story[], pathname: string) {
  const [, familyPath, scene, size] = pathname.replace(/^\/pc\/?/, "/").split("/");
  const legacy = stories.find((s) => s.id === familyPath);
  const family = stories.some((s) => s.family === familyPath) ? familyPath : (legacy?.family ?? "button");
  const items = stories.filter((s) => s.family === family);
  const selected = items.find((s) => sceneOf(s) === scene && s.state.endsWith("-" + size)) ?? legacy ?? items[0];
  return { family, items, selected, basic: !pageFamilies.has(family) };
}
export const stateRoute = (family: string, scene: string, size: string) => `/pc/${family}/${scene}/${size}`;
