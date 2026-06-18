# CodeWhale 工程概览

> 一份面向新贡献者的项目全景介绍：工程是什么、由哪些模块组成、各模块分别用了什么技术。

## 项目定位

**CodeWhale**（原名 deepseek-tui）是一个开源的**终端编码代理（Terminal Coding Agent）**，采用 MIT 协议，当前版本 0.8.62。它本质上是一个 AI 驱动的编程助手，运行在终端中 —— 能够读取代码、编辑文件、执行命令、检查结果、规划多步任务并在出错时自我修正。

它支持几乎所有主流大模型：DeepSeek 和开源模型是一等公民，Claude、GPT、Kimi、以及本地的 vLLM / Ollama 都是完全对等的后端。

- **仓库**: https://github.com/Hmbown/CodeWhale
- **网站**: https://codewhale.net
- **架构文档**: [ARCHITECTURE.md](./ARCHITECTURE.md)（运行时引擎、会话流转、工具调度）

---

## 整体技术栈

| 层级 | 技术选型 |
|---|---|
| 核心语言 | Rust (Edition 2024, rustc 1.88) |
| 构建系统 | Cargo Workspace (resolver v2)，15 个 crate |
| 异步运行时 | Tokio (full features) |
| 配置格式 | TOML |
| 序列化 | Serde + serde_json |
| 数据库 | SQLite（rusqlite，bundled 编译） |
| Web 框架 | Axum 0.8 |
| HTTP 客户端 | Reqwest（rustls-tls） |
| TLS | rustls 0.23（ring 加密后端） |
| TUI 框架 | Ratatui 0.30 + Crossterm 0.28 |
| 网站 | Next.js 15 + React 19 + TypeScript + TailwindCSS 3 |
| 网站部署 | Cloudflare (OpenNext + Wrangler) |
| 分发渠道 | npm、cargo、Docker、Nix、Scoop、Homebrew、CNB 中国镜像 |
| 跨平台 | Windows / macOS / Linux (x86_64, aarch64, riscv64) / HarmonyOS |

---

## 工作区模块（15 个 Crate）

### 1. `codewhale-cli` — CLI 入口

- **路径**: `crates/cli/`
- **描述**: "Agentic terminal facade for open-source and open-weight coding models"
- **产出**: `codewhale` 和 `codew`（旧名兼容别名）两个二进制文件
- **技术**: `clap`（命令行解析）、`clap_complete`（shell 自动补全）、`reqwest`（HTTP）、`sha2`（哈希校验）、`semver`（版本比较）
- **角色**: 整个项目的 CLI 门面，负责认证配置 (`auth`)、子命令分发、新版本检查、release 发现等

### 2. `codewhale-tui` — 终端用户界面

- **路径**: `crates/tui/`
- **描述**: "Terminal UI for open-source and open-weight coding models"
- **产出**: `codewhale-tui` 二进制
- **核心技术**:
  - **Ratatui** — Rust 异步终端 UI 框架
  - **Crossterm** — 跨平台终端控制（光标、颜色、事件）
  - **schemaui** — 声明式 UI 架构，支持 `tui` / `web` 双渲染后端
  - **portable-pty** — 伪终端（嵌入 shell 进程）
  - **starlark** — Starlark 脚本引擎（Google Bazel 的配置语言，用于工作流 DSL）
  - **pdf-extract** — PDF 文本提取
  - **image** — PNG 图片处理
  - **tar + flate2** — 压缩包处理
  - **similar** — 文本 diff 渲染
  - **arboard** — 跨平台剪贴板
  - **qrcode** — 二维码生成
  - **lru** — LRU 缓存
  - **parking_lot** — 高性能同步原语
  - **平台特定**: Windows API (`Win32_*`)、macOS (`objc2`)、Linux (`libc`)
- **特性标志**: `tui`（默认终端渲染）/ `web`（浏览器渲染）、`json` / `toml` 格式支持

### 3. `codewhale-core` — 核心运行时

- **路径**: `crates/core/`
- **描述**: "Core runtime boundaries"
- **依赖**: 聚合了 `agent`、`config`、`execpolicy`、`hooks`、`mcp`、`protocol`、`state`、`tools` 几乎全部内部 crate
- **角色**: 核心运行时，连接所有子系统、管理会话生命周期

