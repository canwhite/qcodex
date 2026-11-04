# Plan: Restore compact Codex-style TUI history content

> **User's request (restated)**: Make the QCodex TUI show appropriate content like the desktop Codex TUI shown in the reference screenshot: stop showing content that should be hidden, restore content that should be visible, and use the desktop Codex TUI code and behavior as the reference.
>
> **What this plan answers**: How to align QCodex's streamed reasoning and tool-history presentation with the compact, event-oriented desktop Codex transcript without replacing QCodex's unrelated TUI features.
>
> Goal: A completed turn has a compact, readable history in which reasoning, exploration, command execution, tool output, and final assistant text appear only at the level of detail appropriate to the event.

## Context

The current screenshot shows two separate display failures: intermediate model narration is retained as large visible history content, and tool activity is rendered with expanded cards and arguments that crowd out the useful result. The reference desktop Codex screenshot instead presents a small chronological activity feed: adjacent read/search commands are grouped under `Explored`, command invocations retain a short `Ran` entry and relevant output, and finished reasoning does not consume normal scrollback.

This is a content-selection and history-rendering problem, not a theme-palette problem. QCodex currently owns this behavior in `code-rs/tui/src/chatwidget.rs` and the `history_cell` modules. The desktop Codex checkout provides a behavioral reference, but its newer TUI architecture differs substantially, so the implementation must port the rendering contract rather than copy whole modules.

The reference implementation establishes useful seams:

- `/Users/doing/Desktop/codex/codex-rs/tui/src/chatwidget/streaming.rs` records completed reasoning as a transcript-only item.
- `/Users/doing/Desktop/codex/codex-rs/tui/src/history_cell/messages.rs` makes transcript-only reasoning invisible in compact scrollback while retaining it for the expanded transcript.
- `/Users/doing/Desktop/codex/codex-rs/tui/src/exec_cell/model.rs` and `render.rs` group adjacent read/list/search calls as exploration and cap command-output previews.

The QCodex implementation already has `ExploreAggregationCell` and the event handling in `code-rs/tui/src/chatwidget/exec_tools.rs`; it also deliberately skips collapsed reasoning while locating a trailing exploration group. This existing grouping is close to the reference and must be verified, not replaced with a second aggregation mechanism. In contrast, QCodex's generic `ToolCallState` persists a bounded `result_preview`, not a universal full-result expansion payload. The compact policy must therefore never hide information on the assumption that an unavailable detail panel can recover it.

QCodex must preserve its stable stream ordering rule: every Answer or Reasoning insertion needs its existing non-empty stream id and `(request_ordinal, output_index, sequence_number)` order metadata. Presentation changes must not create a path that drops, duplicates, or reorders streamed content.

## Goal

At the reference terminal size and for the same event sequence, the QCodex history is scannable in the same way as desktop Codex:

1. completed reasoning is retained for replay/transcript needs but does not leave verbose intermediate narration in normal collapsed history;
2. adjacent exploratory reads, lists, and searches appear as one compact `Explored` activity with concise child entries;
3. executed commands show an invocation plus a bounded, useful result preview, including clear non-zero-exit output;
4. non-exploration tools use compact status rows and a bounded, useful preview or error by default, without losing any information that has no persisted detail representation; and
5. final assistant prose and user-visible errors remain visible, chronological, and readable.

## Plan

