# Plan: Surface raw-reasoning status and remote tool lifecycles

> **User's request (restated)**: Write a repair plan for the case where the TUI shows a large unlabelled block of activity while neither Thinking nor a tool card explains what the agent is doing.
>
> **What this plan answers**: How to give the TUI privacy-safe reasoning status and structured tool lifecycle events when a provider supplies raw reasoning and function-call outputs without summaries or local exec events.
>
> Goal: Every visible foreground turn has an accurate compact activity affordance—`Thinking…` for active reasoning and a tool card for tool work—without exposing raw reasoning unless the user explicitly enables it.

## Context

The captured 2026-11-05 session has an exact, reproducible wire shape:

- `ResponseItem::Reasoning` contains non-empty `content.reasoning_text`, but its `summary` is empty.
- The default `show_raw_agent_reasoning = false` makes both the streaming `ReasoningContentDelta` path and completed-item mapping omit that content entirely. No `AgentReasoning*` event reaches `ChatWidget`, so the compact thinking renderer added for `<think>` tags has nothing to display.
- The same turn includes a `shell` `FunctionCall` followed by a provider-returned `FunctionCallOutput`. The stream dispatcher currently logs direct outputs as unexpected and drops them instead of completing a structured history cell. The raw result is consequently visible without a command/tool identity.

This plan supersedes the unfinished raw-reasoning and tool-lifecycle portion of [reasoning-tool-call-visibility.md](reasoning-tool-call-visibility.md). It does not alter the prior leading-`<think>` parser fix: that path already produces reasoning events and is covered separately.

## Goal

For the captured response shape and all equivalent supported paths:

1. A non-empty raw-reasoning stream produces one collapsed `Thinking…` row immediately, then `Thinking` at completion, even when raw content remains disabled.
2. Raw reasoning bytes appear only when `show_raw_agent_reasoning = true`; the default compact row contains no private text and Ctrl+R has no body to reveal.
3. Every `FunctionCall`/`FunctionCallOutput` pair produces or completes one matching tool card, with no bare tool output and no duplicate local command execution.
4. Stable ids and `OrderMeta` preserve the strict per-turn history order across reasoning, tool calls, outputs, and final answer.

## Plan

1. Freeze the reported session as a minimal synthetic protocol fixture. Represent: raw-only reasoning with an empty summary, a `shell` function call, its direct function-call output, and a final answer. Use synthetic short strings only; assert event kinds, call ids, stream ids, order metadata, and visible labels rather than copying production thought or output.
2. Define a metadata-only reasoning lifecycle contract at the core-to-TUI protocol boundary. The event must carry a stable non-empty stream id and `OrderMeta`, distinguish start/update from completion, and never carry raw content merely to express activity. Reuse an existing protocol mechanism only if it can express this without blank-string sentinels; otherwise add the smallest explicit event/type and handle persistence/replay deliberately.
3. Normalize raw reasoning at both sources:
   - For streamed `ResponseEvent::ReasoningContentDelta`, emit the metadata-only start/update event whenever the delta is non-empty; emit raw `AgentReasoningRawContentDelta` only when `show_raw_agent_reasoning` is enabled.
   - For completed `ResponseItem::Reasoning` with empty summaries and non-empty content, emit a matched metadata-only start/completion lifecycle so terminal-only providers still get one compact row. When raw display is enabled, append the actual body through the existing raw-reasoning path.
   - Keep `hide_agent_reasoning = true` authoritative: it must suppress both status and content, as it does for other reasoning.
