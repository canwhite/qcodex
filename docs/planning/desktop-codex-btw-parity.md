# Plan: Replicate Upstream Codex TUI `/btw` Semantics

> **User's request (restated)**: 先调研 `/Users/doing/Desktop/codex` 中上游 Codex TUI 的 `/btw` 完整流程，再检查当前项目的 `/btw`、找出差距，并写出复刻上游 Codex TUI `/btw` 的实施方案；持续校验并加固方案，直到未缓解的 High risk 为 0。
>
> **What this plan answers**: 如何在保留本项目 `/btw` 命令名和 direct-core 架构的前提下，复刻上游 Codex TUI 当前 side-conversation 的生命周期、路由、切换、状态提示、清理和可选双栏体验，并用可执行约束把残余 High risk 降为 0。
>
> Goal: `/btw` 成为一个与父会话并行存活、可来回切换、状态可观察、可安全关闭且不会污染父会话的 ephemeral conversation，并在可选宽屏模式下显示父/侧双栏。

## Context

调研基线与完整差距见 [`docs/flow/desktop-codex-btw-parity.md`](../flow/desktop-codex-btw-parity.md)。上游 Codex TUI 源码基线是 `/Users/doing/Desktop/codex` 的 `fbf5f0d3a7`；当前项目基线是 `cfa9459c0` 加工作区现有未提交改动。这里的 `/Users/doing/Desktop/codex` 是本机目录名，不是 Codex 桌面 App。

当前实现不是从零开始：`ConversationManager::fork_conversation()` 已能创建临时 rollout，`App` 已有 `Idle/Forking/Active/Returning` 状态，父任务在 side 可见期间也能继续处理事件。方案不推翻这些成果，而是把“一次性替换 widget”升级成“由 App 持有的 parent/side conversation pair”。

上游 Codex TUI 已全面使用 app-server thread APIs，而本项目 TUI 仍直接驱动 `ConversationManager`。本方案追求行为等价，不把迁移整个 TUI 到 app-server 纳入 `/btw`；但会借鉴上游 TUI 的核心约束：统一按 session id 路由、模型继承而 UI 不回放、两个会话同时存活、关闭失败不伪装成功。

默认产品取舍：继续只公开 `/btw`，不新增 `/side`；`Ctrl+/` 用于 parent/side 切换，侧线程空 composer 下 `Ctrl+C` 用于关闭；双栏放在默认关闭的 `tui_workspace` feature 后，与上游 TUI 一致。

## Goal

完成后应满足：

- `/btw` 与 `/btw <message>` 保留文本、mention、图片和 ordered items；父会话未开始时给出明确错误并保留 draft。
- side 继承父会话稳定上下文以及当前 model、reasoning、service tier、权限、cwd 和 workspace 配置；继承 transcript 不显示在 side UI。
- parent 与 side 同时存活，任一端运行时都能用 `Ctrl+/` 切换；切换不会 interrupt、重建或丢失 draft、滚动位置和 streaming cell。
- side 可见时，父任务继续推进，并显示“需要输入、需要审批、完成、失败、中断”等摘要状态。
- side 中默认禁止会改变会话归属或造成嵌套生命周期的命令；明确要求的普通文件修改仍遵循原 sandbox/approval 策略。
- 空 composer 且无 modal 时，side 内 `Ctrl+C` 回父线程并后台关闭 side；准备阶段若关闭失败，必须保留可恢复状态或显示可重试错误。
- side 不出现在 resume 列表、不允许 rename、不保留 worktree attachment、不创建 sub-agent，并在返回、退出、父会话切换和异常路径中无后台孤儿任务或临时 rollout。
- 开启 `tui_workspace`、owned transcript 且宽度至少 100 列时，父线程和 side 以双栏显示，只有聚焦栏接受输入。

## Delivery Gates

