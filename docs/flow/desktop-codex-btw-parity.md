🥷

# Desktop Codex `/btw` flow and local parity gap

## 📊 流程概览

本次以 `/Users/doing/Desktop/codex` 的 `main`（`fbf5f0d3a7`）作为桌面 Codex 事实基线，以当前工作区 `/Users/doing/Desktop/qcodex`（`cfa9459c0` 加未提交改动）作为本地基线。OpenAI 官方文档没有公开 `/btw` 的生命周期细节，因此以下结论来自两边真实源码、测试和提交历史，不把旧方案文档当作事实来源。

桌面 Codex 中 `/btw` 是 `/side` 的隐藏别名：命令创建一个继承父线程上下文和运行配置、但具有新 thread id 的 ephemeral fork。父线程和侧线程同时存活，`Ctrl+/` 在两者之间切换；侧线程可观察父线程“需要输入、需要审批、完成、失败”等状态；空 composer 下按 `Ctrl+C` 才会关闭侧线程并回到父线程。开启 `tui_workspace`、使用 owned transcript 且终端宽度至少 100 列时，两条线程还能以父宽侧窄的双栏形式同时显示。

当前项目已经实现了临时 fork、隐藏继承 transcript、父任务后台推进、侧线程 `Ctrl+C` 返回和临时 rollout 清理，但状态模型仍是一次性 `Idle → Forking → Active → Returning → Idle`。进入 `/btw` 后只能销毁侧线程返回，不能保留父/侧两端并来回切换，也没有父状态、双栏工作区、失败后保留侧线程等桌面行为。

## 🔀 桌面 Codex 主流程

```text
用户输入 /side 或 /btw [message]
              │
              ▼
┌──────────────────────────┐
│ 解析命令并保留完整 UserMessage │
│ 文本 / mention / 图片          │
└─────────────┬────────────┘
              │
              ▼
┌──────────────────────────┐
│ 校验当前 thread / review /     │
│ permission update / 已有 side  │
└───────┬───────────┬──────┘
        不允许│           │已有 side
             ▼           ▼
       恢复 draft     切回已有 side
       显示错误        并提交新消息
                         │
              新建 side  │
                         ▼
┌──────────────────────────┐
│ 复制当前 model / effort / tier │
│ permission / workspace roots   │
│ 设 ephemeral，关闭 daybreak    │
│ 追加 side developer guardrail  │
└─────────────┬────────────┘
              │
              ▼
┌──────────────────────────┐
│ app-server thread/fork         │
│ fork 失败：恢复 draft 和父界面   │
└─────────────┬────────────┘
              │成功
              ▼
┌──────────────────────────┐
│ UI snapshot 不回放继承 transcript│
│ 注入隐藏 boundary user message │
└─────────────┬────────────┘
              │
              ▼
┌──────────────────────────┐
│ 注册 child → parent 关系        │
│ 切到 child，提交可选首条消息      │
└─────────────┬────────────┘
              │
       ┌──────┴─────────────────────┐
       ▼                            ▼
┌───────────────┐       ┌────────────────────┐
│ child 正常对话      │       │ parent 后台继续收事件    │
│ 限制 rename/voice/  │       │ 汇总 input/approval/    │
│ worktree/嵌套 side  │       │ finished/failed 状态    │
└───────┬───────┘       └─────────┬──────────┘
        │                             │
        └─────────────┬───────────────┘
                      ▼
        ┌────────────────────────┐
        │ Ctrl+/：保留两端并切换焦点   │
        │ 宽屏+tui_workspace：双栏显示 │
        └────────────┬───────────┘
                      │侧线程空 composer 下 Ctrl+C
                      ▼
        ┌────────────────────────┐
        │ 切回 parent，再后台 interrupt│
        │ + unsubscribe + 清本地状态   │
        └────────────┬───────────┘
                      │
                      ▼
        side 从本地导航移除；RPC 失败记日志

准备阶段若 boundary 注入或切换失败：
同步关闭成功 → 恢复 parent；同步关闭失败 → 保留 side 可见并报告错误
```

## 🔀 当前项目主流程

```text
/btw [message]
      │
      ▼
校验 parent session，进入 Forking
      │
      ▼
从 SessionCatalog 找 rollout 文件
      │
      ▼
复制 rollout + 追加简化 boundary
创建临时 child conversation
      │
      ▼
用新 ChatWidget 替换当前 widget
原 parent widget 放入 Active 状态
      │
      ├── parent 的未标记 CodexEvent → parent widget
      └── child 的 session-tagged event → 当前 widget
      │
      ▼
Ctrl+C（无 modal、空 composer）
      │
      ▼
立即恢复 parent，停止 child forwarder
后台 Interrupt + Shutdown + 删除临时 rollout
      │
      ▼
Returning → cleanup finished → Idle
```

## 差距矩阵

