# Plan: Isolate Built-in Subagents from MiniMax-Compatible Providers

> **User's request (restated)**: 当 `.env` 配置为 MiniMax provider 时，检查工具层及相关路径是否还错误使用了写死的其他模型/工具；解释截图中失败的 `code-gpt-5.6-terra` 等代理并解决残留问题。
>
> **What this plan answers**: Make external subagent exposure provider-aware so a MiniMax-compatible session keeps its local generic tools but never receives or launches unrelated built-in agent CLIs by default.
>
> Goal: A session using the configured `MiniMax-M3` OpenAI-compatible chat endpoint cannot receive the built-in `code-*`, Claude, Gemini, Copilot, Qwen, or cloud subagent defaults unless the user explicitly configures a runnable local agent.

## Context

The project-local `.env` selects MiniMax through the built-in OpenAI-compatible provider (`OPENAI_BASE_URL`, `OPENAI_WIRE_API=chat`, and `OPENAI_MODEL=MiniMax-M3`). That is transport selection, not an instruction to replace every local tool with a MiniMax API equivalent: shell, file, browser, MCP, and approval tools are client-hosted and remain provider-neutral as long as the selected wire API supports function calling.

The screenshot is caused by a separate subagent feature. `code-rs/core/src/codex/streaming.rs` unconditionally fills an empty `config.agents` list from `default_agent_configs()`, and `code-rs/core/src/openai_tools.rs` always adds the `agent` tool. Its schema advertises hard-coded slugs such as `code-gpt-5.6-terra`; calls for those slugs resolve to local executables (for example `coder`), not MiniMax models. Those executables are unavailable in the shown session, so the run fails. Auto Drive, slash subagent commands, the client fallback, and the TUI Agents settings have parallel defaulting paths that can recreate the same leak.

This plan preserves explicitly configured `[[agents]]` commands as an opt-in local integration. It does not invent a MiniMax-native subagent protocol.

## Goal

For a non-first-party OpenAI-compatible provider with no enabled `[[agents]]` entries:

- request payloads contain no `agent` tool and no built-in-agent model guide;
- Auto Drive and slash subagent command resolution do not fall back to the built-in external-agent catalog;
- TUI agent settings do not advertise built-in agents as available;
- ordinary client-hosted tools continue to work unchanged.

For an explicit enabled agent configuration, the agent tool is exposed only with that configured name and retains its existing command availability checks.

## Plan

1. Define one provider-aware subagent capability/resolution helper in `code-rs/core` that distinguishes the first-party OpenAI/command-auth path from an ordinary OpenAI-compatible endpoint. It must take the resolved provider, provider id, authentication mode, and configured agents; return no implicit built-in agent models for MiniMax and other third-party compatible providers.
2. Move the streaming setup in `code-rs/core/src/codex/streaming.rs` onto that helper. Set `ToolsConfig` agent models only from enabled explicit agents for compatible providers, omit the model-guide developer prompt when no agents are available, and preserve the current built-in behavior for supported first-party/command-auth sessions.
3. Give `ToolsConfig` an explicit agent-tool availability flag (or derive it from a non-empty resolved model list) and update `get_openai_tools` in `code-rs/core/src/openai_tools.rs` to omit `create_agent_tool` entirely when no models are allowed. Do not leave an empty-schema `agent` function that a MiniMax model can still call.
4. Apply the same resolver to parallel execution paths: `ModelClient` tool configuration, `code-rs/code-auto-drive-core/src/auto_coordinator.rs`, and `code-rs/core/src/slash_commands.rs`. Ensure `/plan`, `/solve`, `/code`, and Auto Drive fail early with an actionable “configure a local [[agents]] command” message or run without subagents, rather than silently selecting `code-gpt-*`.
5. Update `code-rs/tui/src/chatwidget.rs` agent overview/selection construction so compatible-provider sessions list only explicitly configured agents. Do not mark absent built-in CLI routes as installed or selectable. Keep the normal first-party settings display unchanged.
6. Audit all built-in agent fallback call sites (`default_agent_configs`, `enabled_agent_model_specs*`, `get_default_models`, and `model_guide_markdown_with_custom`) and route production behavior through the new helper. Retain catalog-only uses and unit-test fixtures where no runtime provider is involved.
7. Add focused regressions covering: MiniMax-compatible chat provider with no agents (no `agent` tool/default slugs/model guide); MiniMax with one enabled explicit local agent (only that name is advertised); first-party OpenAI remains unchanged; Auto Drive/slash/TUI resolution cannot reintroduce a default built-in slug; and an attempted stale built-in-agent request yields a clear non-spawning error.
8. Update the user-facing provider/subagent documentation and `.env.example` comments to distinguish provider-neutral local tools from external subagent CLIs, show the opt-in `[[agents]]` configuration boundary, and state that OpenAI-compatible providers require function-calling support for client tools.