- **Gate 0 — Clean baseline**: 在任何 `/btw` 功能修改前，保存当前 dirty-worktree 清单，并让原始基线的 `./build-fast.sh` 零错误、零 warning。当前已知基线失败为 `code-tui` 14 个错误和 1 个 warning；这些修复必须独立归因、独立 review，不能混入 `/btw` diff。
- **Gate 1 — Session routing**: 所有 conversation event 都带 session id；删除或封死无 id 的 parent 默认路由。事件路由测试通过后才能改 side 焦点。
- **Gate 2 — Core ownership and fork**: parent/side 共用一个 manager、每个 conversation 只有一个 event pump、一致性 snapshot barrier 可证明；未达到时 `/btw` fail closed。
- **Gate 3 — Single-pane parity**: 状态机、`Ctrl+/`、父状态、审批隔离、capability policy 和安全 cleanup 完成；这是可独立交付的基础版本。
- **Gate 4 — Navigation retention**: 只有通用 conversation registry 能在 resume/cwd/navigation 后恢复 pair 时才开放留存；否则继续明确拒绝导航，不能做半留存。
- **Gate 5 — Optional dual pane**: `tui_workspace` 默认关闭；只有单栏 parity 稳定后才加入 transcript companion 和宽屏渲染。

## Plan

0. **先恢复可归因的绿色基线。** 记录 `git status --short`、当前 HEAD 和 `./build-fast.sh` 的既有失败；在独立变更中修复 `code_otel` 链接、private module 引用和 warning，或等待这些用户改动完成。基线未绿时禁止开始 `/btw` 实现，也禁止把既有错误标成 `/btw` 回归。

1. **先写 parity contract 测试并冻结交互语义。** 在 `code-rs/tui` 增加 table-driven 测试，覆盖 `/btw` 空参数、inline 参数、mention/图片保留、未启动 session、review/嵌套 side、active parent turn、`Ctrl+/` 切换、`Ctrl+C` modal-first/draft-first、side 内命令 allowlist。每个 slice 先运行测试确认红，再实现到绿。将 `docs/slash-commands.md` 的陈旧 `Ctrl+\\` 和“idle 后才能返回”更新为实际 `Ctrl+C`/`Ctrl+/` 语义。

2. **统一所有主/侧 conversation 事件的 session envelope。** 把 parent runtime 当前发送的 `AppEvent::CodexEvent(Event)` 改为与 child 一致的 `CodexEventForConversation { session_id, event }`，或引入等价的 `ConversationEvent` 单一变体；普通新会话、resume、fork 和 side 必须原子迁移到同一路由。App 根据 session id 投递给 parent 或 side widget；在 `Forking` 缓冲仅属于待创建 child 的早到事件；在 cleanup 后用 tombstone 丢弃已关闭 id 的迟到事件。加编译期穷举 match 和测试断言，迁移结束后全局搜索必须不存在业务路径使用无 id `CodexEvent`。完成这一步前不实现焦点切换，因为现有“未标记事件默认属于 parent”的约定在 parent 可见时会串流。

3. **把生命周期升级为 conversation pair 状态机。** 用一个集中 owner 替代 `Active { parent: Option<Box<ChatWidget>>, ... }` 的单向模型，建议状态为 `Idle`、`Forking`、`Open { parent, side, focused, metadata }`、`Closing`、`ClosingFailed`。`Open` 同时持有两个 widget、两个 session id、线性 cleanup owner、parent status 和各自 draft/display 状态；cleanup token 只能被 `take()` 一次，所有 transition 通过返回 `Result<NextState, TransitionError>` 的方法执行，禁止事件分支直接拼装半状态。保持 request id、generation 和 session id 三重校验，确保取消、超时、重复 close 和 stale fork result 不能替换新会话或二次清理。

4. **让 parent 与 child 使用同一个 ConversationManager 所有权域。** 当前普通 `ChatWidget` 在 `spawn_conversation_runtime()` 内部新建 manager，而 App 的 `_server` 只管理 side，这使 live fork、枚举和统一清理都依赖磁盘 catalog。把 `Arc<ConversationManager>` 由 App 注入普通 widget runtime，让 parent/child 都由同一 manager 注册；明确“manager 只持有 conversation，唯一 event-pump task 才能调用 `next_event()`”，禁止 App、widget 和 cleanup 同时消费同一 receiver。先以不改变普通 new/resume 行为的注入式构造完成迁移，再开放 fork API；为 manager 增加面向 ephemeral fork 的窄 API，不把内部 map 或 cleanup path 暴露给 TUI。