| 能力 | 桌面 Codex | 当前项目 | 结论 |
|---|---|---|---|
| 命令表面 | `/side` 可见，`/btw` 是隐藏别名 | 只公开 `/btw` | 保留本项目 `/btw` 品牌即可，不必新增 `/side` |
| fork 来源 | app-server 对活 thread 做 `thread/fork` | 从 session catalog 重新读 rollout | 当前可能受落盘时序影响，且靠“父任务运行时丢一条消息”规避边界 |
| 配置继承 | model、effort、tier、权限、workspace roots、developer instructions | 克隆 widget config + user instruction boundary | 缺显式、可测试的继承契约；guardrail 比桌面简化 |
| 历史可见性 | 模型继承，UI 从 boundary 后开始 | 模型继承，新 widget UI 为空 | 基础已对齐 |
| 双向切换 | `Ctrl+/` 切 parent/side，二者都存活 | 只能 `Ctrl+C` 销毁 side 返回 | 核心差距 |
| 父线程后台状态 | 显示需输入、需审批、完成、失败、关闭 | 父事件会推进，但 side UI 不显示状态 | 核心差距 |
| 事件路由 | 所有 server 事件按 thread id 路由 | parent 主要靠未标记 `CodexEvent`，child 才带 id | 切换焦点前必须先统一路由 |
| 清理失败 | 正常返回走后台清理；准备失败的同步清理若失败则保留 side 可见 | 先恢复 parent，超时后只提示 detached | 基础接近，但缺准备失败的可恢复路径与 unsubscribe 语义 |
| 导航留存 | 切到其他 thread 后 side 仍可恢复 | resume/cwd/jump-back 在 side 期间被禁用 | 明显差距 |
| 命令限制 | 默认拒绝，仅显式 allowlist；另禁 rename、voice、daybreak、worktree attachment、多 agent | 有 allowlist，但允许 `/init`、`/compact`、`/cmd`、多 agent prompt 等较宽能力 | 需按本项目命令逐项复核 |
| 双栏工作区 | `tui_workspace` opt-in，owned transcript，宽度 ≥100 | 无对应 feature 和 companion transcript | 增强差距，可在基础 parity 后实现 |
| 生命周期测试 | 覆盖 fork、重放、状态、审批、导航、切换、清理、宽屏快照 | 覆盖解析、基础隔离、并发 parent、返回和清理 | 缺会话对与显示层回归测试 |

## 🎯 核心方法

### 桌面 Codex

| 方法 | 职责 | 位置 |
|---|---|---|
| `ChatWidget::request_side_conversation()` | 从 slash command 发出带完整 `UserMessage` 的 `StartSide` | `/Users/doing/Desktop/codex/codex-rs/tui/src/chatwidget/slash_dispatch.rs:107` |
| `App::handle_start_side()` | 校验、fork、注入 boundary、注册、切换并提交首条消息 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:738` |
| `App::side_fork_config()` | 继承运行配置并追加 side guardrail | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:660` |
| `AppServerSession::fork_side_thread()` | 以 ephemeral side presentation 调用 app-server fork | `/Users/doing/Desktop/codex/codex-rs/tui/src/app_server_session.rs:1192` |
| `App::install_side_thread_snapshot()` | 隐藏继承历史，只让模型继承上下文 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:708` |
| `App::sync_side_thread_ui()` | 生成 parent/side 标签、父状态和快捷键提示 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:246` |
| `App::toggle_side_conversation()` | 在 parent 与 side 之间切换而不销毁任何一端 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:429` |
| `App::maybe_return_from_side()` | 在 modal 和 draft 均为空时用 `Ctrl+C` 关闭 side | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:397` |
| `App::discard_side_thread_in_background()` | 后台 interrupt、unsubscribe 并回收本地状态 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side.rs:478` |
| `side_pane::layout()` | feature 开启且宽屏时计算父/侧双栏布局 | `/Users/doing/Desktop/codex/codex-rs/tui/src/app/side_pane.rs:25` |

### 当前项目

| 方法 | 职责 | 位置 |
|---|---|---|
| `ChatWidget::side_initial_message()` | 去掉 `/btw` 前缀并保留剩余消息内容 | `code-rs/tui/src/chatwidget.rs:30074` |
| `ConversationManager::fork_conversation()` | 从 rollout 创建临时 fork 并返回清理 token | `code-rs/core/src/conversation_manager.rs:200` |
| `ConversationManager::dispose_ephemeral_conversation()` | interrupt、shutdown、移除 manager 记录和临时文件 | `code-rs/core/src/conversation_manager.rs:288` |
| `ChatWidget::new_from_existing()` | 为 child conversation 创建空历史 widget 和带 session id 的事件 forwarder | `code-rs/tui/src/chatwidget.rs:7299` |
| `ChatWidget::set_side_conversation_mode()` | 禁用 ghost snapshot 并显示简单 side 状态 | `code-rs/tui/src/chatwidget.rs:25824` |
| `ChatWidget::side_return_shortcut_available()` | modal 关闭且 composer 为空时允许 `Ctrl+C` 返回 | `code-rs/tui/src/chatwidget.rs:25848` |
| `App::begin_side_cleanup()` | 监督 child forwarder 和临时 conversation 清理 | `code-rs/tui/src/app/events.rs:208` |

## 📚 概念解析

### Ephemeral fork（临时分叉）
- **是什么**：从父对话复制上下文、但使用新 id 的短命对话。
- **为什么需要**：像从主笔记复印一页做草稿，能问旁支问题又不污染主线。
- **怎么用**：模型可以读继承历史，但 side 的新消息、状态和 rollout 不能合回父线程。

### Boundary prompt（边界提示）
- **是什么**：放在继承历史之后、side 新请求之前的隐藏指令。
- **为什么需要**：明确告诉模型“前面的任务只是参考，不要继续执行”。
- **怎么用**：边界必须进入模型上下文但不能显示在 transcript，也不能写回父 rollout。

### Session-scoped routing（按会话路由）
- **是什么**：每个事件都携带 thread/session id，由 App 投递给对应 widget。
- **为什么需要**：父、侧线程同时运行时，不能再靠“当前哪个 widget 可见”猜事件归属。
- **怎么用**：切换、延迟事件和清理阶段都必须校验 id，未知或已关闭 id 的事件直接隔离。

### Companion transcript（伴随视图）
- **是什么**：未聚焦线程的 transcript、draft、滚动和状态快照。
- **为什么需要**：切换焦点或双栏显示时，用户能看到另一端又不会让两个 composer 同时接收输入。
- **怎么用**：只有聚焦线程拥有光标和交互，伴随视图只绘制，不处理按键。