4. Teach `ChatWidget` and `StreamController` to render a status-only reasoning cell. It must use the existing collapsed `Thinking…`/`Thinking` projection, retain zero raw lines when raw display is disabled, finalize at every terminal boundary, and never create a phantom cell for an empty lifecycle event. Reconcile new metadata events with existing `AgentReasoning*`, section breaks, interruption, replay, Ctrl+R, and closed-id guards.
5. Normalize direct provider function outputs into the existing TUI tool lifecycle rather than treating them as unexpected. On `FunctionCall`, register a call-id keyed pending lifecycle with its tool name, arguments, and order. On `FunctionCallOutput`, locate that lifecycle and issue the appropriate begin/update/end history events; if the call already has a local exec lifecycle, attach/complete that cell rather than executing or rendering a second command. If an output arrives without a visible begin, create a completed generic tool card labelled by provider tool name and call id. Never insert the output as plain assistant/reasoning text.
6. Audit all tool variants at the normalization boundary—`shell`/`container.exec`, `LocalShellCall`, browser, custom, MCP, tool-search, dynamic tools, and replayed response items. Make a small table of which source owns execution and which event completes the card. Preserve current local-exec behavior and reject malformed or orphaned outputs with a bounded diagnostic, not a raw-history fallback.
7. Add focused regression coverage:
   - core event-sequence tests for raw-only streaming and terminal-only reasoning under each `hide_agent_reasoning × show_raw_agent_reasoning` combination;
   - a duplicate-safe function-call/output test proving one card and no second execution for the captured remote-output form;
   - TUI/VT100 tests showing `Thinking…` during activity, `Thinking` after completion, no private text in default mode, raw text only in opt-in mode, one labelled tool card, and preserved final answer order;
   - replay/resume and interruption tests for an active status-only row and pending tool call.
8. Update user-facing configuration documentation only to clarify the new distinction: raw reasoning remains opt-in, while a privacy-safe `Thinking` activity indicator appears when the provider reports reasoning. Do not claim support for unclassified provider fields.

## Think — Debug Methodology

- Start from the two event boundaries, not the renderer: `ResponseEvent::ReasoningContentDelta` / completed `ResponseItem::Reasoning`, then `ResponseItem::FunctionCall` / `FunctionCallOutput` in `code-rs/core/src/codex/streaming.rs` and `code-rs/core/src/event_mapping.rs`.
- Before changing behavior, add temporary redacted `[DEBUG-activity-lifecycle]` logs with only source variant, stream/call id, request/output/sequence order fields, raw-enabled flag, chosen lifecycle event, and duplicate/orphan decision. Never log thought, tool arguments, or output. Remove the logs before completion.
- Build the primary feedback loop from the synthetic fixture and collect the emitted `EventMsg` sequence before rendering. A failing assertion must distinguish “no lifecycle emitted,” “event emitted but wrong id/order,” and “TUI dropped an otherwise valid event.”
- Then use `ChatWidgetHarness` plus the VT100 renderer to inspect the exact frame. Verify status-only cells at narrow width, after history insertion, after a function output, and after a final answer.
- Grep all producers and consumers of `AgentReasoning*`, `ReasoningContentDelta`, `ResponseItem::Reasoning`, `FunctionCallOutput`, `ExecCommandBegin`, `ExecCommandEnd`, `CustomToolCall*`, and tool-card factories. The first local fix is insufficient if replay or a non-shell tool still has the old bypass.

## Do — Verification Strategy

- **Focused core regressions**: run the new event-sequence tests. They must prove that raw-only reasoning emits a status lifecycle in default mode without raw bytes, and that direct `FunctionCallOutput` completes exactly one matching lifecycle.
- **Focused TUI regressions**: run the new ChatWidget/VT100 target(s). Assert live `Thinking…`, completed `Thinking`, Ctrl+R behavior in raw-disabled/enabled modes, tool-card label/state, no bare synthetic tool output, and final-answer order.
- **Config truth table**: test all four combinations of `hide_agent_reasoning` and `show_raw_agent_reasoning` for summary, raw-only, and no-reasoning input. `hide_agent_reasoning` wins; raw bytes require explicit opt-in.
- **Replay/runtime verification**: replay the sanitized fixture through the same persisted-session or response-item reconstruction path as the captured session. Confirm it produces the same cards after resume and does not re-run a completed remote tool call.
- **Logic correctness**: walk streaming delta → final reasoning, terminal-only reasoning, section break, interruption, duplicate/late deltas, function output before/after local completion, output without call, concurrent calls, and interleaved answer/reasoning/tool order. Every path needs a stable `(request_ordinal, output_index, sequence_number)` and a non-empty stream or call id at insertion.
- **Build**: run `./build-fast.sh` from the repository root. It must pass cleanly with no errors or warnings. Do not run rustfmt.

## Adjust — Rollback and Global Scan

