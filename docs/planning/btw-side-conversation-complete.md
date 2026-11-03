# Plan: Complete `/btw` Side Conversations

> **User's request (restated)**: Write a complete implementation plan for bringing the desktop Codex side-conversation capability to this project, with `/btw` as the public command.
>
> **What this plan answers**: How to replace the current minimal fork-and-widget swap with a correct, isolated, observable, and testable side-conversation lifecycle that matches the desktop reference behavior.
>
> Goal: An idle saved conversation can start `/btw [message]` as an isolated temporary child, use it safely, and return or exit without affecting the parent.

## Context

The editable code is `code-rs`; `codex-rs` is a read-only reference mirror. The desktop reference at `/Users/doing/Desktop/codex/codex-rs/tui` implements `/side` plus the `/btw` alias through dedicated thread/session APIs, a hidden boundary prompt, explicit side-thread state, UI labels, command capability rules, and cleanup on return, navigation, and shutdown.

The local implementation currently only forks a rollout, swaps a `ChatWidget`, and restores the parent with `Ctrl+\`. That bypasses the reference's snapshot/bootstrap contract. As a result, inherited context can be rendered as visible child transcript, first-turn timing is fragile, and lifecycle/UI restrictions are incomplete. The local command surface must expose `/btw` only; internal types may continue to use `Side` terminology.

## Goal

Deliver a production-safe local equivalent with these measurable outcomes:

- `/btw` and `/btw <message>` are the sole public entry points and preserve the complete prepared `UserMessage`, including text, mentions, and supported attachments.
- The child sees the parent history only as reference context, never as an active instruction or replayed visible transcript.
- The parent transcript, active session, working directory, goals, and persistent metadata remain unchanged.
- The UI always identifies side mode, provides one return action, and prevents unsupported mutations.
- Every terminal state—success, fork failure, stale completion, return, navigation, shutdown, child close—cleans up child resources without losing the parent.
- Focused deterministic tests cover lifecycle, ordering, isolation, and user-visible behavior.

## Plan

1. Establish a local/reference responsibility map before changing behavior. Trace desktop `app/side.rs`, `thread_routing.rs`, `chatwidget/side.rs`, `slash_dispatch.rs`, composer command filtering, and side tests. Map each role to a local owner: command parsing (`slash_command.rs` and composer), child request (`chatwidget.rs`), lifecycle (`app/events.rs` and `app/state.rs`), conversation manager/session catalog, UI (`bottom_pane`), and shutdown/navigation handlers. Record any reference behavior that cannot be represented by the local conversation API before writing replacement code.

2. Replace the ad-hoc `side_parent` boolean pair with an explicit local state machine. Model `Idle`, `Forking { parent_id, request_id, message }`, `Active { parent, child_id }`, `Returning`, and `Failed`. Put parent identity, child identity, cleanup ownership, and a monotonically unique request id in one state owner. Every completion event must validate the request id and parent identity before mutating the visible widget; stale results must be disposed without switching UI.

3. Define a dedicated child bootstrap contract rather than treating a normal resumed conversation as a visible fresh session. Extend the local fork/session API as needed to return a child snapshot that distinguishes inherited rollout history from child-visible history. Construct the child widget with an explicit side display mode, no welcome/prelude duplication, and a prepared initial message installed before its first `SessionConfigured` event can be processed. Move event-forwarder startup after widget initialization so the ordering contract is structural, not timing-dependent.

4. Add the side boundary to the child model context. Adapt the desktop boundary/developer instructions to local protocol types and inject them after the parent snapshot but before the optional `/btw` message. The boundary must state that inherited history is reference-only, that prior tasks/tools/approvals are inactive, and that side work is temporary. Keep the boundary hidden from the user transcript and prohibit it from being persisted into the parent rollout.

5. Make child persistence and teardown explicit. Decide whether local child rollouts can be created under a temporary path; if so, use one and delete it on child disposal. If the current core API cannot provide a non-persistent rollout, add that API rather than silently leaving child history on disk. On return, stale completion, fork failure after allocation, parent replacement, app shutdown, and child close: interrupt an active child if necessary, detach its event forwarder, remove it from `ConversationManager`, remove its temporary rollout, and retain/restore the parent only after cleanup outcome is known.

6. Add side-mode UI and input policy. Give `ChatWidget`/`BottomPane` an explicit `side_conversation_active` flag and a context label such as `Side · parent unchanged`. Display a stable return hint and use the established keymap/escape ordering rather than adding conflicting global handlers. Centralize per-command capability in `SlashCommand::available_in_side_conversation()`; filter the popup and reject direct typed forms consistently. Cover session navigation, rename, goals, working-directory changes, Auto Drive, realtime/voice, safety retry, worktree operations, cloud actions, and nested `/btw`.

7. Preserve parent/child routing contracts. Parent events must not render into the child transcript; child events must not mutate the parent widget. If the local architecture cannot maintain both live streams, pause/interrupt the parent before forking and surface that policy in the UI. Track parent terminal states relevant to a return (approval needed, input needed, failed, interrupted, finished) and restore the exact parent view without replaying or duplicating its history.

8. Implement user-facing command behavior. Add `/btw` as the canonical visible command in the enum, parser, popup, documentation, command history, and inline-argument support. Strip only the command prefix from the first text item while preserving the remaining `UserMessage` ordered items. Reject use before a saved parent session, while forking, while a parent/child task is active according to the chosen policy, and while already in a side conversation. Do not retain `/side` as a public spelling.

9. Build regression coverage before final wiring. Add unit tests for command parsing, message-prefix removal, capability filtering, state-machine transitions, request-id rejection, boundary contents, and cleanup ownership. Add app/session tests with controllable fork success/failure/staleness. Add `make_chatwidget_manual()` or VT100 tests for `/btw`, inline text, label/hint, disabled popup entries, return behavior, and no visible inherited AGENTS/context transcript. Gate snapshot rewrites behind an explicit environment variable.

10. Perform integration hardening. Exercise: saved parent → `/btw` empty → child turn → return; inline text with mention/image; missing parent; busy parent; duplicate submit; fork failure; stale result after navigation; child task active during return; parent approval/state change; child shutdown; app exit; resume and working-directory flows. Run a global scan for all `SlashCommand`, `DispatchCommand`, `SessionConfigured`, `new_from_existing`, `ConversationManager`, event-forwarding, shutdown, Esc, Auto Drive, realtime, and persistence paths. Remove temporary diagnostic logs after proving event order.

## Think — Debug Methodology

- Treat the child bootstrap as the primary boundary: log request id, parent/child session ids, state transition, snapshot install, boundary injection, first-message acceptance, widget switch, and disposal using a temporary `[DEBUG-btw]` prefix. Never log user text, attachments, or full rollout content.
- Read local `ConversationManager::fork_conversation`, rollout recorder/resume logic, `ChatWidget::new_from_existing`, `SessionConfigured` handling, and app event loop before adapting desktop code. Then compare one responsibility at a time against desktop `app/side.rs` rather than copying the thread architecture wholesale.
- Localize failures in this order: parsing/prepared message → state-machine admission → fork snapshot → child bootstrap and event ordering → boundary injection → child first turn → UI switch → return/cleanup.
- Build a deterministic test seam for fork results so tests can deliver success, failure, duplicate, and stale completion events without real network/model traffic. Use it before changing race-sensitive code.
- After every lifecycle fix, search for `StartSide`, `SideConversation`, `SessionConfigured`, `new_from_existing`, `remove_conversation`, `SwitchCwd`, `DispatchCommand`, and `ReturnFromSide` to find bypasses.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root. It is the completion gate and must pass with zero errors and warnings.
- **Static analysis / type check**: N/A as a separate final command; `./build-fast.sh` is the required compiler/warning check. Do not run formatters or unrelated checks.
- **Focused regression tests**: Run the newly added, explicitly scoped TUI/app tests while developing; they must demonstrate a failure before each lifecycle fix and pass afterward. Do not refresh snapshots without reviewing them.
- **Runtime verification**: Launch the freshly built binary in a saved session. Verify `/btw`, `/btw 讲个笑话`, a mention, and a supported image. Confirm the child label appears; inherited AGENTS/instructions do not render as child transcript; only the child receives the new request; `Ctrl+\` restores the unchanged parent; and side artifacts are absent after return and exit.
- **Logic correctness**: Manually walk no parent, unsaved parent, busy parent, duplicate start, child bootstrap failure, stale completion, active child return, failed child return, parent navigation while forking, blocked commands, popup filtering, Esc/approval ordering, child close, and application shutdown. Assert parent transcript/session metadata byte-for-byte unchanged by child activity.

## Adjust — Rollback and Global Scan

- **Rollback plan**: Keep side lifecycle behind one isolated state owner and one child-bootstrap entry point. If an incident occurs, disable `/btw` command registration and reject `StartSide`; safely dispose any live child before restoring its parent. Do not use a partial rollback that leaves a background child/event forwarder alive.
- **Global scan**: Search all direct command dispatches, composer submission paths, app events, conversation/session constructors, rollout writers, session switching, working-directory changes, Auto Drive, realtime/voice, safety retry, approvals, Esc handlers, shutdown, and resume paths for paths that bypass side state.
- **Backwards compatibility**: Existing sessions retain their normal format and behavior. `/side` is not recognized. No migration is needed for parent sessions. Child artifacts are temporary and must never become resumable sessions or alter history/catalog entries.

## Open Questions

- Can `ConversationManager` create a child from a temporary/non-resumable rollout and reliably delete it, or must the core API grow a dedicated ephemeral-fork primitive?
- Does the local TUI support preserving a paused parent event stream while a child is visible? If not, should starting `/btw` require an idle parent (the conservative default) or explicitly pause it?
- Which established key binding should be the canonical return action: retain `Ctrl+\`, or align with the desktop keymap’s side-toggle binding? The implementation should provide exactly one documented route.
- Are image/attachment payloads fully supported by resumed local conversations? Unsupported kinds must be rejected visibly, not dropped.

## Out of Scope

- Porting the desktop app-server/thread-routing architecture wholesale.
- Nested side conversations, cross-device child restoration, merging child transcript/work back into the parent, or persistent child history.
- Changing the read-only `codex-rs` mirror.
- Redesigning unrelated session, Auto Drive, approval, voice, or workspace behavior beyond the guards needed for side mode.

---

> Next step: run `/pre-mortem docs/planning/btw-side-conversation-complete.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] A Resumed Child Replays Parent Context as Visible or Active Work

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: The local resumed-conversation path feeds inherited rollout events into the child widget or model without a boundary. AGENTS instructions and past turns appear as current child content, or the agent resumes the parent task instead of answering `/btw`.

