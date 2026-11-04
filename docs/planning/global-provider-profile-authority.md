# Plan: Make the Global Provider Profile Authoritative

> **User's request (restated)**: Analyze why qcodex configured through `qcodex config-start` does not reliably use one global provider configuration after changing projects, then write a repair execution plan.
>
> **What this plan answers**: Make the active profile saved by `qcodex config-start` the visible, authoritative default for every qcodex invocation, regardless of its current working directory.
>
> Goal: A profile selected in `qcodex config-start` supplies the endpoint, credential, wire API, and model for interactive and non-interactive qcodex sessions in every project until the user explicitly changes or deactivates it.

## Context

The reported 400 response rejects `include: ["codex-lite"]`. That parameter appears only on a Responses Lite request, so it is evidence that the effective wire/model configuration changed after qcodex was launched from a different directory; it is not a network reconnect failure.

The installation target installs the `qcodex` binary only. The binary's startup path currently reads the global `.code/.env` and then unconditionally imports `$(pwd)/.env`. Independently, `qcodex config-start` persists global provider profiles in `CODE_HOME/provider_profiles.json` and credentials in the credential store. `Config::load` resolves an active profile and is designed to let that profile own endpoint, model, wire API, and credential.

Read-only inspection on this machine found `qcodex config status --json` reporting `active_profile_id: null`, source `config-file`; `qcodex config list --json` found no stored profiles. Therefore there is currently no global profile for the running CLI to select. The current directory's `.env`, or defaults if it has none, is consequently determining transport behavior.

The product problem is not that the binary failed to copy a project `.env`: `config-start` is the intended global configuration mechanism. The problem is that the global-profile lifecycle and the legacy `.env` fallback jointly select providers without an unmistakable authority boundary or sufficient diagnostics.

## Goal

After a user saves and activates a provider in `qcodex config-start`:

- `qcodex config status --json` reports that profile, its redacted endpoint, model, wire API, and `provider-profile` source.
- Launching qcodex from two directories containing contradictory `.env` provider settings uses the same active profile.
- A provider without Responses Lite support is never sent a Responses Lite request merely because the working directory changed.
- No API key is copied from a project file to global storage without an explicit user action.

## Plan

1. Establish a deterministic reproduction with an isolated `CODE_HOME`, one active profile, and two temporary project directories whose `.env` files select different endpoint, model, and wire API values. Exercise interactive startup/config loading, `qcodex config status`, and the non-interactive/`exec` path. Record the effective provider source for each case without logging credentials.

2. Define and centralize the provider-authority contract: an explicitly active `config-start` profile owns endpoint, credential, model, and wire API; command-line provider/model overrides retain their documented conflict behavior; project `.env` is a legacy fallback only when no active profile exists. Make every configuration entry point use that same resolver and source enum rather than deriving provider fields independently from environment variables.

3. Harden startup loading in `code-rs/arg0` and configuration construction in `code-rs/core/src/config.rs` so importing `$(pwd)/.env` cannot silently alter an active profile's effective request settings. Preserve project `.env` compatibility when the global store has no active profile, and add an explicit opt-in mechanism for users who deliberately need a project-scoped override despite an active global profile.

4. Repair the `config-start` lifecycle: inspect save/activate callbacks, repository writes, keychain secret writes, and the configuration-center state handoff. On successful save, atomically persist the profile, make it active when requested, and present the active profile id plus the global storage location. On failure, keep the prior active profile and return a clear actionable error. Ensure a fresh process and a live TUI profile watcher observe the same persisted state.

5. Improve `qcodex config status` and startup diagnostics. Status must distinguish: no profile configured, profiles exist but none is active, active profile selected, an explicit override conflict, and legacy `.env` fallback. Add a redacted `[DEBUG-provider-resolution]` record at dotenv load, profile resolution, and request-client construction containing source, profile id/revision, base-URL host, wire API, and model; never emit a key, Authorization header, or full request payload.

6. Add a deliberate migration path from an existing compatible `.env` into `config-start`'s global profile store. It must require an explicit command/UI confirmation, show the redacted values that will be imported, write the key through `ProviderSecretStore`, and optionally activate the imported profile. Do not make `make install` copy `.env`: installation must remain binary-only and must not silently promote a repository secret to a user-global credential.

7. Update concise user documentation and install output: direct users to `qcodex config-start` (or equivalent CLI commands) for portable configuration, explain fallback behavior when no global profile is active, show how to inspect the effective source, and document the intentional project-override opt-in.