- **Rollback plan**: keep metadata lifecycle emission, tool-output normalization, fixtures, and TUI consumers in a focused commit. Reverting it restores the former silent raw-reasoning / direct-output behavior without a schema migration or persisted-data rewrite.
- **Global scan**: search all response adapters, alternate task/side-session paths, rollout replay, headless `exec`, transcript export, and standard-terminal mirroring for a raw-only reasoning or direct function-output bypass. Scan every tool type for duplicate begin/end ownership.
- **Backwards compatibility**: preserve existing summary reasoning, explicit raw reasoning, local `ExecCommandBegin/End` cards, and Ctrl+R semantics. Existing records that lack lifecycle events remain renderable; replay must not fabricate or execute tools merely because it sees an old output item.

## Open Questions

- Should a default-mode status-only row remain expandable but empty, or should Ctrl+R omit it entirely until raw reasoning is explicitly enabled? This plan assumes the row stays consistent with all reasoning entries, while expansion reveals no content by design.
- Which protocol event type is the narrowest durable vehicle for metadata-only reasoning lifecycle without imposing a blank-text convention on non-TUI consumers?
- Is the direct `FunctionCallOutput` always an already-executed remote result, or can it coexist with local execution for the same call id? The fixture and call-id ownership table must answer this before changing dispatch to avoid duplicate side effects.

## Out of Scope

- Displaying raw chain-of-thought by default, inferring thought from natural-language answer text, or weakening `show_raw_agent_reasoning` consent.
- Changing compatible-provider `<think>` parsing rules, provider authentication, model selection, or tool argument repair.
- Redesigning tool-card visuals or unrelated scrollback/history layout.
- Editing the read-only `codex-rs` mirror; Rust implementation work belongs under `code-rs`.

---