1. Create a failing VT100 regression fixture in `code-rs/tui/tests/vt100_chatwidget_snapshot.rs` before changing production code. Drive the real QCodex event path with consecutive reasoning-summary deltas, read/search commands, a successful command, and a failed command with stderr. Encode the reference contract in assertions and a deterministic snapshot: no completed reasoning prose in compact history, one existing exploration group, readable command headers, and only bounded output. Add a second narrow test that validates the preserved transcript/history record still contains the completed reasoning after a save/rebuild cycle.
2. Trace the QCodex event-to-cell pipeline from `EventMsg::AgentReasoning*`, exec begin/output/end, custom/MCP/browser tool begin/end, and patch events in `code-rs/tui/src/chatwidget.rs` through `streaming`, `history_cell::registry`, and the persisted history state. Record which events become `CollapsibleReasoningCell`, `ExecCell`, `ToolCallCell`, `RunningToolCallCell`, or custom cards, and identify the existing insertion order key for each path. Use this map to distinguish model reasoning from user-facing assistant commentary and tool status; do not hide final responses or error messages simply because they originated while a tool was active. Treat a missing stream id or incomplete `OrderMeta` as a tested error boundary; do not extend synthetic-key fallbacks to make a compact-rendering failure look ordered.
3. Change the completed-reasoning contract in `code-rs/tui/src/history_cell/reasoning.rs` and the corresponding `chatwidget.rs` insertion/finalization paths. The desktop reference buffers reasoning in the status row and stores its completed content as transcript-only, optionally attaching it to an active activity group. QCodex must make compact-history visibility a deterministic policy derived from `show_reasoning` and reapplied after restore; its current runtime-only `hide_when_collapsed` flag is not serialized, so setting that flag alone would make reasoning reappear after replay. Retain the full structured reasoning state and stream identity for replay, transcript expansion, copy/search behavior, and the explicit reasoning-view control. During streaming, render at most the compact live activity indication required by the reference behavior, then fold it at the terminal event.
4. Verify and preserve the existing QCodex exploration aggregation in `history_cell/explore.rs` and `chatwidget/exec_tools.rs` against the desktop `ExecCell` rules. Add only the missing parity cases: adjacent read/list/search calls may remain in one group only when they are contiguous in the same turn/order sequence and every parsed command fragment is independently classified as read/list/search. An unknown or mixed fragment, environment mutation, shell control operator, state-changing command, approval boundary, or separate run must flush the group. Preserve current failed exploration summaries and their exit/error text. Do not add a parallel grouping path or reimplement `ExploreAggregationCell`.
5. Replace the default expanded generic-tool presentation in `code-rs/tui/src/history_cell/tool.rs` and `tool_factory.rs` with an event-specific compact summary policy. First inventory each tool's persisted data contract: `ToolCallState` offers a result preview and error message, while specialised cells may own richer data. Define and test one summary for exec, file operation, web/search, browser, MCP/custom, patch, agent, and wait. Redact secrets from arguments and results before formatting, normalize terminal control sequences with the existing safe text/ANSI path, and cap bytes before JSON compaction, display-width truncation, and line truncation. Do not suppress a result that has no durable detail payload; either keep its semantically useful preview/error visible or explicitly add a serializable detail representation with replay coverage. Make status, duration, success, cancellation, and failure transitions distinct without adding permanent card chrome around ordinary completed events.
6. Update the central history layout in `code-rs/tui/src/chatwidget.rs` only where a classified compact cell requires it. Do not globally remove backgrounds, gutters, or spacers: change only the reasoning/tool classes proven by the fixture, while retaining deliberate treatment for assistant prose, approvals, modal UI, and explicitly expanded content. Validate narrow widths, wrapped CJK text, scrolling, and partially visible cells so calculated cell heights, virtual-scroll offsets, and gutter anchors remain consistent.
7. Compare the new VT100 output against the two supplied screenshots and the local desktop Codex behavior at a representative terminal size. Exercise: a streaming turn, repeated exploration, command success, command failure, a long tool result, an explicit reasoning reveal, transcript/replay, and interruption while a tool or reasoning stream is active. Tune shared summary/preview helpers rather than adding one-off behavior for the sample commands.

## Think — Debug Methodology

- Start with the failing VT100 fixture and capture the ordered event sequence, cell kinds, stream ids, and history ids at the boundaries where `chatwidget.rs` inserts or replaces a history cell. Use temporary tracing such as `[DEBUG-history-parity]`; do not log every rendered terminal cell, and remove the trace after the defect is localized.
- Read source before making a visual inference: first the QCodex event handlers and cell constructors, then its history persistence/registry code, then the desktop Codex `streaming.rs`, `messages.rs`, and `exec_cell` reference behavior. Treat the screenshots as acceptance evidence and the desktop source as the authority for event semantics.
- Localize each mismatch upstream: determine whether the wrong information arrives from the provider, is classified into the wrong cell type, is retained when the cell finishes, or is only rendered too verbosely. Check the cell's compact display, expanded/transcript display, and raw/replay representation separately.
- Search globally for `CollapsibleReasoningCell`, `show_reasoning`, `new_running_*tool*`, `new_completed_*tool*`, `ExecCell`, `display_lines`, `custom_render_with_skip`, and hard-coded card backgrounds. Also audit every insertion path for `OrderMeta`/stream id so coalescing cannot violate strict per-turn ordering.

