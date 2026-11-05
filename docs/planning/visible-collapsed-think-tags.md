# Plan: Show Compatible-Provider Think Content as Collapsed Reasoning

> **User's request (restated)**: Investigate the project’s earlier trimming of `<think/>` content, then plan to show that thinking in the existing UI style while keeping it collapsed and visibly indicating that thinking is in progress.
>
> **What this plan answers**: How to retain already-parsed compatible-provider `<think>` content in TUI history as a compact, expandable reasoning entry without changing the answer stream.
>
> Goal: A leading `<think>…</think>` segment from a compatible provider is visible as one collapsed reasoning entry, shows `Thinking…` while it streams, and can be expanded with the existing reasoning control.

## Context

The parser work is already present. `ThinkTagRouter` in `code-rs/core/src/chat_completions.rs` recognizes a leading, including SSE-fragmented, `<think>…</think>` segment and emits it as `ResponseEvent::ReasoningContentDelta`; it deliberately leaves literal tags that occur after visible answer text untouched. That event reaches the existing `CollapsibleReasoningCell` through the normal `AgentReasoningDelta` / `StreamKind::Reasoning` path, so no parser recovery is needed.

The apparent trimming happens in the TUI presentation layer. A new reasoning cell is collapsed when `tui.show_reasoning` is false. `ChatWidget::refresh_reasoning_collapsed_visibility` then sets the runtime-only `hide_when_collapsed` flag to true, and `CollapsibleReasoningCell::{display_lines,custom_render_with_skip}` returns no rows at all. The bottom pane does set its existing `thinking` status while deltas arrive, but there is no history-row affordance and completed thinking is wholly invisible in compact history.

This plan changes that compact-history policy. It does not make `<think>` answer text, change provider parsing, or alter the `hide_agent_reasoning` suppression contract.

## Goal

For a visible reasoning event—including reasoning sourced from `<think>` tags—the default TUI transcript shows exactly one compact collapsed entry. While the stream is active, the entry and existing status surface communicate `Thinking…`; after finalization it remains as a compact expandable entry. Ctrl+R continues to switch all reasoning entries between compact and expanded views, with the full content retained for history/replay.

## Plan

1. Add a failing TUI regression at the real event boundary before production edits. Feed fragmented leading `<think>` SSE content through `process_chat_sse`, then through the ChatWidget reasoning-event path with a non-empty stream id and stable `OrderMeta`. Assert that the answer contains only final answer text, while the history contains one collapsed reasoning entry that is visibly labeled `Thinking…` during streaming.
2. Make collapsed reasoning a compact projection rather than an invisible one. In `code-rs/tui/src/chatwidget.rs`, separate the user’s collapsed/expanded preference (`tui.show_reasoning`) from the obsolete `hide_when_collapsed` policy, so visibility refreshes do not suppress a valid collapsed cell. Apply the same policy on initial insertion, stream updates/finalization, history restore, replay, and Ctrl+R toggles.
3. Give `CollapsibleReasoningCell` an explicit compact header in `code-rs/tui/src/history_cell/reasoning.rs`. Reuse the project’s existing thinking wording and dim/spinner styling: render `Thinking…` while `in_progress` is true, and a stable collapsed `Thinking` affordance after completion. Do not put raw thought text in this compact line; retain the current structured sections solely for the expanded view.
4. Keep stream lifecycle ownership unchanged. Continue to set `in_progress` when reasoning lines are inserted and clear it at terminal `AgentReasoning`, task completion, interruption, and stream switching. Verify that the compact header updates in place for the same reasoning id rather than creating a second cell, and that answer or tool events do not leave `Thinking…` stuck.
5. Preserve accessibility and existing controls. Keep the footer’s Ctrl+R hint/state behavior, transcript export/history serialization, history ordering, and standard-terminal transcript mirroring. If collapsed rendering needs an additional hint or glyph, use existing reasoning/footer vocabulary rather than adding a new interaction or configuration key.
6. Add targeted regressions for: fragmented tags; reasoning-only then answer; final reasoning without deltas; tool-call/interruption boundaries; narrow-width and CJK wrapping; Ctrl+R expansion/collapse; and serialize/rebuild/replay. Each must assert one reasoning cell, non-empty stable id/order metadata, no `<think>` tag or thought text in the answer cell, and no duplicate/misordered cell.
7. Update only the user-facing documentation that now describes completed reasoning as entirely absent from compact history. State the new compact-header behavior and retain the warning that `hide_agent_reasoning = true` suppresses reasoning events altogether.

## Think — Debug Methodology

