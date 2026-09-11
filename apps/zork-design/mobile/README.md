# Zork 移动端设计

[移动端可视入口](index.html) · [返回总手册](../index.html) · [桌面与移动差异](../docs/10-mobile.md)

此目录汇集移动端 App 的设计规范、视觉概念、HTML 交互原型和验证记录。归档日期：2026-09-06。

**当前设计以 [移动端设计规范](design-spec.md) 和 [HTML 原型](prototype/index.html) 为准。** 最初三屏图片用于记录概念，后续用户反馈已修改导航选中状态、按压反馈、底部入口和返回行为，不能再把初稿图片当作最终交互规范。

| 成果 | 入口 | 状态 |
| --- | --- | --- |
| 移动端设计规范 | [design-spec.md](design-spec.md) | 按本任务最新用户反馈整理 |
| HTML / CSS / JavaScript 原型 | [prototype/index.html](prototype/index.html) | nav7；页面内存中的本地交互 |
| 原型说明与修订记录 | [prototype/README.md](prototype/README.md) | 包含 nav4–nav7 反馈修订 |
| 初版三屏概念 | [zork-mobile-v1.png](zork-mobile-v1.png) | 历史概念，非最终交互真值 |
| 初版概念说明 | [concept-v1.md](concept-v1.md) | 保留原始简报与生成提示概要 |
| SVG 标识、头像、图标、字体 | [prototype/assets/](prototype/assets/) | 原型所需的自包含资源快照 |
| 验证记录 | [prototype/design-qa.md](prototype/design-qa.md) | DOM 交互检查通过；浏览器视觉复核未完成 |
| 统一素材映射 | [asset-map.json](asset-map.json) | 自包含副本与共享源文件逐项校验 |
| 移动端 tokens | [../tokens/mobile-tokens.json](../tokens/mobile-tokens.json) | 从 nav7 源文件提取 |
| 机器可读清单 | [manifest.json](manifest.json) | 入口、来源与验证边界 |

可使用下方命令启动本地原型。

独立启动（从仓库根目录，先选择空闲端口）：

```sh
python3 -m http.server 49236 --bind 0.0.0.0 --directory apps/zork-design/mobile/prototype
```


资源来自已有 Zork 品牌素材；Inter 字体许可证随文件保存。原型中的 SVG 副本用于独立预览，不取代统一品牌素材的源文件。
