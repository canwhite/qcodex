# Plan: Add Persistent `/goal` Support

> **User's request (restated)**: Learn from the `/goal` capability in `/Users/doing/Desktop/codex` and add equivalent `/goal` support to this project.
>
> **What this plan answers**: How to introduce a durable, user-visible `/goal` workflow while adapting the upstream design to this fork's existing Rust architecture.
>
> Goal: Users can create, inspect, modify, pause, resume, and clear a persistent session goal, and the agent can safely account for and finish that goal across turns.

## Context

`painpoints/pp3.md` asks to extend this project from the desktop Codex implementation. The upstream implementation is not a single slash-command patch: it has a `codex-goal-extension` crate, persisted per-thread goal state, app-server goal RPCs, TUI goal commands and UI state, model tools (`create_goal`, `get_goal`, `update_goal`), usage accounting, continuation steering, and feature gating.

This fork currently has no `code-rs/ext` workspace members, no goal-state storage, no goal RPCs, and no `SlashCommand::Goal`. It does have a mature TUI slash-command/event path and Auto Drive, which supplies useful existing lifecycle and completion concepts but must not be conflated with a persistent thread goal. Therefore the implementation should adapt upstream behavior in small, buildable slices instead of copying its extension framework wholesale.

Assumption: “add `/goal`” means functional parity for the core terminal workflow, with a durable per-session objective and explicit lifecycle controls. Advanced upstream-only integrations such as full app-server API compatibility and analytics may follow once the local TUI workflow is sound.

## Goal

For a saved interactive session, `/goal <objective>` stores one active objective and makes it visible to the agent and user. `/goal`, `/goal clear`, `/goal edit`, `/goal pause`, and `/goal resume` behave deterministically, survive resume, reject unavailable or invalid session states with actionable feedback, and never start unbounded continuation without an explicit lifecycle decision.

## Plan

1. Define the compatibility boundary before porting code. Read the upstream goal crate (`ext/goal`), its TUI dispatcher/menu/status files, goal-state migrations, and app-server request processors alongside this fork's `core`, `tui`, `app-server`, `protocol`, and Auto Drive ownership boundaries. Record an implementation map that names the local owner for storage, lifecycle, tool exposure, slash dispatch, rendering, and resume. Keep `codex-rs` read-only and make all Rust changes under `code-rs`.
2. Introduce a local goal domain model and persistence boundary. Add a versioned, session-keyed record containing objective, status, optional explicit token budget, timestamps, and accounting baselines; define the allowed status transitions (`active`, `paused`, `complete`, `blocked`, and system-owned limit states if enabled). Ensure migration/creation is atomic, only one unfinished goal can exist per session, and resumed sessions restore the record before accepting goal commands.
3. Add the core lifecycle service and model-facing tools. Implement `create_goal`, `get_goal`, and `update_goal` using the existing tool registration path, with schemas and instructions adapted from upstream. Enforce that a budget is accepted only when explicitly requested, a completed goal is truly finished, and `blocked` is allowed only after the defined repeated-blocker audit. Keep user-controlled pause/resume separate from model-controlled completion/blocking; make tool errors structured and stable.
4. Integrate goal state at turn and session boundaries. Capture token/usage baseline when a goal turn begins, update accounting from completed turns, preserve goal context across compaction/resume, and inject a continuation only when the session is idle, the goal remains active, and the feature policy permits it. Explicitly prevent continuation for plan mode, review/subagent contexts, cancelled turns, empty/error responses, paused goals, and session-limited goals. Reuse Auto Drive only where its existing stop/escape semantics fit; do not make `/goal` implicitly launch Auto Drive.
5. Add `/goal` to `code-rs/tui/src/slash_command.rs`, `chatwidget` dispatch, `AppEvent`, and the app command handler. Support bare `/goal` as a readable status/action menu; validate a non-empty objective for creation; route `clear`, `edit`, `pause`, and `resume` through typed events; request confirmation before replacing an unfinished goal; preserve pasted text and attachments when they form an objective; and append canonical command history without submitting the command as a normal model prompt.
6. Add focused TUI presentation. Render a compact footer/status indicator with objective progress and current state, plus clear informational/error history entries for unavailable persistent sessions, invalid transitions, storage failures, completion, and limit stops. Use the established modal-first key handling so Esc stops or pauses only the appropriate active flow and does not let an approval pane swallow the key.
7. Cover the behavioral contract with regression tests before and during implementation. Add core/service tests for persistence, transitions, tool schemas, explicit budgets, accounting, completion/blocking constraints, resume, and continuation suppression. Add TUI tests using `make_chatwidget_manual()` plus rendered snapshot/string assertions for command parsing, bare-menu states, replacement confirmation, disabled/ephemeral handling, attachment/paste preservation, footer status, and Esc behavior. Update `docs/slash-commands.md` only after behavior and syntax are final.
8. Perform integration hardening. Exercise create → work across multiple turns → pause/resume → complete; clear/replace; interrupted/error/empty turns; session resume; compaction; concurrent command submission; and Auto Drive coexistence. Scan all slash-command entry points, noninteractive `code exec` paths, agent/subagent creation paths, and session persistence paths for routes that could bypass the lifecycle policy or accidentally continue a goal.