## Do — Verification Strategy

- **Regression first**: run the one new focused VT100 test while it is failing, then until it passes after each relevant change. The fixture must exercise the actual event routing rather than constructing only pre-rendered text lines.
- **Focused behavior checks during implementation**: run only the directly affected TUI test target(s) needed to validate reasoning persistence/replay and the compact activity snapshot. Include a mixed-command classification case, a missing-id/order-metadata case, escaped control-sequence and secret-bearing tool text, and oversized JSON/plain result cases. Review each intentional snapshot update; do not invoke `rustfmt`.
- **Runtime verification**: launch the built TUI in the same terminal profile used for the screenshots. Reproduce a turn that reads/searches files, runs a successful command, runs a failing command, and emits reasoning. Confirm compact history, explicit detail/reasoning reveal, scrolling, and resize behavior visually. Repeat after resuming the turn if history persistence is touched.
- **Logic correctness**: walk live delta, terminal reasoning event, section break, task completion, cancellation/interruption, delayed tool completion, non-zero command exit, history serialization/rebuild, and replay paths. Confirm each item has one stable order key, no duplicated reasoning or tool row, no missing final assistant response, no reappearance of hidden completed reasoning after restore, and no loss of data that lacks a persisted detail representation.
- **Build**: run `./build-fast.sh` from the repository root as the final required gate. It must pass cleanly with no errors or warnings. Do not run extra formatting, lint, or test commands after this final gate unless specifically requested.

## Adjust — Rollback and Global Scan

- **Rollback plan**: keep the changes localized to QCodex history-cell contracts, event insertion, and regression fixtures. A revert restores the existing display policy without changing the wire protocol or deleting persisted history. If a new persisted visibility field is unavoidable, deserialize its absence to the current safe default and keep a compatibility reader before writing it.
- **Global scan**: after the first fix, inspect every history producer—not only exec and reasoning—including MCP/custom tools, browser tools, patches, agent batches, waits, approvals, Auto Drive, replay, and restored sessions. Find analogous code that promotes detail into compact scrollback and route it through the same summary policy.
- **Backwards compatibility**: preserve existing session/history records, current stream ids and ordering metadata, the explicit reasoning-toggle behavior, persisted tool previews/errors, and user-visible final/error content. Existing saved turns may retain their old visual layout when replayed if their historical state lacks the new classification metadata; they must still be readable and replayable.

## Open Questions

- The screenshots establish the compact default, but do not show whether QCodex should offer a persistent configuration option for verbose reasoning/tool history. The proposed default preserves the current explicit reasoning toggle; decide during implementation whether a separate configuration option is necessary.
- The desktop checkout is ahead of QCodex and has a different TUI architecture. The implementation should port observed behavior and focused algorithms, not attempt a wholesale upstream merge.
- Confirm whether all custom plugin tools should share one generic compact summary or whether frequently used QCodex tools need named summaries beyond the categories listed above.

## Out of Scope

- Replacing QCodex's whole TUI with the newer desktop Codex TUI architecture.
- Changing model/provider reasoning generation, tool protocols, or command-execution semantics merely to alter presentation.
- Theme palette redesign, terminal emulator configuration, or unrelated composer/sidebar changes.

---

> Pre-mortem completed: the implementation gates below must be satisfied before this plan is executed.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] The implementation hides commentary or final answers instead of completed reasoning

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 16.0

**Failure Scenario**: The screenshot's verbose prose is assumed to be one event type, but QCodex can receive reasoning, commentary, final agent messages, and tool progress through different paths. A broad display filter hides useful assistant output while the actual source of the unwanted rows remains visible.

**Mitigation**:

- Build the failing fixture from real `EventMsg`/stream-controller routes and assert separately for reasoning, commentary, final answer, and tool-error rows before changing display code.
- Add an event-to-cell matrix to the implementation review: producer, cell type, compact rendering, transcript rendering, persisted record, and stable order key. Change only the cell type proved to produce each unwanted row.
- Use the desktop `chatwidget/streaming.rs` split as behavioral evidence: live reasoning updates the status surface; completed reasoning is transcript-only; completed agent messages remain visible.
- Exit criterion: the fixture contains visible final prose and an error while asserting that completed reasoning prose is absent. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] Hidden reasoning returns after history restore or replay

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 16.0

**Failure Scenario**: QCodex sets `CollapsibleReasoningCell::hide_when_collapsed` on a live cell, but that runtime flag is not represented by the persisted `ReasoningState`. The live turn looks correct, then the same verbose reasoning reappears after resume or replay.

**Mitigation**:

- Make compact visibility a deterministic rendering policy derived from the restored configuration and cell state; do not use an unpersisted flag as the sole source of truth.
- Preserve all `ReasoningState` sections and stream/history identities, then apply the same collapsed-policy refresh after initial load, insertion, replacement, replay, and a reasoning-visibility toggle.
- Add save/rebuild/replay coverage that checks compact output, transcript availability, and the explicit reveal path for the same completed reasoning record.
- Exit criterion: a serialized and reconstructed fixture produces the same compact screen and still exposes the full reasoning through the supported transcript/reveal route. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] A second exploration grouper corrupts command boundaries

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: Work copies the desktop `ExecCell` grouping design into QCodex even though QCodex already has `ExploreAggregationCell` and `exec_tools.rs`. Two aggregation paths then merge non-contiguous calls, split live calls, or attach a failure to the wrong exploration row.

**Mitigation**:

- Treat `ExploreAggregationCell` as the only QCodex owner of exploration grouping. Restrict changes to its existing eligibility, flush, replacement, and status-update boundaries.
- Add ordered tests for read → read, read → reasoning → read, read → state-changing run, read → approval, and failed search. Assert group count, child order, status/error text, and that no command crosses a turn/order boundary.
- Verify the existing `find_trailing_explore_agg` behavior when reasoning is hidden and shown before changing it; document every intentional parity difference from the desktop code.
- Exit criterion: all grouping cases use one aggregator and no test can merge across a non-exploration or order boundary. Residual target: `4 × 1 × (1 - 0.9) = 0.4`.

### [Risk] Compact tool rows silently lose the only available output

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75

**Failure Scenario**: The plan assumes every tool has an expandable detail view. In QCodex, generic `ToolCallState` stores `result_preview` and `error_message`, so hiding those rows or their arguments can make a failure, URL, command result, or recovery instruction unrecoverable after the tool completes.

**Mitigation**:

- Inventory each tool-cell state before altering its renderer and classify the output as preview-only, separately persisted detail, or intentionally nonpersistent.
- Keep a bounded result preview and non-empty error visible for preview-only tools; compact arguments before compacting result/error data.
- Where full detail is a product requirement, add an explicit serializable detail field plus replay/copy tests before hiding the primary content. Do not use a visual-only truncation as a substitute for retained data.
- Add redaction assertions for `ArgumentValue::Secret` and representative JSON arguments so the compact renderer does not reveal secrets while reformatting them.
- Exit criterion: each tool category has a data-retention decision and a test proving that its visible compact row preserves the only persisted result/error. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] Cell replacement breaks strict stream ordering or duplicates terminal rows

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14.0

**Failure Scenario**: Coalescing, hiding, or replacing history cells changes an index while a reasoning/tool stream is still active. A late delta then targets a stale cell, or an event without the expected order key creates a duplicate/misordered row. This violates the TUI's strict ordering contract even if the final screenshot looks compact.

**Mitigation**:

- Keep stream insertion and ordering ownership in the existing stream controller. Presentation code may change a cell's compact projection but must not synthesize a new Answer/Reasoning insert or discard its non-empty stream id.
- Test interleavings: reasoning delta → tool begin → reasoning section break → tool end → terminal reasoning, plus cancellation and a delayed tool end. Assert ordered cell identity/count and the absence of duplicate final rows.
- Instrument only insertion/replacement boundaries with temporary order-key diagnostics, then remove them after verifying that each replacement retains its history identity and invalidates the correct render cache.
- Exit criterion: every streamed fixture event carries its expected id/key and the ordered-cell assertions pass under delayed completion. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] Compact rendering destabilizes virtual scrolling and wrapped layouts

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: A renderer returns fewer lines than `desired_height`, or changes padding without synchronizing cached layout. Users see a jump while scrolled up, clipped output, a CJK wrap mismatch, or the existing history-height mismatch panic only at a narrow terminal size.

**Mitigation**:

- For every changed cell, keep `display_lines`, custom rendering, `desired_height`, and cache invalidation in one review unit. Avoid special background/spacer painting that is not reflected in the height calculation.
- Extend the VT100 scenario across narrow and normal widths, CJK/wide characters, a multi-frame resize, a partial viewport, and a scroll-up streaming update. Assert the reading position and final row remain stable.
- Keep the existing mismatch detector enabled and treat any height mismatch or truncation warning from `./build-fast.sh` as a release blocker.
- Exit criterion: deterministic frames and layout metrics agree for all width/scroll cases. Residual target: `4 × 1 × (1 - 0.9) = 0.4`.

### [Risk] The large desktop architecture is copied instead of its behavior

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: The newer desktop checkout uses a different transcript/activity architecture. A broad import or partial port conflicts with QCodex's history state, Auto Drive, tool routing, and pending local changes, creating a large unstable refactor that still does not reproduce the screenshots.

**Mitigation**:

- Maintain a small parity table before coding: desktop behavior, QCodex owner, evidence test, and the smallest change needed. Port only behavior and isolated algorithms, never whole desktop modules.
- Keep the first implementation limited to completed-reasoning visibility, existing exploration-group verification, and generic-tool compact rendering. Require a separate plan for transcript architecture replacement or unrelated Auto Drive/card redesign.
- Review the final diff by subsystem; if it crosses unrelated history producers, split the work before merging.
- Exit criterion: every changed QCodex file maps to one parity-table behavior and no desktop module is copied wholesale. Residual target: `4 × 1 × (1 - 0.9) = 0.4`.

### [Risk] The work expands into a visual redesign and obscures the defect

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.6
**Risk Score**: 3.6

**Failure Scenario**: Theme colors, unrelated assistant backgrounds, cards, composer work, or settings UI changes are bundled with history compaction. Reviewers cannot tell whether the requested content behavior is fixed, and regressions become hard to isolate.

**Mitigation**:

- Keep the VT100 parity fixture as the scope gate and reject changes not needed to make its event content match the reference behavior.
- Use class-specific styling only after a test identifies the responsible cell; defer broad visual work to a separately scoped plan.

### [Risk] The reference behavior changes before the work is reviewed

**Severity**: 2 | **Likelihood**: 2 | **Detectability**: 0.8
**Risk Score**: 0.8

**Failure Scenario**: The desktop checkout advances and its current TUI details change, leading to rework based on an unpinned visual target.

**Mitigation**:

- Treat the supplied screenshots plus the checked local desktop revision as the acceptance baseline; record the desktop revision in the implementation PR or commit message.
- Keep QCodex's own regression snapshots as the durable contract, so future parity updates are intentional rather than accidental drift.

### [Risk] Compact rendering exposes secrets or lets tool control text alter the terminal

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: A new summary path formats raw JSON or preview text directly. It includes a credential in a nested result, or it bypasses the ANSI/control-sequence handling used by desktop Codex's command renderers. The compact row then leaks a secret or renders terminal control text instead of inert content.

**Mitigation**:

- Define one shared safe-preview pipeline for every new compact renderer: bounded input, secret-aware redaction of arguments and result fields, ANSI/control-sequence normalization, then display-width truncation. No renderer may format raw result text independently.
- Reuse the repository's established safe text/ANSI helpers where applicable; do not copy the desktop formatter without adapting it to QCodex's structured state.
- Add regression cases containing `ArgumentValue::Secret`, nested credential-shaped JSON fields, OSC/CSI escape sequences, backspaces, and normal ANSI-coloured output. Assert neither secrets nor executable control sequences reach the rendered frame.
- Exit criterion: all compact tool categories use the shared safe-preview path and the adversarial fixture renders stable escaped text with redacted values. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] A mixed or misparsed command is incorrectly merged into `Explored`

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75

