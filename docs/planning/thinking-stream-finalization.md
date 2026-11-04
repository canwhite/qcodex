# Plan: Fix Thinking Stream Finalization

> **User's request (restated)**: Based on the identified thinking-stream issues, write a repair plan first.
>
> **What this plan answers**: It defines how to make Chat Completions thinking, answer, and tool-call events have one stable finalization path without duplication or reordering.
>
> Goal: Preserve visible answers and reasoning exactly once, in provider order, for plain and tool-calling Chat Completions turns.

## Context

`ThinkTagRouter` correctly separates leading MiniMax-style `<think>…</think>` content, but the resulting event stream conflicts with `AggregatedChatStream`.

- `process_chat_sse` emits deltas, then terminal `OutputItemDone` items, then `Completed`.
- The aggregator accumulates deltas but currently forwards terminal assistant items and synthesizes another final item at `Completed`.
- At a tool-call boundary, accumulated visible answer text is not flushed before the function-call event.

The TUI now ignores blank assistant finals, but that is defensive downstream behavior. This work repairs the core event contract so downstream consumers receive a single, ordered final representation.

## Goal

For each assistant or reasoning stream, downstream consumers receive either its provider terminal item or its synthesized aggregate item, never both. A tool call must not appear before visible assistant text that preceded it.

## Plan

1. Document the existing raw event sequences from `process_chat_sse` for: plain answer, leading `<think>` plus answer, reasoning plus tool call, and whitespace-only assistant content. Capture item IDs and the point at which `Completed` arrives.
2. Define `AggregatedChatStream` ownership rules per mode:
   - `AggregatedOnly` consumes answer and reasoning deltas plus their duplicate terminal items, then emits exactly one synthesized terminal item per stream at the correct boundary.
   - `Streaming` forwards deltas and uses a single terminal event only to finalize the active stream; it must not synthesize a second equivalent final.
3. Add a pending-event mechanism for tool-call boundaries. Before forwarding a function call, flush accumulated assistant text and, when applicable, reasoning in their original order; queue the function-call event behind those flushed items.
4. Make terminal-item handling type-aware. Assistant terminal items must be suppressed after their content has already been accumulated. Reasoning terminal items must follow the mode-specific ownership rule and must not be both forwarded and synthesized.
5. Keep `ThinkTagRouter` narrowly scoped to leading tags. Preserve literal `<think>` markup once visible answer content begins, retain fragmented-marker behavior, and make EOF handling deterministic for partial opening/closing markers.
6. Add regression tests at the `process_chat_sse` + `AggregatedChatStream` seam. Assert the full emitted sequence and content, not just delta routing.
7. Run the required build and review all compatible-provider response paths for other duplicate-final patterns.

## Think — Debug Methodology

- Start at the core boundary: construct in-memory SSE chunks and collect the raw `ResponseEvent` sequence from `process_chat_sse`.
- Feed that sequence into both aggregation modes and assert event kind, payload, item ID, and order. This isolates parser behavior from TUI rendering.
- Use one case per boundary: ordinary stop, `<think>` stop, tool-call stop, and stream closure without `[DONE]`.
- If temporary diagnostics are needed, add only boundary logs with a unique `[DEBUG-thinking-final]` prefix around parser emission and aggregate forwarding; remove them before completion.
- Read `code-rs/core/src/chat_completions.rs` first; do not infer behavior from the TUI because the bug is upstream of rendering.

## Do — Verification Strategy

- **Focused regression tests**: run the new `code-core` tests covering all four sequences in both aggregate modes. Each must prove no duplicate `OutputItemDone(Message)` or `OutputItemDone(Reasoning)` is emitted.
- **Logic correctness**: manually inspect these paths:
  - visible text only;
  - fragmented leading `<think>` followed by visible text;
  - visible text and reasoning before a tool call;
  - raw reasoning enabled versus disabled;
  - EOF / `[DONE]` with a fragmented marker;
  - absent provider item IDs, ensuring separate output items cannot overwrite each other.
- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with no warnings.

## Adjust — Rollback and Global Scan

- **Rollback plan**: keep the change limited to the Chat Completions parser/aggregator seam. Reverting its commit restores the former compatible-provider transport without touching TUI history behavior.
- **Global scan**: search all `ResponseEvent::OutputItemDone`, `OutputTextDelta`, and `ReasoningContentDelta` adapters for a pattern of forwarding a terminal item after buffering its deltas; inspect Responses API, fixture replay, and WebSocket adapters separately.
- **Backwards compatibility**: preserve the public `ResponseStream` event types and item payloads. Consumers should observe fewer duplicates, not a schema change.