## Think — Debug Methodology

- Start at boundary events: log a stable `[goal]` event with session/thread identifier, goal identifier, prior and next status, origin (user command, tool, lifecycle), and turn identifier. Do not log the full objective, attachments, or tool payloads.
- Read local source before making assumptions: `code-rs/tui/src/slash_command.rs`, `code-rs/tui/src/chatwidget.rs`, `code-rs/tui/src/app/events.rs`, the current tool-registration path, session persistence, and Auto Drive coordination. Compare each chosen seam with the corresponding upstream goal owner rather than copying an upstream module by filename.
- Localize in order: slash input parsing → typed app event → persistence/service call → lifecycle/tool registration → TUI redraw/status. Verify the persisted record and transition log before investigating continuation prompts or rendering.
- Search for `SlashCommand`, `DispatchCommand`, `submit_text_message`, `AppEvent`, session/thread resume, tool registration, Auto Drive coordinator start/stop, compaction, and `Esc` handlers. After each fix, search all matches for the same bypass or stale-state pattern.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — the mandatory completion gate; it must complete with zero errors and zero warnings.
- **Static analysis / type check**: N/A as a separate completion command — `./build-fast.sh` is this repository's required compiler and warning gate; do not add format, lint, or test commands unless the user explicitly requests them.
- **Runtime verification**: Start a saved interactive session and verify `/goal <objective>`, bare `/goal`, pause, resume, edit/replace, clear, and completion feedback. Restart or resume the session to prove persistence, then run an ordinary turn, an interrupted turn, and an Auto Drive run to prove no unintended continuation or Esc regression.
- **Logic correctness**: Walk every legal and rejected status transition, replacement confirmation decision, ephemeral/session-unavailable error, explicit-versus-implicit budget case, empty/error/cancelled turn, compaction/resume path, tool response, and concurrent/duplicate creation attempt. Confirm an active goal never becomes complete merely because a turn ends, and that only the designated owner can impose system limit statuses.

## Adjust — Rollback and Global Scan

- **Rollback plan**: Place goal support behind a dedicated feature/config gate until the persistence schema and lifecycle are stable. Disabling the gate must stop tool exposure, command availability, footer rendering, and automatic continuation while retaining records for a later compatible re-enable. If a schema migration must be removed before release, ship a forward migration that preserves historical records rather than deleting user goals.
- **Global scan**: Search every slash-command dispatch path, tool registry, turn/session lifecycle hook, state migration, footer/status renderer, compaction path, and Auto Drive entry/exit path for duplicate status models, direct prompt submission, missing feature checks, or continuation triggered without a persisted active goal.
- **Backwards compatibility**: Existing sessions without a goal record must behave exactly as today. Unknown persisted enum values should fail closed to a visible recoverable state rather than auto-continuing. Keep `/auto` behavior unchanged; `/goal` should be additive until an explicit product decision joins the workflows.

## Open Questions

