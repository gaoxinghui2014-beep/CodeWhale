# Headroom Token 消耗优化方案分析

> **分析对象**: `headroom-main/` — Headroom: The context compression layer for AI agents  
> **版本**: 基于 2026-06-21 代码树  
> **仓库**: [chopratejas/headroom](https://github.com/chopratejas/headroom)

---

## 目录

1. [项目概述](#1-项目概述)
2. [总体架构](#2-总体架构)
3. [输入 Token 优化（Input Token Compression）](#3-输入-token-优化)
   - [3.1 内容类型智能检测](#31-内容类型智能检测)
   - [3.2 多算法压缩管道](#32-多算法压缩管道)
   - [3.3 缓存安全机制（CacheAligner）](#33-缓存安全机制)
   - [3.4 Python 端前缀缓存跟踪层](#34-python-端前缀缓存跟踪层)
   - [3.5 缓存感知的压缩策略（CompressionPolicy）](#35-缓存感知的压缩策略compressionpolicy)
   - [3.6 Live-Zone 字节级手术分发器](#36-live-zone-字节级手术分发器)
   - [3.7 CCR 可逆压缩架构](#37-ccr-可逆压缩架构)
   - [3.8 相关性评分与智能选择](#38-相关性评分与智能选择)
   - [3.9 Rust 高性能压缩管道](#39-rust-高性能压缩管道)
   - [3.10 第三方工具协同（RTK + lean-ctx）](#310-第三方工具协同rtk--lean-ctx)
4. [输出 Token 优化（Output Token Reduction）](#4-输出-token-优化)
   - [4.1 Verbosity Steering（简洁性引导）](#41-verbosity-steering)
   - [4.2 Effort Routing（思考强度路由）](#42-effort-routing)
   - [4.3 省量测量与对照组设计](#43-省量测量与对照组设计)
5. [跨 Agent 共享记忆](#5-跨-agent-共享记忆)
6. [安全与容错机制](#6-安全与容错机制)
7. [实际效果数据](#7-实际效果数据)
8. [技术栈与集成矩阵](#8-技术栈与集成矩阵)
9. [总结](#9-总结)

---

## 1. 项目概述

**Headroom** 是一个本地优先的 AI Agent 上下文压缩层，运行在用户机器上，在 prompt 发送到 LLM provider **之前**对其进行压缩。它宣称能达到 **60–95%** 的 token 节省，同时保持答案质量不变。

**核心定位：**

- **Library** — 在任何 Python/TypeScript 应用中内嵌使用：`compress(messages)`
- **Proxy** — 零代码修改的中间代理：`headroom proxy --port 8787`
- **Agent Wrap** — 一键包装主流编码 Agent：`headroom wrap claude|codex|cursor|aider|copilot`
- **MCP Server** — 对任何 MCP 客户端暴露压缩/检索工具

**关键文件索引：**

| 文件 | 功能 |
|------|------|
| `headroom/compress.py` | 单函数压缩 API 入口 |
| `headroom/transforms/pipeline.py` | Python 端压缩管道编排 |
| `headroom/transforms/content_router.py` | 内容检测与压缩器路由（3244行） |
| `headroom/transforms/cache_aligner.py` | 缓存对齐检测器 |
| `headroom/transforms/smart_crusher.py` | JSON 数组压缩（Rust 后端） |
| `headroom/transforms/kompress_compressor.py` | ML 文本压缩（1391行） |
| `headroom/compression/universal.py` | 通用压缩器入口 |
| `headroom/ccr/` | 可逆压缩（CCR）模块 |
| `headroom/relevance/` | 相关性评分模块 |
| `headroom/agent_savings.py` | 编码 Agent 预设省量配置 |
| `crates/headroom-core/src/compression_policy.rs` | Rust 端压缩策略（505行） |
| `crates/headroom-core/src/transforms/live_zone.rs` | Live-zone 字节手术分发器（2967行） |
| `crates/headroom-core/src/transforms/pipeline/mod.rs` | Rust 端压缩管道编排 |
| `crates/headroom-core/src/cache_control.rs` | 缓存标记解析器 |

---

## 2. 总体架构

```
  Your Agent / App
       │   prompts · tool outputs · logs · RAG results · files
       ▼
   ┌────────────────────────────────────────────────────┐
   │  Headroom  (本地运行 — 数据不离开本机)              │
   │  ─────────────────────────────────────────────────  │
   │  CacheAligner → ContentRouter → CCR                 │
   │                   ├─ SmartCrusher   (JSON)          │
   │                   ├─ CodeCompressor (AST)           │
   │                   └─ Kompress-base  (文本, HF ML)   │
   │                                                     │
   │  跨 Agent 记忆 · headroom learn · MCP               │
   └────────────────────────────────────────────────────┘
       │   compressed prompt  +  retrieval tool
       ▼
   LLM Provider  (Anthropic · OpenAI · Bedrock · …)
```

**管道生命周期：**

```
SETUP → PRE_START → POST_START → INPUT_RECEIVED → INPUT_CACHED
→ INPUT_ROUTED → INPUT_COMPRESSED → INPUT_REMEMBERED
→ PRE_SEND → POST_SEND → RESPONSE_RECEIVED
```

**双语言架构：**

- **Rust 核心** (`crates/headroom-core`)：高性能压缩原语、Live-zone 分发器、SmartCrusher、LogCompressor、DiffCompressor、SearchCompressor、JSON Minifier、LogTemplate、并行 bloat 估算器
- **Python SDK** (`headroom/`)：变换管道编排、用户 API、CCR 工具注入、MCP server、跨 Agent 记忆、Kompress ML 集成

---

## 3. 输入 Token 优化

### 3.1 内容类型智能检测

Headroom 使用 **双重检测体系** 识别内容类型，做到精准路由：

#### Magika ML 检测器

使用 Google 的 Magika 深度学习模型进行内容类型检测：

```
文件: headroom/compression/detector.py
```

- **本地运行**，~5ms 延迟，无需网络调用
- 支持 **100+ 内容类型**（Python、JavaScript、JSON、YAML、Markdown、Log、Diff 等）
- **99%+ 准确率** 在支持的类型上
- 输出: `ContentType`（JSON / CODE / LOG / DIFF / MARKDOWN / TEXT / UNKNOWN）+ 置信度

#### 规则检测器（Fallback）

当 Magika 不可用时，使用基于规则的 `ContentDetector` 作为后备，支持 `JSON_ARRAY`、`SOURCE_CODE`、`SEARCH_RESULTS`、`BUILD_OUTPUT`、`GIT_DIFF`、`HTML`、`TABULAR`、`PLAIN_TEXT` 等类型。

**关键设计：不用正则** — 整个项目使用结构化解析器（serde_json、aho-corasick、std 类型检查），避免正则带来的模式漂移和编译成本。

### 3.2 多算法压缩管道

Headroom 通过 `ContentRouter` 自动检测每条消息的内容类型，并路由到最适合的压缩器。管道编排如下：

```
CacheAligner → ContentRouter → 按类型的压缩器
                                  ├─ JSON 数组  → SmartCrusher
                                  ├─ 源代码     → CodeCompressor (AST)
                                  ├─ 搜索结果   → SearchCompressor
                                  ├─ 构建输出   → LogCompressor
                                  ├─ git diff   → DiffCompressor
                                  ├─ HTML       → HTMLExtractor
                                  ├─ 表格数据   → SmartCrusher (CSV/TSV)
                                  └─ 纯文本     → Kompress (ML)
```

#### 六大压缩算法详解

##### 3.2.1 SmartCrusher — JSON 数组统计压缩

```
Rust: crates/headroom-core/src/transforms/smart_crusher/  (20+ 子模块, 388 测试)
Python: headroom/transforms/smart_crusher.py (PyO3 桥接, 1029行)
```

Headroom 中最复杂的压缩算法，通过统计分析压缩 JSON 数组。

**工作流程：**

1. **统计分析** — 扫描 JSON 数组，计算每个字段的统计特征：
   - 字段熵值（高熵 = 多值变化 = 需保留的内容）
   - 顺序模式（递增 ID、时间戳序列）
   - 稀有值标记（异常值应保留）
   - 结构异常点（某些行比其他行多字段）
2. **锚点选择** — 基于位置选取代表行：
   - 前 N 行（开始的样本）
   - 后 N 行（结尾的样本）
   - 中间 N 行（信息密度最高的行，使用 `AdaptiveSizer` 检测饱和点）
   - 用户查询关键词匹配的行（高相关性加分）
3. **压缩规划** — 分类数组类型，映射到锚点模式
4. **无损压缩先行** — 优先使用紧凑格式：`csv-schema`、`json`、`markdown-kv`
5. **有损行裁剪** — 超出预算的行通过 CCR 标记丢弃：
   `{"_ccr_dropped": "<<ccr:HASH N_rows_offloaded>>"}`

**TOIN 学习循环：** 压缩后调用 `toin.record_compression()` 记录压缩模式，持续改进后续压缩决策。

##### 3.2.2 Diff 压缩

```
Rust: crates/headroom-core/src/transforms/diff_compressor.rs (1685行)
```

压缩冗长的 `git diff` 输出。

**工作流程：**

1. **解析统一 diff 格式** → 文件 + hunk 结构
2. **文件级别截断**：限制文件数（默认 20），按变更密度排序
3. **Hunk 级别截断**：每文件限制 hunk 数（默认 10），使用相关性评分（优先级模式 + 查询词重叠）
4. **上下文裁剪**：每个 `+`/`-` 变更前后保留 2 行上下文
5. **噪声过滤**（DiffNoise）：自动丢弃 lockfile 变更 + 仅空白变化的 hunk
6. **CCR 标记**：当节省 >20% 时注入检索标记

**相关性评分权重：** 变更密度 0.03、上下文词 0.2、优先级模式加成 0.3

**典型效果：** 3-10× 压缩。

##### 3.2.3 日志压缩（Log Compressor）

```
Rust: crates/headroom-core/src/transforms/log_compressor.rs (1295行)
```

压缩构建/测试输出（pytest、npm、cargo、jest、make 等）。

**六阶段管道：**

1. **格式检测** → 识别 pytest/npm/cargo/jest/make/generic 格式
2. **逐行分类** → ERROR / FAIL / WARN / INFO / DEBUG / TRACE / stack-trace / summary
3. **逐行评分** → 级别基础分 + stack-trace 加成 + summary 加成
4. **自适应预算计算** → 使用 **Kneedle 算法**（`compute_optimal_k`）找到曲率拐点，确定最优保留行数
5. **分类选择**：
   - 错误行：前 N/后 N/全部保留
   - 失败行：前 N/后 N
   - 警告行：去重后保留
   - stack-trace：关联错误附近的上下文窗口
   - summary：保留
6. **CCR 存储**：当压缩比 < 0.5 时存储原始内容

**典型效果：** 10,000+ 行日志中只有 5-10 个实际错误 → 10-50× 压缩。

##### 3.2.4 搜索压缩（Search Compressor）

```
Rust: crates/headroom-core/src/transforms/search_compressor.rs (902行)
```

压缩 `grep`/`ripgrep`/`ag` 的输出。

**工作流程：**

1. 解析为 `{文件: [(行号, 内容)]}` 结构
2. 逐匹配评分：上下文词重叠 + `LineImportanceDetector` 优先级信号 + 配置关键词
3. 按文件总匹配分排序，截断到 `max_files`
4. 运行 `compute_optimal_k`（偏置自适应总量）
5. 每文件内选择：始终保留首/尾匹配，按分数填充，排回行号顺序
6. 格式化输出：`文件:行号:内容` + `[... and N more]` 摘要

**典型效果：** 5-10× 压缩。

##### 3.2.5 Kompress — ML 文本压缩

```
Python: headroom/transforms/kompress_compressor.py (1391行)
模型: chopratejas/kompress-v2-base (HuggingFace)
```

基于 ModernBERT 的 token 级别保留/丢弃分类器。

**技术细节：**

- ModernBERT 架构，从 HuggingFace 自动下载
- 三种 ONNX 精度：`int8-wo` (261MB, fp32 等效, 99.6% 决策一致)、`fp32` (601MB)、`int8` (v1 保留)
- 支持后端：ONNX CPU、CoreML (Apple GPU)、PyTorch、PyTorch MPS
- 线程安全：全局模型缓存 + `BoundedSemaphore` 控制并发
- 懒加载 + 热缓存：首次使用前不阻塞代理启动
- 可配置备用模型：`kompress_model` 参数支持 HuggingFace 上的自定义域模型

**评估数据（labeled dataset_v2, n=500）：**
- int8-wo: f1=0.9130, must_keep_recall=0.9765, keep_rate=0.8097
- fp32: f1=0.9128, must_keep_recall=0.9770, keep_rate=0.8100

##### 3.2.6 CodeCompressor — AST 保留代码压缩

```
Python: headroom/transforms/code_compressor.py (2036行)
```

基于 tree-sitter 的源代码压缩，保留 AST 结构而裁剪冗余。

- 使用 tree-sitter 解析 AST
- 保留函数签名、类定义、导入语句等结构
- 裁剪函数体中的实现细节、注释、空白
- 标记保留：`@preserve` 装饰器强制保留

> **注意：** 这是当前唯一未移植到 Rust 的压缩算法，依然使用 Python 实现。

#### 辅助系统

| 系统 | 功能 |
|------|------|
| **AdaptiveSizer** | 信息饱和度检测，识别选中行不再提供新信息的拐点 |
| **AnchorSelector** | 基于位置的条目选择（首/尾/中权重分配） |
| **LineImportanceDetector** | 使用 aho-corasick + ASCII 词边界识别重要行 |
| **TabularIngest** | CSV/TSV/markdown 表格 → SmartCrusher 的桥接 |

#### 压缩算法总览

| 算法 | 目标内容 | 核心技术 | 实现 | 典型压缩 |
|------|----------|----------|------|:---:|
| SmartCrusher | JSON 数组 | 统计分析 + 锚点选择 + 模式去重 | Rust | 5-30× |
| Diff 压缩 | git diff | 文件/hunk 截断 + 上下文裁剪 | Rust | 3-10× |
| 日志压缩 | 构建/测试输出 | Kneedle 自适应 + 行分类评分 | Rust | 10-50× |
| 搜索压缩 | grep/ripgrep | 文件聚类 + 相关性排序 | Rust | 5-10× |
| Kompress | 自然语言文本 | ModernBERT ONNX ML | Python | 可变 |
| CodeCompressor | 源代码 | tree-sitter AST | Python | 20-40% |

#### Kompress ML 模型细节

```
模型: chopratejas/kompress-v2-base (HuggingFace)
文件: headroom/transforms/kompress_compressor.py
```

- **ModernBERT 架构**，从 HuggingFace 自动下载
- 三种 ONNX 精度：`int8-wo` (261MB, fp32 等效, 99.6% 一致)、`fp32` (601MB, 无损参考)、`int8`（v1 保留）
- 支持后端：ONNX CPU、CoreML (Apple GPU)、PyTorch、PyTorch MPS
- 线程安全：全局模型缓存 + BoundedSemaphore 控制并发
- 懒加载 + 热缓存，首次使用前不出块代理启动

#### SmartCrusher 核心能力

```
文件: headroom/transforms/smart_crusher.py  (Python 桥接)
      crates/headroom-core/src/transforms/smart_crusher/ (Rust 实现, 388 测试)
```

- **模式去重**：识别 JSON 数组中结构相同的元素，只保留代表性样本
- **行采样**：基于相关性评分选择保留哪些元素
- **锚点感知选择**：保留与用户查询上下文相关的条目
- **CCR 标记注入**：lossy 路径下被裁剪的行通过 `{"_ccr_dropped": "<<ccr:HASH N_rows>>"}` 标记，LLM 可检索
- **无损压缩选项**：`csv-schema`、`json`、`markdown-kv` 格式的只读压缩

### 3.3 缓存安全机制（CacheAligner）

```
文件: headroom/transforms/cache_aligner.py
```

**核心原则：缓存热区（系统提示词）必须永远不被修改。**

CacheAligner 是一个 **仅检测不修改** 的变换：它扫描系统提示词中可能导致 provider KV 缓存不稳定的动态内容，发出警告但不触碰提示词本身。

#### 检测的易变内容类型

| 类型 | 识别方式 | 示例 |
|------|----------|------|
| UUID | `uuid.UUID()` 解析 | `550e8400-e29b-...` |
| ISO 8601 时间戳 | `datetime.fromisoformat()` 解析 | `2026-06-21T10:30:00Z` |
| JWT | shape 检查（三段 base64url） | `eyJ...eyJ...SflK...` |
| Hex 哈希 | 长度 + 字母表检查（32/40/64 字符） | `d41d8cd98f00b204e980...` |

**关键设计选择：不使用正则**，全部使用结构化解析器，避免误匹配和性能开销。

### 3.4 Python 端前缀缓存跟踪层

除了 Rust 端的 `compute_frozen_count`（解析 Anthropic 的 `cache_control` 标记），Headroom 的 Python 层也维护了一个**会话内前缀缓存跟踪器**和**自适应反馈系统**。

#### PrefixCacheTracker — 会话内缓存跟踪

```
文件: headroom/cache/prefix_tracker.py
```

跟踪当前会话中哪些消息已被 LLM provider 缓存：

- `get_frozen_message_count()`：第 0 轮或缓存 token < 1024 时返回 0；否则返回已缓存的消息计数
- `should_force_compress()`：当压缩节省比例超过 provider 读折扣时（对 Anthropic 为 90%），决策**违反缓存稳定性进行压缩** — 因为节省的 token 成本大于缓存失效的损失

**Provider 缓存经济学配置：**

| Provider | 读折扣 | 写惩罚 | 最小可缓存 token |
|----------|:---:|:---:|:---:|
| Anthropic | 0.90 | 0.25 | 1,024 |
| OpenAI | 0.50 | 0.00 | 1,024 |
| Gemini | 0.90 | 0.00 | — |
| Bedrock | 0.90 | 0.25 | — |

#### CompressionCache — 内容寻址 LRU 缓存

```
文件: headroom/cache/compression_cache.py
```

基于内容哈希的内容寻址缓存，避免对最近已见过的内容重复压缩：

- 使用 `OrderedDict` 实现 LRU（`max_entries=10000`）
- 线程安全（`RLock`）
- `should_defer_compression()`：如果内容在 TTL 窗口内被再次看到，推迟压缩以保护已有缓存条目
- `compute_frozen_count()`（Python 版）：从头统计连续稳定消息，遇到未缓存的 tool_result 时停止

#### CompressionFeedback — 自适应学习回路

```
文件: headroom/cache/compression_feedback.py
```

基于 LLM 对压缩内容的**检索率**反馈，自适应调整后续压缩力度：

| 检索率 | 判定 | 应对措施 |
|--------|------|----------|
| >50% | 压缩过度 | max_items=50, aggressiveness=0.3 |
| >20% | 压缩偏激 | max_items=30, aggressiveness=0.5 |
| <20% | 压缩合适 | max_items=15, aggressiveness=0.7 |
| 首次出现 / 搜索率>80% | 值得保留 | 跳过压缩 |

#### DynamicContentDetector — 波动内容检测

```
文件: headroom/cache/dynamic_detector.py
```

三层检测体系，识别系统提示词中可能导致缓存不稳定的动态内容：

| 层级 | 方法 | 延迟 |
|------|------|:---:|
| Tier 1 | 正则表达式（键名匹配） | ~0ms |
| Tier 2 | NER（命名实体识别） | ~5-10ms |
| Tier 3 | 语义分析 | ~20-50ms |

覆盖约 **60 个动态标签**：时间、标识符、用户、系统状态、订单相关键名等类别。

### 3.5 缓存感知的压缩策略（CompressionPolicy）

```
文件: crates/headroom-core/src/compression_policy.rs (505行)
      headroom/transforms/compression_policy.py  (Python 镜像)
```

Headroom 引入了**认证模式感知**的压缩策略，不同用户类型有不同的优化目标：

#### 按认证模式分类

| 认证模式 | live_zone_only | cache_aligner_enabled | volatile_threshold | max_lossy_ratio | toin_read_only |
|----------|:---:|:---:|:---:|:---:|:---:|
| **Payg** (按量付费) | false | true | 128 tokens | 0.45 (45%) | false |
| **OAuth** (第三方授权) | false | true | 128 tokens | 0.45 (45%) | false |
| **Subscription** (订阅) | true | **false** | 32 tokens | 0.25 (25%) | **true** |

**设计理念：**

- **订阅用户保守优先**：不启用 CacheAligner 的修改能力（防止缓存不稳定），更低的 lossy 上限，更严格的易变阈值
- **PAYG 用户节省优先**：更激进的有损压缩（45% vs 25% 丢弃上限），允许 TOIN（工具输出智能通知）写入学习数据

#### net_mutation_gain — 缓存经济模型公式

这是 Headroom 实现的关键创新：**不是简单地问"能省多少 token"，而是问"在缓存存在的情况下，修改是否经济"**。

```
公式 (文件: compression_policy.rs:265):
  gain = ΔT · (w + r·(R − 1))  −  P_alive · (w − r) · (S + ΔT)

参数:
  ΔT = 要删除的 token 数 (delta_t)
  R  = 预期剩余读取次数 (expected_reads)
  S  = 缓存后缀长度 (suffix_tokens)
  P_alive = 缓存存活的概率
  w = 1.25 (Anthropic cache write multiplier — 缓写成本)
  r = 0.10 (Anthropic cache read multiplier — 缓读成本)
```

**公式直觉：**

- **第一项** `ΔT · (w + r·(R−1))`：节省的 token 成本（不再写入 + 不再反复读取）
- **第二项** `P_alive · (w−r) · (S+ΔT)`：破坏缓存的惩罚（后缀需要重写，多付 w-r 的差价）

**数值示例：**

- 在 50K 缓存后缀下删除 2K token → 需要 287.5 次剩余读取才划算（极少盈利）
- 在 10K 缓存后缀下删除 50K token → 仅需 2.3 次读取就回本（任何有几次剩余轮次的会话都盈利）
- 当缓存死亡（P_alive=0）时，任何有效的缩减都盈利（这是**空闲期压缩窗口**）
- 编辑缓存边界处的内容（S=0）时，只要还有至少一次读取就盈利

### 3.6 Live-Zone 字节级手术分发器

```
文件: crates/headroom-core/src/transforms/live_zone.rs (2967行)
```

这是 Phase B 的核心交付 — Rust 实现的 Anthropic `/v1/messages` 请求体压缩器。

#### 核心原理

**Live Zone** = LLM 将生成下一个响应时所依赖的消息块，即 `cache_control` 标记之后的"活跃区域"。只有这些字节可以安全修改。

```
Live Zone 边界:
  Floor  (下界): frozen_message_count — 缓存标记之前，不可触碰
  Ceiling (上界): 最新的 user 消息 — 最新的 assistant 消息也不可触碰

  ┌───────── 缓存热区 ─────────┐┌── Live Zone ──┐
  │ messages[0..N] │ system │...││ message[N]     │
  │ 永不修改，字节完全相同      ││ 可安全压缩     │
  └─────────────────────────────┘└────────────────┘
```

#### 字节范围手术（Byte-Range Surgery）

**关键创新：不反序列化 → 修改 → 重新序列化**，而是直接在原始字节上做手术：

```text
  out = body[..block_start] || replacement || body[block_end..]
```

被修改范围外的字节从输入**原样复制**，保证 SHA-256 与输入完全一致。这确保了：

- Provider KV 缓存的**前缀和后缀字节完全相同**
- 不会因为 JSON 重新序列化导致空白符/键序/数字格式变化
- CI 中的 `byte_fidelity_outside_compressed_block` 测试持续验证这一约束

#### 缓存标记解析 (`compute_frozen_count`)

```
文件: crates/headroom-core/src/cache_control.rs
```

遍历 Anthropic 请求体中所有 `messages[*].content[*].cache_control` 标记，计算出最小的不可修改消息索引：

- 对每个 `cache_control` 标记：设置 `frozen_count = max(frozen_count, i+1)`
- `system` 和 `tools[*]` 中的标记不影响消息索引下限（它们无条件属于缓存热区）
- 无标记时返回 0（live-zone 分发器可压缩所有消息）

### 3.7 CCR 可逆压缩架构

```
目录: headroom/ccr/    (Compress-Cache-Retrieve)
```

**CCR 的核心思想：压缩不是丢弃信息，而是将信息从 prompt 中临时移除，放到本地存储中，LLM 需要时可通过工具调用检索回来。**

#### 四个核心组件

| 组件 | 文件 | 功能 |
|------|------|------|
| **Tool Injector** | `ccr/tool_injection.py` | 当压缩发生时，自动向请求注入 `headroom_retrieve` 工具定义 |
| **Response Handler** | `ccr/response_handler.py` | 拦截 LLM 响应，自动处理 CCR 工具调用 |
| **Context Tracker** | `ccr/context_tracker.py` | 跨轮次跟踪压缩内容，支持主动扩展 |
| **Batch Processor** | `ccr/batch_processor.py` | 处理批处理 API 结果的异步 CCR 检索 |

#### Context Tracker — 防止"上下文失忆"

```
文件: ccr/context_tracker.py (660行)
```

这是 CCR 的关键智能层 — 跟踪整个对话中所有被压缩的内容，当用户的后续查询可能涉及之前被压缩的数据时，**主动扩展相关内容**，而不是等 LLM 来问。

**工作机制：**

1. 每次压缩时，记录压缩哈希、原始行数、压缩后行数、查询上下文、内容预览
2. 当新查询到来时，分析查询是否与之前压缩的内容相关
3. 如果相关性超过阈值（默认 0.3），主动将原始内容扩展到 prompt 中

**跨项目隔离：** 使用 `workspace_key`（项目标识）防止不同项目之间的上下文泄漏。

**配置参数：**

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `max_tracked_contexts` | 100 | 最多跟踪的上下文（LRU 淘汰） |
| `relevance_threshold` | 0.3 | 建议展开的相关性阈值 |
| `max_context_age_seconds` | 300 | 上下文最大有效期（5分钟） |
| `proactive_expansion` | true | 是否主动扩展 |
| `max_proactive_expansions` | 2 | 每轮最多主动扩展次数 |

#### 两个分发渠道

- **Tool Injection**：代理在压缩发生时自动注入工具定义
- **MCP Server**：通过 MCP 协议独立暴露 `headroom_retrieve` 工具

当 MCP 已配置时，tool injection 自动跳过以避免重复。

### 3.8 相关性评分与智能选择

```
目录: headroom/relevance/
```

当 SmartCrusher 需要选择保留 JSON 数组中的哪些元素时，相关性评分决定哪些条目最可能对 LLM 有用：

| 评分器 | 文件 | 方法 |
|--------|------|------|
| **BM25** | `relevance/bm25.py` | 基于词频-逆文档频率的文本相关性 |
| **Embedding** | `relevance/embedding.py` | 基于语义嵌入的相关性（支持 PyTorch MPS） |
| **Hybrid** | `relevance/hybrid.py` | BM25 + Embedding 混合评分 |

### 3.9 Rust 高性能压缩管道

```
文件: crates/headroom-core/src/transforms/pipeline/mod.rs
```

Rust 端的压缩管道采用**两阶段架构**：

#### 阶段1：Reformat（无损重排）

输出字节语义上等价于输入，不丢弃任何信息：

- **JsonMinifier**：通过 `serde_json` 往返去除空白
- **LogTemplate**：Drain 风格的模板挖掘，输出 `[Template Tn: ...] (Nx)` + 变体表，每行原始日志皆可重构

#### 阶段2：Offload（通过 CCR 裁剪）

丢弃 prompt 中的字节，但通过 CCR 存储原始内容，供 LLM 按需检索：

- **JsonOffload**：包装 SmartCrusher，处理 dict 数组
- **LogOffload**：基于行重复率 + 优先级稀释的启发式门控
- **DiffOffload**：基于上下文/变更比例的启发式门控
- **DiffNoise**：丢弃 lockfile hunk + 仅空白变化的 hunk
- **SearchOffload**：基于匹配跨文件聚类的门控（默认不注册，现代 agent 使用限定范围的 rg/grep）

#### 关键设计亮点

- **并行 bloat 估算器**：通过 `rayon::join` 并行运行所有 offload 的 bloat 估算，大数据输入不支付串行扫描成本
- **领域特异性**：每种 offload 有自己的结构化估算器（日志看行重复，diff 看上下文比例），不使用通用压缩比
- **估算器必须 O(n)**：每个估算器不超过输入长度的一次扫描，不产生额外分配

### 3.10 第三方工具协同（RTK + lean-ctx）

Headroom 与以下优秀项目协同工作：

- **RTK** (`github.com/rtk-ai/rtk`)：Shell 输出重写 — `git show --short`、限定范围的 `ls`、摘要安装器。Headroom 将 RTK 编译为二进制文件作为第一方工具分发
- **lean-ctx** (`github.com/yvgude/lean-ctx`)：配置编码 Agent 将工具输出路由到上下文过滤层。可通过 `HEADROOM_CONTEXT_TOOL=lean-ctx` 切换

---

## 4. 输出 Token 优化

### 4.1 Verbosity Steering（简洁性引导）

**原理：** 在系统提示词的**末尾**（而非开头）追加简洁性指令，这样：

1. 不会扰动前缀，Provider 的 prompt cache 依然命中
2. LLM 生成响应时最后看到的指令是"保持简洁"

**开关：**

```bash
export HEADROOM_OUTPUT_SHAPER=1     # 默认关闭
headroom proxy --port 8787
```

**自动学习：** `headroom learn --verbosity` 读取历史会话，根据用户实际行为（提前中断长响应、快速跳过等）自动选择合适的简洁级别：

```bash
headroom learn --verbosity            # 预览（dry run）
headroom learn --verbosity --apply    # 保存，代理立即生效
```

### 4.2 Effort Routing（思考强度路由）

**原理：** 根据当前 turn 的上下文动态调整模型的 thinking effort：

| 场景 | Thinking Effort | 原因 |
|------|:---:|------|
| 新问题、新错误 | **完整** | 需要深度推理 |
| 工具结果返回后（文件读取、测试通过） | **降低** | 只是继续执行，不需要深度思考 |
| 常规步骤 | **降低** | 减少不必要的思考 token |

这避免了 LLM 在"读取文件 → 返回结果"这种简单步骤上的过度思考开销。

### 4.3 省量测量与对照组设计

输出 token 节省无法直接测量（我们永远不会看到模型"本来会写什么"），Headroom 采用**科学的反事实估计**：

```bash
headroom output-savings
# Reduction: 31.7%  (95% CI 27.7% … 35.7%)   [estimated]
```

**对照组设计：**

```bash
export HEADROOM_OUTPUT_HOLDOUT=0.1
```

将 10% 的对话不进行输出优化，作为对照组。这样控制面板可以显示**实测值**而非估算值，带有置信区间标注。

---

## 5. 跨 Agent 共享记忆

Headroom 实现了跨 Agent 的共享上下文存储：

```python
from headroom import SharedContext

ctx = SharedContext()
ctx.put("project_auth.py", "认证逻辑：JWT + refresh token + rate limit")
ctx.get("project_auth.py")  # 在任何 agent 中都可检索
```

- 基于文件路径的 key-value 存储
- 自动去重
- 跨 Claude Code、Codex、Gemini 等主流 Agent

---

## 6. 安全与容错机制

### Inflation Guard（膨胀保护）

**所有压缩路径**（library、proxy、每 provider handler）都有膨胀保护：

```python
if tokens_after > tokens_before:
    # 回退到原始消息，不发送压缩结果
    return original_messages  # 标记: "inflation_guard:reverted"
```

### Circuit Breaker（断路器）

```
环境变量: HEADROOM_PIPELINE_BREAKER_THRESHOLD (默认 3)
```

连续 3 次变换失败 → 60 秒冷却期，避免级联失败。

### Hard Import 策略

Python 端不使用 try/except fallback 来"静默降级"：如果 Rust 扩展（`headroom._core`）不可用，直接抛出 `ImportError`，而非退回到性能更差或行为不同的 Python 实现。

### 缓存安全不可违反

- `CacheAligner` 现在**仅检测不修改**（PR-A2 修复）
- 系统提示词永远不会被修改（违反者 = 破坏用户缓存 = 静默增加账单）
- `compute_frozen_count` 严格标记不可触碰区域

---

## 7. 实际效果数据

### Token 节省

| 工作负载 | 压缩前 | 压缩后 | 节省 |
|----------|-------:|-------:|------:|
| 代码搜索（100 条结果） | 17,765 | 1,408 | **92%** |
| SRE 事件调试 | 65,694 | 5,118 | **92%** |
| GitHub issue 分类 | 54,174 | 14,761 | **73%** |
| 代码库探索 | 78,502 | 41,254 | **47%** |

### 准确性保持

| 基准测试 | 类别 | 样本数 | 基线 | Headroom | 变化 |
|----------|------|-------:|------:|---------:|------|
| GSM8K | 数学 | 100 | 0.870 | 0.870 | **±0.000** |
| TruthfulQA | 事实 | 100 | 0.530 | 0.560 | **+0.030** |
| SQuAD v2 | 问答 | 100 | — | 97% | 19% 压缩 |
| BFCL | 工具 | 100 | — | 97% | 32% 压缩 |

### 输出 Token 节省

| 模式 | 输出节省 | 置信区间 |
|------|---------|----------|
| estimate | ~31.7% | 95% CI 27.7%–35.7% |

---

## 8. 技术栈与集成矩阵

### 技术栈

| 层 | 技术 |
|----|------|
| 核心压缩引擎 | Rust (serde_json, rayon, aho-corasick, PyO3) |
| SDK / 编排 | Python 3.10+ |
| ML 模型运行 | ONNX Runtime / PyTorch / CoreML |
| 内容检测 | Google Magika (DL) |
| 相关性评分 | BM25 + Sentence Embeddings |
| 缓存存储 | 嵌入式 SQLite / 内存 LRU |

### Agent 兼容性

| Agent | `headroom wrap` | 备注 |
|-------|:---:|------|
| Claude Code | ✅ | `--memory` · `--code-graph` |
| Codex | ✅ | 与 Claude 共享记忆 |
| Cursor | ✅ | 打印配置，粘贴一次 |
| Aider | ✅ | 启动代理 + 启动 |
| Copilot CLI | ✅ | 启动代理 + 启动 |
| OpenClaw | ✅ | 作为 ContextEngine 插件安装 |

### SDK / 框架集成

| 设置 | 集成方式 |
|------|---------|
| Python 库 | `compress(messages, model=…)` |
| TypeScript 库 | `await compress(messages, { model })` |
| Anthropic / OpenAI SDK | `withHeadroom(new Anthropic())` |
| Vercel AI SDK | `wrapLanguageModel({ middleware })` |
| LiteLLM | `litellm.callbacks = [HeadroomCallback()]` |
| LangChain | `HeadroomChatModel(your_llm)` |
| Agno | `HeadroomAgnoModel(your_model)` |
| ASGI 应用 | `app.add_middleware(CompressionMiddleware)` |
| MCP 客户端 | `headroom mcp install` |

---

## 9. 总结

### Headroom 的 Token 优化方案全景

```
                    输入优化                             输出优化
              ┌─────────────────────┐           ┌─────────────────────┐
              │ 内容检测 (Magika ML) │           │ Verbosity Steering   │
              │         ↓            │           │ (提示词末尾简洁指令)   │
              │ 智能路由 (ContentRouter)│         │         ↓            │
              │    ↙    ↓    ↘       │           │ Effort Routing       │
              │ Smart  Code  Kompress│           │ (简单步骤降低思考强度)  │
              │ Crusher  Comp  (ML)  │           │         ↓            │
              │    +      +     +    │           │ Output Savings       │
              │ Log/Diff/Search/HTML │           │ (估算+对照组+CI)      │
              │         ↓            │           └─────────────────────┘
              │ 缓存安全 (CacheAligner)│
              │ 缓存经济 (net_mutation)│
              │ 字节手术 (byte-range) │
              │ 可逆压缩 (CCR)        │
              │ 相关性评分 (BM25/Emb)  │
              │ 跨轮次追踪 (Tracker)  │
              │ 跨Agent记忆 (Shared)  │
              └─────────────────────┘
```

### 核心创新点

1. **缓存经济模型** (`net_mutation_gain`) — 不是简单节省 token，而是评估在 KV 缓存存在的情况下修改是否真的省钱。这是区分"聪明压缩"和"不计后果压缩"的关键。

2. **字节级手术** — 用 `body[..start] + replacement + body[end..]` 替代"反序列化→修改→重新序列化"，保证缓存前缀完全不变。

3. **CCR 可逆压缩** — 将"有损"转化为"延迟加载"：prompt 中移除的信息存储到本地，LLM 需要时可主动检索。

4. **主动上下文扩展** — ContextTracker 跨轮次记忆被压缩的内容，当前查询可能涉及历史压缩数据时自动恢复，防止上下文失忆。

5. **领域特异性 bloat 估算** — 不使用通用压缩比判断，而是根据内容类型定制化评估：日志看行重复率、diff 看上下文/变更比、搜索看匹配聚类。

6. **输出 token 的科学测量** — 使用对照组 + 置信区间，而非凭空宣称省了多少。

7. **自适应学习回路** — 通过 `CompressionFeedback` 根据 LLM 对压缩内容的实际检索率反馈，自动调整后续压缩力度（检索率高→降低压缩率，检索率低→提高压缩率），形成闭环优化。

8. **分层波动检测** — `DynamicContentDetector` 使用三层检测（正则→NER→语义）覆盖约 60 个动态标签类型，标识系统提示词中可能破坏缓存的动态内容，做到事前预防而非事后修复。

### 与 CodeWhale 的关联

CodeWhale 项目本身就是 AI 编码 Agent，其在 `headroom-main/` 目录中包含的是上游 Headroom 项目的完整代码。CodeWhale 的 Constitution 和 AGENTS.md 中描述的**前缀缓存经济（prefix cache economics）**、**缓存响应中的内容避免重新引用**、**验证原则**等理念，与 Headroom 的 `CacheAligner`、字节级手术、`inflation_guard` 等机制高度一致。CodeWhale 的 compaction（`/compact`）功能与 Headroom 的 CCR + ContextTracker 在精神上一脉相承 — 都是在 Agent 生命周期中智能管理上下文窗口。