**Mitigation**:

- Make child bootstrap a dedicated API contract with separate inherited-model context and child-visible transcript state; prohibit ordinary resume replay from populating visible history.
- Inject and test a hidden boundary item after inherited context and before the first child message. It must make prior work reference-only and inactive.
- Add deterministic VT100 coverage that forks a parent containing AGENTS/context and asserts neither becomes a child transcript cell while the child receives the intended first message.
- Do not release while the screenshot-driven runtime scenario fails; make it a mandatory manual completion gate.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no high risk remains when these gates pass.

### [Risk] Fork Completion Races Switch the Wrong Widget or Lose the First Message

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.15
**Risk Score**: 17.0 — HIGH before mitigation

**Failure Scenario**: Duplicate `/btw`, parent navigation during creation, or `SessionConfigured` arriving before child construction lets a stale result replace the current chat, submits the message to the parent, or drops it entirely.

**Mitigation**:

- Use one state machine with a monotonic request id, parent id, and explicit `Forking` ownership; every completion validates both identities before any widget replacement.
- Install the prepared `UserMessage` in the child constructor before its event forwarder is started; never set it after construction.
- Dispose stale children through a single cleanup path and leave the currently visible parent unchanged.
- Add event-order tests for double submit, parent switch before completion, completion after return, child configuration before first message, and fork failure after allocation.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no high risk remains when these gates pass.