> Next step: run `/pre-mortem docs/planning/raw-reasoning-tool-lifecycle-visibility.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

**Scenario**: It is six months later. A provider's raw reasoning was exposed despite the default privacy setting, a reflected tool result ran twice, or protocol ordering corruption made the activity cards misleading. The model appeared active but the UI still could not truthfully explain what it was doing.

### [Risk] Privacy-safe status accidentally carries or reveals raw reasoning

**Initial Severity**: 5 | **Initial Likelihood**: 3 | **Initial Detectability**: 0.1
**Initial Risk Score**: 5 × 3 × (1 - 0.1) = 13.5 (HIGH)

**Failure Scenario**: A metadata-only lifecycle is implemented by reusing a text-bearing reasoning event with an empty/placeholder convention. One renderer, replay path, log, or export later treats the body as ordinary reasoning and exposes raw thought while `show_raw_agent_reasoning = false`.

**Mitigation**:

- Use an explicit status-only protocol representation whose payload cannot contain raw text; do not encode lifecycle state with sentinel reasoning strings or a hidden content field.
- Make the config truth-table test assert raw-token absence in live TUI frames, replay, session persistence, standard-terminal output, and transcript export whenever raw display is disabled.
- Add a redaction-focused code review gate: every new producer/consumer of the status event must prove it only reads ids, order metadata, and lifecycle state.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.95
**Residual Risk Score**: 5 × 1 × (1 - 0.95) = 0.25

### [Risk] Reflected function output causes a duplicate or destructive tool execution

**Initial Severity**: 5 | **Initial Likelihood**: 4 | **Initial Detectability**: 0.2
**Initial Risk Score**: 5 × 4 × (1 - 0.2) = 16.0 (HIGH)

**Failure Scenario**: The provider returns a `FunctionCall` and an already-executed `FunctionCallOutput`, while the local dispatcher also treats the call as executable. Normalizing the output without a call-id ownership state executes a command twice, potentially mutating files or network state twice.

**Mitigation**:

- Define a call-id state machine (`remote-reflected`, `local-running`, `completed`) at the core boundary. Only `local-running` may start a local executor; a reflected output can only complete or replace the matching UI lifecycle.
- Create a test double that increments a counter if execution is attempted. Replay FunctionCall → FunctionCallOutput and assert exactly zero or one execution according to the explicitly selected ownership, never two.
- Reject duplicate terminal outputs idempotently and log only call id, source ownership, and transition—never tool arguments/output.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.95
**Residual Risk Score**: 5 × 1 × (1 - 0.95) = 0.25

### [Risk] Synthetic status or tool cards violate strict stream ordering

**Initial Severity**: 5 | **Initial Likelihood**: 3 | **Initial Detectability**: 0.15
**Initial Risk Score**: 5 × 3 × (1 - 0.15) = 12.75 (HIGH)

**Failure Scenario**: A lifecycle event uses an empty/reused id or sequence number. Under interleaved reasoning, function outputs, and answer deltas, the history registry replaces the wrong cell, drops an insert, or puts the final answer before the operation that produced it.

**Mitigation**:

- Derive status stream ids from the provider reasoning item id and tool cards from call id; retain the original `(request_ordinal, output_index, sequence_number)` on every lifecycle transition. Missing ids must follow the existing strict drop/error contract, never a broad fallback.
- Add table-driven sequence assertions before UI rendering for raw-only reasoning, terminal-only reasoning, two interleaved calls, output-before-answer, and duplicate/late events.
- Add a multi-frame VT100 test that checks visual order and exact one-cell cardinality after every lifecycle transition.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.95
**Residual Risk Score**: 5 × 1 × (1 - 0.95) = 0.25

### [Risk] Status-only reasoning remains stuck or creates phantom activity

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.45
**Risk Score**: 4 × 3 × (1 - 0.45) = 6.6

**Failure Scenario**: A cancellation, stream switch, terminal-only item, or duplicate final skips lifecycle completion. The UI stays on `Thinking…`; conversely, empty provider fragments create a Thinking row when no thought occurred.

**Mitigation**:

- Make completion ownership explicit for final response items, task completion, interruption, and stream switching; reuse the closed-id guard rather than inventing a parallel flag.
- Test empty, duplicate, late, interrupted, and terminal-only inputs in both raw-enabled and raw-disabled modes.

### [Risk] Old sessions or replay manufacture activity or re-run tools

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.55
**Risk Score**: 5 × 2 × (1 - 0.55) = 4.5

**Failure Scenario**: Replaying prior response items synthesizes fresh cards from historical outputs or treats an old function call as executable. The resumed transcript changes and can trigger side effects.

**Mitigation**:

- Separate live dispatch from replay projection with an explicit non-executable replay mode; replay may render persisted terminal state but cannot invoke a tool runner.
- Test an old fixture without lifecycle records and the new fixture with them, asserting stable transcript output and zero executor invocations.

### [Risk] Scope broadens into provider-specific or visual redesign work

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.8
**Risk Score**: 3 × 3 × (1 - 0.8) = 1.8

**Failure Scenario**: The fix expands into parsing arbitrary natural-language thought markers, changing tool styling, or repairing malformed provider JSON, delaying the two protocol contracts that caused the reported failure.

**Mitigation**:

- Restrict the implementation review to lifecycle normalization, config/privacy tests, and existing card renderers. Record unsupported provider shapes with redacted diagnostics for a separate follow-up.
- Split any provider parser change or visual redesign into a separate issue/plan before coding it.

### [Risk] Status events create unbounded history churn for long streams

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.55
**Risk Score**: 4 × 3 × (1 - 0.55) = 5.4

**Failure Scenario**: Emitting a new history event for every raw-reasoning delta invalidates height caches continuously and makes long turns sluggish, even though the visible UI has one status row.

**Mitigation**:

- Coalesce start/update transitions by stable reasoning id; only the first non-empty delta creates a cell, and subsequent updates mutate lifecycle state without adding history rows.
- Add a many-delta fixture with assertions for one cell, bounded event/card count, and normal commit completion.

### [Risk] New protocol events are not understood by an older replay or app-server consumer

**Severity**: 4 | **Likelihood**: 2 | **Detectability**: 0.65
**Risk Score**: 4 × 2 × (1 - 0.65) = 2.8

**Failure Scenario**: A consumer built against the prior event enum rejects the metadata lifecycle event, making sessions unreplayable or silently losing a status row.

**Mitigation**:

- Check protocol serialization/versioning before selecting the new event shape; prefer an additive, forwards-safe event and retain a no-op fallback for non-TUI consumers.
- Add protocol round-trip and replay tests that include an unknown/absent lifecycle record.

### [Risk] Configuration semantics make hidden reasoning appear unexpectedly

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.7
**Risk Score**: 5 × 2 × (1 - 0.7) = 3.0

**Failure Scenario**: Users who set `hide_agent_reasoning = true` see the new Thinking status, or users who enable raw reasoning still see no expandable content because the two controls are evaluated in different layers.

**Mitigation**:

- Centralize the `hide_agent_reasoning` and `show_raw_agent_reasoning` truth table at the event-emission boundary and test it before TUI rendering.
- Document the two settings in one short contract and test all combinations for live and replayed events.

### [Risk] Provider sends a new tool shape or incomplete identifiers

**Severity**: 4 | **Likelihood**: 2 | **Detectability**: 0.6
**Risk Score**: 4 × 2 × (1 - 0.6) = 3.2

**Failure Scenario**: An unknown provider tool shape bypasses normalization or has no usable call id, leading to an unlabelled output despite the new known-shape tests.

**Mitigation**:

- Restrict normalization to typed supported `ResponseItem` variants; emit a bounded redacted diagnostic and safe generic notice for unknown structured calls rather than displaying their raw output.
- Preserve the strict id contract for insertions and add a missing-id fixture that proves no unrelated card is overwritten.

### Convergence review

- **Iteration 1** found three HIGH risks: raw-reasoning privacy leakage, duplicate/destructive tool execution, and lifecycle ordering corruption. Their mitigations establish typed status payloads, call-id ownership, and order/cardinality assertions; residual scores are each 0.25.
- **Iteration 2** swept stream volume, event-version/replay compatibility, configuration semantics, provider evolution, scope, and operational ownership. It found no previously unrecorded HIGH risk.
- **Remaining HIGH risks**: 0. Every recorded risk has a mitigation; the highest remaining non-HIGH score is 6.6.

---

## Post-Mortem Audit (2026-11-05)

### Issues fixed during the audit

1. **Expanded status-only reasoning could vanish.** A blank structural seed created a cell, but expanded rendering returned no lines when the cell had no raw body. The renderer now shows `Thinking…` / `Thinking` for an empty status-only cell in both collapsed and expanded modes, with a direct unit regression.
2. **Provider-reflected tool output could arrive before its call.** The first implementation only claimed an output after seeing a pending call. The lifecycle now records every direct `FunctionCallOutput` by call id, claims it whichever item arrives first, and emits exactly one structured card without invoking the local executor.
3. **Deferred projection could invert card/answer order.** A matched output now emits its begin/end lifecycle as soon as the pair is known, rather than waiting until `response.completed`; output-before-call retains the same immediate projection once the call arrives.
4. **Historical direct outputs replayed as anonymous text.** Replay now uses a completed generic tool card instead of a background message, so raw output is never shown without an operation container.
5. **Raw-only reasoning disappeared after session recovery.** Replay previously reconstructed only summary-bearing reasoning items. It now creates a completed, status-only `Thinking` cell for raw-only history while retaining zero raw lines; the regression covers both collapsed and expanded rendering.

### Audit coverage

- Traced streaming raw reasoning delta, terminal reasoning item, task completion, session persistence, replay, headless output, TUI insertion, and collapsed/expanded rendering.
- Traced `FunctionCall` and `FunctionCallOutput` in both arrival orders, duplicate terminal outputs, local execution ownership, tool-card projection, and replay.
- Compiled the non-TUI protocol consumers (`code-mcp-server`, `code-exec`) after adding the additive status event.
- Ran the required root `./build-fast.sh` after all audit fixes: passed with no warnings or errors.

### Final risk state

- **HIGH risks remaining: 0.** The replay omission was the final audit finding and is covered by a direct regression; all three original HIGH risks retain their explicit mitigations and cardinality/privacy/order checks.

### Prevention

- **AFK — keep lifecycle tests at the event seam.** Every newly supported provider item must prove its id/order, its privacy policy, and exactly-one-card behavior before renderer tests. Acceptance: a fixture covers raw-only thinking, call-before-output, output-before-call, duplicate output, and replay.
- **HITL — decide provider capability metadata.** The present call-id ownership rule safely handles reflected outputs in one response. If a provider can send the same call id across separate responses, its adapter needs an explicit execution-owner capability rather than inference from arrival order.
