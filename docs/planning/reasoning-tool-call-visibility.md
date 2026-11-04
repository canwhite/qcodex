# Plan: Normalize reasoning and tool-call visibility for compatible models

> **User's request (restated)**: Learn from `/Users/doing/Desktop/codex` how current Codex presents thinking and tool calls—collapsing what should be collapsed and showing what should be shown—and fix the compatibility issue where MiniMax thinking is displayed directly, as in the supplied screenshot.
>
> **What this plan answers**: How qcodex will trace provider events into the TUI, adopt the reference visibility contract, and prevent raw MiniMax thinking from being rendered as ordinary assistant output.
>
> Goal: Make reasoning and tool activity legible and compact by default across OpenAI-compatible providers, without losing intentionally enabled raw reasoning or tool results.

## Context

qcodex already has `CollapsibleReasoningCell`, strict stream ordering, and dedicated running/completed tool cards. Its configuration also distinguishes reasoning summaries from opt-in raw reasoning. The desktop reference routes `ReasoningSummaryTextDelta` to collapsed reasoning and only routes `ReasoningTextDelta` when `show_raw_agent_reasoning` is enabled. qcodex's streaming and replay paths must be compared with that contract, especially where a MiniMax-compatible response labels thought text differently or places it in an assistant-text field.

The supplied screenshot is evidence of a presentation regression, not proof of one wire shape. The implementation must first capture the actual MiniMax response/SSE events and classify them before changing rendering behavior. Preserve the existing dirty working tree; this work must touch only files required by the finalized implementation and tests.

## Goal

For a normal turn, show a concise, collapsed reasoning summary while it is available; render tool calls as their existing progress/completion cards; and never render raw provider thinking as a normal assistant answer. When `show_raw_agent_reasoning = true`, raw reasoning remains explicitly available in the reasoning surface, not the answer surface. Verify equivalent behavior for streaming, completed/replayed turns, interrupted turns, and multiple interleaved tool calls.

## Plan

1. Record the reference behavior from `/Users/doing/Desktop/codex` and state a small display contract: summary reasoning is a collapsible status/summary; raw reasoning is opt-in; active tool calls show progress; completed calls retain an appropriately scoped result; ordinary assistant text is final-answer content only. Inspect the reference protocol/replay handlers and the corresponding qcodex paths in `code-rs/tui/src/chatwidget.rs`, `chatwidget/streaming.rs`, `chatwidget/tools.rs`, `chatwidget/exec_tools.rs`, and `history_cell/{reasoning.rs,tool_factory.rs}`.
2. Reproduce and characterize the MiniMax failure before editing code. Add a sanitized, deterministic fixture or mock SSE transcript that contains its actual thought, summary, assistant-text, tool-call, and ordering fields; redact credentials and user content. Trace the event sequence from compatible-provider decoding through `code-rs/core/src/client.rs` and `code-rs/core/src/codex/streaming.rs` to `EventMsg` and history insertion. Establish whether the failure is a provider-normalization problem, an event-mapping problem, or an incorrect TUI stream kind.
3. Define one normalization boundary in core for compatible response events. Map provider-specific thought fields to either summary reasoning or opt-in raw reasoning, preserve stable non-empty stream IDs and `OrderMeta`, and ensure final assistant text cannot be synthesized from a thought field. Keep unknown fields fail-safe: do not expose them as answer text until their semantics are known, while retaining a redacted diagnostic at the boundary.
4. Align TUI routing with the reference contract. Feed both reasoning-summary and explicitly enabled raw-reasoning events through `StreamKind::Reasoning` and `CollapsibleReasoningCell`; preserve default collapsed state, section boundaries, completion state, and strict per-turn ordering. Do not solve this by hiding all reasoning or by changing the global `show_raw_agent_reasoning` default.
5. Audit tool-call presentation across native, custom, MCP, shell/exec, and replayed calls. Confirm each lifecycle uses a running card then replaces/finalizes that same card, retains errors/output where useful, and does not get absorbed into a reasoning or answer stream. Repair any ordering/deduplication edge exposed by the MiniMax fixture rather than adding a provider-only UI branch.
6. Add focused regression coverage at the lowest useful boundaries: core decoding/event mapping for the sanitized MiniMax shape, TUI history/VT100 snapshots for collapsed summaries and visible tool cards, opt-in raw reasoning, raw-thinking suppression from assistant output, replay/completed-event behavior, and interleaved ordering. Name assertions in terms of the display contract, not MiniMax internals.
7. Update the concise configuration/documentation wording only if user-observable behavior or supported compatible-provider semantics change. Explain the difference between summary reasoning, opt-in raw reasoning, and tool output; do not document transient implementation details.

