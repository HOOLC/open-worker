---
name: zork-mesh-diagnostics
description: 排查 zork 的设备接入、Mesh 连接、远程任务投递和后台节点生命周期问题，或验证这些模块的改动。
---

# Mesh 诊断

以下路径均相对仓库根目录。按症状读取 `docs/mesh-onboarding.md`、`docs/desktop-node.md` 或 `docs/local-mesh.md`；具体协议行为以当前代码和测试核对。

涉及客户端实现修复或审查时，应用 [zork-client-boundary](../zork-client-boundary/SKILL.md)：邀请、成员同步、授权判断、业务网络请求与重连生命周期由 Rust core 维护，不通过 UI 回调或定时器补偿。只读设备诊断不要求执行客户端整改。

1. 先确定访问客户端、执行节点和节点数据目录。读取当前配置与服务状态，端口和服务 URL 不从示例或其他设备复制。存在多个候选时使用明确的 `--data`，不任选节点。
2. 区分连接失败、邀请兑换/成员权限、任务执行失败和系统服务问题。使用现有 `zork mesh status`、`zork service status` 及相关日志/API 读取状态；诊断不需要重新安装、加入、撤权或停止节点。
3. 邀请由 Station 维护，安装器复用兼容节点。重复 join 不应重置身份、Profile、任务或工作目录。邀请和管理凭据不写入报告。成员目录维护节点离线可能阻止成员变更，但不等于已有节点间执行必然停止。
4. 区分独立后台 Station、客户端拥有的节点、系统服务接管的节点。`service uninstall` 撤销看护但不等于停止进程；`zork stop` 会停止节点。当前 supervisor 管理 Station，Agent 嵌入其中。只有已授权的操作范围包含重启/配置变更时才执行这些动作。
5. 客户端切换不迁移执行设备或工作目录。断线时检查缓存、outbox、请求 ID 和最后确认状态；丢失回执不能视为未送达，也不能改用新请求 ID 重发。停止操作需确认实际任务结束状态。
6. relay/discovery 从节点或客户端实际服务配置读取。它们与账号控制面不同；不要拿 dashboard 域名充当 relay。客户端服务覆盖不会自动改写已运行 Station。

## 对应验证

修改相关逻辑后先构建所需产物，再选匹配脚本，读取脚本确认 fixture 和操作范围：

- 邀请、成员同步、撤权：`scripts/test-mesh-enrollment.py`。
- 安装与实例复用：`scripts/test-native-installer.py`、`scripts/test-mesh-installer.py`；后者使用真实用户服务管理器。
- 客户端权限、outbox 和文件：`scripts/test-client-mesh.py`；远端 Worker 重启、回执、返工与撤权：`scripts/test-remote-workers.py`。
- relay/discovery 实际流量：`scripts/test-local-relay.py`。允许直连升级的测试不能证明全部流量强制经 relay，也不能代替物理设备跨网验收。
- 原生 UI 接入与后台接管：`scripts/test-mesh-experience-ui.py`；需要可用原生桌面。

输出故障所属层、证据和验证结果。源码 fixture 成功不表示邀请指向的公开 Release 已存在；需要公开安装时单独检查对应版本资产。