- Reproduce at the smallest boundary first: replay synthetic SSE chunks (`<thi`, `nk>private`, `</think>answer`) through `process_chat_sse` and record only event kinds, item ids, and ordering metadata—never real thought text in diagnostics.
- Next, feed the resulting reasoning delta and terminal event to `make_chatwidget_manual()` and render a deterministic VT100/TestBackend frame. Inspect `CollapsibleReasoningCell::display_lines`, `custom_render_with_skip`, and `ChatWidget::refresh_reasoning_collapsed_visibility` before changing code.
- If instrumentation is needed, add temporary boundary logs under `[DEBUG-think-visibility]` at parser emission, reasoning-cell insertion, visibility refresh, and finalization. Include stream id/state/order key but not content; remove them before completion.
- Diagnose upstream to downstream: parser classification → core event mapping → ChatWidget stream ordering → cell collapsed renderer → height cache/render frame. Do not compensate for a display defect by reclassifying thought as assistant text.

## Do — Verification Strategy

- **Focused TUI regressions**: run the directly affected existing test target(s), including the new ChatWidget/VT100 cases, and review every intentional snapshot update. Tests must cover live, completed, interrupted, expanded, and restored states.
- **Logic correctness**: walk the paths for fragmented opening/closing tags, whitespace before `<think>`, ordinary literal tags after answer text, no final reasoning event, final reasoning with no deltas, tool transition, Ctrl+R before/after completion, `hide_agent_reasoning = true`, and replay. Confirm `<think>` never enters the answer stream, and all visible reasoning uses the original non-empty stream id plus order metadata.
- **Runtime verification**: use a compatible provider/model that emits a leading think block, or the project’s synthetic local fixture if credentials are unavailable. Observe `Thinking…` while streaming, a one-line collapsed entry after completion, Ctrl+R expansion, resize/scroll behavior, and a normal final response.
- **Build**: run `./build-fast.sh` from the repository root. It must finish cleanly with no errors or warnings; do not run rustfmt.

## Adjust — Rollback and Global Scan

- **Rollback plan**: keep changes limited to reasoning-cell compact projection, its visibility refresh, focused tests, and matching documentation. Reverting the implementation restores the prior hidden-collapsed policy without changing provider event schemas or persisted reasoning contents.
- **Global scan**: inspect all `hide_when_collapsed`, `CollapsibleReasoningCell`, `set_in_progress`, `clear_reasoning_in_progress`, `refresh_reasoning_collapsed_visibility`, and history-registry/replay call sites. Also inspect standard-terminal export and Auto Drive reasoning separately so they do not acquire a second, inconsistent compact header.
- **Backwards compatibility**: retain `ReasoningState` serialization and the Ctrl+R behavior. Existing sessions must rebuild safely; the changed policy may reveal a compact header for old retained reasoning, but must not expose hidden content as assistant-answer text or require history migration.

## Open Questions

- The current compact cell surfaces a section-derived summary when it is not hidden. This plan assumes the desired default is the neutral `Thinking` / `Thinking…` header rather than showing the first raw thought line; confirm during visual review if a short sanitized summary is preferred.
- The exact glyph/animation should reuse the existing TUI token/style rather than introduce a visual system. Verify it in the target terminal themes while implementing.

## Out of Scope

- Parsing non-leading, malformed, or case-insensitive think tags.
- Enabling model/provider raw reasoning that is not already emitted, or changing `show_raw_agent_reasoning` / `hide_agent_reasoning` semantics.
- Redesigning tool cards, Auto Drive panels, answer rendering, or the global history layout.

---