### 4. `codewhale-config` — 配置管理

- **路径**: `crates/config/`
- **描述**: "Config schema and precedence model"
- **技术**: `toml` + `toml_edit`（TOML 读写）、`dirs`（跨平台用户目录）
- **角色**: 配置 schema 定义、多源优先级合并（项目级 `> ` 用户级 `>` 默认值）、与 `secrets` 和 `execpolicy` 联动

### 5. `codewhale-state` — 状态持久化

- **路径**: `crates/state/`
- **描述**: "Session/thread persistence and recovery model"
- **技术**: **rusqlite**（SQLite，bundled 编译进二进制，无需外部依赖）、`chrono`（时间戳）
- **角色**: 会话与线程的持久化与恢复，存储对话历史、检查点、工具调用记录

### 6. `codewhale-protocol` — 协议帧

- **路径**: `crates/protocol/`
- **描述**: "Codex-style app-server protocol frames"
- **技术**: `serde` + `serde_json` + `uuid` + `chrono`
- **角色**: 定义请求/响应/流式事件的消息结构，是整个系统的类型契约层

### 7. `codewhale-tools` — 工具调度

- **路径**: `crates/tools/`
- **描述**: "Tool invocation lifecycle, schema validation, and scheduler parallelism"
- **技术**: `tokio`（异步并发）、`async-trait`（异步 trait）、`thiserror`、`uuid`
- **角色**: 工具调用的完整生命周期 —— schema 校验、调度并行执行、结果聚合

### 8. `codewhale-agent` — 模型与提供方注册表

- **路径**: `crates/agent/`
- **描述**: "Model/provider registry and fallback strategy"
- **依赖**: `codewhale-config`、`serde`
- **角色**: 模型/提供方注册表与降级策略（例如 DeepSeek V4 Pro → Flash、GLM-5.2 → GLM-5-Turbo）

### 9. `codewhale-app-server` — App Server 传输

- **路径**: `crates/app-server/`
- **描述**: "Codex-style app-server transport"
- **技术**: **Axum**（HTTP 框架）、`tower-http`（CORS 中间件）、`rustls`、`uuid`
- **角色**: HTTP API 传输层，为外部客户端提供 RESTful 接口

### 10. `codewhale-execpolicy` — 执行策略

- **路径**: `crates/execpolicy/`
- **描述**: "Execution policy and approval model parity"
- **依赖**: `codewhale-protocol`、`serde`
- **角色**: 执行策略与用户审批模型 —— 定义哪些命令可自动执行、哪些需用户确认（yolo vs 审批模式）

### 11. `codewhale-hooks` — 钩子系统

- **路径**: `crates/hooks/`
- **描述**: "Hook dispatch and notifications parity"
- **技术**: `reqwest`（webhook HTTP 通知）、`async-trait`、`tokio`、`chrono`
- **角色**: 在工具调用前后、会话事件时触发外部 webhook 或本地脚本

### 12. `codewhale-mcp` — MCP 协议

- **路径**: `crates/mcp/`
- **描述**: "MCP server lifecycle and tool proxy compatibility"
- **依赖**: `serde` + `serde_json`
- **角色**: **Model Context Protocol** 服务器生命周期管理与工具代理，使 CodeWhale 可连接外部 MCP 服务器扩展能力

### 13. `codewhale-secrets` — 密钥存储

- **路径**: `crates/secrets/`
- **描述**: "Secret storage backends (OS keyring with file fallback)"
- **技术**: **keyring**（操作系统原生密钥链：macOS Keychain / Windows Credential Manager / Linux Secret Service）、`thiserror`、`dirs`
- **角色**: 多后端密钥安全存储，优先 OS 原生密钥链，不可用时回退到加密文件

### 14. `codewhale-release` — 版本/发布发现

- **路径**: `crates/release/`
- **描述**: "Shared CodeWhale release discovery and version comparison helpers"
- **技术**: `reqwest`（blocking）、`semver`、`serde_json`
- **角色**: GitHub Release 发现与版本比较，用于 `codewhale update` 命令

### 15. `codewhale-whaleflow` — 工作流引擎