### [Risk] Temporary Children Leak Into Session History, Memory, or Disk

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: Return removes only the visible widget while the child remains in `ConversationManager`, stays subscribed, writes a resumable rollout, or survives application exit. Cleanup on one event path then masks leakage on stale completion or shutdown.

**Mitigation**:

- Introduce an ephemeral-fork primitive or temporary rollout owner with a single idempotent dispose operation: interrupt, detach event forwarding, remove manager entry, and delete temporary rollout.
- Call that operation from return, failed bootstrap, stale result, parent navigation, child close, and application shutdown; preserve the parent if disposal fails and surface an actionable error.
- Add tests that inspect manager membership, temporary paths, and active tasks after every terminal transition.
- Define child persistence as a release-blocking contract: a child must never appear in resume/catalog discovery.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no high risk remains when these gates pass.

### [Risk] Side Mode Bypasses Mutating or Unsafe Capabilities

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75 — HIGH before mitigation

**Failure Scenario**: A direct typed command, popup route, key binding, Auto Drive, realtime/voice, safety retry, working-directory switch, or approval flow does not consult side state. The child mutates persistent context, starts nested work, or produces conflicting Esc behavior.

**Mitigation**:

- Centralize policy in `SlashCommand::available_in_side_conversation()` plus explicit capability predicates for non-slash entry points; default any new capability to unavailable until reviewed.
- Drive popup filtering and typed-command rejection from the same policy. Keep unavailable exact matches resolvable only to provide a clear error.
- Audit every listed capability and add a focused regression test for each class; preserve modal-first Esc handling and use one documented return binding.
- Add a code-review checklist item requiring new commands/capabilities to declare side availability.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no high risk remains when these gates pass.

