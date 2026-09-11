## 渲染记录 {{index}}

检查 `REPOS_ROOT`，保留 **高亮** 与[文档链接](https://example.com/tasks/{{index}})。

1. 读取配置并验证输入。
2. 保留代码缩进、原文和文本选择。
3. 返回可复现的结果。

```rust
fn check_{{index}}() -> Result<String, String> {
    let name = "任务 {{index}}：中文与 UTF-8";
    // Each message has different source; a single hot code cache is insufficient.
    Ok(format!("{}: {}", name, {{index}}))
}
```

```json
{"task": {{index}}, "status": "ready", "labels": ["中文", "markdown"]}
```

```python
def check_{{index}}(items):
    return [item for item in items if item.get("task") == {{index}}]
```