5. **替换“读 catalog + 猜测丢一条消息”的 fork 边界。** 在 core 增加从已注册 parent conversation 获取一致性历史快照的序列 barrier：请求方拿到 `captured_sequence` 和不可变 items，child 只继承不超过该序列的完整 item，不能出现半个 tool call/response pair。删除 `usize::from(parent_task_running)` 这种按运行状态截断最后消息的启发式。fork API 要显式返回 child session、cleanup token、captured sequence 和 boundary version，并保证取消时临时 rollout guard 回收文件。若 core 暂时只能从 rollout fork，则必须增加 flush/ack 和 sequence 校验；超时或序列不一致时 `/btw` fail closed 并恢复 draft，禁止回退到可能陈旧的 catalog 文件。

6. **对齐 side bootstrap 和 guardrail。** 将上游 TUI 的双层约束移植到本项目协议：一层追加到 developer/user instructions，另一层作为继承历史之后的隐藏 boundary item。为 child config 增加不可伪造的 `SessionKind::Side`（或等价内部标记），让 core/tool registry 强制禁用 worktree attachment、sub-agent 和 side 不支持的后台自动化，不能只依赖 prompt。明确 inherited history 仅供参考、旧任务/工具/审批无效、默认只读探索、用户在 side 中明确要求时才允许最小修改。新 widget 只渲染 boundary 后的新内容；首条 `/btw` 消息必须是正常 `UserInput`，不能因隐藏 developer op 产生额外 turn。

7. **实现非破坏性的 `Ctrl+/` 双向切换。** 在统一 keymap registry 中新增可配置的 `toggle_side_conversation` action，默认 `Ctrl+/`，启动时检测冲突并兼容终端可能上报的 `Ctrl+7` 等价键码。输入优先级固定为 terminal overlay → modal/approval → non-empty composer → side toggle/close →普通 widget → app exit，并用同一张 table 测试 `Ctrl+/`、`Ctrl+C`、Esc 和 Vim insert mode。切换只改变 `focused` 并交换 active display，不停止 forwarder、不销毁 widget。父视图显示“`Ctrl+/` for side”，侧视图显示来源、父状态、“`Ctrl+/` to switch”和“`Ctrl+C` to close”。

8. **补齐后台 parent 状态与交互请求。** 所有 request-user-input/approval 以 `(session_id, request_id, turn_id)` 为键存入独立队列，响应 API 必须携带同一三元组，禁止“回答当前 modal”式全局入口。从 parent 的 turn lifecycle、request-user-input、approval、failure、interrupt 和 completion 事件归纳 `SideParentStatus`，优先级按“input > approval > terminal status”。side 可见时不弹出 parent modal，只更新标签；切回 parent 后再恢复待处理 modal，且启动一帧输入 quarantine，避免用户为 side 输入的按键误答 parent 审批。parent 与 side 的 Auto Drive、approval 和 background notices 必须按 session id 分流。

9. **收紧 side capability 和共享工作区策略。** 以“默认拒绝、逐项放行”为原则审计本项目所有 `SlashCommand`、快捷键、直接 AppEvent、tool registry 和 MCP mutation 入口。至少禁止 `/new`、`/clear`、`/resume`、`/rename`、`/goal`、`/auto`、`/branch`、`/merge`、`/push`、`/undo`、`/settings`、`/login`、嵌套 `/btw`、jump-back、cwd 切换、ghost snapshot、自动 review 和 sub-agent 启动；只放行明确只读或局部 turn 操作。引入按 workspace root 键控的单写者 lease：parent 或 side 任一端持有 mutating tool/patch/git lease 时，另一端的 mutation 必须排队或 fail closed；read-only 检查可并发。popup 过滤、手输命令拒绝和 core tool admission 共享同一 capability descriptor，UI 不能成为唯一安全边界。

10. **重做关闭与退出语义。** side 内 `Ctrl+C` 只有在 overlay/modal 关闭且 composer 为空时才触发 close；触发后立刻把 child 标成 `Closing`、关闭新 submission、发出 interrupt，并释放/等待其 workspace mutation lease。parent 可以先恢复可见，但在 child 收到 terminal ack 或超时隔离前显示“closing side”且禁止新 `/btw`，避免用户误以为后台 mutation 已停止。随后后台 shutdown child、移除 manager entry 和 rollout，并把 RPC 失败写入诊断。若 boundary 注入、首次切换等准备阶段失败，则走同步关闭：关闭失败时保留 side 或进入 `ClosingFailed`。App exit、fatal exit、fork cancellation 和 parent replacement 都调用同一线性 cleanup owner；event pump 有界终止后才释放 conversation，迟到 child event 由 tombstone/session id 隔离。