## Think — Debug Methodology

- Add redacted `[DEBUG-compatible-reasoning]` logs at the provider decoder and the core event-emission boundary: provider/model identifier, source field classification, item/stream ID presence, output index, sequence number, and chosen `EventMsg`/stream kind. Never log API keys, authorization headers, or unredacted thought text.
- At the TUI boundary, log `[DEBUG-reasoning-render]` only with stream ID, order key, cell type, collapsed state, and insertion/replacement decision. Inspect those logs before changing rendering code.
- Read source in this order: MiniMax-compatible response parser and fixture, core response-event normalization/emission, `ChatWidget::handle_codex_event`, stream controller/history state, then reasoning and tool-card renderers. Compare each hop against the desktop reference's protocol and replay handling.
- Localize upstream first: verify the normalized event type and order before blaming the cell renderer. Then exercise streaming, final-item, replay, interrupt, and two-tool interleaving paths so a fix does not only handle the captured happy path.
- Grep for all producers and consumers of `AgentReasoning*`, `AgentMessage*`, `ReasoningContentDelta`, `ReasoningSummaryDelta`, `StreamKind::Reasoning`, and `new_*tool_call` before landing a change.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with zero warnings.
- **Static analysis / type check**: N/A — `./build-fast.sh` is this repository's required single completion check; do not add separate completion commands unless the implementation specifically requires one.
- **Runtime verification**: Run qcodex against a sanitized local/mock compatible-provider transcript (or a MiniMax test account only if credentials and authorization are already configured). Capture a normal turn with reasoning plus at least one tool call and confirm: collapsed summary shown, no raw thought in the answer stream, running/completed tool card visible, and final answer remains visible.
- **UI regression verification**: Run the focused TUI regression target needed by the new fixture/snapshot during development; render the same scenarios through the VT100 harness and assert the visible transcript. Review any intentional snapshot update before accepting it.
- **Logic correctness**: Walk these paths manually and in tests: summaries only; raw reasoning disabled/enabled; provider sends only raw thought; streaming then final item; replay/resume; interrupt before completion; tool success/failure; simultaneous/interleaved tool calls; missing/duplicate stream IDs; and output-order changes across request ordinals.

## Adjust — Rollback and Global Scan

- **Rollback plan**: Keep normalization and TUI changes small and independently reviewable. Revert the new provider-field classifier/adapter and its fixtures together to restore prior wire handling; no persisted data or migration is involved.
- **Global scan**: Search every compatible-provider parsing path and every producer of reasoning or assistant-message events, plus replay/session-history projection, headless `exec`, transcript export, and agent/delegated streams. Confirm no second renderer promotes raw thought to answer text.
- **Backwards compatibility**: Retain the public configuration semantics: `hide_agent_reasoning` suppresses reasoning events, and `show_raw_agent_reasoning` alone permits raw thought. Existing OpenAI response summaries, raw-reasoning opt-in users, and tool-call event formats must continue to work without config migration.

## Open Questions

- Which exact MiniMax wire API and model/version produced the screenshot, and does it emit thought text as `reasoning_content`, a separate SSE event, or ordinary assistant text?
- Should unclassified compatible-provider thought fields be silently suppressed, surfaced as a redacted warning, or exposed behind an additional diagnostic flag? The safer default is suppress-plus-diagnostic until field semantics are verified.
- Does the desired desktop parity include only TUI behavior, or should `code exec`, exports, and app-server/replay transcripts also use the exact same visibility policy? This plan assumes all user-facing transcript projections should agree.

## Out of Scope

