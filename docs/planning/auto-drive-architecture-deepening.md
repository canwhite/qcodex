# Plan: Deepen Auto Drive Architecture

> **User's request (restated)**: Create an implementation plan from `painpoints/pp11.md` for the identified Auto Drive architecture improvements.
>
> **What this plan answers**: It orders the four deepening opportunities into a reversible implementation sequence while preserving Auto Drive behavior and its existing user-facing flows.
>
> Goal: Make Auto Drive lifecycle, coordinator, transcript projection, and model-routing behavior easier to change and test by concentrating each concern in one deep module.

## Context

`painpoints/pp11.md` identifies four related sources of architectural friction: direct lifecycle phase mutation, an oversized coordinator run loop, transcript projection inside `ChatWidget`, and duplicated model-routing normalization. The lifecycle module is the first priority because its transition logic already exists but production code can bypass it.

The work must keep Auto Drive's TUI and non-interactive exec behavior compatible. `code-rs` is the editable Rust workspace; `codex-rs` remains a read-only mirror. The repository's required completion gate is `./build-fast.sh`, which must finish without warnings.

## Goal

After the work, lifecycle changes flow through one owned module, coordinator internals are grouped by responsibility behind their existing external seam, transcript conversion is independently testable outside `ChatWidget`, and model-routing rules have one authoritative implementation. Existing launch, prompt, countdown, review, recovery, stop, and exec flows retain their current observable behavior.

## Plan

1. Establish behavioral baselines before refactoring. Inventory every production `set_phase` and direct `phase` mutation, every `AutoCoordinatorEvent` consumer, every conversation-export path, and every routing normalization/resolution path. Classify each lifecycle mutation by intent: launch, prompt scheduling, manual pause/resume, diagnostics, review, transient recovery, restart, or stop.
2. Deepen the Auto Drive lifecycle module in `code-rs/code-auto-drive-core/src/controller.rs`. Consolidate all valid phase transitions and resulting effects there, make direct phase mutation unavailable to production callers, and update `ChatWidget` to request lifecycle intent rather than assemble state changes itself. Keep test-only setup helpers narrowly scoped when direct initial state is needed.
3. Add lifecycle regression coverage at the controller interface. Cover successful and failed launch, coordinator prompt scheduling and acknowledgement, countdown expiry, manual edit/resume, diagnostics/review lock acquisition failure, transient failure/restart token mismatch, and stop from every active phase. Assert both final phase and emitted effects.
4. Extract transcript projection from `ChatWidget` into a focused module owned by the TUI crate. Move history-cell role mapping, diff and browser screenshot conversion, source-index tracking, and compaction-overlay correspondence together. Preserve `ChatWidget` as the caller that supplies visible history and applies the returned projection.
5. Add transcript-projection tests using constructed history cells and response items. Verify ordering, omitted display-only cells, user versus assistant role conversion, diff summary generation, screenshot attachment behavior, and compaction replacement correspondence without rendering a terminal.
6. Split `auto_coordinator.rs` by internal responsibility without changing its external seam: isolate route preparation, coordinator prompt/schema construction, and conversation/decision-loop maintenance. Keep ordering, acknowledgement sequencing, cancellation, compaction notices, and event payloads unchanged at the existing coordinator seam.
7. Add focused coordinator tests at the existing event/command seam for bootstrap planning, queued updates while awaiting acknowledgement, stop/cancellation, compaction, and retry/recovery. Treat this as an in-process dependency: test behavior directly rather than adding a new adapter.
8. Create one authoritative model-routing rules module near the shared Auto Drive configuration types. Migrate validation, normalization, default selection, available-model filtering, and authorization-aware resolution into it. Let config loading persist canonical values, while TUI and coordinator consume the same rules at their existing seams.
9. Add table-driven routing tests covering invalid and duplicate entries, default fallback, enabled/disabled routes, allowed reasoning levels, ChatGPT versus API-key eligibility, and no available CLI models. Update TUI and coordinator tests only for their adapter-specific behavior.
10. Perform a global scan for remaining direct lifecycle mutation, transcript serialization in `ChatWidget`, and independently implemented routing normalization. Remove obsolete tests that inspect replaced shallow modules; retain only tests crossing the deepened modules' interfaces.

## Think — Debug Methodology