**Failure Scenario**: The current action helper can receive multiple parsed fragments and choose an exploration-like action even when the shell command also writes, mutates environment, chains commands, or contains an unknown fragment. Compaction then hides a meaningful state change under an innocuous exploration label.

**Mitigation**:

- Make exploration eligibility an all-fragment predicate rather than a first matching action: every parsed fragment must be an explicitly safe read/list/search form, and command metadata must show no shell-control or mutation boundary.
- Default unknown, mixed, or incomplete parser output to a standalone `Ran` entry; optimize only after safety classification, never before it.
- Add table-driven tests for `read`, `read && write`, `search | write`, `unknown`, environment-prefixed commands, and command arrays with mixed parsed entries. Assert the group boundary and the visible command/error result for every case.
- Exit criterion: no fixture containing a non-read/list/search fragment enters `ExploreAggregationCell`. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] Missing order metadata is silently converted into a colliding compact row

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: Existing event paths can log a missing `OrderMeta` and synthesize a key. A new compaction/replacement path accepts that fallback while another late stream event uses the real key, producing a row that appears in the right visual group but is duplicated, misplaced, or lost when the history is rebuilt.

**Mitigation**:

- Audit every changed Answer/Reasoning insertion and replacement boundary for non-empty stream id plus fully populated ordering coordinates. Preserve the existing explicit drop/error behavior for invalid insertions and do not introduce a new synthetic key path.
- Add a negative harness case for missing id, missing ordinal, missing output index, and missing sequence number. Assert no compact history cell is inserted or merged from an invalid stream event, and assert the diagnostic boundary is observable.
- Keep compaction after ordered insertion: it may alter a completed cell's projection but cannot decide stream identity or event order.
- Exit criterion: all compact-stream tests reject incomplete ordering metadata and every accepted event has a unique stable order tuple. Residual target: `5 × 1 × (1 - 0.9) = 0.5`.

### [Risk] Large or adversarial tool results make the TUI stall during every redraw

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.1
**Risk Score**: 14.4

**Failure Scenario**: A formatter parses or JSON-compacts an entire multi-megabyte result before applying the preview limit, then repeats the work in `display_lines`, `desired_height`, and custom rendering. The TUI becomes slow or unresponsive precisely when a tool emits a large failure log.

**Mitigation**:

- Enforce byte and line caps before JSON parsing, syntax/ANSI normalization, wrapping, and allocation. Keep a deterministic truncation marker that reports omitted content without scanning it during redraw.
- Store the derived compact preview in the persisted/owned tool state when the tool completes, or cache it with a key that changes when the source state changes; never recompute it independently in height and render paths.
- Add a bounded-time regression scenario with a deliberately oversized plain result and an oversized JSON-shaped result. Render repeated frames and resize/scroll them while asserting a fixed preview size and stable frame content.
- Exit criterion: preview construction is bounded before expensive formatting, repeated renders reuse the same compact data, and oversized fixtures retain their responsiveness contract. Residual target: `4 × 1 × (1 - 0.9) = 0.4`.

### Risk Sweep Completion

Iteration 1 identified seven new high risks: source misclassification, restored reasoning visibility, duplicate exploration aggregation, tool-data loss, stream-order corruption, virtual-scroll instability, and an accidental desktop-architecture port. Each now has a concrete testable mitigation and exit criterion.

Iteration 2 found four additional high risks during a source-level audit: output redaction/control-sequence safety, mixed-command classification, missing-order-metadata fallback, and unbounded result formatting. Each now has a concrete testable mitigation and exit criterion.

Iteration 3 rechecked technical, team, and external categories after adding these gates. The remaining risks are scope/review drift and reference drift, both below the high-risk threshold; no previously unrecorded high risk remains.

**High-risk constraints remaining: 0.** The eleven initially high risks may not be waived: their exit criteria are mandatory implementation gates, and their planned residual scores are all below 1.0.