11. **用通用 registry 实现导航留存，再做可选双栏。** 建立 `ConversationWorkspace` map，以 session id 存 live widget/event pump/display snapshot，以 side id 存 parent id；new/resume/cwd/navigation 统一通过 registry 切焦点，不能只为 `/btw` 增加散落缓存。未达到 Gate 4 时继续明确拒绝这些导航。达到 Gate 4 后，parent/side pair 可临时离焦并恢复，关闭 parent 时必须级联关闭 side。随后增加默认关闭的 `tui_workspace` feature：仅在 owned transcript 且宽度 ≥100 时启用；父栏占剩余宽度，side 栏至少 36 列、约三分之一宽；非聚焦栏绘制 retained transcript、stream、draft 和 status，但不拥有光标、modal 或按键。

12. **覆盖完整状态矩阵并清理旧兼容代码。** 增加 core 测试验证一致性 fork、完整 tool pair、boundary 顺序、取消 guard、单写者 lease 和 cleanup exactly-once；App 测试验证早到/迟到事件、stale request、父子并发、切换中 streaming、session-scoped approval、导航和 cleanup failure；`make_chatwidget_manual()`/VT100 snapshot 验证标签、命令 popup、窄屏单栏、宽屏双栏、resize、draft 与滚动恢复。每个 milestone 都必须通过自己的 focused tests，最终再跑唯一完成门禁 `./build-fast.sh`。最后全局扫描 `CodexEvent`、`CodexEventForConversation`、`ConversationManager::new`、`SlashCommand`、所有 tool/MCP admission、`SwitchCwd`、`ResumeFrom`、jump-back、approval、Auto Drive、exit 和 cleanup 分支，删除旧的无 id 路由与一次性 Active 假设。

## Think — Debug Methodology

- 在 framework boundary 临时增加统一 `[DEBUG-btw]` 结构化日志：slash admission、parent snapshot/flush、fork request/result、boundary install、首条 user input、事件 route、focus switch、parent status change、close/cleanup/tombstone。只记录 request id、session id、状态和事件种类，不记录用户文本、图片路径或 rollout 内容。
- 先读 `code-rs/core/src/conversation_manager.rs`、`code-rs/tui/src/app/state.rs`、`code-rs/tui/src/app/events.rs` 和 `ChatWidget` 两种 runtime 构造路径，再改事件流；不要在 UI 分支里猜 core 是否已经 flush。
- 上游从“事件是否带正确 session id”开始定位，再查“状态 owner 是否仍持有目标 widget”，最后才查渲染；这样可以区分串流、生命周期和纯 UI 问题。
- 对 fork/cleanup 卡住使用有界测试 clock 和可控 fake conversation；禁止靠 sleep 证明顺序。日志验证完毕后全局搜索并移除 `[DEBUG-btw]`。

## Do — Verification Strategy