- Start at framework boundaries: coordinator commands/events, `ChatWidget` app events, config load/save, and exec's coordinator consumer. Read the existing source and tests before inferring intent from a state name.
- When behavior diverges, first record the incoming command/event, current Auto Drive phase, and returned controller effects using the existing `auto_drive::coordinator` tracing target. Add temporary logs only at those seams and remove them before completion.
- Localize lifecycle issues upstream to the action that requested a transition, then downstream to effect application. Do not debug by mutating `AutoRunPhase` in place.
- Search for `set_phase`, `AutoRunPhase`, `AutoCoordinatorEvent`, `export_auto_drive_items`, `derive_compaction_overlay`, `sanitize_auto_drive_routing_entries`, and `normalize_auto_drive` after each extraction to find latent twins.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with no errors or warnings.
- **Static analysis / type check**: N/A as a separate command — repository policy designates `./build-fast.sh` as the single required completion gate; do not add an unrelated lint or formatter run.
- **Runtime verification**: Exercise one TUI Auto Drive run and one `code exec --auto` run only when an appropriate authenticated environment and safe goal are available. Confirm lifecycle transitions, countdown, review, recovery, completion, and cancellation all reach the intended visible state; otherwise record that the build and interface-level regressions are the available verification.
- **Logic correctness**: Walk launch success/failure, planning decision, normal continue decision, coordinator success diagnostics, coordinator permanent/transient failure, stale acknowledgement, queued conversation update, manual input, review-lock contention, restart token mismatch, Esc stop, and non-interactive exec. For each, verify there is exactly one state owner, one effect application path, and no event reordering.

## Adjust — Rollback and Global Scan

- **Rollback plan**: Land the work as small commits by deepened module. Each extraction preserves existing external seams, so reverting its commit restores the prior implementation without a data migration.
- **Global scan**: Search all Rust crates under `code-rs` for Auto Drive phase writes, coordinator event matching, response-item construction from history cells, and `AutoDriveModelRoutingEntry` normalization. Resolve every production twin or document why it is adapter-specific.
- **Backwards compatibility**: Preserve persisted Auto Drive config shape, coordinator event and command payloads, CLI flags, TUI shortcuts, and visible status messages. Routing canonicalization may normalize invalid local settings but must not silently discard valid configured routes.

## Open Questions

- Whether the four extractions should be one delivery or four independently reviewable deliveries. This plan assumes lifecycle and transcript projection first, followed by coordinator and routing rules.
- Which existing TUI harness can safely drive authenticated Auto Drive runtime verification without contacting production services. The plan assumes controller, coordinator, and transcript tests provide the primary deterministic coverage.
- Whether any non-TUI consumer relies on public raw lifecycle phase mutation. The initial global scan must establish this before making the mutator private.

## Out of Scope

- Changing Auto Drive product behavior, prompts, model choices, routing policy, or stored configuration schema beyond centralizing existing rules.
- Replacing the coordinator transport or introducing a new network seam.
- General `ChatWidget` decomposition unrelated to Auto Drive transcript projection.
- Editing the `codex-rs` mirror.

---

