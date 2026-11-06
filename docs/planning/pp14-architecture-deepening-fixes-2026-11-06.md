# Plan: pp14 架构问题统一修复方案

> **User's request (restated)**: 校验 `painpoints/pp14.md` 中列出的架构问题是否存在；对确认存在的问题统一编写修复 plan。
>
> **What this plan answers**: 汇总四项代码核验结果，并给出可分阶段执行、可测试、可回滚的模块深化方案。
>
> Goal: 在不改变现有协议和用户可见行为的前提下，收拢四类跨层协调逻辑，形成明确的测试 seam。

## Context

对 `painpoints/pp14.md` 的四项候选逐项核验后，均有代码证据支持其存在，但它们属于架构可维护性问题，不是当前已确认的单一运行时 bug：

- **TUI turn-presentation：确认存在，优先级高。** `code-rs/tui/src/chatwidget.rs` 约 45k 行，`handle_code_event` 从约 13,890 行延伸到 16,176 行，单一事件分支同时处理 `EventMsg`、`OrderMeta`、stream、history、spinner、审批和 Auto Drive；同一文件还包含 replay、history 投影与渲染辅助逻辑。
- **App Server Conversation bridge：确认存在，优先级高。** `code_message_processor.rs` 同时承载请求路由、conversation 生命周期、listener 所有权、审批/用户输入回调和 auth/config RPC；`send_user_message`、未使用的 `send_user_turn`、`send_user_turn_compat` 存在重复的 conversation 查找、wire item 映射、提交和应答流程。
- **Invocation configuration：确认存在，优先级中高。** `ConfigOverrides` 有 22 个 optional 字段，CLI、TUI、app-server、MCP server 等多个 adapter 直接构造完整结构；最终 precedence、profile/provider 解析和 effective config 在 `core/config.rs` 中集中，但调用意图没有类型化表达。
- **TUI Auto Drive runtime：确认存在，优先级高。** Auto Drive 的状态转换分布在 `chatwidget.rs` 的事件处理、键盘/Esc 路由、coordinator 回调、history/transcript、review 和 spinner 逻辑中；`auto_state`、`auto_on_*`、`auto_stop`、`describe_esc_context`/`execute_esc_intent` 与底部 pane 共同维护同一生命周期。

当前工作区已有未提交改动，尤其涉及 `code-rs/tui/src/chatwidget.rs` 和 core provider profile 文件。本计划执行时必须先隔离这些改动，避免把既有工作混入重构。

## Goal

完成四个模块的渐进式深化，使：

1. 协议事件到 TUI 状态/历史投影可由独立事件序列测试验证。
2. Conversation 创建、turn 提交、订阅所有权、审批和用户输入转发集中在 bridge 内，兼容入口不再复制提交逻辑。
3. CLI/TUI/app-server/MCP adapter 表达 launch intent，由 core 配置层统一解析 precedence 和默认值。
4. Auto Drive 的状态机、用户 intent 和 presentation decision 有单一拥有者，Esc、pause、stop、review、countdown 的行为保持兼容。

## Plan

1. **建立基线与边界清单。**
   - 在实现前记录当前分支、工作区差异和四个模块的公开入口；不覆盖现有未提交改动。
   - 为每个模块列出输入事件/请求、状态变化、可见输出、异步任务和取消路径，先以现有函数和测试作为行为基线。
   - 确认模块命名、文件位置和依赖方向；只抽取已经存在的稳定边界，不为未来需求预留空泛接口。

2. **先抽取 TUI turn-presentation。**
   - 从 `handle_code_event` 中分离“协议事件分类与顺序元数据处理”以及“产生 history/stream/activity 投影”的决策层。
   - 保留 `ChatWidget` 的终端输入、渲染、AppEvent 发送和最终 widget mutation 职责；新模块通过明确的 input/event 与 projection/output 类型交互。
   - 迁移 Agent message/reasoning delta/final、tool/exec lifecycle、replay、TaskComplete、approval 和 strict stream-id/order 相关路径，逐类保持原有 ordering invariant。
   - 用事件 fixture 覆盖乱序/缺失 `OrderMeta`、空 stream id、late delta、replay、多个 assistant item、工具结束后 spinner 清理等边界。

3. **抽取 App Server Conversation bridge。**
   - 将 conversation 创建/resume、turn submit、listener 注册/移除、connection ownership、interrupt、approval、dynamic tool 和 user-input 转发归入 bridge 模块。
   - 把 `SendUserMessage` 与兼容 `SendUserTurn` 统一到一个“解析请求 → 映射输入 → 提交 Op → 应答”的内部流程；兼容字段的忽略策略必须显式记录并由测试锁定。
   - 将 auth、登录、saved config、rate limit 等无关 RPC 保留在外层 processor，仅通过 bridge 暴露 conversation 生命周期入口。
   - 明确 spawned callback 的 cancellation、listener 移除和 conversation drop 行为，避免 bridge 只搬运代码而继续泄漏所有权。

4. **深化 invocation configuration。**
   - 盘点 CLI、TUI、app-server、MCP server 和测试中所有 `ConfigOverrides { ... }` 构造点，按真实 invocation 语义分组。
   - 在 core 配置层定义有限的、面向调用意图的输入类型或 builder；adapter 只声明模型、cwd、权限、工具等自身可表达的意图。
   - 集中处理 profile/provider、project/config/CLI precedence、默认值、兼容字段和校验；不改变现有 `Config` 的最终语义。
   - 逐个迁移 adapter，删除重复的 22-field 机械初始化，并保留必要的低层入口给测试或内部特殊场景。

