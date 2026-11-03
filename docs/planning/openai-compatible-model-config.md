# Plan: OpenAI-Compatible Model Configuration

> **User's request (restated)**: 我需要你兼容当前模型；最终目标是我输入一组模型和相关配置，它能跑起来。
>
> **What this plan answers**: Define a project-local configuration path that selects and successfully calls an OpenAI-compatible model instead of an existing ChatGPT login.
>
> Goal: A user can fill one `.env` file with an API key, endpoint, wire protocol, and model identifier, then run `code` without hidden global authentication or model settings taking precedence.

## Context

The project already loads `$(pwd)/.env`, supports `OPENAI_BASE_URL`, and supports `OPENAI_WIRE_API=chat|responses`. Its current behavior is incomplete for an OpenAI-compatible endpoint: a persisted ChatGPT login can still win over `OPENAI_API_KEY`, and a model configured in the global config can win over the project's model. A real smoke request reached the configured endpoint but returned HTTP 401 because no API-key Authorization header was sent; its startup summary also selected the global `gpt-5.6-terra` model instead of the project value.

This plan scopes compatibility to APIs that implement either OpenAI Chat Completions or OpenAI Responses wire formats. The provider's model identifier is opaque and is passed through unchanged.

## Goal

With this project-local configuration:

```dotenv
OPENAI_API_KEY=...
OPENAI_BASE_URL=https://provider.example/v1
OPENAI_WIRE_API=chat
OPENAI_MODEL=provider-model-id
```

`code exec` must authenticate with that API key, target that base URL using the selected wire protocol, and send `provider-model-id`. `--model` remains an explicit per-invocation override.

## Plan

1. Define the project OpenAI-compatible configuration contract: validate non-empty API key, absolute HTTP(S) base URL, `chat` or `responses` wire value, and non-empty model identifier; document that the model name is supplied by the compatible provider.
2. Centralize environment resolution into a typed configuration helper so `.env` values are not separately interpreted by the dotenv loader, auth manager, model provider builder, and model selector.
3. Set deterministic precedence: CLI `--model` and explicit config profile selections win; when a project OpenAI-compatible configuration is complete, its API key, endpoint, protocol, and model override persisted ChatGPT authentication and global defaults for that invocation; incomplete project configuration must fail with an actionable error instead of silently mixing sources.
4. Update the auth-manager creation and reload paths to construct API-key authentication from the resolved project configuration before consulting `auth.json`; preserve the existing `CODEX_API_KEY` behavior and redact secrets in diagnostics.
5. Update model resolution so `OPENAI_MODEL` is marked explicit and is used ahead of global default model configuration while retaining CLI/profile overrides. Ensure compatibility checks do not replace an unknown third-party model slug with an OpenAI default.
6. Use the resolved base URL and `OPENAI_WIRE_API` to construct the built-in OpenAI provider for both interactive and `code exec` startup paths. Reject unsupported wire values before networking.
7. Add unit and integration coverage for the complete configuration, precedence against a simulated ChatGPT `auth.json`, empty or malformed values, both wire APIs, and no regression when the project `.env` is absent.
8. Provide an ignored local `.env` template and a tracked, secret-free `.env.example` explaining each setting and its permitted values.

## Think — Debug Methodology

