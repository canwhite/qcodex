# Plan: Invoke Skills from the Composer with Slash Syntax

> **User's request (restated)**: Learn from `/Users/doing/Desktop/codex` and let a user execute a discovered skill by entering `/my-skill` in the prompt input.
>
> **What this plan answers**: Add `/skill-name` as a safe, direct alias for the existing explicit skill invocation mechanism.
>
> Goal: A loaded, enabled skill named `my-skill` can be explicitly invoked as `/my-skill [arguments]` from the TUI composer, with its instructions available to the agent for that turn.

## Context

`qcodex` already discovers skills, requests them with `Op::ListSkills`, stores their metadata and body in the bottom pane, and advertises them to the agent. Its documented and desktop-Codex-compatible explicit syntax is `$my-skill`; the Desktop Codex checkout uses a `$` skill-mention popup rather than treating each skill as a built-in slash command.

The qcodex submission pipeline currently expands `/name` first as a custom prompt (unless `name` is a built-in), then sends recognized built-in slash commands to `AppEvent::DispatchCommand`. Consequently, `/my-skill` falls through as ordinary text instead of becoming an explicit skill request. The change should add an alias at this boundary, not create dynamic `SlashCommand` enum variants or duplicate discovery/loading logic.

Assumptions: “execute” means explicitly request the skill for the submitted agent turn, not execute arbitrary shell code from a skill. Skill body execution continues to follow the existing skill instructions and tool-approval model.

## Goal

For every enabled skill in the current `ListSkills` catalog, an exact, case-insensitive `/skill-name` invocation:

- preserves any trailing user arguments;
- enters the same explicit-skill path as `$skill-name`;
- does not change built-in slash commands, custom-prompt expansion, or ordinary messages; and
- produces a clear error only when the user uses the reserved explicit form for an unavailable/disabled skill, if that form is introduced.

## Plan

1. Map the existing end-to-end explicit-skill path before editing: trace `ListSkillsResponse` into `BottomPane::set_skills`, the `code_core::protocol::Skill` representation, user-instruction injection, and the Desktop Codex `$` mention implementation. Confirm the minimum representation the core needs for a selected skill and whether the TUI must retain a binding/path instead of only rewriting visible text.
2. Add a small, independently testable resolver beside `bottom_pane/prompt_args.rs` (or a focused sibling) that parses only a leading `/name` token and matches it against the loaded enabled-skill catalog. Return an explicit invocation result containing the canonical skill name, its path/binding, and the untouched trailing arguments; return no match for non-slash input, malformed names, and unmatched names.
3. Define and implement precedence in `ChatWidget`’s message-submission pipeline: built-in slash commands retain their existing behavior; existing custom prompts retain their current `/name` and `/prompts:name` behavior; then an exact loaded-skill match is converted to the existing `$skill-name`/binding representation before normal user-message submission. Preserve text/image ordering rules by applying this only to all-text, leading-command messages and do not reinterpret slash-looking text later in a multiline prompt.
4. Use canonical skill metadata rather than user-provided path text. Carry the catalog path/binding through the converted message so the agent receives the correct selected skill body; never read a path inferred from `/name`. Keep arguments as normal user text following the mention so the selected skill can consume them.
5. Make conflict behavior observable and stable: built-ins win over a same-named skill; a legacy bare custom prompt keeps its existing precedence; `$skill-name` remains the unambiguous escape hatch for a skill. If product behavior needs a way to force a shadowed skill, support an explicit namespace such as `/skills:name` only after confirming it cannot conflict with `/skills`; otherwise document the limitation rather than silently changing current custom-prompt behavior.
6. Extend the composer/help documentation and slash-command presentation so users can discover the alias. Update `docs/skills.md` to show `$skill-name` and `/skill-name` with their collision rules, and update any TUI help/completion only if the existing slash menu has a supported dynamic-entry seam; do not register one static slash command per skill.
7. Add regression coverage at the smallest relevant layers: resolver unit tests, `ChatWidget` submission tests with a loaded skill catalog, and a TUI/composer rendering test only if dynamic completion UI is added. Cover exact match, case normalization, hyphenated names, arguments, unknown names, disabled/absent skills, built-in collision, custom-prompt collision, `$` compatibility, and a normal `/unknown` message.