> Next step: run `/pre-mortem docs/planning/visible-collapsed-think-tags.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

**Scenario**: It is six months later. The change shipped, but compatible-provider thinking is either invisible again, stuck on screen, leaked into the final answer, or breaks transcript order. The following risks describe how that happened and the acceptance gates that prevent it.

### [Risk] The compact row implements the wrong product meaning of “show thinking”

**Initial Severity**: 5 | **Initial Likelihood**: 4 | **Initial Detectability**: 0.25
**Initial Risk Score**: 5 × 4 × (1 - 0.25) = 15.0 (HIGH)

**Failure Scenario**: The implementation shows only a generic `Thinking` row after completion while the intended behavior was to expose a useful, safe preview; or it exposes raw thought in the collapsed row when the request meant “keep it folded.” The feature appears implemented but does not satisfy the user-facing contract.

**Mitigation**:

- Before changing production code, encode the selected compact contract in a VT100 snapshot: active state is exactly `Thinking…`; completed state is exactly one neutral `Thinking` affordance; raw thought appears only after Ctrl+R.
- Treat the plan’s explicit neutral-header choice as the implementation default. Any switch to a preview must be a separately reviewed requirement with privacy and clipping tests, not an incidental renderer change.
- Perform one visual acceptance pass in the target terminal theme and narrow width before merging.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.9
**Residual Risk Score**: 5 × 1 × (1 - 0.9) = 0.5

### [Risk] Compact rendering violates virtual-scroll or height-cache invariants

**Initial Severity**: 5 | **Initial Likelihood**: 3 | **Initial Detectability**: 0.15
**Initial Risk Score**: 5 × 3 × (1 - 0.15) = 12.75 (HIGH)

**Failure Scenario**: Replacing a zero-height hidden cell with a one-line cell leaves a stale cached height or an incorrect spacer. The history jumps, clips the row, panics on a height mismatch, or renders different results after resize.

**Mitigation**:

- Change compact projection through the existing `display_lines` / `custom_render_with_skip` path and preserve the established cache invalidation after visibility and `in_progress` changes.
- Add deterministic renders at narrow and normal widths, including CJK text and a partially scrolled reasoning row; assert the compact row is present once and the history-height checks remain clean.
- Exercise both transitions that change height: streaming `Thinking…` → completed `Thinking`, and collapsed ↔ expanded via Ctrl+R.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.9
**Residual Risk Score**: 5 × 1 × (1 - 0.9) = 0.5

### [Risk] Thought content leaks into the answer or bypasses reasoning-visibility policy

**Initial Severity**: 5 | **Initial Likelihood**: 3 | **Initial Detectability**: 0.15
**Initial Risk Score**: 5 × 3 × (1 - 0.15) = 12.75 (HIGH)

**Failure Scenario**: A presentation shortcut sends `<think>` bytes to the assistant-message renderer, places a raw first line in compact history, or makes `hide_agent_reasoning = true` ineffective. Sensitive intermediate content then appears in a normal answer or in exported transcript output unexpectedly.

**Mitigation**:

- Do not modify `ThinkTagRouter`, provider event classification, or `hide_agent_reasoning` behavior for this display change; alter only the reasoning-cell projection.
- Add parser-to-TUI assertions that answer text contains only the visible answer, compact rendering contains no thought bytes, expanded rendering contains the synthetic thought, and `hide_agent_reasoning = true` produces no reasoning row.
- Review standard-terminal export and restored-session output using synthetic, non-sensitive fixtures before merge.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.9
**Residual Risk Score**: 5 × 1 × (1 - 0.9) = 0.5

### [Risk] A compact row duplicates, reorders, or attaches to the wrong reasoning stream

**Initial Severity**: 5 | **Initial Likelihood**: 3 | **Initial Detectability**: 0.15
**Initial Risk Score**: 5 × 3 × (1 - 0.15) = 12.75 (HIGH)

**Failure Scenario**: A display-side insert or an over-eager cell replacement creates a second reasoning row, loses the original stream id, or moves reasoning across an answer/tool boundary. This breaks the TUI’s strict per-turn ordering even though a single happy-path screenshot looks correct.

**Mitigation**:

- Reuse the existing `reasoning_index`, `StreamKind::Reasoning`, and seeded `(request_ordinal, output_index, sequence_number)` key; compact rendering must mutate the existing cell only and must not issue a new stream insert.
- Add an interleaving regression for reasoning delta → tool event → section break/final reasoning → answer. Assert cell count, stream id, order keys, and visual order before and after Ctrl+R.
- Keep the existing strict missing-id drop/error boundary intact; do not add a synthetic fallback to hide an ordering bug.

**Residual Severity**: 5 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.9
**Residual Risk Score**: 5 × 1 × (1 - 0.9) = 0.5

### [Risk] The row remains indefinitely in the active state

**Initial Severity**: 4 | **Initial Likelihood**: 4 | **Initial Detectability**: 0.1
**Initial Risk Score**: 4 × 4 × (1 - 0.1) = 14.4 (HIGH)

**Failure Scenario**: A turn ends through cancellation, a tool boundary, a connection failure, or a final-reasoning-only event without taking the normal completion path. The cell continues to say `Thinking…`, misleading the user and leaving the spinner/status state stale.

**Mitigation**:

- Enumerate and test every terminal owner: `AgentReasoning`, `TaskComplete`, interruption/cancellation, stream switching, and late-delta rejection after a closed id.
- Centralize the visual transition on the existing `set_in_progress(false)` lifecycle rather than creating a separate compact-header state machine.
- Add a regression that injects a late delta after finalization and asserts it is ignored and cannot reactivate the completed row.

**Residual Severity**: 4 | **Residual Likelihood**: 1 | **Residual Detectability**: 0.9
**Residual Risk Score**: 4 × 1 × (1 - 0.9) = 0.4

### [Risk] Saved or standard-terminal transcripts disagree with the live TUI

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.5
**Risk Score**: 4 × 3 × (1 - 0.5) = 6.0

**Failure Scenario**: The live cell gains a compact header but history reconstruction resets a runtime flag, or standard-terminal remirroring expands/omits different content. A resumed session no longer matches the completed live turn.

**Mitigation**:

- Derive compact/expanded rendering from persisted reasoning state plus the current `tui.show_reasoning` preference; do not persist a second presentation-only visibility flag.
- Add save/rebuild/replay and standard-terminal-mirroring snapshots for collapsed and expanded modes.

### [Risk] The scoped UI fix expands into provider or Auto Drive redesign work

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.75
**Risk Score**: 3 × 3 × (1 - 0.75) = 2.25

**Failure Scenario**: Investigation of the row style changes parser semantics, raw-reasoning configuration, tool cards, or Auto Drive screens. The review becomes too broad to prove the simple visibility change safe.

**Mitigation**:

- Limit the implementation diff to the compact reasoning projection, refresh/lifecycle hooks proven necessary by tests, documentation, and focused regressions.
- Record any separate provider, configuration, or Auto Drive finding as follow-up work instead of folding it into this change.

### [Risk] Provider changes its think-tag format and creates an unsupported expectation

**Severity**: 3 | **Likelihood**: 2 | **Detectability**: 0.7
**Risk Score**: 3 × 2 × (1 - 0.7) = 1.8

**Failure Scenario**: A provider later changes tags or sends thought in a different field. The compact UI works for supported leading tags but a user assumes all provider-specific thought formats will be shown.

**Mitigation**:

- Keep the supported boundary explicit: leading `<think>…</think>` already routed to reasoning, plus native reasoning events. Unknown formats remain ordinary compatible-provider behavior until separately classified.
- Retain fragmented-tag parser tests and document the supported behavior without claiming universal provider coverage.

### Convergence review

- **Iteration 1** found five HIGH risks: product-contract mismatch, height/cache breakage, thought leakage, stream ordering, and a permanently active indicator. Each now has a testable mitigation and a residual score below 1.
- **Iteration 2** swept persistence, standard-terminal rendering, scope/ownership, and provider evolution. It found no previously unrecorded HIGH risk.
- **Remaining HIGH risks**: 0. All recorded risks have a mitigation; the highest residual score is 6.0.

---

## Post-Mortem

**Date**: 2026-11-05

The implementation follows the existing event path: compatible-provider think content remains a reasoning event, the stream controller seeds one ordered reasoning cell for the compact view, and the cell projects to `Thinking…` or `Thinking` unless Ctrl+R expands it. It never enters the answer renderer.

| # | Severity | Finding | Fix and regression coverage |
|---|---|---|---|
| 1 | High | Collapsed reasoning cells were deliberately zero-height through `hide_when_collapsed`, so there was no transcript affordance. | Removed the hidden projection; compact cells render one neutral status row. Unit and VT100 regressions assert no raw thought is shown until expansion. |
| 2 | High | A first delta without a completed markdown line did not create a cell, so `Thinking…` arrived late or never appeared. | The reasoning stream seeds an ordered blank line only for compact mode; the cell treats it as structure rather than content. Live-stream regression covers it. |
| 3 | High | Finalization could finish the cell and later animation insertion could mark it active again. | Insertion consults `closed_reasoning_ids` and keeps completed ids non-active. Completed-state regression covers the transition. |
| 4 | High | Empty reasoning delta/final events would otherwise seed a misleading, empty Thinking row. | Empty events without an existing stream are ignored; a VT100 regression asserts no row appears. |
| 5 | High | A non-reasoning history insert during final-drain could restore the latest reasoning cell to active even after its id was closed. | Restoration now requires the active reasoning stream's exact, non-closed id. A test first reproduced this state and now proves the completed row is not reopened. |

### Root causes

1. Compact presentation was coupled to an older “hide collapsed reasoning” policy instead of being a distinct one-line projection.
2. Lifecycle updates matched the latest cell rather than the stable stream id, which is unsafe while ordered stream animation is draining.

### Verification

- Targeted ChatWidget lifecycle tests, including the closed-stream regression, pass.
- `history_cutoff_probe` passes (8 passed, 1 intentionally ignored), including streaming, completed, expansion, and empty-event cases.
- The collapsed/expanded VT100 snapshot passes, and the core SSE routing regression confirms think content stays out of answer deltas.
- `git diff --check` is clean; no obsolete hidden-collapse APIs or `.snap.new` files remain.

### Prevention

The five regressions above are AFK prevention tasks and are implemented in this change. No HITL follow-up is required for the stated feature contract.

**Remaining HIGH risks**: 0. The highest remaining recorded risk score is 6.0, below the HIGH threshold.