- **Baseline gate**: 在任何 feature diff 前运行一次 `./build-fast.sh` 并保存成功结果；若失败，停止 `/btw` 工作并单独修复/等待基线，不能用 feature patch 掩盖失败。
- **Focused red-green checks during implementation**: core slice 使用 `cargo test -p code-core conversation_manager`；TUI state/routing slice 使用 `cargo test -p code-tui --features test-helpers btw`；VT100 slice 使用 `cargo test -p code-tui --test vt100_chatwidget_snapshot --features test-helpers -- btw`。每条新回归测试必须先在未修复代码上失败，再在对应 slice 中变绿。
- **Build**: `./build-fast.sh` — 仓库唯一必需完成检查，必须零错误、零 warning。
- **Static analysis / type check**: 由 `./build-fast.sh` 的 workspace 编译门禁承担；按仓库规则不额外运行 rustfmt、clippy 或全量 lint。
- **Runtime verification**: 启动刚构建出的准确 binary，记录同一 PID；主线程启动一个持续任务，执行 `/btw 只回复 side-ok`，确认 side 回复；用 `Ctrl+/` 往返并观察父任务继续推进；制造 parent input/approval 并确认 side 只显示状态、切回后才出现 modal；side 内空 composer 按 `Ctrl+C` 关闭，确认父 transcript/draft/滚动位置不变且临时 rollout 被删除。
- **Runtime mutation interlock**: parent 运行写文件/patch/git 操作时，让 side 明确请求另一个 mutation，确认第二个写入排队或被拒绝；反向再测一次。返回 parent 后确认 child 已进入 `Closing`、不能再提交工具调用，且 mutation lease 最终释放。
- **Runtime verification（布局）**: feature 关闭、窄于 100 列、宽于等于 100 列各验证一次；宽屏仅聚焦栏出现光标，resize 不丢 stream、draft 或滚动位置。
- **Logic correctness**: 手工走通空/带参数 `/btw`、图片/mention、未启动 parent、active parent、fork timeout/panic、早到 child event、重复 `/btw`、两端各自运行、modal/draft shortcut ownership、cleanup success/failure/timeout、退出时关闭、导航离焦再返回、迟到事件隔离。
- **Transition proof**: 为 `Idle/Forking/Open/Closing/ClosingFailed` 建 transition matrix；每个输入事件必须断言 next state、cleanup owner 是否被消费、parent/side widget 是否仍被持有、返回值是否向用户可见。非法 transition 必须返回错误且不改变状态。
- **Completion discipline**: focused tests 属于实现过程的 red-green 证据；完成时不再追加其他格式化/lint/全量测试命令，最终以仓库规定的 `./build-fast.sh` 为唯一完成门禁。

## Adjust — Rollback and Global Scan

- **Rollback plan**: 每个 Delivery Gate 单独提交且只依赖前一 gate。Gate 5 可只关闭 `tui_workspace`；Gate 4 可恢复为明确拒绝导航；Gate 3 可隐藏 `/btw` admission；Gate 2/1 作为通用正确性基础不做运行时双轨。任何回滚仍调用统一 cleanup owner 回收已创建 child，不能退回“恢复 parent 后把仍运行的 child 留在后台”的半回滚。
- **Global scan**: 搜索所有无 session id 的 conversation event、所有直接替换 `AppState::Chat` 的路径、所有 slash/shortcut capability gate，以及所有 `ConversationManager` 构造点；同类事件必须统一迁移，不留只对 `/btw` 特判的旁路。
- **Backwards compatibility**: 保持 `/btw` 为唯一公开命令、保持父 rollout/schema 不变、旧配置无需迁移；新增 keymap/feature 使用默认值。临时 side 仍不进入 resume catalog。若事件 enum 变更影响 session log，提供一次向后兼容反序列化或明确只修改进程内事件。

## Open Questions

- 已决策：上游 Codex TUI 当前公开 `/side`、隐藏 `/btw`；本项目继续只公开 `/btw`。未来若要命令表面完全一致，另开独立变更加入 `/side` alias 和 popup 排序。
- 已决策：本项目按 direct-core 架构实现行为等价，不在本任务迁移整个 TUI 到 app-server。若 app-server TUI 迁移先于 Gate 2 落地，本方案在 Gate 2 处停止并改为直接移植上游 `App::handle_start_side()` 路径。
- 已决策：`tui_workspace` 双栏是 Gate 5、默认关闭，不阻塞 Gate 3 的单栏 `/btw` parity 发布。

## Out of Scope

- 把整个本项目 TUI 从 direct-core 重写为 app-server client。
- 让 side 会话持久化、可 resume、可 rename，或把 side transcript 合并回 parent。
- 支持嵌套 `/btw`、side 内 sub-agent、side worktree attachment 或多个 side 同时绑定同一 parent。
- 改造与 `/btw` 无关的通用 TUI 视觉风格、agent picker 或 keymap 系统。

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] 红色基线让所有回归结论失去归因

**Severity**: 5 | **Likelihood**: 5 | **Detectability**: 0.1  
**Risk Score**: 22.5 — HIGH RISK

