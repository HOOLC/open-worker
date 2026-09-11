# 原生接入记录

当前设计已接入 mini1 主代码目录 `.`。原生窗口使用隔离测试数据验证；MBA 的正式安装已通过更新脚本替换为 0.1.30，新界面已启动，本机节点就绪。


本轮完成的是新版主工作流。历史记录页仍沿用旧设计；旧版全局收件箱、任务看板、独立 Drive 和人工任务操作仍未统一到新版入口。详见 [迁移范围与遗留项](migration-status.md)。

## 实际挂载位置

| 动效 | 原生位置 |
| --- | --- |
| 图标 | 选择第一位 Leader 的对话引导 |
| Zork 字标 | 设置侧栏页脚 |
| 联动 | 主会话侧栏品牌位 |
| 圆润弹开与跳落、下半蠕动成 Z、展开 ork | 首位 Leader 创建引导 |

52 个功能图标已嵌入。大幅场景插画已移除，设备、文件、收件箱和任务状态直接复用 20–32 px 功能 SVG；对话首页不再叠加重复插图。详情见 [资源挂载清单](native/usage-v2.json)。不为了覆盖素材数量而制造不存在的界面或工作状态。

实际界面代码中的旧 `cue/`、`phosphor` 图标路径已迁移到 `icons/<语义名称>.svg`。历史源文件保留来源；真实外部服务身份与 Zork 产品品牌分开。

## 验证

默认安装检查已切换为 GPUI 无窗口渲染测试：固定种子与虚拟时钟、两次一致回放、每帧绘制行数与 120 Hz CPU 预算。基准使用新版桌面入口。安装结果记录见 mini1 的 `artifacts/mba-install/2026-09-06-modal-selection-fix/verification.json`。

- 68 项 GUI 单元/集成测试通过，2 项既有外部 fixture 忽略；本轮去除粘连后，5 项动效定向测试另行复验通过。
- 1280×800、900×600 完整窗口流程通过：导航、评论、模型绑定、附件、离线投递、重连和重启恢复。
- 四种动效的首/早/中/晚/末帧验证通过（变形增加收拢帧）；前三种回到静止起点，变形到达字标终点。
- 减少动态效果使用 macOS NSWorkspace 偏好；隔离调试开关验证静态端点，未修改系统偏好。
- 主目录 Agent / Gateway / GUI 编译检查与隔离 HTTP 业务回归通过。

[正常动效帧哈希](native/brand-motion/frames.json) · [减少动态帧哈希](native/brand-reduced/frames.json)

## 表格与弹窗修复

MBA 已于 2026-09-06 19:14 更新：模型连接、模型编辑和 Agent 创建/编辑使用真实模态弹窗。表格点击越界已修复，同一 GPUI 单击测试恢复旧代码会复现崩溃，修复版通过；同时覆盖跨列、反向中文拖选。较慢的本机节点冷启动现在允许等待 60 秒，安装检查继续验证就绪、失败回滚。

## 实际窗口截图

[宽窗口会话与评论](native/conversation-comments-1280x800.png) · [较窄窗口](native/conversation-comments-900x600.png) · [文件预览](native/conversation-file-preview-900x600.png)

[变形起点](native/brand-motion/brand-morph-start.png) · [变形中段](native/brand-motion/brand-morph-mid.png) · [变形终点](native/brand-motion/brand-morph-end.png)

客户端上传与完整安装自升级仍没有对应实现，界面明确提示未支持。这些限制与图形资源替换分开记录。