## Think — Debug Methodology

- Read in this order: `arg0::load_dotenv`, `find_code_home`, provider-profile repository/resolver, `Config::load`, client request-family selection, then the configuration-center save/activate handlers and TUI watcher.
- Reproduce in temporary directories with an isolated `CODE_HOME`; never use a real key or mutate the user's existing provider store while diagnosing.
- Add only redacted boundary logs under `[DEBUG-provider-resolution]`: current-directory dotenv presence and accepted keys by name, resolved `CODE_HOME`, active profile id/revision, resolution source, host, wire API, and model.
- Localize from upstream to downstream: prove which store is read and which source wins before inspecting the serialized Responses payload. The appearance of `codex-lite` is a downstream symptom of the selected request family.
- Search all provider construction paths for `OPENAI_BASE_URL`, `OPENAI_WIRE_API`, `OPENAI_MODEL`, `load_dotenv`, `ProviderProfileResolver`, `ProviderProfileRepository`, and `ProviderSecretStore`; interactive TUI, `exec`, config status, watcher reload, and one-shot flows must agree.

## Do — Verification Strategy

- **Focused regression development**: add tests first that fail under the old behavior:
  - active global profile plus contradictory project `.env` resolves to the profile;
  - no active profile retains supported legacy `.env` fallback;
  - save-and-activate through the config-center persistence path survives a fresh process;
  - inactive/missing profile states are distinguished by `config status`;
  - imported dotenv credentials are written only after explicit confirmation;
  - live profile watcher and non-interactive config loading resolve the same profile.
- **Build**: `./build-fast.sh` from the repository root — must pass with zero errors and warnings.
- **Runtime verification**: with an isolated test home, create and activate a Chat-wire profile, launch qcodex from two directories containing incompatible Responses `.env` values, and confirm the redacted diagnostic/status reports the same profile and `chat` wire API both times. Make a harmless request only against a local compatible-provider fixture; do not send credentials to a real endpoint for this check.
- **Logic correctness**: manually walk no profiles, saved-but-inactive profile, active profile, active profile plus project dotenv, explicit project override, CLI provider/model override, corrupted profile store, missing keychain secret, config-center save failure, runtime profile switch while idle, and switch during an active turn.

## Adjust — Rollback and Global Scan

- **Rollback plan**: gate the new authority behavior behind one resolver policy function. Reverting that function and the new explicit project-override option restores legacy dotenv fallback without deleting `provider_profiles.json` or keychain entries.
- **Global scan**: audit all interactive CLI, `exec`, one-shot, background watcher, configuration-center, and client-construction paths for direct environment reads or independently derived request families. Replace production duplicates with the shared resolution result.
- **Backwards compatibility**: existing users without an active profile continue to use their current global/project `.env` behavior. Users who need project-specific settings after activating a global profile receive one explicit opt-in rather than an implicit directory-dependent override.

## Open Questions

- Choose the explicit project-override interface: a per-invocation flag is the preferred default because it is visible in shell history; confirm whether a repository-local config switch is also required.
- Confirm whether `config-start` currently saves nowhere because the UI did not submit, because it writes under a different `CODE_HOME`, or because its persistence callback is failing. The isolated persistence test and current UI trace will settle this before implementation.
- Decide whether the dotenv import is a CLI command, a `config-start` onboarding action, or both. Both can share one import implementation; the UI should not duplicate secret handling.

## Out of Scope

- Automatically copying a repository `.env` or its API key during `make install`.
- Altering a third-party provider's Responses API compatibility or making it accept `codex-lite`.
- Removing all legacy `.env` support; it remains an intentional fallback for users without an active global profile.

---