**Failure Scenario**: 当前工作区的 `./build-fast.sh` 已有 14 个错误和 1 个 warning。团队在此基础上继续开发 `/btw`，最后无法判断失败来自旧改动还是新生命周期代码，并被迫把无关修复混进同一个大 diff。

**Mitigation**:
- Gate 0 要求 feature work 前先恢复零错误、零 warning 基线，并记录 HEAD、dirty files 和构建结果。
- 既有构建修复独立提交、独立 review；基线未绿时 `/btw` 工作 fail closed。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 1 × (1 - Detectability 0.9) = 0.5 — not HIGH.

### [Risk] 部分事件仍无 session id 导致父子 transcript 串流

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 16.0 — HIGH RISK

**Failure Scenario**: 只迁移模型输出事件，遗漏 Auto Drive、approval、background review、browser 或 rate-limit 事件；切换 parent/side 后，事件落入当前可见 widget，污染 transcript 或触发错误状态。

**Mitigation**:
- Gate 1 原子迁移所有 conversation event 到统一 session envelope，并用穷举 match 覆盖事件类别。
- 完成时全局搜索并禁止业务路径使用无 id `CodexEvent`；测试 parent/side 同时 streaming 和迟到事件。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.9) = 1.0 — not HIGH.

### [Risk] 同一 conversation 被多个 event pump 并发消费

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: central manager、ChatWidget 和 cleanup task 都可能调用 `next_event()`；事件被随机消费者拿走，表现为丢 token、丢 `ShutdownComplete` 或 cleanup 永久等待。

**Mitigation**:
- Gate 2 明确每个 conversation 只有一个 event-pump owner；manager 只注册 handle，不消费 event。
- 用测试计数 event-pump 创建/销毁次数，并让重复 attach 返回错误而不是创建第二个 reader。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] manager 注入迁移破坏普通 new/resume 会话

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: 为 `/btw` 统一 manager 时改变所有 ChatWidget 构造和 shutdown 顺序，导致普通新会话、resume、登录切换或 App 退出出现双注册、泄漏或无法收事件。

**Mitigation**:
- Step 4 先做行为不变的 dependency injection slice，普通 new/resume 测试全部绿后才开放 ephemeral fork。
- registry 以 session id 拒绝重复注册；退出测试断言每个普通会话也恰好一次 shutdown/unregister。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.8) = 2.0 — not HIGH.

### [Risk] 活跃父任务 fork 出半截或陈旧上下文

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 16.0 — HIGH RISK

**Failure Scenario**: rollout 尚未 flush，或快照切在 tool call 与 tool output 中间；side 看不到最新用户请求、看到孤立 tool result，甚至把父任务误当成当前任务继续执行。

**Mitigation**:
- Step 5 使用 sequence barrier 和不可变 items，禁止半个 tool pair；删除“父任务运行就丢最后一条消息”的启发式。
- barrier/flush 超时或 sequence 不一致时恢复 draft 并拒绝 `/btw`，绝不回退到陈旧 catalog 文件。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] parent 与 side 并发写同一工作区造成源码或 Git 损坏

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 16.0 — HIGH RISK

**Failure Scenario**: 父任务正在 apply patch 或提交 Git，用户又在 side 明确要求修改；两个 agent 同时写相同文件、index 或 worktree，产生静默覆盖和不可恢复的混合状态。

**Mitigation**:
- Step 9 引入按 workspace root 键控的单写者 lease，覆盖 patch、shell mutation、Git、MCP mutation 和 browser/file bridge mutation。
- 一端持有 lease 时另一端只允许 read-only 工具；mutation 排队或明确拒绝，并用双向 runtime 测试验证。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.75) = 2.5 — not HIGH.

### [Risk] 隐藏的父审批被 side 输入误答

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: parent 在后台请求权限或用户输入，side 正在接收按键；切回时残留 Enter/Y 键直接批准父操作，或回答被发送给 side 的 request id。

**Mitigation**:
- Step 8 用 `(session_id, request_id, turn_id)` 存取和响应交互请求，移除全局“当前 modal”响应入口。
- 切回 parent 后启用输入 quarantine；测试同时存在 parent/side request 时只能回答聚焦 session 的准确 request。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] 返回 parent 后 child 仍继续执行 mutation

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 16.0 — HIGH RISK