- Redesigning the visual styling, keyboard interactions, or expansion affordances of existing reasoning/tool cells.
- Changing provider authentication, model selection, or MiniMax API compatibility beyond event classification required for correct display.
- Exposing chain-of-thought by default or weakening the explicit raw-reasoning opt-in.
- Editing the read-only `codex-rs` mirror; Rust implementation work belongs under `code-rs`.

---

> Next step: run `/pre-mortem docs/planning/reasoning-tool-call-visibility.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Medium] Unobserved MiniMax wire shape bypasses normalization

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 5 × 3 × (1 - 0.4) = 9.0

**Failure Scenario**: The implementation assumes a `reasoning_content`-style field, but the affected MiniMax model emits thought text through a different streamed or completed payload. The patch passes a synthetic fixture yet the production response still reaches `AgentMessage` and is displayed as an answer.

**Mitigation**:
- Capture one sanitized response/SSE trace from the exact failing wire API and model before selecting a classifier; retain it as the primary regression fixture.
- Add a boundary assertion that records the source-field classification and rejects a thought-classified field from the `AgentMessage*` path.
- Gate completion on both the fixture and a local/mock end-to-end playback that contains summary, raw thought, answer, and a tool call.

### [Medium] Fail-safe suppression drops a legitimate final answer

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.5
**Risk Score**: 5 × 2 × (1 - 0.5) = 5.0

**Failure Scenario**: An ambiguous provider field is classified as thought and hidden even though it contains the only user-facing answer, producing an apparently stalled or empty turn.

**Mitigation**:
- Base classification on structured event/field semantics and provider capability metadata, never prose heuristics such as “Thinking:” prefixes.
- Preserve an explicit unknown-field diagnostic with IDs and field names only, and make the turn-completion test assert that expected answer text remains in `StreamKind::Answer`.
- Include a fixture whose final answer is delivered after a reasoning segment and require the answer snapshot to remain visible.

### [Medium] Stable ordering breaks during interleaved reasoning and tool events

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.35
**Risk Score**: 4 × 3 × (1 - 0.35) = 7.8

**Failure Scenario**: Normalizing a provider event changes or omits an item ID, output index, or sequence number. Reasoning and tool cards then reorder, duplicate, or disappear under concurrent calls despite looking correct in a single-stream demo.

**Mitigation**:
- Make the normalization adapter preserve or synthesize a documented non-empty stream ID and forward the original `OrderMeta`; reject missing IDs at the existing strict-stream boundary.
- Add a deterministic multi-frame VT100 regression with interleaved reasoning, two tool lifecycles, and final answer events across output indices.
- Assert event/history ordering keys directly before asserting rendered text, so an ordering regression is diagnosed at its source.

### [Medium] Raw-reasoning privacy/config contract regresses in another transcript surface

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.45
**Risk Score**: 5 × 2 × (1 - 0.45) = 5.5

**Failure Scenario**: The TUI no longer displays raw thought as an answer, but replay, transcript export, app-server history, or headless `exec` still emits it with raw reasoning disabled—or loses it when the user explicitly enables it.

**Mitigation**:
- Write a truth table for `hide_agent_reasoning` × `show_raw_agent_reasoning` × {summary, raw, answer} and use it as the shared acceptance criterion for live and replay paths.
- Scan every `AgentReasoning*` and `AgentMessage*` producer/consumer plus export/replay projection before merging.
- Add focused disabled/enabled tests for live streaming and completed/replayed items; use only synthetic non-sensitive text in fixtures and snapshots.

### [Low] Tool-card audit expands into an unrelated visual redesign

**Severity**: 3 | **Likelihood**: 2 | **Detectability**: 0.8
**Risk Score**: 3 × 2 × (1 - 0.8) = 1.2

**Failure Scenario**: Investigating the MiniMax path broadens into changing all card styling or keyboard behavior, increasing review surface and masking the event-classification fix.

**Mitigation**:
- Treat the display contract as behavioral: retain existing card styles and controls unless a reproduced lifecycle defect requires a minimal change.
- Split any independent redesign finding into a separate plan/issue and keep this diff scoped to normalization, routing, and regression coverage.

### [Low] Sanitized diagnostics or fixtures disclose provider thought or credentials

**Severity**: 5 | **Likelihood**: 1 | **Detectability**: 0.85
**Risk Score**: 5 × 1 × (1 - 0.85) = 0.75

**Failure Scenario**: A copied production trace, debug log, or snapshot inadvertently retains authorization data or real model reasoning while documenting the issue.

**Mitigation**:
- Build fixtures from minimal synthetic payloads after recording field names and ordering separately; never commit captured production bodies.
- Log only provider/model, field classification, IDs, and ordering metadata, with no headers, values, or thought text.
- Review fixture and log diffs specifically for secrets and human-readable provider content before committing.

### [Low] Reference behavior changes while parity work is in flight

**Severity**: 2 | **Likelihood**: 2 | **Detectability**: 0.8
**Risk Score**: 2 × 2 × (1 - 0.8) = 0.8

**Failure Scenario**: The desktop reference evolves and a narrow implementation copies a transient detail rather than the durable visibility policy.

**Mitigation**:
- Record the behavioral contract—not exact visual strings—as acceptance criteria, and cite the inspected reference source revision in the implementation review.
- Favor qcodex's established `CollapsibleReasoningCell`, tool cards, configuration semantics, and strict ordering invariants when reference details conflict.

---

## Post-Mortem: Reasoning and tool-call visibility

**Date**: 2026-11-04

### Implementation map

MiniMax Chat Completions SSE enters at `process_chat_sse` in `code-rs/core/src/chat_completions.rs`. The parser now splits a leading `<think>…</think>` sequence from ordinary `delta.content`, emits thought as `ResponseEvent::ReasoningContentDelta`, and leaves answer content in `ResponseEvent::OutputTextDelta`. Existing core streaming routes reasoning to `StreamKind::Reasoning`; the existing TUI `CollapsibleReasoningCell` renders that stream collapsed, while tool events retain their existing card lifecycle.

### Issues Fixed

| # | Severity | Type | Location |
|---|----------|------|----------|
| 1 | High | Provider contract violation | `code-rs/core/src/chat_completions.rs` |
| 2 | Medium | Over-broad content classification | `code-rs/core/src/chat_completions.rs` |

1. MiniMax emitted private thinking inside ordinary Chat Completions `content` wrapped in `<think>…</think>`. The parser sent it to `OutputTextDelta`, bypassing the reasoning surface. It now recognizes fragmented leading tags, retains partial tag bytes across SSE chunks, and routes only their contents to reasoning.
2. The first implementation classified every `<think>` pair anywhere in the response. A regression test showed that this would hide literal tags in HTML/code answers. Recognition is now limited to the leading, whitespace-only response prefix; after visible answer content begins, tags stay literal.

### Root Cause

The compatible Chat Completions parser assumed `delta.content` was always final answer text. MiniMax used an undocumented-at-this-boundary presentation convention that encoded raw thought in the same field. The parser had no content-classification state machine, so its otherwise correct TUI reasoning path was never reached.

### Verification Evidence

- A pre-fix parser regression test failed because `ThinkTagRouter` did not exist; after implementation it passed for fragmented opening/closing tags.
- The SSE-level regression test replays a three-chunk MiniMax-shaped stream and proves `private thought` is emitted only as `ReasoningContentDelta`, while `visible answer` is emitted only as `OutputTextDelta`.
- The post-mortem literal-tag test failed against the broad classifier, then passed after narrowing recognition to the leading response prefix.
- `./build-fast.sh` passed cleanly on 2026-11-04 after all fixes.

### Prevention Tasks

| Task | Type | Status |
|------|------|--------|
| Keep fragmented MiniMax `<think>` SSE regression coverage at the parser boundary | AFK | Complete |
| Keep literal answer-tag coverage to prevent over-broad classification | AFK | Complete |
| Validate an exact production MiniMax payload when its wire contract changes | HITL | Not required for this implementation; use a sanitized trace before extending field support |

### Residual Risk Review

No residual risk is HIGH (score > 12). The remaining integration uncertainty—whether a future MiniMax model changes its tag or field convention—is bounded by the fail-safe behavior (unknown fields remain ordinary content), the localized parser seam, and the regression suite. **HIGH RISK count: 0.**