- **路径**: `crates/whaleflow/`
- **描述**: "Typed WhaleFlow workflow IR and validation"
- **技术**: **starlark**（Starlark 脚本引擎）、`sha2`、`serde_json`
- **角色**: 类型化的 WhaleFlow 工作流 IR 和校验，用 Starlark 脚本定义可复用自动化流程

---

## 模块依赖关系图（简化）

```
codewhale-cli（CLI 入口）
  ├── codewhale-app-server（HTTP API 传输）
  │     ├── codewhale-core（核心运行时）
  │     │     ├── codewhale-agent（模型注册表）
  │     │     ├── codewhale-config（配置）
  │     │     ├── codewhale-execpolicy（执行策略）
  │     │     ├── codewhale-hooks（钩子）
  │     │     ├── codewhale-mcp（MCP 协议）
  │     │     ├── codewhale-state（SQLite 持久化）
  │     │     └── codewhale-tools（工具调度）
  │     └── codewhale-protocol（消息协议帧）
  ├── codewhale-config（配置）
  ├── codewhale-release（版本检查）
  └── codewhale-secrets（密钥存储）

codewhale-tui（TUI 入口）
  ├── Ratatui + Crossterm（终端渲染）
  ├── starlark（工作流脚本引擎）
  └── 与 core 类似的内部 crate 依赖

codewhale-whaleflow（工作流引擎）
  └── starlark（Starlark IR）
```

---

## 非 Rust 组件

### npm 包

| 包名 | 路径 | 说明 |
|---|---|---|
| `codewhale` | `npm/codewhale/` | npm 安装器，下载 GitHub Release 中 SHA-256 校验过的二进制，提供 `codewhale` / `codew` / `codewhale-tui` 三条命令 |
| `deepseek-tui` | `npm/deepseek-tui/` | 旧品牌兼容包（deepseek-tui → codewhale 迁移期） |
| `@codewhale/runtime-sdk` | `npm/runtime-sdk/` | Runtime API 的 TypeScript 类型化辅助库（ESM 模块） |

### 网站 (`web/`)

| 层面 | 技术 |
|---|---|
| 框架 | Next.js 15 + React 19 |
| 语言 | TypeScript |
| 样式 | TailwindCSS 3 + PostCSS + Autoprefixer |
| 测试 | Vitest 4 |
| 代码检查 | ESLint 9 |
| 部署 | Cloudflare（OpenNext + Wrangler） |
| 可视化 | Mermaid 11 |

### 集成桥接 (`integrations/`)

- `feishu-bridge/` — 飞书/Lark 机器人桥接
- `telegram-bridge/` — Telegram 机器人桥接
- `weixin-bridge/` — 微信机器人桥接

### VS Code 扩展 (`extensions/vscode/`)

- VS Code 插件，在编辑器中集成 CodeWhale 能力

### 构建与部署

| 渠道 | 说明 |
|---|---|
| Docker | 多架构镜像（linux/amd64 + linux/arm64），基于 Debian Bookworm-slim |
| Nix | flake.nix，覆盖 x86_64-linux / aarch64-linux / x86_64-darwin |
| Scoop | Windows 包管理器 |
| Homebrew | macOS 兼容渠道（旧品牌 deepseek-tui） |
| CNB | 中国境内镜像：`cnb.cool/codewhale.net/codewhale` |
| cargo | `cargo install codewhale-cli --locked` |
| npm | `npm install -g codewhale` |

---

## 关键文档索引

| 文档 | 内容 |
|---|---|
| [ARCHITECTURE.md](./ARCHITECTURE.md) | 架构设计：引擎循环、会话管理、工具编排、子代理、RLM |
| [INSTALL.md](./INSTALL.md) | 安装指南：各平台详细步骤与故障排查 |
| [CONFIGURATION.md](./CONFIGURATION.md) | 配置参考：settings.toml 字段说明 |
| [PROVIDERS.md](./PROVIDERS.md) | 提供方注册表：支持的模型与端点配置 |
| [MCP.md](./MCP.md) | MCP 协议集成指南 |
| [SUBAGENTS.md](./SUBAGENTS.md) | 子代理系统说明 |
| [CONTRIBUTING.md](../CONTRIBUTING.md) | 贡献指南 |
| [CHANGELOG.md](../CHANGELOG.md) | 版本变更日志 |