### [Risk] Parent and Child Event Streams Corrupt Each Other’s UI State

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75 — HIGH before mitigation

**Failure Scenario**: While the child is visible, parent events render in the child, child approval/turn state mutates the paused parent, or return replays/drops transcript cells. The user can approve or act on the wrong conversation.

**Mitigation**:

- Associate every forwarded event with an owning conversation id and route it only to the matching active or retained widget state.
- Choose and document a conservative parent policy: require idle parent before start and block/queue parent changes while the child is active unless the local architecture can preserve both streams.
- Capture parent view/history state before the switch and restore it by identity, not by replaying text.
- Add deterministic tests that interleave parent/child events and assert recipient identity, transcript isolation, approval ownership, and exact parent restoration.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no high risk remains when these gates pass.

### [Risk] Reference Architecture Is Copied Instead of Adapted

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.35
**Risk Score**: 7.8 — medium

**Failure Scenario**: Desktop app-server thread concepts are pasted into the older local conversation architecture, creating duplicate owners for session state or unmaintainable compatibility shims.

**Mitigation**:

- Complete the responsibility map before implementation and assign each desktop behavior to exactly one local owner.
- Add local primitives only where the existing `ConversationManager` contract cannot uphold isolation; avoid importing app-server abstractions that have no local owner.
- Require a design review of the state-machine and ephemeral-fork API before wiring UI behavior.

**Residual assessment after mitigation**: Severity 4 | Likelihood 1 | Detectability 0.90 | **Residual Risk Score: 0.4** — low.

### [Risk] Attachment or Mention Context Is Silently Lost at the Command Boundary

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.40
**Risk Score**: 7.2 — medium

**Failure Scenario**: `/btw` strips text correctly but reconstructs a plain string, losing images, local files, remote URLs, or mentions without telling the user.

**Mitigation**:

- Pass `UserMessage` and its ordered items end-to-end; strip only the command token from the first text item.
- Add payload matrix tests for empty input, multiline text, mention, local image, remote image, and unsupported kinds.
- Reject an unsupported item visibly before forking rather than dropping it.

**Residual assessment after mitigation**: Severity 4 | Likelihood 1 | Detectability 0.90 | **Residual Risk Score: 0.4** — low.

### [Risk] Scope Expansion Delays a Correct Core Flow

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.65
**Risk Score**: 3.15 — low

**Failure Scenario**: Work expands into a wholesale desktop app-server port, multiple return gestures, nested conversations, and unrelated session redesign before the core child bootstrap is reliable.

**Mitigation**:

- Ship in gated vertical slices: ephemeral fork/bootstrap/isolation first, then UI policy, then optional parent-status polish.
- Treat nested children, transcript merging, cross-device restoration, and desktop architecture porting as explicitly out of scope.
- Require the runtime screenshot scenario and deterministic lifecycle suite to pass before accepting follow-up features.

**Residual assessment after mitigation**: Severity 3 | Likelihood 1 | Detectability 0.90 | **Residual Risk Score: 0.3** — low.