> Next step: run `/pre-mortem docs/planning/global-provider-profile-authority.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Configuration center and runtime use different CODE_HOME directories

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 16.0

**Failure Scenario**: `config-start` saves a valid profile in one home directory while an installed `qcodex` process resolves another one because `CODE_HOME`, `CODEX_HOME`, legacy home discovery, a shell, and the GUI were not normalized consistently. The UI says the provider was saved, but every ordinary project still reports no active profile and falls back to local `.env`.

**Mitigation**:
- Define one canonical `CODE_HOME` resolution function and route configuration-center startup, CLI config commands, interactive startup, `exec`, watcher, repository, and secret-store construction through it.
- Display the resolved global configuration directory and active profile id in `config status` and after `config-start` save/activate; include the directory in redacted debug diagnostics.
- Add isolated-home tests that invoke the configuration center/CLI writer in one process and load interactive plus `exec` configuration in fresh processes, including `CODE_HOME`/legacy-home compatibility combinations.

**Residual Risk**: Severity 5 | Likelihood 1 | Detectability 0.9 | **Residual Score: 0.5**.

### [Risk] Config-start reports success without atomically persisting and activating a usable profile

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: A configuration-center save loses the profile update, stores it without `active_profile_id`, or writes profile metadata while its credential write fails. The user reasonably believes the global configuration is in force; a later invocation silently follows `.env` or defaults. The current empty profile store makes this an especially relevant failure mode to prove or disprove.

**Mitigation**:
- Make save/activate an explicit state transition with durable profile write, credential-store result handling, repository reopen verification, and a single success event only after all required operations succeed.
- Preserve the previous active profile on every failure; surface a specific UI/CLI error for failed persistence, failed activation, or unavailable credential storage instead of falling back silently.
- Add integration coverage for create+activate, update active profile, credential write failure, repository write failure, and a fresh-process `config status` assertion after each success/failure case.

**Residual Risk**: Severity 5 | Likelihood 1 | Detectability 0.85 | **Residual Score: 0.75**.

### [Risk] One request path bypasses the resolved global profile and reintroduces directory-dependent transport

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14.0

**Failure Scenario**: The interactive TUI honors the active profile but `code exec`, a one-shot call, model switching, a background watcher, or a newly added client constructor reads `OPENAI_*` directly. A profile that specifies `chat` can then become a Responses Lite request in only one path and reproduce the `codex-lite` rejection.

**Mitigation**:
- Introduce one resolved-provider value carrying source, profile revision, endpoint, wire API, model, and credential reference; prohibit production request construction from separately selecting these fields from environment variables.
- Scan and gate every direct read of `OPENAI_BASE_URL`, `OPENAI_WIRE_API`, `OPENAI_MODEL`, model-family derivation, and `ProviderProfileResolver` use; document justified test/catalog-only exceptions.
- Use a local fixture and contradictory `.env` directories to assert identical serialized transport settings in TUI, `exec`, one-shot, profile watcher reload, and runtime profile-switch paths when a profile is active.

**Residual Risk**: Severity 5 | Likelihood 1 | Detectability 0.5 | **Residual Score: 2.5**.

### [Risk] Dotenv migration leaks, overwrites, or activates the wrong credential

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.1
**Risk Score**: 13.5

**Failure Scenario**: An automatic installer/importer copies a repository's `OPENAI_API_KEY` into global storage, prints it in diagnostics, overwrites an existing secret reference, or activates a profile whose endpoint/model does not match that key. The attempted portability fix creates a credential disclosure or breaks another working global profile.

**Mitigation**:
- Do not copy `.env` in `make install`; make import an explicit CLI/UI confirmation flow that displays only redacted endpoint/model/wire data and never renders the key.
- Store keys only via `ProviderSecretStore`; require an explicit replace decision for an existing profile credential, use a new secret reference by default, and delete an orphaned newly written secret if profile persistence fails.
- Add tests that capture stdout/stderr and debug records for absence of raw keys, plus failure-atomicity tests for every write ordering and a test that import without confirmation makes no mutation.

**Residual Risk**: Severity 5 | Likelihood 1 | Detectability 0.85 | **Residual Score: 0.75**.

### [Risk] Legacy project-env users lose a required workflow during the authority change

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.4
**Risk Score**: 9.6

**Failure Scenario**: A team intentionally uses separate `.env` providers by repository. Making the global profile authoritative changes requests without an obvious escape hatch, leading users to disable the configuration center or maintain local forks.

**Mitigation**:
- Retain legacy dotenv behavior when no profile is active and provide one documented, explicit project-override mode when a profile is active.
- Make `config status` state the winning source and add compatibility tests for no-profile fallback and explicit project override.

**Residual Risk**: Severity 4 | Likelihood 1 | Detectability 0.85 | **Residual Score: 0.6**.

### [Risk] Provider switching leaves an active TUI turn on stale credentials or transport metadata

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.5
**Risk Score**: 6.0

**Failure Scenario**: The global profile updates while a session is streaming; some request state uses the old provider while the watcher publishes the new profile, producing mixed logs or retries against the wrong endpoint.

**Mitigation**:
- Apply profile revisions only at a terminal turn boundary and bind each started request to one immutable resolved-provider snapshot.
- Cover idle switch, active-turn deferred switch, failed reload, and restart with watcher tests; log old/new profile revision without credentials.

**Residual Risk**: Severity 4 | Likelihood 1 | Detectability 0.8 | **Residual Score: 0.8**.

### Pre-Mortem Iteration 2

The second sweep covered provider schema compatibility, installation/update behavior, stale binaries, corrupted stores, keychain availability, CLI/UI mismatch, concurrent profile writes, and third-party endpoint changes. No previously unrecorded HIGH risk remained: configuration-location split, false successful activation, source bypass, and credential migration account for the causal paths with a risk score above 12. The mitigations reduce every recorded residual score below 12.

---

## Post-Mortem: Global Provider Profile Authority

**Date**: 2026-11-04

The original symptom was a third-party provider rejecting `codex-lite` after
qcodex was launched from another project. The provider was not intermittently
disconnecting: no global profile was active, so the process fell back to the
working directory's `.env` and selected a different request family.

### Issues Fixed

| # | Severity | Type | Location |
|---|---|---|---|
| 1 | High | Authority violation | `arg0/src/lib.rs`, `core/src/provider_profiles/resolver.rs` |
| 2 | High | State-transition gap | `core/src/provider_profiles/repository.rs`, `core/src/provider_profiles/config_center/` |
| 3 | High | Credential consistency | `cli/src/config_cmd.rs`, `core/src/provider_profiles/config_center/` |
| 4 | High | Configuration injection | `arg0/src/lib.rs` |
| 5 | Medium | Observability gap | `cli/src/config_cmd.rs`, `docs/provider-configuration.md` |

1. An active global profile could coexist with unconditional project dotenv
   loading. Startup now suppresses project dotenv provider loading while a
   profile is active, and the resolver enforces the same decision. An explicit
   `QCODEX_PROJECT_PROVIDER_OVERRIDE=1` shell setting is the only project
   override path.
2. Creating a source in `config-start` could leave it unselected. New sources
   now default to activation and repository `add_and_activate` persists the
   profile plus active selection in one atomic document write. CLI `config add
   --activate` uses the same operation.
3. Replacing a credential could delete the old secret before profile metadata
   was safely updated. Replacement now writes a fresh opaque secret reference,
   updates profile metadata, and only then removes the old reference; failure
   removes the new reference instead.
4. A project `.env` could set the new override environment variable after
   startup's authority check, silently bypassing the active profile during
   configuration resolution. Project dotenv imports now reject all
   `QCODEX_*` variables; the override can originate only in the caller's shell
   or a global configuration context.
5. `config status` previously made no-profile, inactive-profile, and active
   profile situations hard to distinguish. It now exposes `profile_count` and
   `profile_state`; configuration documentation covers global authority,
   explicit override, deactivation, and one-time dotenv import.

### Verification Evidence

- Targeted regressions passed for global-profile dotenv suppression, atomic
  add-and-activate, configuration-center activation payload, dotenv import
  validation, project override resolution, and project dotenv rejection of
  `QCODEX_*` values.
- Provider-profile test sweep: 59 passing tests; CLI test sweep: 16 passing
  tests; arg0 test sweep: 5 passing tests.
- `./build-fast.sh` passed cleanly with final binary hash
  `2da67ef441eb4813c846494d1d1aa21229a4e024b5d9f5a7963d0303495225ff`.
- The final binary was installed as `~/.local/bin/qcodex`. A redacted status
  from another project directory reports the active MiniMax profile, `chat`,
  and source `provider-profile`; no project dotenv setting changed it.

### Root Causes

1. Provider selection had two independent entry points: global profiles and
   startup dotenv injection. The product did not establish one default owner.
2. The configuration-center create flow treated save and activation as
   separate user actions despite its role as the portable global setup flow.
3. Credential identity was coupled to profile id, which made an in-place key
   replacement impossible to roll back safely.
4. The dotenv allow-list protected `CODEX_*` variables but had no boundary for
   qcodex-specific process controls introduced later.

### Prevention Tasks

| Task | Type | Acceptance criterion |
|---|---|---|
| Keep provider authority tests at every startup/request entry point | AFK | A new direct `OPENAI_*` or resolver call without shared coverage fails review/test audit. |
| Keep fresh credential references for replacements | AFK | A failed metadata update leaves the prior secret reference loadable. |
| Document process controls separately from project dotenv settings | AFK | Every new `QCODEX_*` control is rejected from project dotenv and has a regression test. |

All four originally HIGH issues have fixes and regression coverage. A final
post-mortem sweep found no unmitigated residual score above 12: **HIGH risks
remaining: 0**.