- Should the first release include full upstream automatic continuation and token-budget accounting, or ship the persistent `/goal` command/menu and model tools first behind a gate? This plan assumes staged delivery, with continuation introduced only after the state machine is validated.
- Does this fork have an intended durable per-thread database/session-store abstraction to extend, or should the goal record initially live beside existing local session metadata? The decision determines the persistence implementation but not the public contract.
- Is app-server / SDK protocol parity required for the first release, or is the interactive TUI the only supported client initially?
- Should a user be able to set an explicit token budget in `/goal` syntax, or should budgets initially be model-tool-only as upstream recommends unless the user explicitly requests one?

## Out of Scope

- Copying the upstream extension framework, analytics/telemetry, or app-server protocol surface verbatim without adapting it to this fork.
- Changing existing Auto Drive semantics, command execution architecture, approval-pane Esc bubbling, or unrelated slash commands.
- Migrating or editing the read-only `codex-rs` mirror.
- A graphical dashboard, multi-goal prioritization, cross-device synchronization, or a public API beyond the agreed first-client scope.

---

> Next step: run `/pre-mortem docs/planning/goal-command.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Upstream Architecture Is Ported Instead of Adapted

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15.0 — HIGH

**Failure Scenario**: The team copies the upstream extension/app-server/state design into a fork that lacks those seams. The result is a partial parallel architecture, duplicated session ownership, broken builds, and a `/goal` feature that works only on one path.

**Mitigation**:

- Make the compatibility map in plan step 1 a blocking design-review artifact: every upstream responsibility must map to exactly one existing local owner or an explicitly approved new local module.
- Deliver the durable TUI workflow behind a feature gate before adding app-server protocol parity, analytics, or automatic continuation; do not add an extension framework merely to mirror upstream layout.
- Require a slice-level acceptance test and a clean `./build-fast.sh` before the next slice starts.

**Residual assessment after mitigation**: Severity 5 | Likelihood 2 | Detectability 0.70 | **Residual Risk Score: 3.0** — not high.

### [Risk] Goal Persistence Loses, Corrupts, or Misattributes Objectives

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10
**Risk Score**: 13.5 — HIGH

**Failure Scenario**: A goal is written non-atomically, associated with the wrong resumed session, or decoded from an incompatible schema. Users either lose their objective or see another session's objective and the agent acts on stale context.

**Mitigation**:

- Define one session-keyed record with schema version, unique active-goal constraint, explicit transition validation, and atomic create/replace/clear operations before wiring the TUI.
- Add migration fixtures for no record, old record, malformed record, and unknown status; fail closed with a recoverable visible error rather than restoring an active goal.
- Test restart/resume, concurrent create/replace attempts, and write failure injection; include goal id and session id—not objective text—in boundary logs.

**Residual assessment after mitigation**: Severity 5 | Likelihood 2 | Detectability 0.75 | **Residual Risk Score: 2.5** — not high.

### [Risk] Continuation Runs Work Without Current User Intent

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10
**Risk Score**: 13.5 — HIGH

**Failure Scenario**: Idle/resume/compaction hooks treat any old goal as active and automatically submit another turn after the user has paused, cancelled, entered plan mode, switched contexts, or reached a system limit. This can spend budget, mutate files, and make Esc unreliable.

**Mitigation**:

- Start with continuation disabled behind a separately default-off gate; enable it only after the persisted state machine, pause/resume, and stop semantics pass focused tests.
- Centralize continuation eligibility in one pure predicate requiring current saved session, active status, no user cancellation, no limit state, no active turn, supported mode, and an explicit policy flag. No other hook may enqueue a continuation directly.
- Test every suppressing condition plus a stale queued event after pause/clear; preserve the existing Auto Drive Esc routing and prove `/goal` never starts Auto Drive implicitly.

**Residual assessment after mitigation**: Severity 5 | Likelihood 2 | Detectability 0.80 | **Residual Risk Score: 2.0** — not high.

### [Risk] Lifecycle Races Create Duplicate Turns or Invalid Statuses

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75 — HIGH

**Failure Scenario**: Simultaneous tool calls, slash commands, resumed sessions, and turn-completion hooks observe different goal versions. The system double-continues, overwrites completion with pause, or charges one turn to two goals.

**Mitigation**:

- Serialize mutations through the goal service and use goal version/turn identifiers for compare-and-set transitions; make repeated requests idempotent.
- Make the turn-start baseline and terminal outcome commit one transaction or guarded sequence, then discard stale lifecycle events whose goal id/version no longer matches.
- Add deterministic tests for pause-versus-idle, clear-versus-tool-update, replacement-versus-resume, duplicate completion, and out-of-order turn events.

**Residual assessment after mitigation**: Severity 5 | Likelihood 2 | Detectability 0.75 | **Residual Risk Score: 2.5** — not high.

### [Risk] Model Tools Falsely Complete or Prematurely Block Work

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10
**Risk Score**: 13.5 — HIGH

**Failure Scenario**: A model calls `update_goal` at a plausible stopping point, causing the UI to represent unfinished work as complete, or labels a first obstacle as blocked and abandons work. Budget/reporting then records a misleading successful outcome.

**Mitigation**:

- Adopt the upstream completion and blocked-audit constraints verbatim in tool descriptions and enforce state legality in the service, including the repeated-blocker threshold.
- Keep a requirement checklist and final-verification evidence in the active goal context; refuse `complete` when required validation has not been recorded or the goal has unfinished queued work.
- Test false completion, first/second/third identical blocker, resumed-after-blocked audit reset, invalid system status request, and tool error wording.

**Residual assessment after mitigation**: Severity 5 | Likelihood 2 | Detectability 0.75 | **Residual Risk Score: 2.5** — not high.

### [Risk] Undefined First-Release Scope Leaves an Incomplete Feature

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.20
**Risk Score**: 12.8 — HIGH

**Failure Scenario**: “Equivalent `/goal`” is interpreted simultaneously as a TUI command, full app-server protocol, storage subsystem, continuation engine, budgets, telemetry, and SDK support. The team spends weeks on integration without a usable vertical slice and cannot tell what parity means.

**Mitigation**:

- Freeze a written release-1 contract before code: saved interactive TUI, one active persistent goal, command lifecycle, model tools, and no automatic continuation until explicitly promoted from the gated phase.
- Track app-server parity, SDK exposure, analytics, and cross-client behavior as separately approved milestones with owners and acceptance criteria.
- Treat every open question in the plan as a go/no-go decision at the design review; do not start the affected milestone without a recorded decision.

**Residual assessment after mitigation**: Severity 4 | Likelihood 2 | Detectability 0.80 | **Residual Risk Score: 1.6** — not high.

### [Risk] Objective or Attachment Data Leaks Through Diagnostics

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.50
**Risk Score**: 5.0 — medium

**Failure Scenario**: Debugging lifecycle issues leads to raw objectives, pasted content, attachment paths, or tool arguments being added to durable logs or telemetry.

**Mitigation**:

- Log opaque identifiers, command category, status, and counts only; redact objective and attachment payloads at the goal-service boundary.
- Add log assertions or a review checklist for goal diagnostics before enabling the feature gate.

**Residual assessment after mitigation**: Severity 5 | Likelihood 1 | Detectability 0.85 | **Residual Risk Score: 0.75** — low.

### [Risk] Feature Gate Creates Inconsistent Client Behavior

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.50
**Risk Score**: 4.5 — medium

**Failure Scenario**: The slash-command popup, TUI dispatcher, tools, footer, and resume lifecycle disagree about whether goals are enabled, exposing controls that later fail or continuing a disabled feature.

**Mitigation**:

- Resolve feature enablement once per session and pass the resulting policy into command discovery, service/tool registration, rendering, and lifecycle eligibility.
- Test disabled, enabled-new-session, enabled-resume, and gate-toggled scenarios; default to no tool exposure and no continuation on ambiguity.

**Residual assessment after mitigation**: Severity 3 | Likelihood 1 | Detectability 0.85 | **Residual Risk Score: 0.45** — low.

### [Risk] Budget Accounting Is Mistaken for a Hard Permission Limit

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.55
**Risk Score**: 5.4 — medium

**Failure Scenario**: Token budget baselines are missing after resume or model context compaction, and a negative/incorrect delta either stops work too early or allows work past the advertised goal budget.

**Mitigation**:

- Make budgets absent by default and introduce budget enforcement only after baseline provenance is available at every turn boundary.
- Store baseline source/version with the accounting record, reject impossible deltas, and surface “budget unavailable” instead of fabricating consumption.

**Residual assessment after mitigation**: Severity 4 | Likelihood 1 | Detectability 0.85 | **Residual Risk Score: 0.60** — low.

### [Risk] Upstream Changes Make the Learned Behavior Stale

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.60
**Risk Score**: 3.6 — medium

**Failure Scenario**: The desktop Codex source evolves while this fork is being ported, so copied policy text or assumptions no longer match a known-safe upstream behavior.

**Mitigation**:

- Pin the inspected upstream commit/path in the implementation-map artifact and record any deliberately diverged behavior.
- Before each later milestone, rescan upstream goal tests and public behavior for changes relevant to that milestone; port behavioral contracts, not source layout.

**Residual assessment after mitigation**: Severity 3 | Likelihood 1 | Detectability 0.85 | **Residual Risk Score: 0.45** — low.

### [Risk] Ownership Gaps Leave the Feature Unmaintainable

**Severity**: 4 | **Likelihood**: 2 | **Detectability**: 0.60
**Risk Score**: 3.2 — medium

**Failure Scenario**: Storage, core tools, TUI events, and continuation logic land in separate changes without a named contract owner. Later changes alter status semantics in one layer and silently break another.

**Mitigation**:

- Keep the status transition table and compatibility map beside the implementation and require changes to update them with the relevant tests.
- Split implementation reviews by vertical slice but require one final cross-layer review of the command-to-persistence-to-tool-to-render path.

**Residual assessment after mitigation**: Severity 4 | Likelihood 1 | Detectability 0.85 | **Residual Risk Score: 0.60** — low.

### Risk Sweep Result

- **Iteration 1 — technical and delivery sweep**: found 6 new HIGH risks: architecture mismatch, persistence corruption, unauthorized continuation, lifecycle races, premature model terminal status, and undefined release scope. Each received a mandatory mitigation and now has a residual score at or below 3.0.
- **Iteration 2 — security, feature-gating, budget, upstream-change, and ownership sweep**: found no new HIGH risks; five medium/low risks were recorded and mitigated.
- **High-risk constraint**: 0 residual risks have a score greater than 12. The feature must not leave design review or its gated release phase if a new or revised risk exceeds 12 without first adding and validating a mitigation.

---

## Post-Mortem: Persistent `/goal` Support

**Date**: 2026-11-03

### Implementation Map

The TUI parses and dispatches `/goal` through `slash_command.rs`, `chatwidget.rs`, and `app/events.rs`. Session-scoped state is stored atomically under the Code home directory using the restored session UUID. The core registers `create_goal`, `get_goal`, and `update_goal`, dispatching each through the existing function-tool path to the same record.

### Issues Fixed

| # | Severity | Type | Resolution |
|---|---|---|---|
| BUG-1 | High | Data-contract loss | Preserved blocker metadata in the TUI representation so pause/edit actions cannot reset the model-side blocked audit. |
| BUG-2 | High | State overwrite | Reject creation when an active or paused goal exists; explicit `/goal edit <objective>` is the safe editing route. |
| BUG-3 | High | Terminal-state violation | Require evidence for completion and three identical blocker observations before `blocked` can be recorded. |
| BUG-4 | Medium | Partial-write exposure | Changed record writes to same-directory temporary-file replacement. |
| BUG-5 | Medium | UX contract gap | Added `/goal edit <objective>` and bottom-pane confirmation feedback. |

### Verification

- `cargo check -p code-core -p code-tui` passed.
- `./build-fast.sh` passed with zero errors and warnings.
- The TUI goal-record regression covers persistence round-trip and clear; slash-command recognition is covered in `slash_command.rs`.

### Residual Risk Review

- Concurrent writers remain serialized by the single interactive session event loop; the atomic replacement prevents readers from observing partial JSON. Residual score: 4.0.
- Completion evidence is supplied by the model rather than independently proved by a verifier; the tool now requires non-empty evidence and status transitions are durable. Residual score: 6.0.
- Automatic continuation, app-server protocol parity, and analytics were intentionally not introduced because they would change existing Auto Drive/session semantics; the `/goal` feature is additive and does not auto-run work. Residual score: 6.0.

**High-risk constraint**: 0 residual risks score greater than 12.
