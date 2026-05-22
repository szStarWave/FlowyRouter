# 配置示例

| 文件 | 说明 |
|------|------|
| [config.toml](./config.toml) | 推荐默认：Ollama 端侧 + DeepSeek 云端，`route = auto` |
| [config.edge-only.toml](./config.edge-only.toml) | 固定走端侧 `route = edge` |
| [config.minimal.toml](./config.minimal.toml) | 最小模板；聊天前须补上至少一侧 `[upstream.*]` |

复制到用户配置目录：

```bash
mkdir -p ~/.flowy-router
cp example/config.toml ~/.flowy-router/config.toml
# 编辑 base_url 等（gateway / upstream 的 api_key 均为选填）
flowy gateway restart
```

或使用 `--config` 直接指向示例文件（无需复制）：

```bash
flowy --config example/config.toml gateway start
```

完整字段说明见仓库根目录 [README.md](../README.md#配置文件说明)。
