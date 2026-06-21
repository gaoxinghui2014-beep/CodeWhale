# Ponytail：编程智能体代码精简策略分析

> **源码来源**：`ponytail-main/` 目录（上游仓库：[DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail)，MIT 协议）
>
> **核心理念**：*"The best code is the code never written."*

---

## 一、概述

Ponytail 是一套面向 AI 编程智能体的代码精简策略系统。它通过"懒惰资深开发者"（lazy senior dev）的隐喻，在智能体生成代码之前强制执行一套决策阶梯，使其在满足需求的前提下产出最少量的代码。

**核心数据**（基于真实 Claude Code 会话对 FastAPI + React 项目的 12 项功能任务，Haiku 4.5，n=4）：

- **代码量减少 54%**（中位数，最极端场景可达 94%）
- **Token 消耗减少 22%**
- **成本降低 20%**
- **耗时缩短 27%**
- **安全守卫 100% 保持**（对比裸"one-liner" prompt 的 95%）

---

## 二、核心策略：六级决策阶梯（The Ladder）

Ponytail **每次响应都保持激活状态**，不回退到过度构建。不确定时依然生效。仅在 `stop ponytail` / `normal mode` 时关闭。

在智能体每次编写代码之前强制执行以下阶梯式决策流程，在满足条件的第一级阶梯停下：

```
1. 这东西需要存在吗？            → 不需要：跳过（YAGNI 原则）
2. 标准库能做吗？                → 能：用标准库
3. 原生平台功能覆盖了吗？        → 覆盖了：用原生功能
4. 已安装的依赖能解决吗？        → 能：用已有依赖
5. 能写成一行吗？                → 能：一行搞定
6. 只有以上都不行时：            写出能工作的最少代码
```

**设计原则**：阶梯是一个条件反射，不是研究项目。两条阶梯同时满足 → 选更高级的那条，然后继续前进。第一个能工作的"懒惰方案"就是正确方案。

### 各级阶梯详解

#### 第 1 级：YAGNI —— 根本不需要写

任何推测性需求直接跳过，用一句话说明原因。这是最激进的精简手段——消除代码的最佳方式是不写。不是推迟，是不写。

**示例**：
- 用户要求"加个缓存" → 回应："先不加，等 profiler 确认瓶颈再说。真有瓶颈时，一行 `@lru_cache` 就够了。"

#### 第 2 级：标准库优先

任何标准库已有的功能绝不手写。这不仅仅是减少代码量，更是减少 bug 面和维护负担。

**示例**：
- Python: `functools.lru_cache` 替代手写缓存类
- Python: `dict(zip(keys, values))` 替代手动循环构建字典
- JavaScript: `new Set()` 替代手写去重逻辑
- 邮件校验：`"@" in email` 而非 27 行 EmailValidator 类——真正的校验是确认邮件本身

#### 第 3 级：原生平台功能

浏览器、操作系统、数据库等底层平台已提供的原生能力优先使用，而不是引入第三方库或手写组件。

**示例**：
- `<input type="date">` 替代 flatpickr 日期选择器库（404 行 → 23 行）
- `<input type="color">` 替代自定义颜色选择器组件（287 行 → 23 行）
- CSS 动画替代 JS 动画库
- 数据库约束（`UNIQUE`、`CHECK`）替代应用层校验代码
- `Intl.DateTimeFormat` 替代 moment.js（0 依赖）

#### 第 4 级：已安装依赖

利用项目中已有的依赖解决问题，绝不为了几行代码能做的事引入新依赖。每个新依赖代表持续的维护成本、安全攻击面和版本冲突风险。

#### 第 5 级：一行代码

如果逻辑可以用一行表达式干净地完成，就写成一行。不是"代码高尔夫"，是可读的一行。

#### 第 6 级：最少代码

以上所有阶梯都不适用时，写出满足需求的绝对最少代码。无抽象、无样板、无"为了以后"的脚手架。