> Next step: run `/pre-mortem docs/planning/auto-drive-architecture-deepening.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Lifecycle extraction changes effects while preserving phase names

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 16

**Failure Scenario**: The refactor reaches the expected `AutoRunPhase`, but omits an effect previously performed beside a direct mutation: clearing a view, cancelling a coordinator, releasing a review lock, restoring input focus, or updating the terminal hint. Auto Drive appears stuck or keeps work alive after the user stops it.

**Mitigation**:

- For every removed production phase write, record the old side effects and map it to one controller intent before changing code.
- Make lifecycle tests assert the complete ordered effect list as well as the resulting phase, particularly for stop, recovery, review completion, and launch failure.
- Land lifecycle ownership separately from unrelated extraction work and manually exercise Esc stop before proceeding.

### [Risk] Coordinator extraction reorders acknowledgement, cancellation, or queued updates

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15

**Failure Scenario**: Splitting the coordinator loop accidentally processes a conversation update before an acknowledgement, accepts an old acknowledgement after restart, or emits a decision after cancellation. The TUI and exec adapters then run a stale CLI prompt or show an incorrect Auto Drive card.

**Mitigation**:

- Treat decision sequence, pending acknowledgement, queued update order, and cancellation as one explicit stateful unit during extraction; do not distribute their ownership across new modules.
- Add event/command sequence tests for ack-before-update, update-while-awaiting-ack, stale ack, stop-before-ack, and cancellation during streaming.
- Preserve event payloads and order as a compatibility contract; compare pre- and post-refactor traces for the baseline scenarios.

### [Risk] Transcript projection corrupts compaction correspondence or visible ordering

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: A projection extraction loses a diff or screenshot, assigns an item the wrong role, or mismatches source indices after compaction. Later coordinator turns see a different conversation from the user-visible history, causing repeated work or unsafe decisions.

**Mitigation**:

- Establish fixture-based before/after projections from representative live history, including overlays, diffs, screenshots, notices, and mixed user/assistant turns.
- Make source index and compaction correspondence explicit outputs of the projection module and test them alongside response items.
- Keep the original projection implementation available until fixture outputs match byte-for-byte except for intentionally documented corrections.

### [Risk] Shared routing rules silently alter valid saved configuration

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.15
**Risk Score**: 13.6

**Failure Scenario**: Centralization conflates persistence validation with runtime availability or auth filtering. A valid configured route is removed or replaced on load, or a route displayed by the TUI cannot be used by the coordinator for a particular authentication mode.

**Mitigation**:

- Specify normalization, defaulting, persistence, authorization filtering, and runtime availability as separate rule stages before moving code.
- Use table-driven tests with serialized legacy and current settings; assert canonical persisted output independently from auth-specific runtime resolution.
- Retain unknown-but-valid saved entries unless the existing behavior explicitly rejects them, and display a reason when runtime filtering hides a route.

### [Risk] Four coupled extractions become one unreviewable behavioral rewrite

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.15
**Risk Score**: 13.6

**Failure Scenario**: Lifecycle, transcript, coordinator, and routing changes land together. A regression cannot be attributed to one change, reviews focus on file moves rather than behavior, and rollback removes unrelated fixes.

**Mitigation**:

- Use four independently buildable commits or pull requests in the planned order; each preserves existing external seams and includes its regression coverage.
- Require a behavior inventory and passing build for each delivery before starting the next extraction.
- Reject opportunistic `ChatWidget` cleanup and Auto Drive product changes from these deliveries; log them separately.

### [Risk] Test setup reintroduces raw phase mutation as a production escape hatch

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.35
**Risk Score**: 7.8

**Failure Scenario**: Tests need convenient state setup, so a public raw phase setter remains. Future production code uses it again and bypasses the deepened lifecycle module.

**Mitigation**:

- Restrict state-fixture construction to test-only code and give it a name that cannot be mistaken for a lifecycle action.
- Scan production crates for phase setters after the refactor and make direct production mutation impossible through visibility.

### [Risk] New modules introduce dependency cycles across core, TUI, and exec

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.8
**Risk Score**: 1.8

**Failure Scenario**: A transcript or routing extraction pulls TUI-only history types into core, or coordinator behavior into config, producing cyclic crate dependencies or a leaky seam.

**Mitigation**:

- Keep transcript projection in the TUI crate and routing rules beside shared configuration types; pass protocol values rather than UI types across crate seams.
- Check intended crate ownership before creating each module and let `./build-fast.sh` gate every extraction.

### [Risk] Live Auto Drive verification is unavailable or unsafe

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.6
**Risk Score**: 3.6

**Failure Scenario**: Authenticated model access or a safe project fixture is unavailable, so the refactor ships based only on compilation and incomplete unit coverage.

**Mitigation**:

- Make deterministic controller, coordinator, transcript, and routing regression cases the required evidence before live verification.
- When live verification is unavailable, retain a documented event trace for TUI and exec adapters and schedule authenticated smoke verification before release.

---

## Post-Mortem

**Date**: 2026-11-04

### Issues Fixed

| # | Severity | Type | Location |
|---|---|---|---|
| 1 | Medium | Behavioral regression | `code-rs/tui/src/chatwidget.rs` |

The countdown bypass path was initially changed from a direct transition to the manual-resume intent. That intent also bumps the snapshot epoch, which was not part of the prior behavior. It now uses the side-effect-free turn-continuation intent, preserving the original transition semantics.

### Audit Results

- Lifecycle: production phase mutation is private to `AutoDriveController`; production callers use named lifecycle intents. Remaining raw setters are compiled only for tests or test helpers.
- Coordinator: CLI routing preparation is isolated in `coordinator_routing`, while the existing coordinator command/event seam remains unchanged.
- Transcript: compaction prefix/suffix correspondence is isolated in `auto_transcript` and covered by focused regression cases.
- Routing: config loading, TUI filtering, and coordinator resolution now share canonical persisted-entry normalization. Runtime availability and authentication filtering remain separate stages.
- Verification: `./build-fast.sh` passed cleanly after the final fix, with no errors or warnings.

### Root Cause

The lifecycle refactor initially grouped two transitions that both lead to `Active` but have different side effects. The phase value alone was an insufficient representation of the caller's intent.

### Prevention Tasks

| Task | Type | Status |
|---|---|---|
| Keep named lifecycle intents distinct when their effects differ | AFK | Completed |
| Keep compaction correspondence outside `ChatWidget` rendering code | AFK | Completed |
| Centralize persisted routing normalization before adapter filtering | AFK | Completed |

### High-Risk Constraints

Unresolved high-risk findings: **0**. The five pre-mortem high-risk areas now have implemented ownership seams, focused regression coverage where behavior was extracted, and a clean repository build gate.