## Open Questions

- Whether a provider may send legitimate multiple assistant messages in one Chat Completions request without distinct `item_id` values. If so, the fallback ID policy needs an explicit per-item counter rather than the turn ID.
- Whether partial closing tags at EOF should remain literal answer text or be treated as reasoning text. The plan assumes they remain reasoning only when an opening tag was fully recognized.

## Out of Scope

- Repairing malformed tool-call JSON produced by a provider/model.
- Changing TUI rendering, theme, or strict ordering rules beyond the already committed blank-final safeguard.
- Adding support for non-leading or case-insensitive `<think>` tags.

---

> Next step: run `/pre-mortem docs/planning/thinking-stream-finalization.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Tool-call boundary reverses user-visible order

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14

**Failure Scenario**: The implementation flushes accumulated content only at `Completed`, so a function-call event is rendered before the assistant text or thinking that caused it. Tool-heavy compatible-provider turns become confusing and may produce an invalid replay history.

**Mitigation**:

- Make the tool-call boundary an explicit aggregation flush point; enqueue the function-call event only after any preceding reasoning and answer terminal events.
- Add sequence assertions for answer → reasoning → function call → completed in both aggregation modes.

### [Risk] Terminal-item ownership loses or duplicates content

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14

**Failure Scenario**: A broad rule suppresses a provider terminal item even when no delta was received, losing a complete answer; or it forwards the terminal item and later synthesizes the same content again.

**Mitigation**:

- Track, per content kind, whether non-empty deltas were accumulated before deciding whether to consume or preserve the terminal item.
- Add paired tests for delta-only, terminal-only, and delta-plus-terminal inputs, with exact expected event counts and payloads.

### [Risk] Fallback stream IDs merge distinct assistant items

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: A provider omits `item_id` for multiple assistant items in a turn. They inherit the turn fallback id, so a later blank, progress, or final item overwrites an earlier visible answer in the TUI or persisted history.

**Mitigation**:

- Define and test a per-output-item fallback ID generated at the parser boundary; it must stay stable for that item's deltas and final but differ from all other items in the turn.
- Retain the TUI blank-final guard as defense in depth, but do not rely on it for core identity correctness.

### [Risk] Provider-specific stream variants bypass the new contract

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: A compatible provider uses `message.reasoning`, nested reasoning objects, early stream closure, or terminal-only content. The main MiniMax fixture passes while another provider silently drops or duplicates output.

**Mitigation**:

- Build table-driven fixtures for every already-supported content shape and for `[DONE]` versus socket closure.
- Preserve unknown event types unchanged where safe, and log a bounded diagnostic at the parser boundary rather than silently assigning them to a stream.

### [Risk] Tests assert implementation details instead of the external contract

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: Tests inspect internal buffers or merely assert parser deltas. A later refactor recreates duplicate UI events even though all unit tests remain green.

**Mitigation**:

- Assert the public `ResponseEvent` sequence after the full parser-plus-aggregator pipeline, including event order, content, and item IDs.
- Add one TUI-level regression that feeds the resulting events through the history path and asserts exactly one visible assistant cell.

### [Risk] Raw-reasoning visibility behavior changes unintentionally

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.5
**Risk Score**: 4.5

**Failure Scenario**: Fixing aggregate-only behavior inadvertently suppresses finalization required by the raw-reasoning streaming mode, leaving a stuck thinking cell.

**Mitigation**:

- Treat `AggregatedOnly` and `Streaming` as separately specified modes in tests; assert active streams are finalized after a stop and after a tool-call boundary.

### [Risk] Scope expands into unrelated provider or TUI changes

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.7
**Risk Score**: 2.7

**Failure Scenario**: The implementation tries to repair malformed model tool arguments or redesign TUI ordering while changing the core event contract, making regressions difficult to isolate and roll back.

**Mitigation**:

- Limit the implementation commit to `chat_completions` aggregation, its core tests, and only a necessary fallback-ID seam; file independent defects separately.
- Keep the existing TUI guard unchanged unless a test demonstrates that the core fix no longer needs a compatible behavior adjustment.
