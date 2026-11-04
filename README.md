# qcodex

> 面向真实工程工作的本地优先编码代理：保留 Codex 的安全执行与扩展基础，把多代理协作、可复用 Skills、长任务编排和终端体验整合为一个 `qcodex` 命令。

qcodex 是一个开源衍生项目，不是 OpenAI 官方产品。它以 OpenAI Codex 为技术基线，承接 Every Code 的多代理工作流，并在此仓库持续演进 qcodex 自己的命令、Skills 体验和终端交互。每项能力的归属见下方的“能力与来源”。

## 为什么是 qcodex

编码代理不应只会生成一段代码。qcodex 的目标是让你在同一个终端中完成：理解仓库、调用工具、安全地修改文件、把复杂任务拆给多个代理、沉淀团队方法，并在需要时把任务交给可观察、可暂停的长流程。

- **本地终端优先**：在你的工作目录、Git 仓库和现有工具链中工作。
- **有边界的自动化**：支持只读、工作区写入、审批和沙箱策略；高权限模式需要你明确选择。
- **把经验变成能力**：Skills 使用可版本管理的 `SKILL.md` 指令包，项目规则则由 `AGENTS.md` 提供。
- **从单次对话到工程流程**：交互 TUI、`exec`/JSONL 自动化、MCP、子代理和 Auto Drive 共用一套工作上下文。

## 能力与来源

| 能力 | 你能做什么 | 来源标识 |
| --- | --- | --- |
| 安全的编码代理基础 | 对话式改码、命令执行、审批、沙箱、会话恢复、图片与项目指令 | **[OpenAI Codex]**：上游基础能力，随同步更新进入本项目 |
| MCP 与自动化接口 | 连接本地或远程 MCP 服务；用 `qcodex mcp` 管理服务；以 `exec --json` 接入 CI | **[OpenAI Codex]**：MCP/非交互执行基础；**[MCP]**：开放协议与第三方服务器生态 |
| 多代理协作 | 用 `/plan`、`/solve`、`/code` 或自定义子命令分派不同 CLI 代理，再汇总结果 | **[Every Code]**：协作编排工作流；**[qcodex]**：在当前代码线维护与适配 |
| Auto Drive | 用 `/auto <目标>` 或 `qcodex exec --auto` 推进多步骤任务；可查看状态、倒计时、审查与诊断 | **[Every Code]**：Auto Drive 工作流；**[qcodex]**：当前仓库的稳定性、生命周期和交互改进 |
| Skills | 在 `/skills` 中浏览或创建 Skills；输入 `$skill-name`，也可用 `/skill-name` 触发；用 `SKILL.md` 保存可复用方法 | **[OpenAI Codex]**：Skills 发现与指令模型；**[qcodex]**：斜杠补全、`/skill-name` 别名和终端管理界面 |
| 临时旁路讨论 | 用 `/btw` 从当前会话创建临时 side conversation，完成后返回主线 | **[qcodex]**：当前项目功能 |
| 终端体验 | 主题、设置面板、代理/Skills 管理、流式输出排序、原始推理可见性控制 | **[Every Code]**：TUI 与主题基础；**[qcodex]**：当前交互与稳定性改进 |
| 模型与供应商配置 | 使用 ChatGPT 登录或 API Key；配置 OpenAI 兼容的 Chat Completions / Responses 服务与多个 profile | **[OpenAI Codex]**：配置与提供商基础；**[qcodex]**：兼容层和本地集成 |

### 来源说明