**Failure Scenario**: `Ctrl+C` 只切回 UI，child 的 exec、MCP 或 patch 仍在后台运行；用户以为 side 已关闭并开始新的父操作，最终出现意外文件修改。

**Mitigation**:
- Step 10 在恢复 parent 可见前把 child 标为 `Closing`、关闭 submission 并立即 interrupt；mutation lease 未释放前显示 closing 状态且禁止新 `/btw`。
- cleanup 等待 terminal ack 或有界超时；超时后 tombstone 隔离事件并保留明确诊断，退出时仍继续统一回收。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] cleanup token 重复消费或丢失导致孤儿会话

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: `Ctrl+C`、App exit、fork timeout 和 fatal exit 同时触发清理；token 被二次使用、提前 drop 或根本没人拥有，造成 panic、临时 rollout 泄漏或后台 agent 永久存活。

**Mitigation**:
- Step 3/10 使用线性 cleanup owner 和 `take()` exactly-once；所有 terminal path 只调用同一 transition API。
- 加竞争测试同时触发 close/exit/timeout，断言 interrupt、shutdown、unregister、file delete 各执行一次。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] `Ctrl+/` 与 `Ctrl+C` 优先级冲突导致误关闭或无法切换

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 12.8 — HIGH RISK

**Failure Scenario**: 某些终端把 `Ctrl+/` 报成 `Ctrl+7`；approval、terminal overlay、Vim insert 或非空 composer 与全局 handler 冲突，按键被错误层消费。

**Mitigation**:
- Step 7 把 action 放入统一 keymap registry，启动时检测冲突并规范化等价键码。
- 用一张输入优先级表驱动实现和 table test，覆盖 modal、draft、overlay、Esc、Vim 和普通退出路径。

**Residual Risk After Mitigation**: Severity 4 × Likelihood 2 × (1 - Detectability 0.9) = 0.8 — not HIGH.

### [Risk] 一次性追求全 parity 导致范围爆炸和长期不可合并

**Severity**: 4 | **Likelihood**: 5 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: session routing、manager 重构、navigation registry 和双栏渲染堆进一个 PR，diff 上千行且无法 bisect；任何 review 反馈都会阻塞整个功能。

**Mitigation**:
- Delivery Gates 强制按 Gate 0→5 独立提交/PR，每个 gate 有独立测试和退出条件。
- Gate 3 单栏 parity 可独立发布；Gate 4 导航和 Gate 5 双栏不能反向阻塞基础功能。

**Residual Risk After Mitigation**: Severity 4 × Likelihood 2 × (1 - Detectability 0.8) = 1.6 — not HIGH.

### [Risk] 导航只缓存 widget 外壳却丢掉 live runtime

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2  
**Risk Score**: 16.0 — HIGH RISK

**Failure Scenario**: `/resume`、cwd 切换或线程导航后，UI 看似可返回 side，但 event pump、approval queue 或 cleanup owner 已丢失；恢复时出现静态 transcript、孤儿 child 或重复 runtime。

**Mitigation**:
- Gate 4 要求通用 `ConversationWorkspace` registry 同时持有 widget、event pump、display snapshot 和关系元数据，不能做 `/btw` 专用散落缓存。
- Gate 4 未完全通过前继续明确拒绝导航；关闭 parent 时用 registry 级联关闭 side。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] 测试只证明 helper，不覆盖真实 runtime

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: 单元测试用假 channel 验证状态机，但没有经过真实 `ConversationManager`、event pump、rollout、TUI key routing 和 cleanup；测试全绿，实际 `/btw` 仍丢消息或留后台任务。

**Mitigation**:
- Do 部分要求 core、App、VT100 三层 red-green checks，并在最终 binary 上走同 PID runtime 场景。
- 每个 milestone 的 focused test 先证明未修复代码会失败；最终 `./build-fast.sh` 零 warning 后才完成。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 1 × (1 - Detectability 0.9) = 0.5 — not HIGH.

### [Risk] capability 只在 UI 拦截而被直接 AppEvent 或工具调用绕过

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25  
**Risk Score**: 15.0 — HIGH RISK

