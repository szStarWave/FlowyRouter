# Flowy Router — 端云 AI 模型智能路由

**版本**：v0.5  
**状态**：草案  
**最后更新**：2026-05-22

---

## 1. 产品概述

### 1.1 背景

**自主 Agent**（如 [OpenClaw](https://github.com/openclaw/openclaw)、[Hermes Agent](https://github.com/NousResearch/hermes-agent)）在一次用户意图下往往会触发 **多轮 LLM 推理**：ReAct 循环中的「是否调用工具 / 调用哪个工具 / 如何解析工具结果 / 最终回复」每一步都是独立的 Chat Completions 请求。Agent 还会在配置中挂载 **超长 System Prompt**（SOUL.md、Skills、MCP 工具 schema）、**持久化会话历史** 与 **子 Agent 委派**，导致每次调用的 **输入 Token 体量极大**。

**成本结构（实测，FlowyAIPC / OpenClaw）**：账单与流量几乎全部由 **输入 Token** 构成。当前 **单次 LLM 请求** 统计：

| 指标 | 均值 |
|------|------|
| 输入 Token \(\bar{T}_{in}\) | **52,830.23** |
| 输出 Token \(\bar{T}_{out}\) | **356.59** |
| 合计 | 53,186.83 |
| 输入占比 \(\bar{T}_{in}/(\bar{T}_{in}+\bar{T}_{out})\) | **99.33%**（输出约 **0.67%**） |

因此 Flowy 的首要优化目标不是「少生成几个字」，而是 **少把约 5.3 万 token/步 的巨型 prompt 送进云端计价模型**；端云路由、静态 system 缓存、升云时 prompt 压缩，都应围绕 **\(T_{in}\)** 设计。

端侧/本地可部署 **小参数或 MoE 稀疏激活模型**（如 Qwen3.6-35B-A3B，激活约 3B 参数），适合承担 Agent 循环中的「轻推理」步骤；云端大模型（如 DeepSeek-V4-Pro）保留给 **规划、复杂工具链编排、长文档理解、高难度代码** 等步骤。

**Flowy Router** 是面向上述 Agent 运行时的 **外部模型转发服务（Model Gateway）**：Agent 将 `base_url` 指向 Flowy，由 Router 对 **每一次** `POST /v1/chat/completions` 做端/云选择与质量兜底，在 Agent 行为不变的前提下降低云端 Token 成本。

### 1.2 产品定位

| 维度 | 说明 |
|------|------|
| **是什么** | Agent 专用的 **OpenAI 兼容模型代理**：端云统一接入 + 按推理请求粒度的路由决策 + 成本/质量可观测 |
| **主要服务对象** | **OpenClaw**、**Hermes** 及同类「自带 Gateway + Agentic Loop + 可配置 OpenAI-compatible endpoint」的 Agent 运行时 |
| **不是什么** | 不是 Agent 本体（不负责工具执行、记忆、消息渠道）；不是面向 C 端聊天 App 的独立产品 |
| **核心价值** | 在可控质量下 **减少云端输入 Token 次数与体量**（输出占比极小，省输入即省钱） |

### 1.3 典型集成：OpenClaw / Hermes

两类产品的共同模式：**本地 Gateway 进程** 组装上下文 → 调用配置的 **LLM Provider（OpenAI 兼容 HTTP）** → 解析 `tool_calls` → 执行工具 → 再次调用 LLM，直至产出回复。

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────────┐
│ OpenClaw /      │     │  Flowy Router    │     │ Edge Runtime        │
│ Hermes Gateway  │────►│  (base_url 替换)  │────►│ Qwen3.6-35B-A3B     │
│ Agentic Loop    │     │  逐请求路由       │     │ 或                  │
└─────────────────┘     └────────┬─────────┘     │ Cloud Adapter       │
                                 │               │ DeepSeek-V4-Pro     │
                                 └──────────────►└─────────────────────┘
```

| Agent | 配置入口（示例） | Flowy 接入方式 |
|-------|------------------|----------------|
| **OpenClaw** | `~/.openclaw/openclaw.json` 中 models / provider | 将 provider `baseUrl` 设为 `http://<flowy-host>/v1`；`apiKey` 选填（仅当 `gateway.api_key` 已配置时需一致）；保留原 `model` 字段或映射为 Flowy 逻辑名 |
| **Hermes** | `hermes setup model` → Custom OpenAI-compatible endpoint | `base_url` = Flowy；`provider` = `custom`；路由由 `messages[]` 启发式推断，**不用**请求头 |

**Router 与 Agent 的边界**

| 职责 | Agent（OpenClaw / Hermes） | Flowy Router |
|------|---------------------------|--------------|
| 工具执行、浏览器、文件系统 | ✅ | ❌ |
| 会话记忆、Compaction、Skills 加载 | ✅ | ❌（仅读取请求中已组装的 messages） |
| 单次 LLM 调用的端/云选择 | ❌ | ✅ |
| 端侧低质量时的 Fallback / Cascade | ❌ | ✅ |
| 按会话/Agent 实例的成本统计 | 可选 | ✅ |

### 1.4 目标用户

- **Agent 部署者 / 自托管用户**：在 OpenClaw、Hermes 配置中把模型 endpoint 改为 Flowy，降低 API 账单  
- **平台运维**：为多租户配置 Profile、预算熔断、强制云端的关键工具白名单  
- **Agent 框架维护者**：路由不依赖 `X-Flowy-*` 请求头，仅依赖 `messages[]` / `tools[]` 正文与配置  

### 1.5 成功指标（North Star）

| 指标 | 目标（示例，上线后按基线校准） |
|------|-------------------------------|
| **云端输入 Token** 下降 | 相对直连云端基线降低 **40%–70%**（主指标；按会话/Agent 分层） |
| 云端输出 Token 下降 | 参考指标（通常 <5% 总节省，因输出占比 ~0.67%） |
| 单次请求 \(\bar{T}_{in}\)、\(\bar{T}_{out}\) | 相对基线（52,830 / 357）漂移监控；突增可能为 history 膨胀或 cache 失效 |
| 输入占比 \(T_{in}/(T_{in}+T_{out})\) | 监控是否仍 ≈ **99.33%**；异常下降可能意味统计口径或输出膨胀 |
| Agent 任务完成率 | 相对全云端基线 **≥ 98%**（同等工具集与超时配置） |
| 用户满意度 | CSAT / 点赞率不低于全云端基线的 **95%** |
| P95 延迟 | 简单任务 P95 延迟优于纯云端 **30%+**（端侧命中时） |
| 路由误判率 | 需云端却走端侧且导致明显质量问题的比例 **< 2%** |
| 单次会话成本 | 平均 $/session 或 ¥/1k requests 可观测且可配置上限 |

---

## 2. 问题与机会

### 2.1 现状痛点

1. **Agent 每步推理都走云端**：单轮用户消息可能触发 5–20 次 LLM 调用；每一步都把 **完整 system + 全历史** 计入云端 **输入** 计费（输出仅几十～几百 token）。  
2. **输入重复上传是主因**：SOUL.md、Skills、MCP schema、tool 结果在 **每一步** 重发；在 \(T_{in}\) 占比 99%+ 时，重复 10 次 ≈ 10 倍账单。  
3. **手工规则难维护**：Agent 内写「长度 > N 换模型」无法感知 **当前是规划步还是总结步**、是否子 Agent、是否含敏感工具。  
4. **Ollama/本地与云端割裂**：Agent 虽支持 custom endpoint，但缺少 **按步质量兜底、统一观测、会话级升级策略**。

### 2.2 机会

- **MoE 端侧模型**（如 Qwen3.6-35B-A3B）在端上算力可承受范围内，覆盖大量 **短上下文、结构化、模板化** 任务。  
- **云端旗舰模型**（DeepSeek-V4-Pro）保留给 **高难推理、创意、复杂代码、多文档 RAG** 等场景。  
- **端侧承接 loop 步**：每多一步走端侧，即 **整步 \(T_{in}\) 不再计入云端单价**（边际节省 ≈ 该步全量输入，而非输出）。  
- **级联升云时压缩输入**：升云仅送摘要 + 必要片段，避免把 OpenClaw 全历史再次全额送入云端。

### 2.3 成本结构：输入主导（设计约束）

**基线（单次 `chat/completions`，FlowyAIPC / OpenClaw 实测）**

```
T̄_in  = 52,830.23 tokens/request
T̄_out =    356.59 tokens/request
T̄_sum = 53,186.83 tokens/request
α     = T̄_in / T̄_sum = 99.33%   （输出占 0.67%）
```

| 事实 | 对 Router 的含义 |
|------|------------------|
| 每步平均 **~52.8k 输入、~357 输出**（\(\alpha=99.33\%\)） | 路由收益 ≈ **每步避免 ~52,830 云端输入 Token**；压 `max_tokens` 对账单几乎无感 |
| 输出仅为输入的 **1/148**（357 vs 52,830） | 「本步输出很短」≠ 便宜；须用 **`tok_loop_delta`** 评估步态 |
| 同会话 N 次 loop | 全云端输入 ≈ **N × 52,830**；端侧化一步即省 **~52.8k 云端输入** |
| 端侧边际成本多为算力摊销 | 端侧仍处理 ~52.8k 上下文，但 **不计入云端 \(c_{in}^{cloud}\)** |

**节省估算（单次 Inference Step）**：

\[
\Delta \text{\$} \approx c_{in}^{cloud} \cdot T_{in}^{step} + c_{out}^{cloud} \cdot T_{out}^{step}
\approx c_{in}^{cloud} \cdot 52{,}830 + c_{out}^{cloud} \cdot 357
\approx c_{in}^{cloud} \cdot T_{in}^{step}
\]

**会话级示例（对齐 North Star 40%–70% 云端输入降幅）**

| 场景 | 云端输入 Token | 云端输出 Token（参考） |
|------|----------------|------------------------|
| 10 步全云端 | **528,302** | 3,566 |
| 7 步端侧 + 3 步云 | **158,491**（↓70%） | 1,070 |
| 10 步全端侧 | **0** | 0（不占云端计费） |

**端侧命中一步 ≈ 节省 ~52,830 云端输入 Token**；Cascade 升云若仍送全量 ~52.8k prompt，则几乎抵消收益——须 **\(T_{in}^{compressed} \ll 52{,}830\)**。

---

## 3. 产品目标与非目标

### 3.1 目标（MVP → 完整版）

1. **Agent 透明接入**：OpenAI 兼容 `v1/chat/completions`，OpenClaw / Hermes 仅替换 `base_url`，不改 Agent 代码。  
2. **多信号路由决策**：任务难度、延迟预算、成本预算、隐私级别、历史质量反馈。  
3. **可配置策略**：按应用、用户 tier、场景（客服/编码/写作）下发不同路由 profile。  
4. **质量兜底**：端侧失败、超时、低置信度时 **自动 Fallback** 到云端。  
5. **全链路可观测**：每次路由原因、**tokens_in / tokens_out / input_ratio**、Cloud Input Saved、耗时、是否回退均可追溯。

### 3.2 非目标（首期不做或弱做）

- 训练/微调自有路由模型（首期以 **规则 + 轻量分类器 + 可选 LLM-as-judge 离线标定** 为主）  
- 跨厂商任意模型的全自动竞价（首期聚焦 **既定端云模型对**）  
- 替代 OpenClaw / Hermes 的 Gateway、记忆、Skills、工具运行时  

---

## 4. 模型与部署假设

### 4.1 端侧模型（Edge）

| 属性 | 说明 |
|------|------|
| **示例** | Qwen3.6-35B-A3B（MoE，激活参数约 3B 级） |
| **部署** | 用户设备 / 企业边缘节点 / 专用推理 Pod |
| **优势** | 低边际 Token 成本（多为算力摊销）、低延迟、数据不出域 |
| **局限** | 上下文长度、复杂推理、最新知识、部分工具调用能力弱于云端旗舰 |

### 4.2 云端模型（Cloud）

| 属性 | 说明 |
|------|------|
| **示例** | DeepSeek-V4-Pro |
| **接入** | 官方或私有化 API，按 Token 计费 |
| **优势** | 复杂任务质量上限高、长上下文、可集中升级 |
| **局限** | 成本高、网络依赖、隐私与合规需额外设计 |

### 4.3 路由的本质

在 **质量约束** \(Q \geq Q_{min}\) 下，最小化 **期望云端输入成本** \(E[c_{in}^{cloud} \cdot T_{in}]\)（输出项权重可忽略），并满足 **延迟** \(L \leq L_{max}\) 与体验策略。

等价理解：最大化 **在端侧完成的 Inference Step 占比**，且升云时最小化 **`T_{in}` 增量**。

---

## 5. 核心概念

| 术语 | 定义 |
|------|------|
| **Route Profile** | 一组策略参数：成本权重、质量下限、是否允许端侧、Fallback 规则 |
| **Inference Step** | Agent 循环中的 **单次** `chat/completions` 调用（与「用户一轮对话」不同） |
| **Task Signal** | 从该次请求 messages / tools / headers 提取的可观测特征 |
| **Difficulty Score** | 综合难度分 \(d \in [0,1]\)，由可解释公式计算，用于端/云决策 |
| **Agent Context** | `agent_id`、`session_id`、`turn_index`、`step_kind` 等会话级路由状态 |
| **Confidence** | 端侧模型对自身输出的置信估计（logprob、自评、一致性检查） |
| **Cascade** | 端侧生成 → 评估 → 不满足则云端续写或重答 |
| **Escalation** | 单次会话内从端升级到云（可带端侧草稿作为 prompt 压缩） |
| **Input Dominance** | 输入占比；基线 **99.33%**（\(\bar{T}_{in}\)=52,830，\(\bar{T}_{out}\)=357/次） |
| **Cloud Input Saved** | 相对「该步直连云端」少计的云端 \(T_{in}\)（主节省指标） |

---

## 6. 路由决策框架

> **粒度说明**：OpenClaw / Hermes 每完成一次工具往返至少产生 **2 次** LLM 请求。Flowy 的「任务判断」针对 **每一次 Inference Step**，而非仅针对用户发来的那条 IM。

### 6.1 决策流水线（总览）

```
┌──────────────────────────────────────────────────────────────┐
│  Agent Gateway 发起的单次 chat/completions                    │
│  messages[] · tools[] · tool_choice · stream · headers       │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────┐
│ 1. 硬约束层 (Hard Gates) — 命中任一则直接云端，不算分          │
│    强制云标签 · 超长上下文 · 高危工具 · 端侧不可用 · privacy   │
└────────────────────────────┬─────────────────────────────────┘
                             ▼ 未命中
┌──────────────────────────────────────────────────────────────┐
│ 2. 信号提取 (Signal Extraction) — 纯函数、可回放、<5ms        │
│    结构特征 + Agent 步态 + 会话状态 + 可选 Hermes/OpenClaw 头  │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────┐
│ 3. 难度评分 (Difficulty Score d) — 加权公式，输出 [0,1]       │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
┌──────────────────────────────────────────────────────────────┐
│ 4. 策略映射 (Policy) — Profile 将 d 映射为 route + mode       │
│    single / cascade · θ_edge · θ_cloud · 会话粘性加成         │
└────────────────────────────┬─────────────────────────────────┘
                             ▼
              ┌──────────────┴──────────────┐
              ▼                             ▼
      Edge (Qwen3.6-A3B)              Cloud (DS-V4-Pro)
              │ Cascade Quality Gate 失败
              └────────────────────────► Cloud (压缩升云)
```

### 6.2 任务判断的具体原理

任务判断的目标：在 **不访问 Agent 私有状态**（不读磁盘记忆、不拦截工具执行）的前提下，仅根据 **本次 HTTP 请求可观测内容** 与 **Router 维护的会话缓存**，估计该 Inference Step 的 **认知负荷** 与 **失败风险**，从而选择端侧或云端。

> **标定样本**：以下规则以 FlowyAIPC（OpenClaw 衍生）真实 `chat/completions` 请求为依据标定——单条请求可含 **数万 token 的固定 system**、**20+ 工具 schema**、**数十轮 tool loop 历史**；模型字段可为 `deepseek-v4-flash` / `flowy-cloud/Auto`，由 Router 映射到实际端/云后端，**不以请求中的 model 字符串作为路由依据**。

#### 6.2.0 OpenClaw 请求解剖（Router 可见部分）

一次 OpenClaw Gateway 发起的推理请求，结构大致如下：

```
POST /v1/chat/completions
├── model: "deepseek-v4-flash" | "flowy-cloud/Auto"   ← 逻辑名，Router 自行映射
├── stream: true
├── tool_choice: "auto"
├── tools[]: 20+ function definitions（exec/read/browser/...）
└── messages[]
    ├── [0] role=system  （巨型，多段拼接）
    │     ├── 固定策略块：Tooling / Execution Bias / Safety / OpenClaw Control
    │     ├── <available_skills>…</available_skills>
    │     ├── # Project Context（AGENTS.md、SOUL.md、USER.md、TOOLS.md 注入）
    │     ├── # Dynamic Project Context（HEARTBEAT.md 等，注释标明 cache boundary）
    │     ├── ## Inbound Context → openclaw.inbound_meta.v2 JSON
    │     └── ## Runtime → agent=main | channel=heartbeat | model=flowy-cloud/Auto …
    ├── [1..n-1] user / assistant / tool 交替（Agentic Loop 历史）
    └── [n] 触发本次推理的「当前步」← step_kind 主要看这条及前一条 assistant
```

**OpenClaw 专有片段（用于解析，非用户意图）**

| 片段 | 示例 | Router 用途 |
|------|------|-------------|
| Inbound Meta | `"schema":"openclaw.inbound_meta.v2","channel":"heartbeat"` | 识别 `HEARTBEAT` / 渠道 / 是否 direct |
| Runtime 行 | `channel=heartbeat \| model=flowy-cloud/Auto \| thinking=off` | 会话元数据、是否逻辑自动路由 |
| 用户信封 | `[OpenClaw heartbeat poll]` | → `step_kind=HEARTBEAT_ACK` |
| 语音占位 | `[Audio transcript (machine-generated, untrusted)]: "Skipping …"` | 非真实语义输入，常伴随 tool 修复循环 |
| 助手失败 | `[assistant turn failed before producing content]` | → 升云粘性、`RECOVERY_AFTER_FAILURE` |
| 合成 tool 错误 | `[openclaw] missing tool result in session history` |  transcript 修复，非真实工具输出 |
| 会话锁 | `session file changed while embedded prompt lock was released` | 环境噪声，不计入用户意图难度 |

**成本关键点**：OpenClaw **每一步 loop 都重发完整 messages**（含巨大 system + 全历史）。因此路由评分除 `tok_total_in` 外，必须引入 **`tok_loop_delta`**（相对本会话上一步推理新增/变更的 token），否则 mid-loop 的 `TOOL_RESULT_DIGEST` 会被误判为「超难任务」。

#### 6.2.1 设计原则

| 原则 | 说明 |
|------|------|
| **可解释** | 每次决策附带 `reason_codes[]`（如 `HIGH_TOOL_FANOUT`、`STEP_TOOL_RESULT`），可在控制台回放 |
| **保守默认** | 不确定时倾向云端或 Cascade，避免 Agent 工具链因端侧幻觉中断 |
| **步态优先于字数** | 「末条是 tool 结果」「assistant 将发起 exec」比「用户消息短」更有判别力 |
| **增量上下文优先于全量** | OpenClaw 全量 prompt 极大；用 `tok_loop_delta` 评估 **本步新增负荷** |
| **会话一致性** | 同 `session_id` 内升级云端后短时粘性；`assistant turn failed` 后强制粘性 |
| **与 Agent 解耦** | 无 Header 时靠 OpenClaw 正文标记 + messages 结构推断；有 Header 时覆盖启发式 |

#### 6.2.2 信号 taxonomy（提取什么）

**A. 结构特征（从 body 计算）**

| 信号 ID | 计算方式 | 含义 |
|---------|----------|------|
| `tok_system` | role=system 消息 token 数 | Agent 人格 / Skills 注入量 |
| `tok_tools_schema` | `tools[]` 序列化 token 数 | 工具定义复杂度 |
| `tok_user_last` | 最后一条 user 消息 token 数 | 当前用户意图体量 |
| `tok_assistant_last` | 最后一条 assistant 消息 token 数 | 上一步模型输出长度 |
| `tok_tool_results` | 最近一条/多条 `role=tool` 合计 | 工具回传数据量 |
| `tok_total_in` | 整包 messages 输入 token（含图片估算） | 硬上限 / `GATE_CTX_OVERFLOW` |
| `tok_loop_delta` | 相对会话缓存的上次请求，messages 新增 token | **本步真实难度**（OpenClaw 必备） |
| `tok_static_system` | system 中 `# Dynamic Project Context` **之前** 的 token | 可缓存静态前缀，不计入步态难度 |
| `tok_dynamic_system` | system 中 Dynamic + Inbound + Runtime 段 token | 随渠道/心跳变化 |
| `n_turns` | messages 中 user 轮次数 | 对话深度 |
| `n_loop_steps` | 本会话累计 inference 次数（Router 计数） | 越深越倾向粘性云 |
| `openclaw_channel` | 解析 `inbound_meta.v2.channel` 或 Runtime `channel=` | heartbeat / telegram / … |
| `assistant_failed_recent` | 最近 k 条 assistant 含 `assistant turn failed` | 触发恢复态 |
| `synthetic_tool_result` | 末条 tool 含 `missing tool result` / `prompt lock was released` | 降权或忽略语义难度 |
| `n_tool_defs` | `tools` 数组长度 | 工具扇出复杂度 |
| `has_tool_calls_pending` | 最近 assistant 含未闭合 `tool_calls` | 处于工具调用态 |
| `tool_choice_mode` | `tool_choice`: auto / required / none | `required` 常伴随硬决策 |
| `response_format` | 是否 JSON schema / strict mode | 结构化输出失败成本高 → 偏云 |
| `multimodal` | 是否含 image_url 等 | 首期偏云（端侧多模态弱） |

**B. Agent 步态特征（Step Kind 推断）**

Router 用 **有序规则树**（先匹配先返回）推断 `step_kind`。对 OpenClaw，**只看 messages 尾部窗口**（默认最近 4 条）+ system 中的 Runtime/Inbound 段，避免被万行历史干扰。

**OpenClaw 规则树（优先级从高到低）**

| 优先级 | 条件 | step_kind |
|--------|------|-----------|
| 1 | 末条 user 匹配 `^\[OpenClaw heartbeat poll\]`，或 `inbound_meta.channel=heartbeat` 且末条 user 为心跳类 | `HEARTBEAT_ACK` |
| 2 | 末条 assistant 将输出 `HEARTBEAT_OK`（上条 user 为心跳 poll） | `HEARTBEAT_ACK` |
| 3 | 最近 2 条 assistant 含 `[assistant turn failed` | `RECOVERY_AFTER_FAILURE` |
| 4 | 上条 assistant 含 `tool_calls` 且 `content` 为空或极短 | `TOOL_SELECT` |
| 5 | 上条 assistant 含 `tool_calls` 且 arguments 已为完整 JSON | `TOOL_ARG_FILL` |
| 6 | 末条 `role=tool`（且非 synthetic_tool_result） | `TOOL_RESULT_DIGEST` |
| 7 | 末条 user 仅含 `[Audio transcript` / `[media attached` 且历史中已有 exec/read | `TOOL_RESULT_DIGEST`（语音修复环） |
| 8 | 上条 tool 的 `name` ∈ `sessions_spawn` 或 user/system 含 `sessions_spawn` 指引 | `SUBAGENT_SPAWN` |
| 9 | system/user 含 `compaction` / `summarize` / `Dynamic Project Context` 重写长文 | `MEMORY_COMPACT` |
| 10 | 末条 user 为真实自然语言问句，且 `n_loop_steps=0` 或本轮尚无 tool | `INITIAL_PLAN` |
| 11 | `tool_choice=none` 或本次请求无 `tools`，且已有 tool 往返 | `FINAL_REPLY` |
| 12 | 默认（有 tools、末条 user） | `INITIAL_PLAN` 或 `TOOL_SELECT` |

**步态与典型负荷**

| step_kind | 典型负荷 | 默认路由倾向 |
|-----------|----------|--------------|
| `HEARTBEAT_ACK` | 极低 | **端侧**（固定模板回复） |
| `TOOL_RESULT_DIGEST` | 低–中（看 `tok_loop_delta`） | **端侧** / Cascade |
| `TOOL_ARG_FILL` | 中低 | 端侧 |
| `TOOL_SELECT` | 中 | 视工具数；`n_tool_defs>15` 偏云 |
| `RECOVERY_AFTER_FAILURE` | 高 | **云端** + `cloud_sticky` |
| `INITIAL_PLAN` | 中高 | **云端**（尤其含语音/多模态新任务） |
| `FINAL_REPLY` | 中 | Cascade；面向用户略偏云 |
| `SUBAGENT_SPAWN` | 高 | **云端** |
| `MEMORY_COMPACT` | 中–高 | 云或 Split |
| `CRON_BACKGROUND` | 低–中 | economy → 端侧偏多 |

可选 **Agent 显式标注**（FlowyAIPC / OpenClaw 插件后续写入）：

```http
X-Flowy-Agent: openclaw | hermes
X-Flowy-Step: initial_plan | tool_select | tool_result | final_reply | compaction
X-Flowy-Session-Id: <gateway session>
X-Flowy-Loop-Index: 3
```

有显式 `X-Flowy-Step` 时 **覆盖** 启发式 `step_kind`。

**C. 语义与风险特征（轻量、可离线标定）**

| 信号 ID | 方法 | 作用 |
|---------|------|------|
| `intent_hard` | 关键词 + 正则（中英）：证明、架构、跨仓库、legal、medical… | 加分 → 云 |
| `intent_easy` | 分类、提取、格式化、翻译短句、是/否判断 | 减分 → 端 |
| `code_complexity` | 末条 user/工具结果中代码块行数、语言数 | 超阈值加分 |
| `domain_risk` | 支付、删库、生产配置等工具名黑名单命中 | **硬约束** 走云 |
| `embedding_sim` | 二期：与「已知易/难」样本库余弦相似 | 微调 \(d\) |

**D. 会话状态（Router 侧 Redis/内存）**

| 状态键 | 更新时机 | 影响 |
|--------|----------|------|
| `cloud_sticky_until` | 曾升云或 Cascade 失败 | 在 TTL 内 \(d := \min(1, d + \delta_{sticky})\) |
| `edge_fail_count` | 端侧超时 / 质检失败 | 连续 ≥2 则临时强制云 |
| `tool_fail_count` | 工具 JSON 解析失败经 Agent 重试 | 加分 |
| `user_regenerate` | 用户点击重新生成 | 下步强制云 |

#### 6.2.3 难度评分公式（首期可解释版）

将特征归一化到 \([0,1]\) 后加权求和：

\[
d = \sigma\left(\sum_i w_i \cdot f_i\right) + b_{step} + b_{session}
\]

其中 \(\sigma\) 为 sigmoid；\(b_{step}\) 由 `step_kind` 查表得到；\(b_{session}\) 来自粘性/失败计数。

**默认权重表（`balanced` Profile，可配置）**

| 特征 \(f_i\) | 权重 \(w_i\) | 说明 |
|--------------|-------------|------|
| `tok_loop_delta / ctx_edge_max` | 0.40 | **OpenClaw 主信号**：本步新增上下文 |
| `tok_total_in / ctx_edge_max` | 0.15 | 仅用于硬上限，权重低于 delta |
| `n_tool_defs / 20` | 0.15 | 工具过多提升规划难度 |
| `tool_choice_required` | 0.20 | 必须为 0 或 1 |
| `code_complexity` | 0.15 | 代码行数归一化 |
| `intent_hard` | 0.25 | 关键词命中 0/1 |
| `intent_easy` | −0.20 | 命中则降难 |
| `multimodal` | 0.30 | 首期常触发 |

**`step_kind` 偏置 \(b_{step}\)（加在 sigmoid 之前）**

| step_kind | \(b_{step}\) | 路由倾向 |
|-----------|-------------|----------|
| `HEARTBEAT_ACK` | −0.60 | 端侧 |
| `TOOL_RESULT_DIGEST` | −0.45 | 端侧或 Cascade |
| `TOOL_ARG_FILL` | −0.25 | 端侧 |
| `TOOL_SELECT` | −0.10 | 视工具数而定 |
| `FINAL_REPLY` | +0.05 | 略偏云（用户体验敏感） |
| `INITIAL_PLAN` | +0.35 | 云 |
| `RECOVERY_AFTER_FAILURE` | +0.55 | 云 + sticky |
| `SUBAGENT_SPAWN` | +0.50 | 云 |
| `MEMORY_COMPACT` | +0.20 | 云或端摘要+云润色 |

**示例 A（Hermes）**：循环第 4 步，末条 2k token `read` 工具结果，`step_kind=TOOL_RESULT_DIGEST`，`tok_loop_delta≈2k`，`tok_total_in≈40k`：

- 用 delta：\(f_{context}\approx 0.1\)，\(b_{step}=-0.45\) → **端侧**；全量则误判为超难。

**示例 B（OpenClaw 标定样本）**：语音转写失败后的 exec 环，末条为 `process` 的 whisper 堆栈 trace（`tok_loop_delta` 大但语义为「读错误决定下一步」），`step_kind=TOOL_RESULT_DIGEST`，`n_tool_defs=24`：

- \(b_{step}=-0.45\)，但 `intent_hard` 未命中 → 仍可走 **端侧** 选下一工具（装 ffmpeg / 换 soundfile）；若连续 2 次 `assistant turn failed` → 转 `RECOVERY_AFTER_FAILURE` → **云端**。

**示例 C（同样本心跳）**：user=`[OpenClaw heartbeat poll]`，`channel=heartbeat` → `HEARTBEAT_ACK`，期望 assistant 仅 `HEARTBEAT_OK` → **端侧**，`reason_codes=[STEP_HEARTBEAT, CHANNEL_HEARTBEAT]`。

#### 6.2.4 硬约束层（Hard Gates，优先于公式）

命中 **任一条** 则 **跳过评分**，直接 `route=cloud`，并记录 `reason_codes`：

| 规则 ID | 条件 |
|---------|------|
| `GATE_CTX_OVERFLOW` | `tok_total_in > 0.8 × ctx_edge_max` |
| `GATE_FORCE_CLOUD` | Header `X-Flowy-Force-Cloud: true` 或 Profile=`premium` 且用户 tier=paid |
| `GATE_PRIVACY_OFF` | Profile=`privacy` 且数据可端 → 相反：能端则端（此条为强制端） |
| `GATE_RISKY_TOOL` | 上条 assistant 的 `tool_calls[].name` 命中 **Tier-1**（见下表） |
| `GATE_ASSISTANT_FAILURE` | `assistant_failed_recent` 且 `step_kind≠HEARTBEAT_ACK` |
| `GATE_OPENCLAW_ELEVATED` | exec 参数含 `elevated:true` 或 choco/winget 安装类系统变更 |
| `GATE_EDGE_DOWN` | 端侧健康检查失败 |
| `GATE_STICKY_CLOUD` | `now < cloud_sticky_until` |
| `GATE_JSON_STRICT` | `response_format.type=json_schema` 且历史端侧 JSON 失败率 > 阈值 |
| `GATE_OPENCLAW_COMPACT` | `step_kind=MEMORY_COMPACT` 且 `tok_total_in > 12k` |

#### 6.2.5 从难度分到路由动作（Policy 映射）

设 Profile 参数 `θ_edge < θ_cloud`（默认 0.35 / 0.55）：

| 条件 | `routing_mode=single` | `routing_mode=cascade`（默认） |
|------|----------------------|--------------------------------|
| Hard Gate 命中 | cloud | cloud |
| \(d < θ_{edge}\) | edge | edge → Quality Gate → 失败则 cloud |
| \(θ_{edge} \leq d < θ_{cloud}\) | cloud | edge → Gate → 大概率 cloud |
| \(d \geq θ_{cloud}\) | cloud | cloud |

**Quality Gate（Cascade 专用，端侧生成后）**

| 检查项 | 失败则升云 |
|--------|------------|
| 平均 token logprob 低于阈值 | ✅ |
| 要求 JSON 时 `json_validate` 失败 | ✅ |
| 输出含「我不确定/无法访问」等回避模式且 step=TOOL_SELECT | ✅ |
| 输出长度 \(<\) 最小长度（空答） | ✅ |
| tool_calls 名称不在 `tools` 白名单 | ✅ |

#### 6.2.6 OpenClaw 工具风险分级（`GATE_RISKY_TOOL`）

从上条 assistant 的 `tool_calls` 解析工具名（OpenClaw 样本约 24 个工具）：

| 层级 | 工具名（示例） | 策略 |
|------|----------------|------|
| **Tier-1 强制云** | `exec`（含 elevated）、`write`、`edit`、`browser`、`sessions_spawn`、`message`(send) | 硬约束云端 |
| **Tier-2 Cascade** | `read`、`process`、`web_fetch`、`web_search`、`knowledge_query` | 默认端侧 digest，失败升云 |
| **Tier-3 端侧优先** | `memory_get`、`memory_search`、`session_status`、`update_plan`、`agents_list` | 端侧 |
| **Tier-4 多模态云** | `image`、`pdf`、`tts` | 首期云端 |

`exec` 但命令仅为 `Test-Path` / `Get-ChildItem` 只读探测：可配置 **Tier-2** 降级（需白名单正则，默认保守）。

#### 6.2.7 OpenClaw / Hermes 特化策略

| 场景 | OpenClaw / Hermes 行为 | Flowy 判断要点 |
|------|------------------------|----------------|
| 每步重发巨型 system | Tooling + Skills + AGENTS/SOUL 注入 | `tok_static_system` 不计入 \(d\)；推动 Gateway 侧 prompt cache |
| 多轮 tool loop（语音修复） | exec → process poll → 再 exec | 多数步 `TOOL_RESULT_DIGEST`；**失败累积** 后 `RECOVERY_AFTER_FAILURE` |
| 每步平均 ~52.8k \(T_{in}\)、~357 \(T_{out}\) | 全量极大、输出极短 | 路由 **不看** \(T_{out}\)；用 `tok_loop_delta` + 是否走云 |
| Heartbeat | `[OpenClaw heartbeat poll]` → `HEARTBEAT_OK` | `HEARTBEAT_ACK` 固定走端 |
| `flowy-cloud/Auto` | Runtime 声明逻辑模型 | Router 忽略该字符串，按 \(d\) 选 Qwen A3B / DS-V4-Pro |
| Connection error 历史 | sessions_history 显示 `stopReason:error` | 下步 `RECOVERY_AFTER_FAILURE` 走云 |
| Hermes subagent | `sessions_spawn` | `SUBAGENT_SPAWN` 强制云；独立 `session_id` |
| Hermes cron | 定时任务 | `economy` Profile |
| 流式 `stream=true` | 常见 | Cascade 二期：首包置信度早停（见 §14） |

#### 6.2.8 标定样本逐步推演（FlowyAIPC / OpenClaw）

以下对应用户提供的真实 payload **最后一轮**推理（`tools` 已展开，历史中多次 exec/process/read/write）：

| 观测 | 推断 |
|------|------|
| system 含 `You are a personal assistant running inside OpenClaw` | `agent_family=openclaw` |
| Runtime `channel=heartbeat` 但末条 user 为 `[media attached]` 语音 | 非纯心跳；`step_kind` 不走 `HEARTBEAT_ACK` |
| 末条 user 含 `Audio transcript … FileNotFoundError` | 机器转写噪声 → 与 `TOOL_RESULT_DIGEST` / 修复环一致 |
| 上几条 assistant 含 `[assistant turn failed` ×3 | `assistant_failed_recent=true` → **`RECOVERY_AFTER_FAILURE`** |
| 上条 assistant 拟调用 `exec`（查 ffmpeg） | Tier-1 → 即使 digest 也 **Hard Gate 云端** |
| `tok_total_in` 极大，`tok_loop_delta` 中等 | 评分以 delta 为主，避免误杀 |

**该样本推荐决策**：`route=cloud`，`reason_codes=[GATE_ASSISTANT_FAILURE, GATE_RISKY_TOOL, STEP_RECOVERY]`，`cloud_sticky_until=+10min`。

后续若 ffmpeg 装好、进入「读 tool 输出 → 用 whisper 转写 → 回复用户」且 **无连续 failed**，`TOOL_RESULT_DIGEST` + 只读 `read` 可回落端侧。

#### 6.2.9 二期：学习式路由（在可解释层之上）

1. **教师标注**：用云端模型对历史 `(request, step_kind) → optimal_tier` 打标。  
2. **蒸馏分类器**：输入固定维特征向量（§6.2.2），输出 \(P(\text{cloud})\)，推理 <10ms，可与公式 **ensemble**：\(d' = \alpha d_{\text{formula}} + (1-\alpha) P(\text{cloud})\)。  
3. **Bandit**：奖励 = 任务成功 + λ·**Cloud Input Saved**（\(\Delta T_{in}^{cloud}\)），而非总 Token 或 \(T_{out}\)。

### 6.3 用户体验（UX）约束

| 策略 | 行为 |
|------|------|
| **透明模式** | UI 展示「本地加速 / 云端增强」（可选，企业版可关闭） |
| **无感模式** | 不展示模型来源，仅保证延迟与质量 |
| **质量优先** | `quality_floor` 高，难度边界保守，云端比例上升 |
| **省钱优先** | 提高端侧比例，允许略高延迟换取成本；低置信才升云 |
| **关键路径保护** | 支付、医疗、法律等标签 **强制云端** 或 **人工审核队列** |

### 6.4 性价比（Cost–Quality）模型

记 \(\alpha = T_{in}/(T_{in}+T_{out})\)。基线：\(\bar{T}_{in}=52{,}830.23\)，\(\bar{T}_{out}=356.59\)，\(\alpha=99.33\%\)。云端按 Token 计费时，单步期望成本可写为：

\[
\text{cost}^{step} \approx c_{in}^{cloud}\,T_{in} + c_{out}^{cloud}\,T_{out}
= c_{in}^{cloud}\,T_{in}\,\bigl(1 + \frac{c_{out}^{cloud}}{c_{in}^{cloud}}\cdot\frac{1-\alpha}{\alpha}\bigr)
\]

当 \(\alpha\to 1\) 时，\(\text{cost}^{step} \approx c_{in}^{cloud}\,T_{in}\)（**定价与路由决策可仅跟踪 \(T_{in}\)**，输出单价与长度仅作校验）。

**端云分流期望成本**（单步）：

\[
E[\text{cost}] =
p_{\text{edge}}\,c_{\text{edge}}^{local}
+ p_{\text{cloud}}\,c_{in}^{cloud}\,T_{in}
+ p_{\text{cascade}}\,\bigl(c_{\text{edge}}^{local} + c_{in}^{cloud}\,T_{in}^{compressed}\bigr)
\]

| 符号 | 含义 |
|------|------|
| \(c_{\text{edge}}^{local}\) | 端侧算力摊销（通常 **不按 Token 向用户收 API 费**） |
| \(T_{in}\) | 该步若走云端时的 **全量计费输入**（system+tools+历史） |
| \(T_{in}^{compressed}\) | Cascade 升云时 **压缩后** 输入，须 \(\ll T_{in}\) 才有正收益 |

**路由目标**：在 \(Q \geq Q_{\min}\) 下最小化 \(E[c_{in}^{cloud} \cdot T_{in}]\)（等价于最大化 **Cloud Input Saved**）。

**输入优先策略（与 §2.3 对齐）**

| 策略 | 作用于 | 预期节省类型 |
|------|--------|--------------|
| 更多步走端侧 | 整步 \(T_{in}\) 不计入云端 | **主**（≈100% 该步 \(c_{in}^{cloud} T_{in}\)） |
| Gateway / Provider **prompt cache** 命中 static system | 重复 \(T_{in}\) 按缓存价或零计费 | **主**（同会话多 loop） |
| 升云只送 `tok_loop_delta` + 摘要 | \(T_{in}^{compressed}\) | Cascade 是否划算的关键 |
| 降低 `max_tokens` / 缩短回复 | \(T_{out}\) | **可忽略**（<1% 总 Token） |

**Profile 示例**

| Profile | 端侧比例目标 | 质量下限 | 典型场景 |
|---------|-------------|----------|----------|
| `economy` | 70–85% | 中 | 内部工具、批量处理 |
| `balanced` | 50–65% | 中高 | OpenClaw / Hermes 默认自托管 |
| `premium` | 20–35% | 高 | 付费用户、专业写作 |
| `privacy` | 90%+（能端则端） | 中+ | 敏感数据不出域 |

---

## 7. 路由模式

### 7.1 单路路由（Single Route）

每个请求 **只选一个** 执行端点，延迟最低、逻辑最简单。

- **端侧**：\(d < \theta_{\text{edge}}\) 且上下文可容纳且非强制云标签  
- **云端**：\(d \geq \theta_{\text{cloud}}\) 或端侧不可用  

### 7.2 级联路由（Cascade）— 推荐默认

1. 端侧完整生成（或流式输出）  
2. **Quality Gate**：置信度、长度、格式校验、敏感词、可选小 judge  
3. 不通过 → **仅将必要上下文 + 端侧草稿** 送云端修正/重写  
4. 通过 → 直接返回，**零云端计费请求**（该步 \(T_{in}\)、\(T_{out}\) 均不产生云端 API 账单）

**收益（输入主导）**：

- 端侧命中一步：节省 ≈ **该步全部云端输入 Token**（占账单 ~99%+），而非仅省输出。  
- Cascade 升云：仍付 \(c_{in}^{cloud} \cdot T_{in}^{compressed}\)；仅当压缩足够激进时净节省为正。  
- 典型 OpenClaw 会话：10 步 loop × 每步 **~52.8k** \(T_{in}\) → 7 步改端侧时，云端输入从 **~528k → ~158k**（降 **70%**，与 North Star 一致）；输出仅从 **~3.6k → ~1.1k**，对账单影响可忽略。

### 7.3 分工路由（Split）

- **规划 / 工具选择 / 长文档理解** → 云端  
- **草稿润色、模板填充、简单总结** → 端侧  
适用于 Agent 流水线，需与编排器（LangGraph 等）集成。

### 7.4 会话级粘性（Session Affinity）

**已实现（Gateway）**：`sessions/{conv_key}.json` 持久化 `cloud_sticky_until`；级联升云或上游失败后设置 TTL（`gateway.cloud_sticky_ttl_secs`，默认 600s），触发 `GATE_STICKY_CLOUD`。

同一会话内维护 `routing_state`：

- 已升级云端 → 后续若干轮保持云端，避免震荡  
- 用户显式「需要更好答案」→ 临时 `premium` 至会话结束（待做）

**全局经验**：`experience.json` 按 `step_kind` 统计 `edge_ok` / `cascade_fallback`，计算 `EXP_BIAS` 微调难度分（`gateway.experience_*`）。

---

## 8. 系统架构

```
┌──────────────────┐     ┌──────────────────────────────────────────┐
│ OpenClaw /       │────►│            Flowy Router Gateway           │
│ Hermes Gateway   │     │  Auth · Rate Limit · Profile · Audit Log  │
│ (Agentic Loop)   │     │  Per-step Routing · Session State         │
└──────────────────┘     └───────┬──────────────────────┬─────────────┘
                             │                      │
                    ┌────────▼────────┐    ┌────────▼────────┐
                    │  Routing Engine │    │  Observability  │
                    │  Signals·Policy │    │  Metrics·Trace  │
                    │  Cascade·Fallback│    │  Cost Dashboard │
                    └────────┬────────┘    └─────────────────┘
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
      ┌─────────────┐ ┌─────────────┐ ┌─────────────┐
      │ Edge Runtime│ │ Cloud Adapter│ │ Prompt Store│
      │ (local API) │ │ (DS API)     │ │ (templates) │
      └─────────────┘ └─────────────┘ └─────────────┘
```

### 8.1 组件职责

| 组件 | 职责 |
|------|------|
| **Gateway** | 统一 OpenAI-compatible API、鉴权、配额、流式 SSE 转发 |
| **Routing Engine** | 信号提取、难度分、Profile 匹配、Cascade/Fallback 状态机 |
| **Edge Runtime** | 端侧模型生命周期、健康检查、GPU/NPU 能力探测 |
| **Cloud Adapter** | 多云厂商抽象（首期 DeepSeek），重试、计费埋点 |
| **Quality Gate** | 置信度、规则校验、可选异步抽检 |
| **Control Plane** | Profile 配置、A/B、模型版本、特性开关 |

### 8.2 对外 API（草案）

与 OpenAI Chat Completions 兼容，扩展可选 Header / Body 字段：

```http
POST /v1/chat/completions
# 禁止用 X-Flowy-* 请求头参与路由；步态/难度仅由 messages[]、tools[] 与 config.toml 推断
```

端/云/级联路由由 **`config.toml` 的 `gateway.route` / `routing_mode` / `default_profile`** 决定。会话上下文以请求体 `messages[]` 为准；`loop_steps` 由 transcript 内 **assistant 条数** 推断。

### 8.3 配置与数据目录（TOML）

Flowy Router **不使用环境变量**承载 Gateway / 上游 / CLI 等业务配置（日志级别等运行时项除外）。统一使用 **TOML** 文件，与持久化数据同目录存放：

| 系统 | 应用根目录 |
|------|------------|
| Linux / macOS | `~/.flowy-router/` |
| Windows | `%USERPROFILE%\.flowy-router\` |

```
~/.flowy-router/
  config.toml      # 主配置（TOML）
  gateway.pid      # 守护进程 PID（运行时）
  sessions/        # 会话状态持久化（预留）
  logs/gateway.log # Gateway 运行日志
```

**`config.toml` 结构（示例）**

```toml
[gateway]
listen = "127.0.0.1:8080"
route = "auto"                 # auto | edge | cloud | cascade（端/云仅配置文件控制）
routing_mode = "cascade"       # single | cascade | split（route=auto 时）
default_profile = "balanced"   # economy | balanced | premium | privacy
ctx_edge_max_tokens = 65536
# api_key = "flowy-local"      # 选填：入站鉴权
# admin_token = "change-me"    # 选填：保护 POST /v1/admin/shutdown

[upstream.edge]
base_url = "http://127.0.0.1:11434/v1"
# api_key = "ollama"           # 选填：转发上游 Bearer

[upstream.cloud]
base_url = "https://api.deepseek.com/v1"
# api_key = "sk-..."           # 选填：转发上游 Bearer

[cli]
# gateway_url = "http://127.0.0.1:8080"
# gateway_bin = "/path/to/flowy-gateway"
```

- 首次 `flowy gateway start`（或 `run` / `restart`、直接启动 `flowy-gateway`）时，若目录或文件不存在则自动创建并写入带注释的默认模板。  
- CLI 与 `flowy-gateway` 默认读取同一路径；调试可用 `--config /path/to/config.toml` 覆盖。  
- Agent（OpenClaw / Hermes）仍使用各自原生配置格式（如 JSON）；仅 **Flowy 自身** 使用上述 TOML。

响应扩展（便于调试与计费）：

```json
{
  "choices": [...],
  "flowy_meta": {
    "route": "edge",
    "fallback": false,
    "difficulty_score": 0.32,
    "step_kind": "tool_result_digest",
    "reason_codes": ["STEP_BIAS_TOOL_RESULT", "TOK_IN_MODERATE"],
    "tokens_in": 52830,
    "tokens_out": 357,
    "input_ratio": 0.9933,
    "cloud_input_saved": 52830,
    "cloud_tokens": 0,
    "latency_ms": { "route": 8, "inference": 420 },
    "profile": "balanced"
  }
}
```

---

## 9. 关键场景与用户故事

| ID | 角色 | 故事 | 验收标准 |
|----|------|------|----------|
| US-01 | 部署者 | Hermes `hermes setup model` 指向 Flowy `base_url` | 对话与工具循环正常，status 显示 custom endpoint |
| US-02 | 部署者 | OpenClaw `openclaw.json` provider 改为 Flowy | 原有 Gateway/Skills 无需改动 |
| US-03 | 部署者 | Agent 一次用户指令触发 8 次 LLM 调用 | ≥5 次 `flowy_meta.route=edge`（简单 tool digest 步） |
| US-04 | 用户 | Telegram 发来「帮我把下载目录前 10 个文件列出来」 | `INITIAL_PLAN` 走云；`TOOL_RESULT_DIGEST` 走端 |
| US-05 | 用户 | 「设计分布式事务方案」类复杂任务 | 首步 `INITIAL_PLAN` 走云且 `cloud_sticky` 生效 |
| US-06 | 运维 | 按 `X-Flowy-Agent` 查看 OpenClaw vs Hermes 成本 | 分 Agent 类型报表 |
| US-07 | 合规 | `privacy` profile + OpenClaw 本地部署 | 用户数据类 tool result 不送云端 |

---

## 10. 质量与风险控制

### 10.1 质量保障

- **Fallback SLA**：端侧超时（如 3s）或 OOM → 自动云端，用户无感重试  
- **Canary**：新端侧模型版本先 5% 流量，对比质量指标  
- **人工反馈闭环**：点踩触发 `difficulty` 样本入库，定期重训分类器  

### 10.2 风险

| 风险 | 缓解 |
|------|------|
| 端侧误判导致答非所问 | Cascade + 低置信升云；关键领域强制云 |
| Cascade 双次调用反而更贵 | 输入主导下：\(c_{in}(T_{in}^{edge}+T_{in}^{compressed}) > c_{in} T_{in}\) 若压缩不足；仅在中难度带启用且 **\(T_{in}^{compressed} \leq 0.3\,T_{in}\)** |
| 端侧版本碎片化 | 最低能力契约（min context、min tool support） |
| 数据隐私 | `privacy` profile + 端侧加密存储 + 云侧日志脱敏 |

---

## 11. 数据与可观测性

### 11.1 埋点事件

- `route.decision`：profile、difficulty、chosen_tier、reason_codes[]  
- `inference.complete`：`tokens_in`、`tokens_out`、`input_ratio`、`cloud_input_billed`、`latency`、`model_version`  
- `cascade.escalate`：端侧置信度、升云原因  
- `user.feedback`：thumb、edit_distance、regenerate  

### 11.2 控制台视图

- 成本：**云端输入 Token** / 步、Cloud Input Saved、\(T_{in}/(T_{in}+T_{out})\) 趋势、节省金额（按 \(c_{in}\) 估算）  
- 质量：分 tier 满意度、Fallback 率  
- 路由：端/云/级联占比、难度分布热力图  

---

## 12. 发布计划

### Phase 0 — 技术验证（2–3 周）

- [ ] 端侧 Qwen3.6-35B-A3B 与云端 DeepSeek-V4-Pro 基准质量曲线（按任务集）  
- [ ] 确认成本基线：\(\bar{T}_{in}=52{,}830\)、\(\bar{T}_{out}=357\)、\(\alpha=99.33\%\)，并建立 P50/P95 \(T_{in}\) 分布  
- [ ] 确定 \(\theta_{\text{edge}}, \theta_{\text{cloud}}\) 初始阈值  
- [ ] Cascade 原型：端答 + 规则升云  

### Phase 1 — MVP（6–8 周）

- [ ] OpenAI 兼容 Gateway + `balanced` / `economy` Profile  
- [ ] 单路 + Cascade 路由、Fallback  
- [ ] **OpenClaw / Hermes 配置文档**（base_url 替换 + 可选 Headers）  
- [ ] OpenClaw 步态推断 v1（§6.2.0–6.2.8 规则树 + `tok_loop_delta`）+ `reason_codes` 透出  
- [ ] 基础控制台（成本 + 路由分布 + step_kind 热力图）  

### Phase 2 — 增强（8–12 周）

- [ ] 轻量难度分类器 + 会话粘性  
- [ ] Split 模式与 Agent 插件  
- [ ] A/B 与 Bandit 实验框架  
- [ ] 多云端厂商 Adapter  

### Phase 3 — 规模化

- [ ] 企业级 SSO、审计、私有化 Control Plane  
- [ ] 自定义 Profile 与租户级预算熔断  

---

## 13. 竞品与差异化

| 方案 | 特点 | Flowy 差异化 |
|------|------|----------------|
| 固定小模型 + 大模型手工切换 | 简单 | 统一入口 + 多信号 + 级联降 Token |
| 纯云端 Router（如 LiteLLM） | 多云切换 | **端云一体** + 成本优先 + 隐私 profile |
| Agent 内手写 if-else / 固定 Ollama | 灵活难维护 | **外部转发** + 按 Inference Step 判断 + 可观测 |
| LiteLLM 等纯云代理 | 多云/计费 | **端云一体** + Agent 步态 + OpenClaw/Hermes 首发 |

---

## 14. 开放问题

1. OpenClaw 是否在 Gateway 侧拆分 **static/dynamic system** 并开启 prompt cache，以降低 Router 面对的 `tok_total_in`？  
2. `exec` 只读探测（`Test-Path`、`Get-ChildItem`）是否默认降级 Tier-2？  
3. ~~OpenClaw / Hermes 是否 upstream 写入 `X-Flowy-Step`~~ **已决**：长期仅依赖 §6.2.0 正文标记，不用请求头。  
4. 端侧 **tool_calls** 在 `n_tool_defs>20` 时是否一律 `TOOL_SELECT` 走云？  
5. **流式 Cascade**：端侧流式首包置信度早停的阈值与 Agent UX？  
6. Hermes **subagent** 会话是否与父会话共享 `cloud_sticky`？  

---

## 15. 附录

### 15.1 路由决策伪代码（Cascade + Agent 步态）

```
function handle(request, profile, session_state):
    if hard_gate(request, session_state, profile):
        return infer(cloud, request), meta(route=cloud, reasons=hard_gate_reasons)

    signals = extract_signals(request)          // §6.2.2 A/B/C + tok_loop_delta
    step_kind = resolve_openclaw_step_kind(request, signals)  // §6.2.0 规则树优先
    d = difficulty_score(signals, step_kind, session_state, profile.weights)
    route_plan = policy_map(d, profile)         // edge | cloud | cascade_band

    if route_plan == cloud or not edge_available():
        return infer(cloud, request), meta(route=cloud, d=d, step_kind=step_kind)

    edge_resp = infer(edge, request)
    if route_plan == edge and quality_gate(edge_resp, step_kind, profile):
        return edge_resp, meta(route=edge, d=d, step_kind=step_kind)

    compressed = compress_for_cloud(request, edge_resp, step_kind)
    cloud_resp = infer(cloud, compressed)
    update_session(session_state, escalated=true)
    return cloud_resp, meta(route=cascade, d=d, step_kind=step_kind)
```

### 15.2 Flowy Router `config.toml`（完整字段）

见 §8.3；实现位于 `crates/config`，序列化格式为 **TOML 0.5+**（`toml` crate + `serde`）。

### 15.3 OpenClaw 配置片段（示例）

Agent 侧仍为 JSON；`baseUrl` 指向 Flowy Gateway（监听地址见 `config.toml` 的 `gateway.listen`）：

```json
{
  "models": {
    "providers": {
      "flowy": {
        "baseUrl": "http://127.0.0.1:8080/v1",
        "apiKey": "",
        "models": [{ "id": "flowy-auto", "name": "Flowy Auto Route" }]
      }
    }
  }
}
```

### 15.4 Hermes 配置片段（示例）

```bash
# hermes setup model → Custom OpenAI-compatible endpoint
# base_url: http://127.0.0.1:8080/v1   # 与 ~/.flowy-router/config.toml 中 gateway.listen 一致
# api_key:  选填；仅当 gateway.api_key 已配置时需与之一致
# 路由 Profile：请求头 X-Flowy-Profile，或 config.toml 的 gateway.default_profile
```

### 15.5 OpenClaw system 分段正则（实现参考）

```
STATIC_END_MARKER = "# Dynamic Project Context"
INBOUND_MARKER    = "## Inbound Context"
RUNTIME_MARKER    = "## Runtime"
INBOUND_JSON      = /"schema"\s*:\s*"openclaw\.inbound_meta\.v2"/
HEARTBEAT_USER    = /^\[OpenClaw heartbeat poll\]/
ASSISTANT_FAILED  = /\[assistant turn failed/
SYNTHETIC_TOOL    = /\[openclaw\] missing tool result|prompt lock was released/
```

### 15.6 名词与模型版本

- 端侧示例：**Qwen3.6-35B-A3B**（MoE，名称以厂商发布为准）  
- 云端示例：**DeepSeek-V4-Pro** / 请求中的 **DeepSeek-V4-Flash**（由 Cloud Adapter 映射，以平台 ID 为准）  
- OpenClaw 逻辑名：**flowy-cloud/Auto** → Router 内部 `flowy-auto` Profile，**不**直连某一固定后端  

---

**文档维护**：产品 / 架构 / 算法共建；重大策略变更需更新版本号与变更记录。