## Think — Debug Methodology

- Start at the framework boundaries: log the catalog received in `EventMsg::ListSkillsResponse`, the leading slash token and resolver outcome immediately before custom-prompt/slash dispatch, and the final submitted text plus selected binding immediately before `submit_user_message`. Use a scoped `[DEBUG-slash-skill]` prefix and remove temporary diagnostics before completion.
- Read source before guessing: first inspect qcodex’s `ChatWidget::submit_message`, `prompt_args`, skill injection in `code-rs/core/src/user_instructions.rs`, and Desktop Codex’s composer/skill mention code. Do not copy the Desktop checkout wholesale because its TUI structure diverges substantially from qcodex.
- Localize failures upstream to downstream: verify a skill reaches the bottom-pane catalog, then verify `/name` resolution and precedence, then inspect the final `UserMessage`/binding, and finally confirm the core emits the selected skill instruction for the turn.
- Grep globally for `expand_custom_prompt`, `process_slash_command_message`, `set_skills`, `ListSkillsResponse`, and `$` mention/binding handling after the focused patch so alternate submit paths, restores, and programmatic submissions cannot bypass or duplicate the alias semantics.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with zero errors and warnings.
- **Static analysis / type check**: N/A — repository policy designates `./build-fast.sh` as the required completion gate; do not add a separate formatter, lint, or test sweep unless specifically requested.
- **Runtime verification**: launch the built TUI with a fixture or local enabled `my-skill`, submit `/my-skill summarize this repository`, and verify the visible user turn and core request carry the canonical skill selection and the argument. Repeat with `$my-skill` to prove the existing route remains intact.
- **Logic correctness**: manually walk built-in (`/plan`), custom prompt (`/custom` and `/prompts:custom`), matching skill (`/my-skill` with and without arguments), unknown slash token, a name collision, disabled skill, image-containing submission, and multiline text where only the first line is slash-like.

## Adjust — Rollback and Global Scan

- **Rollback plan**: remove the submission-boundary resolver/conversion and its help text/tests; this restores the existing `$skill-name` workflow without data migration or persisted-state changes.
- **Global scan**: inspect all `UserMessage` construction, composer submit/retry/restore paths, custom-prompt expansion callers, command-popup sources, and non-TUI clients. Ensure only the intended TUI leading-slash route gains the alias and no path permits a user-controlled filesystem location.
- **Backwards compatibility**: preserve all current built-in slash commands, custom-prompt precedence, `$skill-name` invocation, stored history text, and skill-discovery configuration. Do not serialize a new command type or change protocol schemas unless tracing proves an existing mention binding cannot carry the canonical skill identity.

## Open Questions

- Should a skill intentionally shadowing an existing bare custom prompt gain a forceable slash namespace, or is `$skill-name` sufficient as the explicit escape hatch?
- Should dynamic skills appear in the slash completion popup, or is direct exact-name entry the complete scope for this pain point?
- Does the core’s current explicit-skill machinery require a structured mention binding for reliable body injection, or is canonical `$name` text sufficient in every local and app-server execution path?

## Out of Scope

- Adding a general runtime command-registration system or one `SlashCommand` enum variant per skill.
- Changing skill discovery roots, enable/disable settings, skill file format, or the skill security/approval model.
- Executing scripts or shell commands embedded in a skill without the normal agent and approval flow.
- Porting the Desktop Codex composer, skill popup, or unrelated TUI architecture into qcodex.

---