## Think — Debug Methodology

- Add temporary, redacted boundary diagnostics under a `[DEBUG-agent-routing]` prefix immediately after provider resolution and immediately before request serialization. Log provider id/host category, whether defaults are permitted, configured-versus-resolved agent names, and whether the `agent` tool was emitted; never log API keys or request Authorization headers.
- Read source in this order: `ModelProviderInfo` first-party predicates, configuration/agent resolution, `ToolsConfig`, `get_openai_tools`, then agent invocation and TUI rendering. The screenshot is evidence of incorrect tool advertisement before it is an agent-execution failure.
- Reproduce with a local OpenAI-compatible chat fixture using a MiniMax-shaped model id, inspect the serialized tools array, then verify a real MiniMax session only after the local payload contract passes.
- Search for `default_agent_configs`, `enabled_agent_model_specs`, `get_default_models`, `model_guide_markdown_with_custom`, and `create_agent_tool` after the first patch. Every runtime fallback must either use the provider-aware helper or be justified as catalog/test-only.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass with zero errors and warnings.
- **Static analysis / type check**: N/A — the required Rust build is this repository’s prescribed compilation and warning gate.
- **Focused regression tests**: run only the directly affected Rust test targets while developing, including the new core tool-schema/provider-routing tests and existing Auto Drive/slash-command/TUI agent tests that cover modified behavior.
- **Runtime verification**: run the configured MiniMax session with a harmless request, capture the outgoing tool schema using the local compatible-provider fixture or existing debug boundary, and confirm it lacks `agent` and every `code-gpt-*` default. Verify shell/file tools remain present. Then configure one harmless explicit local agent and confirm the schema exposes only that configured name.
- **Logic correctness**: manually walk first-party OpenAI, command-auth, MiniMax/no agents, MiniMax/explicit enabled agent, MiniMax/explicit disabled agent, unavailable explicit-agent command, Auto Drive, slash subagent commands, and the TUI Agents overview. Confirm no route launches a default CLI for a compatible-provider session.

## Adjust — Rollback and Global Scan

- **Rollback plan**: revert the new provider-aware resolver and its call-site changes as one unit; no persisted configuration migration is required. Explicit `[[agents]]` entries remain untouched.
- **Global scan**: re-run the fallback-symbol search and inspect all tool constructors that advertise model-specific capabilities, especially image generation, web search, remote models, and model guides, to ensure third-party endpoints do not inherit first-party-only features.
- **Backwards compatibility**: first-party OpenAI and command-auth sessions keep their current default subagent catalog. Third-party compatible sessions that relied on implicit unavailable CLIs change to safe non-exposure; users who intentionally use a local external agent must declare it explicitly in `[[agents]]`.

## Open Questions

- Should an explicitly configured `[[agents]]` command be allowed for every provider as planned, or should compatible-provider subagents require an additional explicit feature flag?
- Does MiniMax-M3 support the complete function-calling schema used by the remaining local tools? If not, the follow-up scope should be schema compatibility/simplification, not vendor-specific tool replacement.
- When `/plan`, `/solve`, or `/code` is requested with no eligible agent, should the command fall back to a single-agent in-process prompt or return a configuration error? This plan assumes an actionable error to avoid changing task semantics silently.

## Out of Scope

- Implementing or calling MiniMax-native tools, APIs, or a separate MiniMax subagent service.
- Installing, configuring, or impersonating `coder`, Claude, Gemini, Copilot, Qwen, or cloud CLIs.
- Changing generic client-hosted shell, file, MCP, browser, approval, or sandbox behavior solely because the LLM provider is MiniMax.
- Altering the existing `.env` authentication/model precedence work beyond using its resolved provider identity.

---