**Failure Scenario**: slash popup 隐藏了 `/auto` 或 sub-agent，但快捷键、恢复的 queued op、MCP tool 或模型直接 tool call 仍能启动被禁能力，破坏 side 的隔离承诺。

**Mitigation**:
- Step 6/9 用不可伪造 `SessionKind::Side` 在 core/tool admission 再校验，UI 仅负责可发现性。
- capability descriptor 同时生成 popup、typed dispatch 和 core allow/deny 测试；未知新命令/工具默认拒绝。

**Residual Risk After Mitigation**: Severity 5 × Likelihood 2 × (1 - Detectability 0.85) = 1.5 — not HIGH.

### [Risk] 双 widget 和 companion transcript 带来内存与重绘退化

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.7  
**Risk Score**: 3.6

**Failure Scenario**: 长 parent transcript 加活跃 side 使内存翻倍；宽屏每帧重绘两个完整 transcript，streaming 时出现输入延迟。

**Mitigation**:
- Gate 5 默认关闭并复用现有 virtualization/retained transcript，不复制完整 response payload。
- 为单栏/双栏记录 frame time、内存和 retained cell 数；超过现有预算即不启用双栏。

**Residual Risk After Mitigation**: Severity 3 × Likelihood 2 × (1 - Detectability 0.9) = 0.6 — not HIGH.

### [Risk] 上游 Codex TUI 快速演进使 parity 基线过时

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.7  
**Risk Score**: 3.6

**Failure Scenario**: 实现期间上游修改 side lifecycle、keymap 或 app-server fork contract，本项目按旧 commit 完成后再次出现行为差距。

**Mitigation**:
- 每个 Delivery Gate 开始前记录上游 HEAD，并只审阅 `/btw` 相关提交差异。
- 新变化若影响安全/数据一致性则更新当前 gate；纯视觉变化另开 follow-up，避免无界追赶。

**Residual Risk After Mitigation**: Severity 3 × Likelihood 2 × (1 - Detectability 0.9) = 0.6 — not HIGH.

### [Risk] feature 默认关闭导致双栏实现长期无人验证

**Severity**: 2 | **Likelihood**: 3 | **Detectability**: 0.7  
**Risk Score**: 1.8

**Failure Scenario**: `tui_workspace` 默认关闭后 CI 和开发者很少覆盖，数月后宽屏路径悄然损坏。

**Mitigation**:
- VT100 snapshot 在 CI 中显式开启 feature，覆盖 99/100 列边界和 resize。
- feature 默认值不影响测试矩阵；每次 transcript/layout 改动都运行相同快照集合。

**Residual Risk After Mitigation**: Severity 2 × Likelihood 2 × (1 - Detectability 0.9) = 0.4 — not HIGH.

### [Risk] 文档与实际快捷键再次漂移

**Severity**: 2 | **Likelihood**: 3 | **Detectability**: 0.8  
**Risk Score**: 1.2

**Failure Scenario**: `docs/slash-commands.md` 继续写 `Ctrl+\\`，而实现已改成 `Ctrl+C`/`Ctrl+/`，用户认为功能坏了。

**Mitigation**:
- Step 1 同一 slice 更新命令文档，并让 keymap descriptor 成为 UI hint 的 source of truth。
- snapshot 断言 footer hint，review checklist 搜索旧快捷键文本。

**Residual Risk After Mitigation**: Severity 2 × Likelihood 1 × (1 - Detectability 0.9) = 0.2 — not HIGH.

### Risk Convergence Audit

- **Iteration 1**: 跨技术、团队/交付和外部演进类别发现 14 个 HIGH RISKS、4 个 medium/low risks；每项均已写入具体 mitigation，并同步回填到 Gate、Plan、Do 或 Adjust。
- **Iteration 2**: 重新扫描架构伸缩、依赖兼容、性能、安全、数据损坏、集成、范围、人员交接、预期错位、文档、上游变化；没有发现新的未记录 HIGH RISK。
- **Residual result**: 18 项风险均有 mitigation；所有 residual score ≤ 2.5，未缓解 HIGH RISKS = **0**。

---

> Next step: satisfy Gate 0 by restoring a clean `./build-fast.sh` baseline, then implement Gate 1 with a failing session-routing regression test first.