5. **抽取 TUI Auto Drive runtime。**
   - 将 Auto Drive lifecycle、coordinator/core event 输入、用户 intent、pause/stop/review/countdown 转换和 summary 状态集中到 runtime 模块。
   - 将 `ChatWidget` 变成 runtime 的宿主：负责把键盘/协议事件送入 runtime，并执行 runtime 返回的 command/presentation effect；不在 view 层新增 Esc handler。
   - 保持 `app.rs` 的 modal-first 路由和 `auto_coordinator_view.rs` 的 Esc passthrough 合同；迁移后继续由 `auto_should_handle_global_esc`/widget key handling 共同决定路由。
   - 用确定性状态序列覆盖 active、awaiting prompt、manual edit、paused、review、countdown、stop、cancel task、coordinator failure 和 late event。

6. **按模块完成行为等价迁移。**
   - 每次只迁移一个完整路径，先让旧路径与新路径的输出对齐，再删除旧分支；不进行跨四个模块的大批量重命名。
   - 对每个新模块执行 deletion test：移除模块后，若 ordering/lifecycle/config/conversation 规则会重新散落到多个调用点，才确认抽取提供了实际 locality。
   - 最后做全局扫描，清理残余重复映射、直接构造 `ConfigOverrides`、绕过 runtime 的 Auto Drive 状态修改和绕过 bridge 的 conversation 操作。

## Think — Debug Methodology

- 先读现有测试、调用入口和状态拥有者，再读实现细节；调试顺序为 protocol/app boundary → bridge/runtime state → history/view projection。
- 事件相关问题从 `handle_code_event`、AppEvent sink、conversation listener callback 和 coordinator callback 入口加日志；不要在每个业务分支盲目加日志。
- 使用现有的 `[order]`、Auto Drive diagnostics 和 tracing 约定；新增日志采用稳定字段，至少包含 conversation/turn/stream id、event kind、request ordinal 和 state transition。
- 先定位上游输入是否缺失/重复/乱序，再判断 projection 或 view 是否错误；对异步路径同时检查 task cancellation、channel close 和 owner connection。
- 全局检索重点：`EventMsg::`、`ConfigOverrides {`、`conversation_manager`、`conversation_listeners`、`auto_state`、`auto_stop`、`KeyCode::Esc`，确保迁移后没有 latent twin。

## Do — Verification Strategy

- **Build**: 从仓库根目录运行 `./build-fast.sh`；必须通过且无 warning。遵守仓库要求，不运行 rustfmt。
- **Static analysis / type check**: 以 `./build-fast.sh` 的 Rust 编译和检查结果为准；任何 error 或 warning 都必须修复后才能完成。
- **Runtime verification**: 不需要单独启动网络服务；使用现有 TUI/app-server 测试 harness 验证事件序列、listener 生命周期、兼容 turn 提交、配置 intent 解析和 Auto Drive 状态序列。若实现阶段需要额外的定向测试，必须明确记录其覆盖路径，并最终仍以 `./build-fast.sh` 作为完成门槛。
- **Logic correctness**: 手工逐条走查以下路径：新建/恢复 conversation、listener 跨 connection 移除、SendUserMessage/SendUserTurn、审批超时/断连、四类 config precedence、stream delta/final/task complete、replay、缺失 stream id、Auto Drive Esc modal-first、pause/resume/review/stop 和 late event。
- **Regression evidence**: 保留迁移前后的 snapshot/fixture 结果；重点验证 history 顺序、stream 合并、spinner 生命周期、审批键路由和最终 wire response 没有变化。

## Adjust — Rollback and Global Scan

- **Rollback plan**: 每个模块以独立小提交或可独立 revert 的变更集交付；若行为回归，先回滚最近一个模块迁移，保留已验证的 fixture 和前一模块结果，不回滚用户已有未提交改动。
- **Global scan**: 迁移后扫描全部 Rust adapter 和 TUI event handlers，确认没有重复的 wire-to-core item mapping、手工 conversation submit、完整 `ConfigOverrides` 复制、直接修改 Auto Drive 状态或额外 Esc handler。
- **Backwards compatibility**: 保留现有 JSON-RPC 方法、协议事件、配置键和 wire response；`SendUserTurn` 的兼容字段行为显式保持当前语义。对配置新入口提供内部兼容转换，避免一次性改动所有调用者。
- **Scope control**: 如果某个模块需要改变协议 schema、持久化格式或用户可见文案，应拆成单独计划，不在本次架构深化中隐式扩大范围。

## Open Questions

- 四个模块是否应按本文顺序串行落地，还是先将 Auto Drive runtime 与 turn-presentation 合并为一个 TUI 重构批次，需要在基线盘点后依据依赖图决定。
- `SendUserTurn` 的被忽略字段是否仅保留兼容注释，还是应在 bridge 内返回显式能力/校验错误，需要结合现有客户端兼容矩阵确认。
- invocation intent 的最终类型应放在 `core` 还是共享 protocol/common crate，需要根据依赖方向避免 core 反向依赖 adapter。
- 是否将 bridge 的审批/用户输入 callback 统一为一个 request/response abstraction，取决于现有 wire 类型差异；在未证明能减少复杂度前不强行合并。

## Out of Scope

- 不在本计划内改变协议 schema、JSON-RPC 方法名、配置键语义或 Auto Drive 用户交互设计。
- 不重写 history cell、渲染器、conversation core 或 provider profile 的业务逻辑；仅调整其被协调和投影的边界。
- 不顺带修复与 pp14 无关的 provider profile、配置中心或其他工作区改动。
- 不创建临时工作文档，不在计划阶段实现代码。

---

> Next step: run `/pre-mortem docs/planning/pp14-architecture-deepening-fixes-2026-11-06.md` to identify failure modes and harden this plan.
