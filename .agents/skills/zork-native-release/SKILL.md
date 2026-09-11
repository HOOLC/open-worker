---
name: zork-native-release
description: 制作或验证 zork 原生节点发布包、安装器及升级流程；区分四平台节点发布与桌面应用分发。
---

# 原生节点发布

以下路径均相对仓库根目录。先读取 `docs/native-releases.md`、`scripts/build/native-release.py`、`.github/workflows/release.yml` 和 `package.json`，核对当前版本、组件、平台与流水线，不复制历史版本号或假定公开资产已存在。

- 节点包包含 supervisor、Station、独立 Agent 工具及 GitHub helper，Synch 以库嵌入。节点包不依赖 Node/npm，也不下载额外传输 daemon。桌面 `.app` 是 `scripts/package-macos-client.py` 的另一条分发流程；本地签名校验不表示已完成公开分发签名及公证。
- 使用冻结依赖和 `cargo build --locked --release -p zork -p zork-station -p zork-agent-server -p zork-gh`，再执行 `python3 scripts/build/native-release.py stage`。stage 只从 release 产物打当前平台包，不用 debug 兜底。
- 使用 `scripts/test-native-installer.py` 验证安装器和组装；在支持的 macOS 用户服务环境执行 `scripts/test-mesh-installer.py` 验证真实安装/复用；升级相关改动使用 `scripts/test-native-upgrade.py` 和 `scripts/test-gateway-upgrade.py`，按 workflow 指定 release 测试产物。
- 本机只证明当前平台。assemble 需要四平台完整资产，核对组件、版本、manifest 和摘要；平台最低要求从当前生成配置读取。
- PR 和手动分支运行生成可检查的 workflow artifacts；匹配 package 版本的 `v*` tag 才启用发布。准备包不隐含推 tag 或公开发布授权；已有发布授权时按流水线继续。失败 draft 可诊断重试，不覆盖已发布版本。
- 安装/join 不等于升级。`zork update` 重启已暂存的二进制；`zork upgrade` 更换完整安装版本。升级可能中断任务，只有用户请求包含升级才对实际节点执行。
- 当前 Station 内嵌 Agent，文档中的旧双进程表述需与实现核对。升级失败先查看对应数据目录的 `logs/update.log`、`run/update.json` 和保存的旧二进制；新代码运行后的数据不自动回滚。

交付报告明确本机构建、四平台流水线、组装验证和公开下载各自状态。涉及 PR 时检查最新 head 的实际 CI、review blocker 与 mergeability，不能把已启动流水线当作可合并。

## 临时桌面包清理

测试结束按 `zork-validation` 的“测试包收尾”清理本任务的临时 `.app` 和 staging 副本、注销临时应用记录。正式交付物与用户明确保留的回滚包除外；长期保留用压缩归档，避免散装测试应用污染系统应用列表。

开发更新默认保留 worktree 固定槽位内的一份最新 app；只有明确导出时使用 `--output` 生成压缩包。临时包清理不得误删固定当前 app。