- Log a redacted `[DEBUG-openai-config]` summary immediately after dotenv loading and immediately before auth-manager/provider construction: config source, host, wire API, model, and auth mode; never emit the key or Authorization header.
- Read the startup path in this order: `arg0` dotenv loading, config resolution, `AuthManager::new`/reload, provider construction, then request client header construction.
- Localize failures at the boundary: verify the resolved auth mode and model before sending a request; then inspect the HTTP status and provider response. A 401 with no bearer key is an auth-selection issue, not a model-compatibility issue.
- Search all `CodexAuth::from_code_home`, `AuthManager::new`, `AuthManager::reload`, `OPENAI_*_ENV_VAR`, and `built_in_model_providers` call sites so the interactive CLI, exec CLI, and reload paths use the same resolved settings.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` — must pass with zero compiler warnings.
- **Static analysis / type check**: N/A — the required Rust build is the repository's prescribed compilation gate.
- **Unit tests**: run the focused tests added for environment resolution, auth precedence, and model precedence as part of the normal development loop; they must cover `chat` and `responses`.
- **Runtime verification**: run `code exec --max-seconds 30 --sandbox read-only "Reply with exactly: OK"` from the configured project. Confirm its startup summary shows the configured provider model and API-key auth, and confirm it receives `OK`.
- **Logic correctness**: manually walk CLI model override, profile model override, complete project `.env`, incomplete project `.env`, no project `.env`, persisted ChatGPT login plus project API key, and auth reload after a long-running session.

## Adjust — Rollback and Global Scan

- **Rollback plan**: remove the project-configuration resolver and restore the prior environment/auth precedence; deleting `.env` returns to persisted authentication and global defaults without migration.
- **Global scan**: inspect all binary entry points that call `arg0_dispatch_or_else`, as well as app-server and TUI auth-manager construction, to avoid a path that still selects ChatGPT credentials.
- **Backwards compatibility**: no existing config key changes. Existing users without these project `.env` settings retain their current auth and model resolution. Keep `CODEX_API_KEY` precedence unless intentionally superseded by an explicit CLI option.

## Open Questions

- Should a config profile explicitly selecting a model remain above the project `.env` model, as assumed here, or should a complete project `.env` always dominate it?
- Does the target provider support only Chat Completions, only Responses, or both? The final smoke test must use the selected supported protocol.
- Should non-OpenAI custom headers (for example, provider-specific API-key headers) be a future supported configuration surface? This plan assumes standard `Authorization: Bearer` authentication.

## Out of Scope

- Translating non-OpenAI protocols (Anthropic, Gemini, Ollama-native, or custom RPC) into OpenAI protocol requests.
- Model discovery, pricing, quotas, or automatic fallback among providers.
- Saving API secrets to tracked files or to `auth.json`.

---

> Next step: run `/pre-mortem docs/planning/openai-compatible-model-config.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Persisted ChatGPT credentials silently override the project API key

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Initial Risk Score**: 16.0 — HIGH
**Residual Risk Score**: 4.0 — not high

**Failure Scenario**: A user supplies a valid project-local `OPENAI_API_KEY`, but one auth-manager initialization or reload path reads `auth.json` first. Requests then carry a ChatGPT token or no compatible Authorization header, producing 401 responses or using the wrong account.

**Mitigation**:

- Introduce one resolved OpenAI-compatible configuration object and pass it to every auth-manager construction and reload path; do not independently re-read auth sources in those paths.
- Add integration tests that seed a ChatGPT `auth.json`, set the project API key, and assert API-key mode and the exact redacted auth-source diagnostic before a request is sent.
- Make a complete project OpenAI-compatible configuration fail startup if API-key mode cannot be constructed; it must never fall back silently to ChatGPT auth.

### [Risk] The configured model is ignored in favor of a global default

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Initial Risk Score**: 14.0 — HIGH
**Residual Risk Score**: 4.0 — not high

**Failure Scenario**: The request succeeds but is sent to a global model such as `gpt-5.6-terra`, rather than the provider model from `.env`; this can fail for unavailable model IDs or incur unintended cost.

**Mitigation**:

- Encode and test the precedence table as CLI `--model` > explicitly selected profile model > complete project OpenAI-compatible configuration > global config > built-in default.
- Mark the resolved project model as explicit and assert the startup summary's model value in runtime tests.
- Reject an empty or whitespace-only `OPENAI_MODEL` in a partially configured project environment instead of substituting a default.

### [Risk] Wire protocol or base URL mismatch only appears after deployment

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.4
**Initial Risk Score**: 7.2 — medium
**Residual Risk Score**: 3.2 — low

**Failure Scenario**: A provider supports only Chat Completions or requires a particular `/v1` path, while the client chooses Responses or constructs a duplicate/missing path; authentication appears correct but every request returns 404 or a schema error.

**Mitigation**:

- Parse and validate the base URL once, normalize only documented path rules, and reject invalid `OPENAI_WIRE_API` values before networking.
- Cover both `chat` and `responses` request construction with local HTTP test servers that assert path, payload shape, and Authorization header presence.
- Require the final live smoke test to use the user's selected wire API and accept a one-token response.

### [Risk] Debugging exposes an API key in logs or test output

**Severity**: 5 | **Likelihood**: 2 | **Detectability**: 0.5
**Initial Risk Score**: 5.0 — medium
**Residual Risk Score**: 1.0 — low

**Failure Scenario**: New startup diagnostics, failed-request dumps, or tests print the API key or full Authorization header while diagnosing the compatibility path.

**Mitigation**:

- Diagnostics may log only source, host, protocol, model, and auth mode; use existing redaction types for any credential-bearing structure.
- Add assertions that debug output and error formatting omit a known test key.
- Keep `.env` ignored and add only a keyless `.env.example` to version control.

### [Risk] Interactive, exec, and reload paths diverge

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.3
**Initial Risk Score**: 8.4 — medium
**Residual Risk Score**: 4.8 — not high

**Failure Scenario**: `code exec` works once, but the TUI or a long-lived session reload selects different credentials or resets the model because it bypasses the new resolver.

**Mitigation**:

- Make the resolved configuration the sole input to auth and provider setup, rather than duplicating precedence logic per frontend.
- Globally scan all `AuthManager` creation/reload and `CodexAuth::from_code_home` call sites; add one regression test per reachable startup mode.
- Exercise an auth reload in a test after the resolved configuration is established and assert that API-key mode and model are unchanged.

### [Risk] A provider requires nonstandard authentication or is not actually OpenAI compatible

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.6
**Initial Risk Score**: 3.6 — low
**Residual Risk Score**: 2.4 — low

**Failure Scenario**: A user supplies a vendor endpoint that needs a custom header or speaks a similarly named but incompatible protocol; supporting it implicitly creates unreliable behavior and unclear security expectations.

**Mitigation**:

- State at validation time that this path requires standard Bearer authentication and either OpenAI Chat Completions or Responses semantics.
- Return a precise error for unsupported headers/protocols and keep vendor-native adapters explicitly out of scope.
- Treat support for custom headers as a separate, reviewed configuration feature with its own secret-handling design.

### Convergence Review

Iteration 1 identified two HIGH risks: persisted-auth override and model-precedence override. The plan now requires a single resolved configuration object, fail-closed handling for complete project settings, an explicit precedence table, and end-to-end assertions before network use.

Iteration 2 reviewed technical, process, and external categories again. No new HIGH risks were found. All five recorded risks have mitigations, and every residual score is at most 4.8.

**Final HIGH RISK count: 0.**

---

## Post-Mortem: OpenAI-Compatible Model Configuration

**Date**: 2026-11-03

### Implementation Map

The startup dispatcher loads the project `.env`; configuration resolves the
provider and model; `AuthManager` selects credentials for initial use and
reload; `ModelClient` constructs the compatible request; streaming performs
the final model-family normalization. The same API-key resolver is now used
by rate-limit account-switch paths.

### Issues Fixed

| # | Severity | Type | Location |
|---|---|---|---|
| 1 | High | Authentication precedence | `core/src/auth.rs` |
| 2 | High | Configuration precedence | `core/src/config.rs` |
| 3 | Medium | Provider-isolation violation | `core/src/config.rs` |
| 4 | Medium | Incorrect compatibility warning | `core/src/codex/streaming.rs` |

1. A persisted ChatGPT login could win over `OPENAI_API_KEY`, omitting the
   compatible provider's Bearer credential. `AuthManager` creation and reload
   now use the shared process API-key resolver first.
2. A global model setting could win over `OPENAI_MODEL`. The OpenAI provider
   now resolves the project model before the global default, while preserving
   CLI and profile overrides.
3. `OPENAI_MODEL` could affect an explicitly selected non-OpenAI provider.
   It is now scoped to the built-in `openai` provider and has a regression test.
4. Unknown provider model IDs were treated as missing first-party OpenAI
   metadata. Third-party compatible endpoints now use their safe fallback
   family without emitting a misleading warning.

### Root Causes

1. API-key precedence was implemented only for `CODEX_API_KEY`; the standard
   OpenAI environment key was not included in the shared auth-manager path.
2. Model selection did not distinguish a project-local compatible-provider
   model from a global Codex default.
3. Initial model precedence was added without constraining it to its provider.
4. First-party metadata diagnostics were applied to custom OpenAI-compatible
   endpoints.

### Prevention Tasks

| Task | Type | Status |
|---|---|---|
| Regression test for persisted-auth and API-key precedence | AFK | Complete |
| Regression tests for OpenAI model precedence and provider isolation | AFK | Complete |
| Live read-only compatible-provider smoke test | AFK | Complete |

### Verification Evidence

- `./build-fast.sh` completed successfully after the final changes.
- Focused authentication and model-precedence tests passed.
- The configured `MiniMax-M3` model returned `OK` through the configured
  `chat` compatible endpoint with no fallback-metadata warning.

**Final HIGH RISK count: 0.**