- **[OpenAI Codex]**：来自或同步自 [openai/codex](https://github.com/openai/codex) 的 Apache-2.0 开源代码与设计。
- **[Every Code]**：来自 [just-every/code](https://github.com/just-every/code) 的社区分支能力和历史实现。
- **[qcodex]**：本仓库 `canwhite/qcodex` 的实现、改造或维护；不表示上游或 Every Code 的官方功能。
- **[MCP]**：来自 [Model Context Protocol](https://modelcontextprotocol.io/) 规范或第三方 MCP 服务；各服务的安全性、可用性与许可证由其作者负责。

这张表刻意区分“技术基线”和“本项目改造”：qcodex 不把上游已有能力包装为原创，也不暗示得到 OpenAI、Every Code 或 MCP 服务作者的背书。

## 快速开始

### 从源码构建并安装

前置条件：Rust 工具链，以及可写且已加入 `PATH` 的用户目录（例如 `~/.local/bin`）。

```bash
git clone https://github.com/canwhite/qcodex.git
cd qcodex
make

# 任意目录中启动
qcodex
```

`make` 会调用 `./build-fast.sh`，然后仅在确认目标目录由当前用户拥有、可写且不会覆盖未知同名命令时安装 `qcodex`。若没有合适的 `PATH` 目录，显式指定：

```bash
make INSTALL_DIR="$HOME/.local/bin"
```

从源码直接运行而不安装：

```bash
./build-fast.sh
./code-rs/bin/code
```

### 登录或配置模型

首次启动后可选择 ChatGPT 登录，或配置 API Key / OpenAI 兼容服务。配置默认位于 `~/.code/config.toml`；也可通过 `CODE_HOME` 或 `CODEX_HOME` 指向其他位置。

```toml
model = "your-model"
model_provider = "your-provider"

[model_providers.your-provider]
base_url = "https://example.com/v1"
env_key = "YOUR_PROVIDER_API_KEY"
wire_api = "chat" # 也可为 responses，取决于服务端兼容性
```

详见 [配置参考](docs/config.md)。请只使用你有权访问的模型服务，并遵守对应服务条款。

## 从一个任务开始

```text
# 普通编码任务
实现订单导出，补齐测试并说明验证方式

# 先并行调研再汇总方案
/plan 为现有鉴权模块设计无停机迁移方案

# 交给长流程，但保持可观察与可停止
/auto 重构缓存层并运行相关验证

# 在不离开主线程的情况下旁路追问
/btw 这个错误是否与刚才的迁移有关？
```

常用入口：

| 入口 | 用途 |
| --- | --- |
| `/skills` | 查看、编辑或新建用户 Skills；输入 `$name` 或 `/name` 使用对应 Skill |
| `/agents` | 配置可用代理及子代理命令 |
| `/auto settings` | 调整 Auto Drive 的代理、审查、诊断与继续策略 |
| `/settings` | 管理模型、审批、主题和终端偏好 |
| `qcodex exec "…"` | 非交互执行；`--json` 输出 JSONL 事件，适合自动化 |
| `qcodex mcp` | 添加、查看或移除 MCP 服务 |

## Skills：把团队工作法带进代理

Skills 是存放在 `SKILL.md` 中的可复用指令包。它适合将发布检查、设计评审、故障排查、代码规范等流程固化下来；项目级 `AGENTS.md` 则适合描述仓库约束与长期上下文。

1. 在 TUI 输入 `/skills`，创建或管理用户 Skill。
2. 在任务中输入 `$skill-name 你的任务`；`/skill-name 你的任务` 是 qcodex 提供的等价快捷写法。
3. 将与项目绑定的 Skills 放在仓库中并纳入版本控制，让团队共享同一套流程。

如果名称与内置命令、子代理命令或自定义 prompt 冲突，优先使用已有命令，避免意外改变工作流。

## MCP、子代理与 CI

### MCP

qcodex 可作为 MCP 客户端连接工具服务，也可通过 `mcp-server` 作为实验性 MCP 服务运行。以下是一个本地文件系统服务示例：

```toml
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/project"]
```

MCP 服务拥有的权限取决于其启动命令和配置。接入前请审查服务代码、令牌传递和可访问目录。

qcodex 还提供两个显式启用的浏览器自动化预设：

```shell
qcodex mcp add-preset playwright
qcodex mcp add-preset computer-use
```

Playwright 预设使用隔离的临时浏览器 profile，不会连接 qcodex 内置浏览器。Computer Use 预设会控制当前交互式桌面；它使用固定版本的社区 MCP 包，因此应先审查其来源，并仅在专用账号或可丢弃桌面中处理不可信任务。两者均沿用现有 MCP 的超时、取消和工具历史机制。

### 子代理

`/plan`、`/solve`、`/code` 是内置协作入口；你可以在 `config.toml` 定义代理命令和自定义子代理。qcodex 只负责调度本机可执行的代理 CLI，不替第三方代理、模型或命令的输出背书。

```toml
[[agents]]
name = "reviewer"
command = "/absolute/path/to/agent"
read_only = true
enabled = true

[[subagents.commands]]
name = "review"
read_only = true
agents = ["reviewer"]
```

更多示例请见 [Agents & Subagents](docs/agents.md)、[Auto Drive](docs/auto-drive.md) 和[高级用法](docs/advanced.md)。

## 安全与边界

- 默认优先选择最小权限：只读分析、受限工作区写入，再到明确授权的高权限自动化。
- `approval_policy = "never"`、`--full-auto` 或绕过沙箱的选项会显著扩大执行权限；仅在你已理解命令与工作区影响时使用。
- API Key、登录凭据和模型输入由你配置的服务处理；qcodex 不替任何模型提供商承诺数据处理方式。
- 使用 MCP、外部代理或自定义脚本前，先审查其权限、网络访问和许可证。

## 开发与贡献

Rust 源码在 `code-rs/`；`codex-rs/` 是上游镜像，除同步外不直接修改。提交前的唯一必跑构建检查是：

```bash
./build-fast.sh
```

请保持归属清晰：同步上游时保留原始许可证与 NOTICE；引入第三方代码、素材或协议实现时，在变更和文档中标出来源与许可证。贡献指南和项目约定见 [AGENTS.md](AGENTS.md)。

## 许可、归属与非关联声明

qcodex 以 [Apache License 2.0](LICENSE) 发布，且是 [OpenAI Codex](https://github.com/openai/codex) 的衍生项目。仓库保留适用的上游许可证与归属文件。

“OpenAI”“Codex”“Every Code”“Model Context Protocol”及相关名称可能是其各自权利人的商标或项目名称；除非另有明确说明，qcodex 与 OpenAI、Every Code、Anthropic、Google、MCP 服务作者或任何模型提供商均不存在隶属、赞助或背书关系。

---

qcodex：把代理能力留在你的终端，把工作方法沉淀在你的仓库。