> Next step: run `/pre-mortem docs/planning/minimax-provider-agent-routing.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Hostname-based classification hides or exposes the wrong agent catalog

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14.0 — HIGH

**Failure Scenario**: The resolver reuses `is_public_openai_*_endpoint`, whose host-based behavior is intended for model metadata rather than subagent ownership. An OpenAI-compatible proxy on `api.openai.com`, a first-party endpoint behind a proxy, or a command-auth provider is misclassified. The release either keeps exposing unusable `code-gpt-*` agents to MiniMax users or unexpectedly removes working defaults for first-party users.

**Mitigation**:

- Introduce a named, narrowly scoped subagent-default capability predicate with documented inputs and semantics; do not infer it incidentally from the model-metadata predicate or host name alone.
- Cover the decision table with unit tests for public OpenAI, a custom endpoint hosted on an OpenAI-looking domain, MiniMax/custom `chat`, MiniMax/custom `responses`, and command-auth providers.
- Emit a redacted diagnostic that identifies the capability decision and reason, then retain it only if it fits the repository’s normal debug logging policy.

### [Risk] One fallback path keeps injecting built-in models after the primary fix

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15.0 — HIGH

**Failure Scenario**: Interactive streaming stops advertising `agent`, but Auto Drive, slash commands, a `ModelClient` rebuild, or a TUI overlay still calls `enabled_agent_model_specs*` directly. Users continue to see or launch `code-gpt-5.6-terra` from only certain modes, making the defect intermittent and difficult to reproduce.

**Mitigation**:

- Make the provider-aware resolver the only production API that returns an effective agent list; mark lower-level catalog functions as catalog/test helpers where practical.
- Add a repository-wide guard test or targeted source assertion for runtime calls to the old fallback symbols outside the resolver, with documented exemptions for catalog display and fixtures.
- Exercise the exact entry points in regression coverage: normal turn serialization, Auto Drive coordinator initialization, each built-in slash subagent command, TUI agent overview, and a session/tool-config rebuild.

### [Risk] Tests validate an internal tool list but not the MiniMax chat request payload

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.35
**Risk Score**: 13.0 — HIGH

**Failure Scenario**: `get_openai_tools` returns the expected list in a unit test, but the Chat Completions serializer, dynamic-tool expansion, or retry path later inserts an `agent` definition or the model guide. Production MiniMax requests still receive the forbidden tool despite green unit tests.

**Mitigation**:

- Add a local HTTP fixture that captures the actual Chat Completions JSON body and asserts the absence of `agent`, `code-gpt-*`, and built-in-agent guide text for the MiniMax-shaped configuration.
- Repeat the assertion for the Responses serializer if that configuration remains supported, and inspect at least one retry/rebuild path.
- Make the final manual smoke check inspect a redacted request-schema summary rather than relying only on UI behavior.

### [Risk] Explicit local-agent users lose a supported workflow

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.45
**Risk Score**: 6.6

**Failure Scenario**: The new capability gate disables the `agent` tool wholesale for every compatible provider, including users who intentionally configured a valid local `[[agents]]` command. This silently breaks an opt-in workflow that does not depend on the LLM provider’s own model catalog.

**Mitigation**:

- Resolve enabled explicit agents before deciding whether to emit the tool; compatible providers may expose only those names.
- Add regression coverage using a temporary harmless command fixture, including enabled, disabled, and missing-command cases.
- Document the changed boundary and return a precise configuration/availability error when an explicit command cannot run.

### [Risk] Removing `agent` changes a provider’s function-call schema enough to break ordinary tools

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 7.2

**Failure Scenario**: MiniMax accepts a minimal tool list but rejects another existing function schema, or changes model behavior after the agent tool disappears. The team attributes all failures to subagent routing and ships without validating the real compatibility contract.

**Mitigation**:

- Keep this change limited to subagent advertisement; snapshot the remaining tool names and schemas before/after the change.
- Use the local compatible-provider fixture for serialization, then perform a read-only live MiniMax smoke request that exercises one ordinary tool only if the user’s credentials and provider quota permit it.
- If the provider rejects a remaining schema, track that as a separate schema-compatibility issue with a captured provider error; do not add vendor-specific behavior to this change opportunistically.

### [Risk] Disabling implicit agents causes unclear failures in slash commands and Auto Drive

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.5
**Risk Score**: 6.0

**Failure Scenario**: `/plan`, `/solve`, `/code`, or Auto Drive now has no runnable subagent, but its error path assumes at least one model. Users see an empty selection, an internal error, or a misleading claim that MiniMax is unavailable.

**Mitigation**:

- Decide and implement one shared no-eligible-agent outcome before changing call sites; include the provider-neutral remediation: configure an enabled local `[[agents]]` command or use the main session without subagents.
- Assert that no-eligible-agent handling does not spawn a process, create a misleading run card, or replace the primary session model.
- Add UI and command-level text assertions so the message stays actionable and does not expose implementation-only model names.

### [Risk] The UI retains cached built-in rows across provider or profile changes

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.45
**Risk Score**: 4.95

**Failure Scenario**: The TUI computes a correct list at cold start but retains `code-gpt-*` rows after a profile/model-provider switch, making users believe the route remains available and allowing a stale action path.

**Mitigation**:

- Trace configuration/profile update events and invalidate or recompute the agents overview from the effective resolved agent list.
- Add a deterministic UI regression that transitions from first-party defaults to MiniMax-compatible configuration and verifies that built-in rows disappear.
- Keep action-time validation authoritative even when a stale UI row exists, so it cannot launch an ineligible default command.

### [Risk] Documentation overstates compatibility and encourages unsafe implicit configuration

**Severity**: 2 | **Likelihood**: 3 | **Detectability**: 0.6
**Risk Score**: 2.4

**Failure Scenario**: The documentation says MiniMax is “supported” without distinguishing OpenAI-compatible chat transport, function calling, and explicit external-agent setup. Users expect vendor-native tools or mistakenly configure arbitrary commands as agents.

**Mitigation**:

- Describe support precisely: standard OpenAI-compatible request/function-call transport; local tools are client-hosted; local subagents are separately explicit commands.
- Use a secret-free, non-destructive example command and link to the existing agent configuration reference rather than duplicating its full syntax.
- Review wording against the final observed request payload and runtime behavior before merging.

### Convergence Review

Iteration 1 found three HIGH risks: an incorrect capability classifier, incomplete fallback removal, and tests that stop before actual request serialization. The plan now requires a dedicated predicate, a single production resolver with global-scan enforcement, and captured wire-payload tests.

Iteration 2 assessed provider schema compatibility, explicit-agent preservation, no-eligible-agent UX, UI caching, and documentation. It found no additional HIGH risks. Every recorded risk has an explicit mitigation.

**Final HIGH RISK count: 3.**

---

## Post-Mortem: MiniMax Provider Agent Routing

**Date**: 2026-11-04

### Implementation Map

`Config` now retains only explicitly configured agents. `provider_supports_builtin_agent_defaults` is the single capability boundary for implicit local-CLI routes. `ModelClient` and streaming build the effective agent list at request setup; `get_openai_tools` serializes `agent` only when that list is non-empty. Auto Drive, exec and TUI slash-command expansion, settings rows/editors, and the one-shot LLM path apply the same boundary.

### Issues Fixed

| # | Severity | Type | Location |
|---|---|---|---|
| 1 | High | Provider-isolation violation | `core/src/config.rs`, `core/src/client.rs`, `core/src/codex/streaming.rs`, `core/src/openai_tools.rs` |
| 2 | Medium | Parallel-path fallback | `code-auto-drive-core/src/auto_coordinator.rs`, `exec/src/{lib,slash}.rs`, `tui/src/chatwidget.rs`, `cli/src/llm.rs` |
| 3 | Low | Documentation drift | `.env.example`, `docs/agents.md`, `docs/config.md` |

1. Config loading merged the built-in local-CLI roster into every provider session. MiniMax-compatible sessions consequently advertised `code-gpt-*` names and the `agent` tool. Configuration now preserves explicit agents only; the built-in roster is resolved at runtime exclusively for first-party OpenAI or command-auth providers.
2. The first review found downstream reintroductions in Auto Drive, exec slash expansion, TUI availability/editor views, and the one-shot LLM prompt path. Each now receives provider-safe defaults or omits agent guidance entirely.
3. The tracked examples and agent documentation now distinguish provider-neutral local tools from opt-in external CLI subagents.

### Root Causes

1. The code treated a model provider selection as if it also selected the local subagent catalog.
2. Default-agent resolution was duplicated at presentation, prompt-expansion, and request-construction boundaries.
3. Documentation described the roster as universal rather than provider-owned.

### Verification Evidence

- `./build-fast.sh` passed cleanly after the final changes, including the dependency guard and Rust compilation with no warnings.
- A new tool-construction regression asserts that an empty eligible-agent list does not serialize `agent`.
- A provider-boundary regression distinguishes a MiniMax-compatible chat endpoint from the first-party OpenAI chat endpoint.
- A final production-call-site scan confirmed all remaining default-catalog resolution is guarded by `provider_supports_builtin_agent_defaults`; catalog-only helpers and tests are excluded.

### Prevention Tasks

| Task | Type | Status |
|---|---|---|
| Keep provider-safe agent resolution at request, Auto Drive, exec, and TUI boundaries | AFK | Complete |
| Keep regression coverage for empty agent-tool serialization and provider classification | AFK | Complete |
| Validate a live MiniMax account’s complete tool-call schema before claiming vendor-wide wire compatibility | HITL | Not required for this local-CLI routing fix |

### Risk Closure

The three pre-mortem HIGH risks are closed: classification has one tested predicate, production fallback paths use that predicate, and the request tool factory cannot emit `agent` when the resolved list is empty. The remaining live-vendor schema check concerns provider-wide function-call compatibility, not this routing change; it does not permit reintroduction of built-in agent names or launchers.

**Final HIGH RISK count: 0.**