> **边界说明**：Ponytail 管的是你**构建什么**，不是你**怎么说话**——想同时控制输出简洁度，可与 [Caveman](https://github.com/JuliusBrussee/caveman) 搭配使用，后者管精炼散文，Ponytail 管精炼代码。

---

## 三、安全边界：绝不精简的红线

Ponytail 的策略精髓在于区分"懒惰"（lazy）和"疏忽"（negligent），明确划定了绝不能触碰的底线：

### 不可精简的领域

- **信任边界的输入校验**：路径穿越检查、SQL 注入防护等。即使"一行 prompt"要求写出更少的代码，Ponytail 也会保留 `../` 路径穿越检查（基准测试中 Ponytail 保留该守卫，而裸 `yagni-oneliner` prompt 则丢弃了它）
- **防止数据丢失的错误处理**：涉及持久化的操作必须有错误处理
- **安全措施**：认证、授权、加密相关的逻辑不可省略
- **无障碍基础**：aria 标签、键盘导航等基本无障碍支持
- **硬件校准**：现实世界的时钟会漂移、传感器会偏差——保留校准旋钮。规格书不是理想世界
- **用户明确要求保留的东西**：不做二次争辩

### 最小检查机制

"懒惰的代码没有检查是不完整的"：

- **非平凡逻辑**（包含分支、循环、解析器、涉及金钱/安全路径）：必须留下一个可运行的最小检查——`assert` 自检、`__main__` 演示代码、或一个小的 `test_*.py` 文件
- **平凡一行代码**：不需要测试（YAGNI 也适用于测试）
- **无需框架、无需 fixture、无需每个函数都写测试套件**

---

## 四、强度等级（Intensity Levels）

Ponytail 支持三级强度可调，同时保持所有安全边界不变：

| 等级 | 触发命令 | 行为变化 |
|------|----------|----------|
| **lite** | `/ponytail lite` | 按用户要求构建，但在一行中指出更懒惰的替代方案。用户决定。 |
| **full**（默认） | `/ponytail` | 阶梯全面执行。标准库和原生功能优先。最短 diff、最短解释。 |
| **ultra** | `/ponytail ultra` | YAGNI 极端主义的。删除优于添加。用一行代码交付的同时质疑需求的剩余部分。 |

**关闭方式**：`stop ponytail`、`normal mode`、或 `/ponytail off`（仅当整条消息为去激活短语时触发，防止误触发）。Pi agent 额外支持 `/ponytail status`（查看当前模式和默认值）和 `/ponytail default lite|full|ultra|off`（持久化修改默认值）。

### 三级对比示例

需求："给这些 API 响应加个缓存。"

- **lite**：*"Done, cache added. FYI: `functools.lru_cache` covers this in one line if you'd rather not own a cache class."*
- **full**：*"`@lru_cache(maxsize=1000)` on the fetch function. Skipped custom cache class, add when lru_cache measurably falls short."*
- **ultra**：*"No cache until a profiler says so. When it does: `@lru_cache`. A hand-rolled TTL cache class is a bug farm with a hit rate."*

---

## 五、输出规范

Ponytail 不仅控制代码的输出量，也控制自然语言解释的输出量：

- **代码优先**：先输出代码，再输出解释
- **最多三行简短说明**：跳过什么、何时需要添加
- **解释比代码长？删掉解释**：每一段为简化辩护的文字，都是复杂化以散文的形式偷偷跑了回来
- **用户明确要求的解释不在此限**：报告、逐步说明等按需完整输出
- **标准格式**：`[code] → skipped: [X], add when [Y].`

---

## 六、`ponytail:` 注释机制与债务追踪

Ponytail 要求在故意简化的位置留下标记注释，用以区分"有意为之"和"无知遗漏"：

### 注释格式

```
// ponytail: <天花板>, <升级路径>
```

- **天花板**（ceiling）：当前方案的已知边界（全局锁、O(n²) 扫描、朴素启发式算法）
- **升级路径**（upgrade path）：何时重新考虑、替换方案是什么

**示例**：
```
# ponytail: global lock, per-account locks if throughput matters
# ponytail: O(n²) scan, hash-map lookup when > 1000 items
# ponytail: this exists  (无已知天花板但有意保留的简化标记)
```

这些注释的意义在于：下一任维护者（或未来的自己）看到它们时，会理解这不是疏忽，而是经过判断的刻意权衡。

### `/ponytail-debt` 债务追踪工具

自动扫描代码库中所有 `ponytail:` 注释，生成债务台账：

- **按文件分组**，每行一个标记
- 标记"无触发条件"的条目（`no-trigger` 标签）——这些是最危险的，"later means never"
- **输出格式**：`<file>:<line>, <what was simplified>. ceiling: <limit>. upgrade: <trigger>.`
- **汇总**：`<N> markers, <M> with no trigger.`

---

## 七、配套工具

Ponytail 不仅是一个代码编写策略，还包含完整的配套工具生态：

### 1. `/ponytail-review` —— 过度工程审查

对当前 diff 进行专项审查，仅关注过度工程，不涉及正确性/bug/性能：

**标签体系**：
- `delete:` 死代码、未用灵活性、推测性功能。替换：无。
- `stdlib:` 手写了标准库已有的东西。指出具体函数。
- `native:` 依赖或代码做了平台原生的功能。指出具体特性。
- `yagni:` 只有一个实现的抽象、没人设置的配置、只有一个调用者的层。
- `shrink:` 同样逻辑、更少行数。展示更短形式。

**输出示例**：
```
L12-38: stdlib: 27-line validator class. "@" in email, 1 line, real validation is the confirmation mail.
L4: native: moment.js imported for one format call. Intl.DateTimeFormat, 0 deps.
repo.py:L88: yagni: AbstractRepository with one implementation. Inline it until a second one exists.
net: -42 lines possible.
```

### 2. `/ponytail-audit` —— 全仓库审计

对整个代码库执行与 `ponytail-review` 相同的审查逻辑，按"最大收益"排序输出：

- 扫描目标：依赖中 stdlib/平台已有的、单实现接口、单产品工厂、仅做转发的包装器、只导出一个东西的文件、死配置和标志、手写的标准库功能
- **输出**：`net: -<N> lines, -<M> deps possible.` 或 `Lean already. Ship.`

### 3. `/ponytail-gain` —— 收益展示

一锤子展示基准测试中测量的影响，使用 ASCII 条形图显示代码量和成本的缩减幅度。明确标注"这些是基准中位数，不是当前仓库的数字"以避免虚构不存在的基线。

### 4. `/ponytail-help` —— 快速参考

显示所有命令、等级和技能的快速参考卡。

### 5. 状态行集成

Ponytail 在 Claude Code/Codex 终端中渲染彩色状态行标记：
- `[PONYTAIL]` —— full 模式
- `[PONYTAIL:ULTRA]` —— ultra 模式
- `[PONYTAIL:LITE]` —— lite 模式

通过读写 `~/.claude/.ponytail-active` 标志文件实现跨钩子状态共享。

---

## 八、基准测试数据

Ponytail 有两套基准测试，分别验证不同维度：

### 单次生成基准（Single-shot）

5 项日常任务（邮件验证器、debounce、CSV 求和、React 倒计时、FastAPI 限流器），3 个模型（Haiku/Sonnet/Opus），10 次运行取中位数：

| 臂 | Haiku | Sonnet | Opus |
|---|------:|------:|-----:|
| 基线（无技能） | 518 | 693 | 256 |
| caveman | 116 | 120 | 67 |
| **ponytail** | **39** | **44** | **51** |

Ponytail 实现了 80-94% 的代码减少，但这部分包含了对话基线偏高的因素。

### 智能体基准（Agentic）

更公平的测量：真实 headless Claude Code 会话编辑真实开源仓库（FastAPI + React），通过 `git diff` 计算新增代码行。12 项功能任务 + 6 项安全任务，n=4：

| vs 无技能基线 | LOC | tokens | cost | time | safe |
|---|--:|--:|--:|--:|--:|
| **ponytail** | **-54%** | **-22%** | **-20%** | **-27%** | **100%** |
| caveman（简练散文对照） | -20% | +7% | +3% | +2% | 100% |
| "YAGNI + one-liner" prompt | -33% | -14% | -21% | -30% | 95% |

关键发现：
1. **Ponytail 是唯一在所有维度上都缩减的臂**。Caveman 写更少的代码但花更多的 token（简洁输出 + 相等推理 → 不省钱）；one-liner 虽然快和省但安全掉了 5% 且缩减幅度不稳定
2. **Ponytail 是唯一保持 100% 安全的臂**，而裸 "one-liner" prompt 在 4 次运行中丢了一次路径穿越检查
3. **54% 是跨任务合计**，单任务从 ~0%（不可约简的 CRUD）到 -94%（日期选择器）

---

## 九、架构设计

### 整体架构

```
ponytail-main/
├── skills/                      # 核心行为定义（与 agent 无关）
│   ├── ponytail/SKILL.md        # 主策略（六级阶梯 + 规则 + 强度等级）
│   ├── ponytail-review/SKILL.md # 过度工程审查
│   ├── ponytail-audit/SKILL.md  # 全仓库审计
│   ├── ponytail-debt/SKILL.md   # ponytail: 注释债务追踪
│   ├── ponytail-gain/SKILL.md   # 基准收益展示
│   └── ponytail-help/SKILL.md   # 快速参考
├── hooks/                       # 生命周期钩子（Claude Code/Codex/Copilot）
│   ├── ponytail-activate.js     # 会话启动：写标志文件 + 注入规则集
│   ├── ponytail-mode-tracker.js # 用户输入拦截：处理 /ponytail 命令
│   ├── ponytail-instructions.js # 共享规则构建器（含模式过滤）
│   ├── ponytail-config.js       # 配置解析器（env var → config → default）
│   ├── ponytail-runtime.js      # 运行时状态读写（标志文件 + 输出适配）
│   ├── ponytail-statusline.sh   # Bash 状态行脚本
│   └── ponytail-statusline.ps1  # PowerShell 状态行脚本
├── commands/                    # 命令定义（Claude Code/Codex 的 / 命令）
│   ├── ponytail.toml
│   ├── ponytail-review.toml
│   ├── ponytail-audit.toml
│   ├── ponytail-debt.toml
│   ├── ponytail-gain.toml
│   └── ponytail-help.toml
├── pi-extension/                # Pi agent harness 扩展
│   └── index.js                 # 扩展入口（命令注册 + 模式管理 + 规则注入）
├── AGENTS.md                    # 紧凑版规则（指令级适配）
├── .cursor/rules/ponytail.mdc   # Cursor 适配
├── .windsurf/rules/ponytail.md  # Windsurf 适配
├── .clinerules/ponytail.md      # Cline 适配
├── .kiro/steering/ponytail.md   # Kiro 适配
├── .github/copilot-instructions.md  # GitHub Copilot 适配
├── benchmarks/                  # 基准测试框架
│   ├── agentic/                 # 智能体级别基准（真实 Claude Code 会话）
│   └── results/                 # 基准结果
└── examples/                    # 效果示例
```

### 关键设计决策

#### 1. "适配器薄、核心厚"原则

核心行为定义在 `skills/` 中（与具体 agent 无关的 SKILL.md 文件），各平台的适配文件（`.cursor/rules/`、`.windsurf/rules/` 等）保持极薄，仅做指向和格式转换。当 agent 支持技能或钩子时，直接引用已有的 `skills/` 和 `hooks/`。

#### 2. 多级配置解析

默认模式解析优先级：
1. `PONYTAIL_DEFAULT_MODE` 环境变量（最高优先级）
2. `~/.config/ponytail/config.json` 的 `defaultMode` 字段（各平台路径适配：`$XDG_CONFIG_HOME` → `~/.config/` → `%APPDATA%`）
3. 硬编码默认值 `full`

#### 3. 模式过滤机制

`ponytail-instructions.js` 中的 `filterSkillBodyForMode()` 函数根据当前激活的强度等级过滤 SKILL.md 内容。强度对比表和示例按模式标签（`lite`/`full`/`ultra`）进行过滤，而非模式特定的规则（如"不做未请求的抽象"）始终保持不变。

#### 4. 去激活防误触发

`stop ponytail` 和 `normal mode` 仅在整条用户消息为这两个短语时才触发生效（忽略大小写和尾随标点）。之前的实现是把包含这些短语的任何消息都当作去激活命令，导致"add a normal mode toggle"这样的普通请求也意外关闭了 Ponytail。

#### 5. 标志文件跨进程通信

Ponytail 使用 `~/.claude/.ponytail-active` 标志文件在钩子间共享状态（SessionStart 写入 → UserPromptSubmit 更新 → 状态行脚本读取）。这是一种无服务器、无守护进程的极简 IPC 方案。

#### 6. 模式持久化

在 Pi agent harness 中，Ponytail 模式作为会话条目（session entry）持久化，类型为 `ponytail-mode`。在会话启动时，`resolveSessionMode()` 从后往前遍历会话条目，取最后匹配的模式作为当前活跃模式。这保证了模式跨会话轮次持续存在。

### 支持平台矩阵

| 平台 | 适配方式 | 能力层级 |
|------|----------|----------|
| Claude Code | 完整插件（`.claude-plugin/` + hooks + commands） | 完整：Session 激活 + 模式追踪 + 命令 + 状态行 |
| Codex | 完整插件（`.codex-plugin/` + hooks + skills） | 完整：同 Claude Code |
| OpenCode | Server 插件（`.opencode/plugins/ponytail.mjs`） | 完整：规则注入 + `/ponytail` 命令 |
| Pi | Package 扩展（`pi-extension/`） | 完整：规则注入 + 命令 + 模式持久化 |
| Gemini CLI | 扩展 manifest（`gemini-extension.json`） | 完整：始终在线规则 + 命令 + 技能 |
| GitHub Copilot CLI | 插件安装 | 完整：同 Claude Code |
| Cursor | `.cursor/rules/ponytail.mdc`（始终应用） | 指令级：始终在线规则，无 `/ponytail` 等级切换 |
| Windsurf | `.windsurf/rules/ponytail.md` | 指令级 |
| Cline | `.clinerules/ponytail.md` | 指令级 |
| GitHub Copilot（编辑器） | `.github/copilot-instructions.md` | 指令级 |
| Kiro | `.kiro/steering/ponytail.md` | 指令级 |
| OpenClaw | `clawhub install ponytail` 或 `.openclaw/skills/` | 完整：技能系统 + `/ponytail` 命令 |
| CodeWhale | `AGENTS.md`（项目根目录） | 指令级 |
| Antigravity | `gemini-extension.json` + `AGENTS.md` | 混合：始终在线规则 + 技能化命令（`/ponytail-review` 以聊天消息键入） |
| VS Code + Codex | `AGENTS.md` → `~/.codex/AGENTS.md` | 指令级 |

---

## 十、策略哲学总结

Ponytail 的精简策略背后有三条核心哲学：

### 1. "不写的代码是最好的代码"

不为推测性需求写代码，不为"以后可能需要"建抽象。不写 → 零 bug，零 CVE，零维护成本。**YAGNI 不是"以后再做"，而是"现在不做，除非事实证明需要"。**

### 2. "平台比我们聪明"

浏览器、操作系统、标准库是几十年的工程积累。Ponytail 强制智能体先检查平台是否已经解决问题，再走自定义实现路径。这不仅减少代码量，还让应用天然获得平台的性能优化、安全更新和无障碍支持。

### 3. "最小检查是完整的保证"

Ponytail 明确区分懒惰和疏忽：安全、校验、无障碍不可触碰。但测试也适用 YAGNI——只留一个最小的可运行检查来验证逻辑正确性。框架、fixture、全套测试套件如果没被要求，就属于过度工程。

### 效果边界

Ponytail 的真实效果是"在有过度构建陷阱的地方大幅削减代码，在已经是最简代码的地方几乎零影响"。它在 benchmark 中从 ~0%（不可避免的 CRUD）到 -94%（日期选择器被 `<input type="date">` 替代），平均 54%。它从不写更多代码。

---

## 附录：核心规则速查

### 六级阶梯
1. 需要存在吗？（YAGNI）
2. 标准库有吗？
3. 原生平台功能覆盖吗？
4. 已安装依赖能解吗？
5. 能一行吗？
6. 最少代码

### 八条规则
- 不写未请求的抽象 —— 没有只有一个实现的接口、没有只有一个产品的工厂、没有从不改动的值的配置项
- 不引入可避免的依赖 —— 几步代码能做的事不值得一个新依赖
- 不写没人要的样板代码 —— "为以后"准备的脚手架，以后会自己搭
- 删除优于添加，无聊优于聪明 —— 聪明是凌晨三点别人要解码的东西
- 尽可能少的文件 —— 最短的能工作的 diff 就是赢家
- 复杂请求？先交付懒惰版本并在同一条回复里质疑需求 —— 别在你能默认回答的事情上卡住
- 两个同等大小的标准库选项？选边界情况正确那个 —— 懒惰是写更少的代码，不是选更脆的算法
- 用 `ponytail:` 注释标记有意简化 —— 有已知天花板则注明：`# ponytail: global lock, per-account locks if throughput matters`

### 五条安全红线
- 信任边界的输入校验
- 防止数据丢失的错误处理
- 安全措施
- 无障碍基础
- 用户明确要求保留的
