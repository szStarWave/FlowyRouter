# Flowy Router

端云 LLM 智能路由：**单一 `flowy` 可执行文件** — CLI 管理命令与 Gateway 守护进程合并在同一二进制中。Agent（OpenClaw、Hermes 等）将 OpenAI 兼容 `base_url` 指向 Gateway 即可。

产品说明见 [prd.md](./prd.md)。

## 架构

```
┌─────────────────────────────────────────────────────────────┐
│  flowy（单一二进制）                                          │
│    flowy gateway start  →  后台 re-exec 自身为 HTTP 守护进程   │
│    flowy env / stats / gateway status  →  CLI 管理           │
└───────────────────────────────┬─────────────────────────────┘
                                │
                    OpenClaw / Hermes ────────┘
                    POST /v1/chat/completions
```

---

## 1. 安装

需要 [Rust](https://rustup.rs/)（`cargo` 可用）。

```bash
git clone <your-repo-url> flowy-router
cd flowy-router
cargo build --release
```

编译产物：

| 二进制 | 路径 |
|--------|------|
| flowy | `target/release/flowy` |

**加入 PATH（推荐）**

```bash
# Linux / macOS
export PATH="$PWD/target/release:$PATH"

# 或复制到已有目录
cp target/release/flowy ~/.local/bin/
```

**Windows（PowerShell）**

```powershell
$env:Path += ";$PWD\target\release"
```

未安装到 PATH 时，可用 `cargo run`（开发调试）：

```bash
cargo run -- gateway start
cargo run -- gateway run
```

---

## 2. 配置文件（TOML）

所有业务配置写在 **TOML** 文件中，不使用 `FLOWY_*` 环境变量。端/云/级联路由**仅由 `config.toml` + 请求体**决定，**禁止**用 `X-Flowy-*` 等 chat 请求头参与判断。

| 系统 | 配置路径 |
|------|----------|
| Linux / macOS | `~/.flowy-router/config.toml` |
| Windows | `%USERPROFILE%\.flowy-router\config.toml` |

```
~/.flowy-router/
  config.toml      # 主配置（Gateway + 上游 + CLI）
  gateway.pid      # 守护进程 PID（运行时）
  stats.json       # 路由/流量累计统计（持久化）
  experience.json  # 按 step_kind 的隐式路由经验
  sessions/        # 每会话状态（含 cloud_sticky）
  logs/gateway.log # Gateway 日志（追加）
```

**示例文件**（仓库内，可复制或 `--config` 引用）：

| 文件 | 用途 |
|------|------|
| [example/config.toml](./example/config.toml) | 推荐：Ollama + DeepSeek，`route = auto` |
| [example/config.edge-only.toml](./example/config.edge-only.toml) | 固定端侧 |
| [example/config.minimal.toml](./example/config.minimal.toml) | 最小模板（须至少配一侧上游才能聊天） |

```bash
# 方式 A：复制为默认配置
mkdir -p ~/.flowy-router
cp example/config.toml ~/.flowy-router/config.toml
# 编辑 upstream 的 base_url 等（api_key 均为选填）

# 方式 B：指定路径启动（不复制）
flowy --config example/config.toml gateway start
```

首次 `flowy gateway start` 时若 `config.toml` 不存在，会自动写入默认模板（同 `example/config.minimal.toml` 结构）。

日志级别（仅此一项可用环境变量）：`RUST_LOG=flowy_router=debug flowy gateway run`

---

## 3. 使用流程（从零到 Agent 接入）

### 3.1 启动 Gateway（自动初始化配置）

```bash
flowy gateway start
```

首次启动会看到类似提示：

```text
Created config at /home/you/.flowy-router/config.toml — edit upstream sections, then restart if needed.
gateway started (pid 12345, listen 127.0.0.1:8080, profile balanced)
```

### 3.2 编辑配置

以 [example/config.toml](./example/config.toml) 为模板，至少配置一侧上游的 `base_url`（`api_key` 均为选填，按上游服务需要填写）。改完后：

```bash
flowy gateway restart
flowy env    # 核对 route、上游是否 configured
```

**上游可用性**：`[upstream.edge]` 与 `[upstream.cloud]` 至少配置一侧，否则 `POST /v1/chat/completions` 返回 **503**。只配端侧时全部走端侧；只配云端时全部走云端；两侧都配时按 `route` / 难度 / 级联策略路由。

### 3.3 查看状态

```bash
flowy gateway status
```

```text
Flowy Gateway
  Status:   running
  Version:  0.1.0
  PID:      12345
  Listen:   127.0.0.1:8080
  Uptime:   42s
  Edge:     configured
  Cloud:    configured
  Profile:  balanced
  ...
```

**开发时前台运行**（日志直接打在终端，不写 pid 文件）：

```bash
flowy gateway run
# 等价于: flowy --config ~/.flowy-router/config.toml __serve --foreground
```

**停止 / 重启**

```bash
flowy gateway stop
flowy gateway stop --force    # 进程无响应时强制结束
flowy gateway restart
```

### 3.4 用 curl 验证路由（与 Agent 相同协议）

```bash
curl -s http://127.0.0.1:8080/health

curl -s http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "flowy-auto",
    "messages": [
      {"role": "user", "content": "[OpenClaw heartbeat poll]"}
    ]
  }' | jq '.flowy_meta'
```

**流式**（`stream: true`，SSE）：

```bash
curl -sN http://127.0.0.1:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "flowy-auto",
    "stream": true,
    "messages": [{"role": "user", "content": "hello"}]
  }'
# 响应头含 X-Flowy-Route、X-Flowy-Step-Kind 等；body 为 data: {...}\n\n 与 data: [DONE]\n\n
```

响应中的 **`flowy_meta`**（非流式 JSON）或 **`X-Flowy-*` 响应头**（流式）含 `route`、`difficulty_score`、`step_kind`、`reason_codes` 等路由信息。

若已配置 `gateway.api_key`，curl 需增加：`-H "Authorization: Bearer <your-key>"`。

路由策略见下文 [§7 配置文件说明](#7-配置文件说明)。

> **路由不读取** `POST /v1/chat/completions` 上的 `X-Flowy-*` 请求头；步态与难度仅由 `messages[]`、`tools[]` 与 `config.toml` 推断。响应仍可带 `X-Flowy-Route` 等（只读）。

> 支持 `stream=true`（SSE）与 `stream=false`（JSON）。**所有 `api_key` 均为选填**：未配置 `gateway.api_key` 时不校验入站请求；配置后客户端须带 `Authorization: Bearer <key>` 或 `x-api-key: <key>`。若配置了 `[upstream.*].api_key`，Gateway 转发时附带 `Authorization: Bearer`。

### 3.5 接入 OpenClaw

编辑 `~/.openclaw/openclaw.json`（路径以 OpenClaw 文档为准），将 provider 指向 Flowy：

```json
{
  "models": {
    "providers": {
      "flowy": {
        "baseUrl": "http://127.0.0.1:8080/v1",
        "apiKey": "",
        "models": [
          { "id": "flowy-auto", "name": "Flowy Auto Route" }
        ]
      }
    }
  }
}
```

- `baseUrl` 须与 `config.toml` 中 `gateway.listen` 一致（默认 `http://127.0.0.1:8080/v1`）。
- `apiKey` 选填：仅当配置了 `gateway.api_key` 时须与之一致；未配置时可留空或任意占位。
- 在 OpenClaw 里选用 `flowy-auto`（或你配置的 model id）。

### 3.6 接入 Hermes

```bash
# hermes setup model → Custom OpenAI-compatible endpoint
# base_url: http://127.0.0.1:8080/v1
# api_key:  选填；仅当 gateway.api_key 已配置时需一致
```

Hermes/OpenClaw 无需也不应通过请求头影响路由。

---

## 4. 指定其它配置文件

调试或多环境时，CLI 与 Gateway 守护进程必须使用 **同一份** `config.toml`（`flowy gateway start` 会以 re-exec 方式启动同一 `flowy` 二进制）：

```bash
flowy --config /path/to/dev.toml gateway start
flowy --config /path/to/dev.toml gateway status
```

---

## 5. CLI 命令速查

| 命令 | 说明 |
|------|------|
| `flowy gateway start [--wait N]` | 后台启动；首次会创建配置目录与 `config.toml` |
| `flowy gateway stop [--force]` | 停止 |
| `flowy gateway status [--json]` | 运行状态 |
| `flowy gateway restart [--wait N]` | 重启 |
| `flowy gateway run` | 前台运行（开发） |
| `flowy env [--json]` | 打印路径、解析后的配置与运行时环境变量 |
| `flowy stats [--json]` | **当前 gateway 进程**的路由与流量统计（本次启动以来） |
| `flowy stats --global [--json]` | **全部历史**累计统计（`stats.json`，跨重启；gateway 未运行时也可读盘） |

全局参数：`--config <path>`。调试 HTTP 可用 `curl`（见 §3.4）。

查看路径与配置（不启动 Gateway、不创建 `config.toml`）：

```bash
flowy env
flowy env --json
flowy stats              # 当前运行会话
flowy stats --global     # 全部历史（含以往 gateway 运行）
flowy stats --json
flowy stats --global --json
```

`flowy env` 示例输出：

```text
Paths
  user_home:      /home/you
  app_dir:        /home/you/.flowy-router
  config_file:    /home/you/.flowy-router/config.toml (exists)
  pid_file:       /home/you/.flowy-router/gateway.pid
  sessions_dir:   /home/you/.flowy-router/sessions
  gateway_pid:    12345
  gateway_bin:    /home/you/flowy-router/target/release/flowy

Config (from config.toml or defaults if missing)
  gateway_listen:       127.0.0.1:8080
  gateway_url:          http://127.0.0.1:8080
  ...
Runtime environment
  RUST_LOG:       (not set)
```

---

## 6. HTTP 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/health` | 存活与上游是否已配置 |
| `GET` | `/v1/admin/status` | 守护进程详情（同 `flowy gateway status` 数据源） |
| `GET` | `/v1/admin/stats` | 当前会话统计（`scope=session`，默认）；`?scope=global` 为全部历史（同 `flowy stats --global`） |
| `POST` | `/v1/admin/shutdown` | 优雅关闭；若配置了 `gateway.admin_token`，需 Header `X-Flowy-Admin-Token` |
| `POST` | `/v1/chat/completions` | OpenAI 兼容聊天（Agent 主入口） |

---

## 7. 配置文件说明

完整示例与注释见 [example/config.toml](./example/config.toml)。

### 7.1 `[gateway]`

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `listen` | string | `127.0.0.1:8080` | Gateway 监听地址；Agent 的 `baseUrl` = `http://{listen}/v1` |
| `route` | string | `auto` | 见下表 |
| `routing_mode` | string | `cascade` | 仅 `route=auto` 时生效，见下表 |
| `default_profile` | string | `balanced` | `economy` / `balanced` / `premium` / `privacy` |
| `ctx_edge_max_tokens` | u32 | `65536` | 端侧上下文 token 上限估算 |
| `api_key` | string? | — | **选填**；配置后入站 `POST /v1/chat/completions` 须带相同密钥 |
| `admin_token` | string? | — | **选填**；配置后 `POST /v1/admin/shutdown` 需头 `X-Flowy-Admin-Token` |
| `experience_enabled` | bool | `true` | 按 `step_kind` 隐式学习，微调难度分 |
| `experience_learning_rate` | float | `0.08` | 经验偏置学习强度 |
| `experience_max_bias` | float | `0.12` | 单步态难度偏置上限 |
| `experience_target_fallback` | float | `0.15` | 级联升云「目标」比例 |
| `cloud_sticky_ttl_secs` | u64 | `600` | 升云后会话粘性 TTL（秒） |
| `session_persist_enabled` | bool | `true` | 会话状态写入 `sessions/` |

#### `gateway.route`（总开关）

| 值 | 行为 |
|----|------|
| `auto` | 按请求难度 + `default_profile` + `routing_mode` 选择 edge / cloud / cascade |
| `edge` | 每个请求走端侧；端未配置或不可用时降级 cloud |
| `cloud` | 每个请求走云端 |
| `cascade` | 每个请求先端侧，质量不过关再升云 |

#### `gateway.routing_mode`（仅 `route = auto`）

结合 `balanced` profile 的阈值（约 θ_edge=0.35、θ_cloud=0.55）：

| 值 | 难度低 | 难度中 | 难度高 |
|----|--------|--------|--------|
| `single` | edge | cloud | cloud |
| `cascade` | edge | **先 edge，可能升 cloud** | cloud |
| `split` | cloud | cloud | cloud |

> `route = cascade` 与 `routing_mode = cascade` 不同：前者强制每请求级联；后者只在 auto 模式下对「中等难度」启用级联。

#### `default_profile`（仅 `route = auto`）

影响难度阈值，越「省钱」越倾向 edge：

| Profile | θ_edge | θ_cloud | 说明 |
|---------|--------|---------|------|
| `economy` | 0.40 | 0.60 | 更多走端 |
| `balanced` | 0.35 | 0.55 | 默认 |
| `premium` | 0.25 | 0.45 | 更多走云 |
| `privacy` | — | — | 尽量 edge（恢复失败等除外） |

### 7.2 `[upstream.edge]` / `[upstream.cloud]`

| 字段 | 说明 |
|------|------|
| `base_url` | OpenAI 兼容 API 根路径，**须含 `/v1`**，如 `http://127.0.0.1:11434/v1` |
| `api_key` | **选填**；配置后 Gateway 转发该上游时附带 `Authorization: Bearer` |

至少配置一侧才能处理聊天请求。只配 `[upstream.edge]` 时，无论路由策略如何，请求均转发到端侧；只配 `[upstream.cloud]` 时全部走云端；两侧都省略则聊天接口报错（`GET /health` 仍可用）。

### 7.3 `[cli]`

| 字段 | 说明 |
|------|------|
| `gateway_url` | CLI 访问 Gateway 的 URL，默认 `http://{gateway.listen}` |

### 7.4 常用组合

```toml
# 智能路由 + 级联（默认推荐，OpenClaw）
route = "auto"
routing_mode = "cascade"
default_profile = "balanced"

# 全部本地 Ollama
route = "edge"

# 全部 DeepSeek 云端
route = "cloud"

# 寒暄也想走端：固定端或提高 edge 阈值
route = "edge"
# 或 route = "auto" + default_profile = "economy" + routing_mode = "single"
```

### 7.5 自定义配置路径

```bash
flowy --config /path/to/config.toml gateway start
```

CLI 与 Gateway 守护进程使用**同一份**配置文件（同一 `flowy` 二进制）。

---

## 8. 常见问题

**`flowy` not found**

- 先 `cargo build --release`，或将 `target/release` 加入 PATH。

**`gateway did not become healthy within 30s`**

- 检查端口是否被占用；`flowy gateway run` 前台看日志。
- 确认 `gateway.listen` 与 `cli.gateway_url` 一致。

**Agent 报错或没有真实模型回复**

- 确认 `[upstream.edge]` / `[upstream.cloud]` 已配置且上游服务可达。
- 未配置任何上游时聊天接口返回 503，请至少填写 `[upstream.edge]` 或 `[upstream.cloud]` 的 `base_url`。

**需要停止但 `flowy gateway stop` 无效**

```bash
flowy gateway stop --force
```

---

## 9. 开发与测试

```bash
# 单元 / 集成测试
cargo test

# 只测路由
cargo test routing
```

项目结构：

```
example/      # 配置文件示例（见 example/README.md）
src/
  config/     # ~/.flowy-router 路径 + config.toml 读写
  gateway/    # 守护进程（路由、API、上游转发）
  main.rs     # CLI + 隐藏 __serve 子命令（daemon 入口）
tests/        # CLI 集成测试
```