> Next step: run `/pre-mortem docs/planning/slash-skill-invocation.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Slash alias looks successful but does not select the skill body

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15.0

**Failure Scenario**: The implementation rewrites `/my-skill` to ordinary text or `$my-skill` without preserving the structured binding required by one execution path. The agent replies without having received `SKILL.md`, so users believe an explicit command executed when it did not.

**Mitigation**:

- Trace and test the complete `ListSkillsResponse` → composer → `UserMessage` → core user-instruction path before choosing the representation; assert the emitted request contains the canonical selected skill and its body/path.
- Add end-to-end regression coverage for both local and app-server-backed submission paths when those paths differ, and retain `$my-skill` as a control case.

### [Risk] Alias precedence regresses existing slash behavior

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.20
**Risk Score**: 12.8

**Failure Scenario**: A dynamic skill name collides with `/plan`, another built-in, or a custom prompt. A change at the wrong point in the pipeline reroutes existing commands or causes custom-prompt expansion to consume a skill invocation unexpectedly.

**Mitigation**:

- Encode and document one precedence table in the resolver: built-ins first, existing custom-prompt behavior second, exact enabled skill aliases third, ordinary text last.
- Add table-driven tests for every built-in collision, bare and namespaced custom-prompt collision, unknown token, and the `$skill-name` escape hatch; reject an implementation that relies on incidental branch ordering.

### [Risk] User-controlled text becomes a filesystem or instruction-injection selector

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.10
**Risk Score**: 13.5

**Failure Scenario**: The alias implementation derives a path from `/name`, trusts stale/disabled metadata, or interpolates unescaped text into a skill payload. This permits an unapproved local file to be read as a skill or injects malformed control content into the turn.

**Mitigation**:

- Resolve names exclusively against the enabled in-memory catalog and copy the matched canonical metadata/binding; never construct or read a path from the entered slash token.
- Validate name boundaries and preserve trailing arguments as user content, then add adversarial tests for path separators, traversal-like names, markup characters, disabled skills, and stale catalog entries.

### [Risk] Catalog freshness creates inconsistent or unavailable aliases

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 10.2

**Failure Scenario**: A skill is enabled, disabled, renamed, or reloaded while a TUI session is active, leaving `/name` resolved against an obsolete catalog or silently unavailable until restart.

**Mitigation**:

- Reuse the same catalog update and enablement lifecycle as `$` mentions, clear obsolete selections on refresh, and show a concise unavailable-skill message rather than falling back to a misleading invocation.
- Exercise catalog replacement in a widget-level test and document the session refresh/restart expectation if hot reload is not supported.

### [Risk] Only the interactive composer path gains the alias

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.15
**Risk Score**: 13.6

**Failure Scenario**: The feature works for direct keystrokes but not for retry, queued input, restored drafts, side conversations, or programmatic `submit_text_message` calls that construct messages through nearby but distinct paths. The same visible `/my-skill` prompt then behaves differently depending on how it was submitted.

**Mitigation**:

- Identify the single lowest shared text-submission boundary that has the loaded skill catalog and normalize aliases there; explicitly document callers intentionally excluded from TUI aliasing.
- Add regression tests for direct composer submit plus every supported retry/restore/programmatic path that reaches that boundary, with an assertion that unqualified messages remain unchanged.

### [Risk] Attachments and multiline inputs lose content or get inconsistent semantics

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.30
**Risk Score**: 6.3

**Failure Scenario**: The all-text shortcut mutates `ordered_items` incorrectly, drops attachments, or interprets a slash on a later line as a command. Users see different behavior between plain text and composed messages.

**Mitigation**:

- Limit conversion to a single leading command in a text-only submission, preserving the existing image/attachment route unchanged.
- Add tests for image-containing submissions, multiline prompts, and newline-separated content; treat non-qualifying messages as ordinary input.

### [Risk] Scope expands into a fragile dynamic slash-command framework

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.60
**Risk Score**: 3.6

**Failure Scenario**: Adding autocomplete or dynamic `SlashCommand` variants broadens a small alias into a cross-TUI command-system rewrite, increasing merge conflicts and delaying the requested behavior.

**Mitigation**:

- Keep the first implementation to a submission-boundary resolver and existing skill binding; defer dynamic slash-menu work behind a separately approved requirement.
- Require any proposal to add protocol variants, persistence changes, or per-skill registrations to justify why the existing mention path cannot meet the acceptance tests.

### [Risk] Documentation promises semantics the agent cannot guarantee

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.50
**Risk Score**: 4.5

**Failure Scenario**: Docs describe `/skill` as direct code execution although skills are instructions interpreted by the agent under existing tool approvals, creating unsafe or misleading user expectations.

**Mitigation**:

- State precisely that the alias explicitly selects a skill for the turn; it does not bypass tool permissions or directly execute skill-provided shell commands.
- Review the help text against the runtime verification trace before release and retain the existing `$skill-name` documentation as the unambiguous alternative.

---

## Post-Mortem Verification

**Date**: 2026-11-04

The completed implementation has two narrow, connected flows: `BottomPane::set_skills` forwards the loaded catalog to `ChatComposer`, whose `/` popup filters, displays, and Tab-completes matching skills; `ChatWidget::submit_user_message` then resolves a text-only leading slash input and emits the established canonical `$skill-name` message. The existing core skill instructions define that syntax as an explicit skill trigger; no new command protocol, path parsing, or direct script execution was added.

### Issues Fixed

| # | Severity | Type | Resolution |
|---|---|---|---|
| 1 | High | Missing behavior | Added exact, case-insensitive `/skill-name` resolution with canonical `$skill-name` emission and argument preservation. |
| 2 | High | Contract violation | Preserved existing built-in and custom-prompt precedence; added regression coverage for both. |
| 3 | High | Control-flow collision | Prevented an alias from preempting a same-named configured subagent command; aligned that route with the shared whitespace-aware slash parser. |
| 4 | Medium | Documentation drift | Documented slash alias semantics and the `$skill-name` collision escape hatch in `docs/skills.md`. |
| 5 | High | Missing interaction path | Added loaded skills to the TUI slash popup with fuzzy filtering and Tab completion; command conflicts are hidden rather than offering a non-executable selection. |
| 6 | Medium | Popup-state inconsistency | Unified programmatic composer updates with normal popup resynchronization so recovered or inserted `/` text cannot have its command popup overwritten by file completion. |

### High-Risk Closure

- **Skill body not selected**: closed by converting only a catalog match to the existing `$` trigger; `slash_skill_alias_submits_canonical_skill_mention` verifies the exact `Op::UserInput` payload.
- **Legacy command regression**: closed by filtering available built-ins in the resolver, allowing custom prompts to expand before aliases, and deferring to custom subagent commands; focused tests cover each route.
- **User-controlled filesystem/instruction selection**: closed because the resolver accepts only a name and selects only a preloaded catalog entry; it performs no path construction or file reads from user input.
- **Submission-path inconsistency**: closed at `submit_user_message`, the shared path used by the composer, side conversations, and programmatic text submission. Non-text messages remain unchanged.
- **Skill not discoverable from `/`**: closed by propagating `ListSkillsResponse` catalog entries through `BottomPane` into `ChatComposer` and `CommandPopup`; `slash_popup_includes_loaded_skill` verifies discovery and Tab completion.

**Residual high risks**: 0

### Verification Evidence

- `cargo test -p code-tui slash_skill_alias --lib` passed.
- `cargo test -p code-tui expands_exact_skill_alias_with_canonical_name_and_arguments --lib` passed.
- `cargo test -p code-tui custom_prompt_keeps_precedence_over_matching_skill_alias --lib` passed.
- `cargo test -p code-tui custom_subagent_command_keeps_precedence_over_matching_skill_alias --lib` passed.
- `cargo test -p code-tui does_not_treat_builtins_or_unknown_names_as_skills --lib` passed.
- `cargo test -p code-tui finds_skills_from_the_slash_popup --lib` passed.
- `cargo test -p code-tui slash_popup_includes_loaded_skill --lib` passed.
- `./build-fast.sh` passed cleanly after the final source change.
