# Plan: Fix `/btw` Runtime Lifetime and Ctrl+C Return Semantics

> **User's request (restated)**: First study the desktop Codex implementation, then plan how to fix `/btw` so it replies normally, one `Ctrl+C` exits `/btw` and restores the original still-running TUI, and two subsequent `Ctrl+C` presses exit the TUI.
>
> **What this plan answers**: How to correct the child-agent lifetime bug and align `/btw` return/quit routing with desktop Codex without replacing the parent TUI process.
>
> Goal: `/btw` runs on a live child agent, returns to the exact parent widget with one `Ctrl+C`, and leaves the parent's normal two-press quit behavior intact.

## Context

The 14:40 reproduction is not a model-response failure. `critical.log` records repeated `failed to submit op: internal error; agent loop died unexpectedly` immediately after the side fork. The local implementation creates the child conversation inside a temporary Tokio runtime owned by a short-lived OS thread. When the fork future returns, that runtime is dropped and aborts the child's submission loop; the UI still accepts and renders the user message, but every child operation is sent to a dead receiver.

The desktop reference in `/Users/doing/Desktop/codex/codex-rs/tui` treats side return as an app-level navigation action. `side_return_shortcut_matches`, `App::maybe_return_from_side`, and `App::discard_side_thread` route `Ctrl+C` before ordinary chat quit behavior, interrupt and dispose the side thread, and restore its parent. The local implementation instead documents `Ctrl+\`, blocks return while the side task is running, and lets `Ctrl+C` enter the child widget's ordinary interrupt/quit path.

The intended local behavior is:

1. Parent TUI starts `/btw` without creating a second TUI process.
2. Child work remains alive on the application's long-lived runtime.
3. In side mode, one `Ctrl+C` means “close side and return to parent,” including while the child is running.
4. Returning restores the retained parent widget and clears any stale quit-arm state.
5. In the restored parent, the first `Ctrl+C` arms the normal quit hint and the second exits the TUI.

## Goal

The change is complete when all of the following are true:

- `/btw 讲个笑话` reaches a live child agent and produces a response; no `InternalAgentDied` error is logged.
- Side creation, event forwarding, and disposal all run on a runtime whose lifetime is at least the lifetime of `App`.
- One `Ctrl+C` in an unobstructed side view restores the exact retained parent widget, whether the child is idle, streaming, or running a tool.
- Returning never exits the process and never requires a prior “stop side task” action.
- After return, two fresh `Ctrl+C` presses are required to exit the parent TUI; the side press does not count as the first parent quit press.
- The side rollout, manager entry, event forwarder, pending events, and running work are disposed exactly once.
- The footer and blocked-command messages say `Ctrl+C to return`; `Ctrl+\` is no longer the documented side-return gesture.

## Plan

1. Preserve a failing reproduction at both boundaries before changing behavior. Add one lifecycle test proving that a child task spawned during fork remains able to receive an operation after the fork future completes. Add one app-input test proving that `Ctrl+C` in `SideConversationState::Active` currently falls through to the child widget instead of requesting a side return. Keep the real-log signature (`InternalAgentDied`) as the runtime acceptance check, but do not log prompt contents.

2. Move side forking onto the application's existing Tokio runtime. Replace the `std::thread::Builder` plus nested `tokio::runtime::Builder` path with a detached `tokio::spawn` owned by the runtime already driving the TUI. Keep the monotonic `request_id` and parent-id validation. Ensure both success and failure send an `AppEvent`, and ensure a stale success disposes its child instead of leaking it.

3. Make side return an explicit app-level state transition. Extend `SideConversationState` with `Returning { parent, child_session_id, child_rollout_path, request_id }` or an equivalent single-owner state. The transition from `Active` must atomically take ownership of the retained parent and child cleanup data so repeated key events cannot start duplicate cleanup or lose the parent.

4. Route side `Ctrl+C` before normal widget quit handling. In the global key-event path, retain modal-first handling where an approval, picker, or focused overlay owns `Ctrl+C`; otherwise, when side mode is active, consume a `KeyEventKind::Press` for `Ctrl+C` and dispatch `ReturnFromSide`. Do not call the child widget's normal `on_ctrl_c` for that same event, and do not arm the process-exit hint in the child. Ignore repeat/release events so a held key cannot both return and arm parent exit.

5. Make return responsible for active-child interruption. Remove the current guard that displays “Stop the side conversation task before returning.” On return, stop routing child events to the visible widget, submit child interruption/shutdown, unregister it from `ConversationManager`, stop its forwarder, and delete its temporary rollout/snapshot through the idempotent ephemeral-disposal API. Restore the parent even when the child was streaming or running a tool; surface cleanup failure on the parent without resurrecting the child UI.

6. Restore parent UI state without carrying the side quit gesture across the boundary. Before displaying the retained parent, clear its `Ctrl+C` quit hint/armed state and any side-only footer status. Replace the child widget with the same boxed parent widget rather than constructing or replaying a new one. Preserve the parent's transcript, scroll position, composer draft, session id, terminal process state, and pending parent events.

7. Align user-facing hints with the new routing contract. Change the side footer from `Ctrl+\ to return` to `Ctrl+C to return`. Update unavailable-command messages consistently. Remove or demote the `Ctrl+\` global side handler so there is one documented return gesture and no conflicting ownership. Keep the normal parent help text `Ctrl+C twice · Quit` unchanged.

8. Harden child-event routing during return. While `Returning`, drop or quarantine late child events by child session id, continue retaining parent events for the parent widget, and reject new `/btw` starts. A late fork completion, task completion, error, or `SessionConfigured` event must not switch the UI back to the child or mutate the restored parent.

9. Add deterministic regressions for the complete control flow. Cover idle child return, running child return, streaming child return, held-key repeat/release, modal precedence, duplicate `Ctrl+C`, cleanup failure, stale child events, stale fork completion, exact parent-widget restoration, and child artifact deletion. Add a sequence test: enter side → press `Ctrl+C` once → assert process continues and parent is visible → press `Ctrl+C` once → assert quit hint only → press `Ctrl+C` again → assert exit request.

10. Build and verify with the binary actually being launched. Run the required root build, identify the produced qcodex executable, and launch that exact path. Record the PID before `/btw`, after entering side, and after returning to prove the same process remains alive. Run `/btw 讲个笑话`, wait for a response, press `Ctrl+C`, verify the parent transcript is unchanged, then press `Ctrl+C` twice to exit. Inspect only new log lines and assert there is no `agent loop died unexpectedly` entry for the run.

## Think — Debug Methodology

- Instrument framework boundaries with a temporary `[DEBUG-btw]` prefix: `StartSide`, fork task start/end, child `SessionConfigured`, first child op submission, `Active → Returning → Idle`, child interrupt, child disposal, parent restoration, and global `Ctrl+C` routing. Log request/session ids and state names only, never user text or inherited transcript.
- Read the local boundaries in this order: `app/events.rs` key routing, `SideConversationState`, `ConversationManager::fork_conversation`, child op/event forwarders, `ReturnFromSide`, and `ChatWidget::on_ctrl_c`. Compare responsibilities—not types—against desktop `app/input.rs`, `app/side.rs`, `app/platform_actions.rs`, and `chatwidget/interaction.rs`.
- Localize failures upstream first: runtime alive → child op receiver alive → first `UserInput` recorded → child events tagged/routed → response rendered. For return: key captured once → state ownership transferred once → child routing stopped → parent restored → cleanup completed.
- Use event/session ids to distinguish old-binary evidence from the new run. Record the executable path and build timestamp before runtime verification.
- Remove all `[DEBUG-btw]` instrumentation after the deterministic tests and real run pass, then search globally for the prefix.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass with zero errors and warnings.
- **Static analysis / type check**: N/A as a separate completion command; repository policy makes `./build-fast.sh` the single required compiler/warning gate. Never run `rustfmt`.
- **Focused tests**: Run the exact lifecycle and app-input tests added for this change. Each root-cause test must be observed failing before the fix and passing afterward.
- **Runtime verification**: Launch the exact freshly built executable. In one process, execute parent → `/btw 讲个笑话` → receive answer → one `Ctrl+C` → parent restored → first parent `Ctrl+C` shows quit hint → second parent `Ctrl+C` exits.
- **Log verification**: Capture the log offset before launch and inspect only appended lines. Assert no `InternalAgentDied`, `failed to submit op`, duplicate cleanup, or child-event-to-parent routing error occurs.
- **Logic correctness**: Walk idle child, active child, tool-running child, fork failure, stale completion, modal open, non-empty composer, key repeat/release, cleanup failure, app shutdown during fork, and app shutdown during return. Every path must identify the sole owner of parent, child, rollout path, and cleanup responsibility.

## Adjust — Rollback and Global Scan

- **Rollback plan**: If side return or lifetime handling regresses, disable `/btw` admission while preserving normal parent `Ctrl+C` behavior. Dispose any allocated child through the existing idempotent cleanup path. Do not restore the temporary-runtime implementation and do not leave a child forwarder detached from state ownership.
- **Global scan**: Search for every temporary Tokio runtime created around a future that returns a long-lived channel/task; every global `Ctrl+C` handler; every direct call to `ChatWidget::on_ctrl_c`; every side-state transition; every child event forwarder; every conversation removal/shutdown path; and every footer string naming `Ctrl+\` or side return.
- **Backwards compatibility**: Parent sessions and rollout formats do not change. `/btw` remains the only public command. Existing normal TUI double-press exit behavior remains unchanged outside side mode. Temporary side artifacts remain non-resumable and are deleted on every terminal path.

## Open Questions

- When a side modal or approval is open, desktop Codex gives the modal first refusal before returning. The default plan preserves that safety ordering; if product intent is literally “any `Ctrl+C`, regardless of modal, always return,” that should be a separate explicit decision.
- A non-empty side composer normally consumes `Ctrl+C` locally in desktop Codex. The default plan preserves draft/modal ownership and guarantees one-press return from the ordinary empty-composer side view shown in the reproduction.
- Desktop Codex also accepts `Ctrl+D` as a side-return shortcut. This plan intentionally implements only the requested `Ctrl+C` behavior to avoid expanding the public contract.

## Out of Scope

- Porting desktop Codex's daemon/app-server thread model into qcodex.
- Persisting or resuming side conversations, merging side content into the parent, or allowing nested `/btw` sessions.
- Changing ordinary parent `Ctrl+C` semantics beyond resetting its quit state when returning from side mode.
- Editing the read-only `codex-rs` mirror or the desktop `/Users/doing/Desktop/codex` repository.

---

> Pre-mortem completed on 2026-11-03. Execute the plan only with every HIGH-risk mitigation treated as a release gate.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Detached Fork Task Dies Without Returning a Failure Event

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20  
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: Replacing the temporary runtime with a bare detached `tokio::spawn` keeps the child loop alive in the happy path, but a panic, cancellation, or application-runtime shutdown can still drop the fork task without sending `SideConversationForkFailed`. The app remains stuck in `Forking`, rejects future `/btw` requests, and gives the user no recovery path.

**Mitigation**:

- Store or supervise the fork `JoinHandle`; convert normal errors, panics, and cancellation into a request-id-tagged terminal app event.
- Add an explicit fork deadline/watchdog that transitions the matching request back to `Idle` and disposes any late child result.
- Test success, returned error, panic, cancellation, timeout, and application shutdown while `Forking`; every case must end in exactly one terminal transition.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — no HIGH risk remains when the supervised-task tests pass.

### [Risk] Return and Late Events Race Into Double Cleanup or Child Resurrection

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.15  
**Risk Score**: 17.0 — HIGH before mitigation

**Failure Scenario**: `Ctrl+C`, child completion, shutdown, and queued `SessionConfigured`/stream events arrive close together. Two paths take or dispose the same child, the parent is restored twice, or a late child event changes the visible state back to `Active` after return.

**Mitigation**:

- Give every side lifecycle a generation/request id and require compare-and-transition semantics for `Forking → Active → Returning → Idle`.
- Put parent ownership, child id, rollout path, forwarder cancellation handle, and cleanup status in one state object; no other location may own cleanup.
- Mark the child generation abandoned before restoring the parent, then reject all later events and completions for that generation.
- Add deterministic event-permutation tests for return/task-complete/error/shutdown/stale-fork ordering and assert one parent restoration plus one disposal.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — the state-transition matrix is release-blocking.

### [Risk] One Physical Ctrl+C Also Arms or Triggers Parent Exit

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20  
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: A terminal emits `Press`, `Repeat`, and `Release`, or queued input is processed after the widget swap. The `Press` returns from side, then a repeat reaches the restored parent and arms or completes its double-press quit sequence. One physical key press can therefore exit the entire TUI.

**Mitigation**:

- Consume the complete side-return key gesture at the app layer and install a short key-release/activation guard tied to the exact key code before restoring parent input.
- Clear both child and parent quit-arm state during the transition; do not count the side-return press toward the parent shortcut.
- Test enhanced-key terminals, legacy terminals without release events, autorepeat, uppercase/control variants, and two intentional parent presses after return.
- Runtime-verify that the PID survives the side-return gesture and the first subsequent parent `Ctrl+C`.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — input-sequence tests and PID checks make the failure detectable before release.

### [Risk] Running Child Tools Survive Return and Mutate the Workspace

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.15  
**Risk Score**: 17.0 — HIGH before mitigation

**Failure Scenario**: The parent UI is restored immediately, but a child exec, browser action, patch, MCP tool, or model turn continues in the background because `Shutdown` only closes the submission loop or cleanup is asynchronous. The user believes `/btw` ended while the child continues changing files.

**Mitigation**:

- Define disposal completion as: interrupt active turn, cancel all child-owned tools/processes, observe terminal task state, stop event forwarding, unregister conversation, then delete artifacts.
- Track child-owned terminal/tool ids and verify no child activity remains after disposal; do not treat channel closure alone as cleanup success.
- Keep the UI responsive by showing the parent with a bounded “closing side conversation” status, but block a new `/btw` until cleanup reaches a terminal result.
- Add real subprocess and fake-tool tests that return during execution and assert cancellation, no post-return output, and no post-return filesystem mutation.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — process/tool quiescence is an explicit completion gate.

### [Risk] Parent Events Accumulate or Become Stale While Side Is Visible

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10  
**Risk Score**: 13.5 — HIGH before mitigation

**Failure Scenario**: The parent is retained but its events are dropped, buffered without bounds, or replayed in the wrong order. On return, approvals, errors, task state, token usage, or terminal events are missing or stale; the restored parent looks unchanged but no longer represents core state.

**Mitigation**:

- Allow side admission while the parent is active; `/btw` is a concurrent path, not an idle-only navigation mode.
- Route parent model and Auto Drive coordinator events directly into the retained parent widget by ownership; never send them to the visible child or wait until side return.
- Preserve `(request_ordinal, output_index, sequence_number)` ordering inside each conversation while parent and child advance independently.
- Add interleaving tests for parent error/status/background/terminal events during side mode and assert exact ordered parent state after return.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — parent routing and ordering tests are mandatory.

### [Risk] Cleanup Deletes a Parent or Unrelated Rollout

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10  
**Risk Score**: 13.5 — HIGH before mitigation

**Failure Scenario**: The ephemeral cleanup API receives a reused, canonicalized, stale, or parent rollout path and deletes durable session data. An idempotent delete then hides the second failure while the parent can no longer resume.

**Mitigation**:

- Replace loose `Option<PathBuf>` cleanup ownership with an opaque ephemeral-rollout owner created by the fork API and bound to the child conversation id.
- Before deletion, validate the owner marker, child id, expected temporary directory/root, and inequality with the parent rollout path; reject symlinks and paths outside the owned root.
- Snapshot parent catalog/rollout identity before side start and assert it is unchanged after every return/failure/shutdown test.
- Add negative tests for parent path, stale owner, symlink, already-deleted artifact, and mismatched child id.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — destructive cleanup requires positive ownership proof.

### [Risk] Synthetic Lifecycle Test Passes While a Real Forked Conversation Is Dead

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20  
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: A generic nested-channel test proves that `tokio::spawn` can outlive its parent future, but it never exercises `ConversationManager::fork_conversation`, `Codex::submission_loop`, resume rollout loading, or the TUI op forwarder. The regression test turns green while `/btw` still reports `InternalAgentDied`.

**Mitigation**:

- Keep the small runtime-lifetime test, but add an integration seam using a real `CodexConversation` or a faithful fake whose receiver lifecycle is owned exactly like production.
- Exercise fork completion, `SessionConfigured`, the TUI op channel, `conversation.submit(Op::UserInput)`, and one returned event in a single test without external model traffic.
- Make the screenshot-driven runtime flow and absence of new `InternalAgentDied` log lines a release gate, not an optional manual check.
- Verify that the test fails when the temporary-runtime implementation is restored.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — both structural and end-to-end seams must pass.

### [Risk] Runtime Verification Launches an Old or Different Binary

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.15  
**Risk Score**: 13.6 — HIGH before mitigation

**Failure Scenario**: The build succeeds but the shell alias, PATH, desktop launcher, or another still-running process launches an older qcodex binary. The same fixed bug appears, or an old binary happens to pass, producing a false diagnosis and wasted iterations.

**Mitigation**:

- Print and record the exact executable path, canonical path, version/commit marker, modification time, and PID used for acceptance.
- Launch the built artifact by absolute path and start with no pre-existing qcodex TUI process in that terminal.
- Add a temporary startup build marker or existing version output that can be matched to HEAD without altering persistent user data.
- Capture log offset and session id after launch so acceptance reads only evidence from that process.

**Residual assessment after mitigation**: Severity 4 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — binary identity is proven, not assumed.

### [Risk] Desktop Reference Semantics Drift During Implementation

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.60  
**Risk Score**: 3.6 — medium

**Failure Scenario**: Developers copy current desktop details too literally or rely on a moving reference branch, introducing app-server concepts that do not belong in qcodex or changing behavior midway through the fix.

**Mitigation**:

- Record the reference commit and extract only invariant responsibilities: app-level side return, modal precedence, child interruption, parent restoration, and cleanup ownership.
- Treat qcodex architecture and the user-stated Ctrl+C contract as authoritative when implementation details differ.
- Keep the change limited to runtime ownership, side state, key routing, cleanup, hints, and focused tests.

### [Risk] Scope Expands Into a General Session-Routing Rewrite

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.70  
**Risk Score**: 2.7 — low

**Failure Scenario**: Fixing `/btw` grows into daemon adoption, general keymap redesign, parent multitasking, or rollout-format migration, delaying the concrete bug fix and increasing regression surface.

**Mitigation**:

- Use the existing local `ConversationManager`, retained widget, and app event loop; introduce only the minimum ownership/cancellation primitives needed by the stated flow.
- Defer daemon architecture, persistent side sessions, nested sides, and general shortcut customization as separate proposals.
- Review the final diff against the Out of Scope section before build acceptance.

### [Risk] Side Fork Is Spawned Where No Tokio Runtime Is Entered

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10  
**Risk Score**: 13.5 — HIGH before mitigation

**Failure Scenario**: The app event loop is synchronous or runs on a thread that is not currently inside a Tokio context. A direct `tokio::spawn` panics at `/btw` admission, crashing the TUI instead of merely killing the child loop.

**Mitigation**:

- Make the long-lived runtime handle an explicit dependency of `App`/the side task owner and spawn through that stored handle rather than relying on ambient runtime context.
- Validate at initialization that the handle remains alive for the whole app lifetime; treat handle shutdown as an app lifecycle failure, not a side-conversation error.
- Add a production-thread test that invokes the same `StartSide` handler from the real synchronous event-loop context and proves the fork task starts without panic.
- Catch and convert task panic to `SideConversationForkFailed` through the supervised handle described above.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — explicit runtime ownership removes ambient-context dependence.

### [Risk] Boundary Injection Starts a Hidden Turn That Races the First User Message

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20  
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: On child `SessionConfigured`, the TUI submits `AddPendingInputDeveloper` and then `UserInput`. The first operation can start a hidden sentinel turn; the user operation then aborts or queues behind it. Depending on timing, `/btw 讲个笑话` is lost, delayed, or answered with the wrong context even though the child agent loop remains alive.

**Mitigation**:

- Make child bootstrap atomic: install reference-only developer instructions in child configuration/history before the agent loop accepts user work, then submit exactly one first user turn.
- Do not use an operation whose idle behavior starts a model turn merely to install the side boundary.
- Add an ordered-op test asserting the child receives configured boundary/context before one and only one `UserInput`, with no hidden task start, abort, or queue event between them.
- Run the test repeatedly under forced scheduler yields and verify the rollout contains one side user turn and one corresponding answer.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — atomic bootstrap removes scheduler-dependent ordering.

### [Risk] Cleanup Hangs Forever and Permanently Blocks Future Side Conversations

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.15  
**Risk Score**: 13.6 — HIGH before mitigation

**Failure Scenario**: Child interrupt, process cancellation, event-forwarder join, recorder shutdown, or file deletion never completes. The parent may be visible, but state remains `Returning`, `/btw` stays unavailable, and shutdown can wait forever.

**Mitigation**:

- Give each cleanup phase a bounded deadline and record phase-specific outcomes; cancellation must never await an unbounded provider/tool response.
- Separate logical abandonment from physical cleanup: immediately quarantine the child generation, then perform bounded cleanup while keeping an owned retry record.
- On timeout, terminate owned local processes, detach closed channels, preserve enough identity for retry, surface a parent notice, and transition to a defined recoverable state.
- Test a never-resolving interrupt, forwarder, recorder, and delete operation with paused Tokio time; assert the parent remains usable and app shutdown is bounded.

**Residual assessment after mitigation**: Severity 4 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — every wait is bounded and tested.

### [Risk] Side Context Becomes Visible or Reactivates Parent Work Again

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.20  
**Risk Score**: 16.0 — HIGH before mitigation

**Failure Scenario**: Refactoring child bootstrap to fix message ordering accidentally restores ordinary resume replay. AGENTS content and parent transcript reappear in the side UI, or inherited requests/tools are treated as active instructions instead of reference-only context.

**Mitigation**:

- Preserve separate contracts for model context and visible child history: inherited items may seed model context but must never be rendered as child replay cells.
- Keep the reference-only boundary in a non-visible developer/config channel and assert prior tasks, approvals, goals, and tool calls are inactive.
- Add a VT100/runtime fixture whose parent contains AGENTS instructions, an unfinished-looking request, tool calls, and approvals; assert none appear or resume in the side transcript.
- Include this fixture in the same release gate as response and Ctrl+C tests so the lifetime fix cannot regress isolation.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — visible-history and model-context assertions both gate release.

### [Risk] Build or Test Side Effects Pollute the Patch

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.45  
**Risk Score**: 6.6 — medium

**Failure Scenario**: A build helper, formatter, snapshot update, or generated-file step modifies many unrelated Rust files. The side fix is committed with noisy mechanical changes or user work is accidentally restored away during cleanup.

**Mitigation**:

- Capture `git status --short` and the exact pre-existing path list before execution; classify owned task files before any build.
- Never run `rustfmt` or snapshot acceptance; after every test/build, inspect `git diff --name-only` and stop on unexpected paths.
- Restore only positively identified tool-generated files, never broad globs or the user's pre-existing changes.

### [Risk] Parent Exits Before Background Child Cleanup Finishes

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15  
**Risk Score**: 12.75 — HIGH before mitigation

**Failure Scenario**: One `Ctrl+C` restores the parent, then the user immediately presses `Ctrl+C` twice to exit as specified. If child cleanup is merely detached in the background, application shutdown drops the runtime before interrupt, process termination, rollout deletion, or manager removal completes. Child work or artifacts survive the TUI.

**Mitigation**:

- Register side cleanup with the app shutdown coordinator instead of fire-and-forget spawning; shutdown must await its bounded terminal result.
- Keep a cleanup registry keyed by side generation/child id and make normal return, app exit, panic handling, and stale-child disposal join the same idempotent operation.
- Preserve fast UI return, but on process exit wait only up to the already-defined cleanup deadline, force-terminate owned local processes, and synchronously finish safe artifact removal.
- Add the exact acceptance sequence as a deterministic test: side running → return → two immediate parent `Ctrl+C` presses → process shutdown; assert no child task, manager entry, forwarder, subprocess, or artifact remains.

**Residual assessment after mitigation**: Severity 5 | Likelihood 0 | Detectability 1.00 | **Residual Risk Score: 0.0** — shutdown joins the same bounded cleanup owner.

### [Risk] Modal or Draft Precedence Violates the User's Meaning of “One Ctrl+C”

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.50  
**Risk Score**: 6.0 — medium

**Failure Scenario**: The implementation preserves desktop modal-first and draft-first routing, but the user expects `Ctrl+C` to return from `/btw` even with an approval popup or non-empty composer. The feature appears inconsistent despite passing the ordinary empty-view test.

**Mitigation**:

- Turn the current Open Questions into an explicit acceptance decision before implementation: define behavior for modal, approval, picker, paste burst, and non-empty draft.
- Default to safety: modal/draft consumes the first press and the footer explains the next action; one-press return is guaranteed for the unobstructed side view.
- Add visible tests and help text for each exception so behavior is intentional rather than accidental.

### [Risk] Returning From Side Is Mistaken for Workspace Rollback

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.45  
**Risk Score**: 6.6 — medium

**Failure Scenario**: The conversation and rollout are ephemeral, but file changes already completed by child tools remain in the shared workspace. Users interpret “parent unchanged” or “temporary” as reverting those edits and discover persistent changes later.

**Mitigation**:

- Define “parent unchanged” narrowly as conversation/session/UI state unless side mode is explicitly made read-only.
- Ensure the side label/help states whether workspace edits are possible and that returning does not undo completed changes.
- If product intent requires no workspace mutation, enforce a read-only sandbox for side conversations and test write rejection; do not imply rollback without an actual transaction mechanism.

### [Risk] Partial Uncommitted Fixes Are Mistaken for the Planned Baseline

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.50  
**Risk Score**: 6.0 — medium

**Failure Scenario**: A prior uncommitted runtime change, a recent commit, and the new plan overlap. Implementation either duplicates the fix, loses the regression test, or evaluates behavior against a mixed baseline.

**Mitigation**:

- At execution start, record HEAD, staged diff, unstaged diff, and which existing changes belong to this task; do not assume a clean tree.
- Review the current runtime-lifetime diff against this hardened plan and retain only behavior supported by failing tests.
- Report pre-existing and newly created changes separately at handoff.

## Pre-Mortem Convergence Check

- Automatic risk-sweep iterations completed: 4.
- Residual HIGH risks after mitigation: 0.
- Total distinct risks recorded: 19.
- Initial HIGH risks (`Risk Score > 12`): 13.
- Medium/low risks: 6.
- Risks missing mitigation: 0.
- HIGH risks missing a residual assessment: 0.
- Residual HIGH risks after required mitigations: 0.
- Stop condition: the final full-category sweep produced no new HIGH risk or unmitigated causal mechanism.

## Post-Mortem — 2026-11-03

### Issues Found and Fixed

| ID | Severity | Finding | Fix |
| --- | --- | --- | --- |
| BUG-1 | High | A temporary Tokio runtime was dropped after side fork setup, killing the child submission loop. | Spawn fork, cleanup, and event-forwarding work through the `App` runtime handle. |
| BUG-2 | High | Stale-child cleanup was detached and could be cut off by immediate process exit. | Register every cleanup with `App` and boundedly join registered cleanup work during shutdown. |
| BUG-3 | High | Exiting while `Forking` could abort after allocation but before the UI received cleanup ownership. | Share an opaque cleanup-token owner between `Forking` state and the completion event; shutdown waits for the terminal fork result and reclaims the token. |
| BUG-4 | High | Timing out `dispose_ephemeral_conversation` cancelled physical cleanup and consumed its only token. | Treat timeout as a UI-unblock notification while physical cleanup continues under app ownership. |
| BUG-5 | High | `/jump-back` used the same temporary-runtime pattern and could create another dead long-lived conversation. | Move replacement-conversation creation onto the same app runtime. |
| BUG-6 | Medium | Global side `Ctrl+C` overrode terminal overlays, modal views, and composer drafts. | Gate side return on an unobstructed, empty-composer view; otherwise the focused UI handles the key first. |
| BUG-7 | High | `/btw` rejected an active `/goal`, and deferring hidden-parent events would stall Auto Drive waiting for coordinator ACK/countdown progress. | Remove all idle admission guards and route parent model/coordinator events to the retained parent widget in real time while the child remains visible. |

### Verification Evidence

- Focused side regressions: 12 passed, including active-parent admission, runtime lifetime, fork panic/error/timeout, cleanup timeout continuation, one user turn with no hidden developer turn, key press/repeat/release routing, draft precedence, and parent double-`Ctrl+C` reset.
- Required build: `./build-fast.sh` passed with zero warnings/errors.
- Final binary: `.code/working/_target-cache/qcodex/main-0d6e4079e367-e484d8df0573/code-rs/dev-fast/code`, SHA-256 `91a6637ac3f3b55f265c082ea3343ddf38a65b4a0a0d70324598165fc792e56a`.
- Runtime: `/btw 只回复 side-ok` returned `side-ok`; one `Ctrl+C` restored the retained parent; PID remained `22353`; the next parent `Ctrl+C` only armed the quit hint and the following press exited with status 0.
- Active-parent runtime: started `/goal`, entered `/btw 只回复 side-ok` while Auto Drive was running, returned with one `Ctrl+C`, and observed the parent had advanced to 19 seconds with plan/tool events before exiting cleanly with status 0.
- Cleanup/logs: child rollout id `2c6cebc7-3c08-40f9-bb9e-fb8aa80dea70` was absent after return; new logs contained no `InternalAgentDied`, `failed to submit op`, or cleanup timeout.

### Residual Risk Gate

All pre-mortem HIGH risks now have an ownership mechanism plus deterministic or runtime evidence. **Residual HIGH constraints: 0.**

### Prevention

- **AFK**: Keep the side lifecycle tests and the global scan for temporary runtimes that return long-lived conversations as release gates.
- **HITL**: When changing global key routing, review modal/draft precedence explicitly against the product interaction contract.
