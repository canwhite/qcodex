# Plan: qcodex config-start provider configuration center

> **User's request (restated)**: 仿照 `claude-code-custom-github` 的 `ccc config-start`，为当前项目增加 `qcodex config-start`：打开配置页面，让用户配置 OpenAI-compatible provider；可保存多个源，并且切换源后当前 agent 的底层调用也切换到所选源。
>
> **What this plan answers**: Add a local configuration center and one authoritative runtime provider-selection path for multiple OpenAI-compatible sources.
>
> Goal: A user can securely create, edit, select, and remove multiple OpenAI-compatible provider profiles through `qcodex config-start`, and every new agent turn uses the selected profile's endpoint, model, protocol, and credential without restarting qcodex.

## Context

The Rust binary is installed as `qcodex` but retains the upstream `code` CLI
implementation. It already supports custom entries in `model_providers`, a
selected `model_provider`, model values, profiles, and Chat Completions or
Responses wire APIs. Project-local `OPENAI_*` compatibility support also
exists. These primitives are file-loaded at startup; there is no user-facing
multi-provider editor, no stable profile/secret lifecycle, and no complete
runtime handoff that prevents an already-running TUI from continuing to call
the old provider after a source switch.

This feature deliberately supports only OpenAI-compatible HTTP providers. A
profile is a provider endpoint plus a selected model and optional request
settings, not a promise to adapt native Anthropic, Gemini, Ollama-native, or
other protocols. The configuration center must coexist with direct
`config.toml`, CLI `--model`/`-c` overrides, environment-based configuration,
and ChatGPT authentication without silently combining credentials or routes.

## Goal

- `qcodex config-start` starts a loopback-only configuration page and opens it
  in the default browser when appropriate; `--no-browser`, `--json`, and
  `config-close` make the flow scriptable and recoverable.
- The page can manage multiple named OpenAI-compatible profiles with a base
  URL, wire API (`chat` or `responses`), model, API-key reference, and safe
  provider options already supported by `ModelProviderInfo`.
- One profile is active. A newly submitted user turn snapshots that active
  profile and all of its request/auth settings. A switch during a turn is
  queued and applies atomically after that turn reaches a terminal state;
  it never changes an in-flight model round or tool-result follow-up.
- Interactive TUI, `qcodex exec`, Auto Drive, app-server-created sessions,
  and any auth reload path resolve the same active profile. Explicit
  one-invocation CLI/profile overrides retain documented precedence.

## Plan

1. Specify the profile contract and precedence table before adding UI. Create
   a versioned qcodex profile record containing a stable id, display name,
   provider id, HTTPS/HTTP base URL, `chat|responses` wire API, selected model,
   supported non-secret provider fields, credential reference, and revision.
   Make active-profile resolution explicit: invocation overrides first;
   otherwise the active qcodex profile; then existing normal config/default
   behavior. Reject a profile that would mix an active provider with another
   provider's credential or model.
2. Add a core profile repository and validator under `code-rs/core`, with
   atomic revisioned read/write and optimistic-conflict errors. Store only
   non-secret profile metadata in qcodex-owned configuration; keep API keys in
   a platform credential store behind a profile-id reference (with a secure,
   permissions-restricted encrypted fallback if the platform store is
   unavailable). Never write a key to `config.toml`, profiles JSON/TOML, UI
   state, logs, CLI output, or test snapshots. Reuse the existing provider
   validation rules wherever possible rather than building a parallel schema.
3. Build a single resolver that turns the selected profile into the existing
   `ModelProviderInfo`, model selection, and API-key auth inputs. Route
   `Config` construction, `AuthManager` creation/reload, `ModelClient`,
   streaming/tool follow-up configuration, and rate-limit/account-selection
   code through it. Preserve project `.env` OpenAI-compatible behavior when
   no qcodex profile is active, and make conflicting explicit environment or
   CLI ownership visible as an actionable error rather than a partial switch.
4. Define runtime switching as a revisioned control message, not a mutable
   process environment update. Add a provider-profile watcher/controller
   owned by each long-lived session host. It observes repository revisions,
   resolves and validates the next profile off the request path, then swaps
   the immutable session configuration only at a turn boundary. It must emit
   applied/failed status to the TUI and app-server, invalidate provider/auth
   clients and relevant model metadata, and leave the old snapshot intact if
   resolving the replacement fails.
5. Wire the controller into every entry point. The interactive TUI must show
   the currently effective provider/model and refresh after an applied
   revision; `exec` and Auto Drive must load the active profile before a run
   and preserve one profile snapshot for its whole run; app-server sessions
   must receive the same revision contract. Audit subagents, planning/review
   model selection, and the provider-aware external-agent restrictions so a
   switch cannot retain old endpoint-dependent defaults or resurrect unrelated
   built-in agent routes.
6. Add the root CLI commands in `code-rs/cli/src/main.rs`: `config-start`,
   `config-close`, an internal server mode, and `config` subcommands for
   `list`, `show`, `add`, `update`, `remove`, `use`, and `status`. Keep the
   qcodex-facing command name in help and documentation while preserving the
   upstream-compatible `code` binary behavior. Accept secret input only via
   stdin or the authenticated local page; redact all command output. Define
   idempotent close/start behavior and a clear migration path for an obsolete
   server-state file.
7. Implement the configuration page as a separate loopback-only server bound
   to `127.0.0.1` on a random port. On `config-start`, first verify and close a
   prior qcodex-owned instance, launch a detached server, wait for a signed
   ready state, then open the browser only for an interactive TTY. Protect all
   mutating endpoints with an unguessable bearer token, same-origin checks,
   CSRF protection, no token in URLs/output, request-size limits, and an idle
   timeout. The page needs profile list/create/edit/delete/select actions,
   per-field validation, active/effective status, and human-readable failures;
   it must never return an API key (not even masked).
8. Make configuration writes preserve unrelated user settings. Either use a
   dedicated qcodex profile store or well-delimited generated sections with
   the existing TOML editor; do not rewrite arbitrary `config.toml` comments,
   custom providers, profiles, MCP servers, or agent definitions. If a
   profile is deleted while active, require an explicit replacement or clear
   the active selection and fall back predictably; do not silently select a
   different source.
9. Add focused coverage: profile validation and precedence; credential
   redaction/secret-store references; atomic persistence and revision
   conflicts; HTTP API authorization and origin/CSRF handling; startup/close
   lifecycle; and local mock-server assertions that profile A and B use their
   own URL, wire payload, Authorization header, and model. Add integration
   tests for switching while idle, while streaming, after failure/cancel, and
   across TUI, exec, Auto Drive, and app-server paths. Include a regression
   test that a stale client cannot make post-switch requests to the old source.
10. Document the supported OpenAI-compatible contract, setup and recovery
    commands, precedence rules, switch timing, secret handling, and direct
    `config.toml` interoperability in the qcodex docs. Include a secret-free
    example with two providers and describe how to diagnose failed validation
    without printing credentials.

## Think — Debug Methodology

- Add redacted boundary diagnostics with a consistent `[DEBUG-provider-profile]`
  prefix at profile load/validation, active-revision observation, pending
  switch scheduling, turn-boundary application, auth/client construction, and
  final request dispatch. Log profile id, revision, provider id, host, wire
  API, model, and auth mode only—never secrets or Authorization headers.
- Read the execution path in this order: root CLI dispatch; profile repository
  and resolver; `Config::load_*`; `AuthManager`; `ModelClient`; streaming
  request/tool-result loop; then the TUI/app-server session owner. Inspect
  source before external documentation when behavior differs.
- Localize a bad switch from upstream to downstream: prove the active revision
  changed, prove the session marked it pending/applied at the right boundary,
  prove the reconstructed config/provider/auth client changed, then inspect a
  mock server's request. A request to the old host after an applied revision is
  a stale-client lifecycle bug, not a UI bug.
- Globally search `Config::load_with_cli_overrides`,
  `load_from_base_config_with_overrides`, `AuthManager`, `ModelClient`,
  `model_provider_id`, `default_agent_configs`, and all config-file writers
  before declaring the routing work complete.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly
  with zero errors and warnings.
- **Static analysis / type check**: N/A — the required Rust build is the
  repository's prescribed compilation gate.
- **CLI/runtime verification**: use an isolated `CODEX_HOME` with two local
  mock OpenAI-compatible servers. Run `qcodex config-start --no-browser --json`,
  create/select each profile through the authenticated API or CLI, and confirm
  `qcodex config status --json` exposes only redacted effective state. Run
  `qcodex exec` once per profile and assert server-specific endpoint, protocol,
  model, and bearer credential reception.
- **Interactive verification**: keep a TUI turn streaming against profile A,
  select profile B, and prove every request in that turn remains at A; submit
  the next turn and prove it uses B. Repeat after cancellation and a failed
  switch, checking status/footer feedback and that no restart is needed.
- **Logic correctness**: manually walk no active profile, profile plus CLI
  override, direct `config.toml` custom provider, project `.env`, persisted
  ChatGPT login, missing/invalid credential, unsupported wire API, concurrent
  browser edits, server restart/idle timeout, active-profile deletion, and
  each supported long-lived frontend.

## Adjust — Rollback and Global Scan

- **Rollback plan**: disable profile resolution and watcher activation behind
  the new configuration center boundary; existing `config.toml`, environment,
  CLI, and authentication behavior then remains usable. `config-close` stops
  the loopback server; deleting only the dedicated profile metadata and secret
  references removes this feature without rewriting user configuration.
- **Global scan**: audit all process entry points and all paths that cache
  provider, model, authorization, model family, or agent capability state.
  Also scan docs, completions, package wrappers, desktop launchers, and the
  globally installed `qcodex` launcher so they expose the same commands and
  no path calls a stale `code`-only implementation.
- **Backwards compatibility**: direct `config.toml` providers, profiles,
  `.env` OpenAI-compatible configuration, ChatGPT login, `code` invocation,
  and per-invocation flags retain their current behavior when no qcodex
  profile is active. Do not auto-import or delete existing credentials; offer
  an explicit migration/import only after its semantics are tested.

## Open Questions

- Should qcodex profiles be user-global only, or should the page offer an
  explicit project-local profile store with a visible scope selector? This
  changes both secret ownership and the watcher scope; the initial plan assumes
  user-global profiles.
- Which secure credential-store implementation is acceptable on every
  supported platform, and what recovery/backup policy is required for its
  encrypted fallback?
- Is a profile switch intended to affect already-created side conversations
  and background Auto Drive work immediately at their next turn boundary, or
  only newly created sessions? This plan assumes every live session observes
  the active global revision at its own safe boundary.
- Which currently supported `ModelProviderInfo` advanced fields should appear
  in the first UI release versus remain editable only in `config.toml`?

## Out of Scope

- Adapting native non-OpenAI protocols or discovering a provider's models,
  pricing, quotas, or capabilities automatically.
- Syncing providers or credentials to a cloud account, sharing secrets across
  users, or storing plaintext API keys in project files.
- Replacing the existing general `config.toml` editor, MCP/agent management,
  or manually configured profiles.

---

> Next step: run `/pre-mortem docs/planning/qcodex-config-start.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] The feature is built against a 5-week-stale upstream and gets rewritten on the next merge

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15.0 — HIGH

**Failure Scenario**: The last upstream sync landed 2026-09-30; `codex-rs/` now
differs from `code-rs/` in 3501 files, and upstream has already restructured
the exact subsystems this plan touches. `codex-rs/core/src/config.rs` became
`codex-rs/core/src/config/` (a directory with `mod.rs`, `edit/`, and a new
`auth_keyring.rs`), and `codex-rs/core/src/model_provider_info.rs` no longer
exists in that form. Upstream also added a `codex-login` crate with a
`KeyringStore` trait (`load/save/delete` keyed by `service` + `account`),
`AuthKeyringBackendKind::{Direct, Secrets}`, and an encrypted-secrets-file
backend — i.e. most of the credential store this plan proposes to build.
The plan therefore invents a parallel profile/keyring/watcher stack in
`code-rs/` that either conflicts on every upstream merge or is silently
deleted by the next "Refresh codex-rs mirror" pass. Either way the feature
ships late or is rewritten.

**Mitigation**:

- Before writing code, diff the specific subsystems this plan depends on
  against the upstream mirror: `core/src/config*`, `core/src/model_provider_info*`,
  `core/src/auth*`, `core/src/client.rs`, `cli/src/main.rs`, and the `login`
  crate. Use `scripts/upstream-merge/diff-crates.sh` to keep the comparison
  scoped to the crates that actually change.
- Treat the upstream `login`/`keyring-store` crate as the reference design
  for the credential store. Port or reuse its `KeyringStore` abstraction
  instead of introducing a new keyring trait, and prefer backporting its
  `AuthKeyringBackendKind::{Direct, Secrets}` behavior over writing a
  second encrypted fallback.
- Keep the new code in small, upstream-shaped seams (one resolver module, one
  repository module, one CLI subcommand module) so a future merge can
  replay it; avoid scattering qcodex-only conditionals through
  `config.rs`, `client.rs`, and `streaming.rs`.
- Add the new crates and modules to the upstream-diff watch list and re-run
  the diff before each upstream merge; resolve upstream conflicts in favor of
  upstream semantics and re-layer qcodex behavior on top.

### [Risk] The plan assumes a "secret reference" credential path that the provider auth code does not support

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.35
**Risk Score**: 13.0 — HIGH

**Failure Scenario**: The plan's step 2 and step 3 require storing a key in a
credential store behind a profile-id reference and resolving it into a
`ModelProviderInfo`. But `ModelProviderInfo` has no field for a secret
reference. Its only three credential paths, in `effective_auth`
(`code-rs/core/src/model_provider_info.rs`), are: `experimental_bearer_token`
(a plaintext token in config), `auth` (an external command), and `env_key`
(a process environment variable). `ModelProviderInfo::api_key()` reads
`std::env::var` and errors when the variable is absent. `AuthManager` is
keyed to a single ChatGPT/API-key account, not to a provider id, and
`ModelClient::new` binds `provider` at construction. A profile-id secret
reference therefore has no existing consumer: implementing it means
either mutating the process environment (which the plan explicitly forbids
and which is not thread-safe or per-session correct), inlining the token as
`experimental_bearer_token` (which writes the secret to `config.toml`, exactly
what the plan forbids), or adding a new field and a new branch to
`effective_auth`. If the implementer takes the shortest path, the
"never write a key to config.toml" guarantee silently breaks.

**Mitigation**:

- Decide explicitly, before UI work, which of the three existing credential
  paths the profile secret uses. The recommended path is to add one
  narrowly-scoped field to `ModelProviderInfo` (for example a resolved
  secret reference) plus one branch in `effective_auth` that resolves it
  through the chosen credential store, and to reject any profile whose
  secret cannot be represented in that form.
- If `experimental_bearer_token` is used as an interim carrier, it must be
  set only on an in-memory `ModelProviderInfo` that is never serialized back
  to `config.toml`; add a test that asserts no `ModelProviderInfo` written
  to disk contains a non-empty `experimental_bearer_token`.
- Never rely on `env_key` for a profile secret: it requires a real process
  environment variable, which cannot represent two simultaneously-selected
  profiles and leaks the secret into the child-process environment.
- Prove the round trip with a test that stores a profile secret, resolves the
  provider, asserts the outgoing `Authorization` header matches, and asserts
  that neither `config.toml` nor the profile metadata file contains the key.

### [Risk] The qcodex "profile" concept collides with the existing config.toml profile mechanism

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 11.2 — medium

**Failure Scenario**: `config.toml` already has a `profile` selection key
(`Config::profile`) and a `[profiles.*]` table whose `ConfigProfile` already
carries `model`, `model_provider`, `planning_model`, `review_model`, and
`reasoning_effort` — that is, an existing "provider + model bundle" concept
that is already wired into `Config::load_*` and surfaced as
`Config::active_profile`. The plan introduces a second, differently-shaped
`qcodex profile` with its own id, revision, wire API, and credential
reference. A user reading `config.toml` sees `[profiles.dev]` and a
`qcodex config list` output that both claim to be "profiles". Worse, the
existing mechanism can select a `model_provider` that points at a
provider id whose credentials belong to a different qcodex profile, which
is precisely the "mixed credential" case the plan says it must reject — but
the rejection logic lives in a different layer and will not see it.

**Mitigation**:

- Give the new concept a distinct name in code, CLI, and UI (`provider source`
  or `connection` rather than `profile`) and state the difference
  explicitly in user-facing docs and in `config-start` help text.
- Make the precedence resolver handle both layers: the existing
  `config.toml` `[profiles.*]` selection and the new qcodex source
  selection must be resolved by one function, with the qcodex source
  supplying provider credentials and the config profile supplying model /
  reasoning settings, or an explicit conflict error when both claim
  `model_provider`.
- Add a validation test for the mixed case: a config profile that names a
  `model_provider` different from the active qcodex source must produce a
  `configuration-conflict` error, not a silent merge.
- Document the mapping in the precedence matrix, including which layer owns
  the wire API (`chat` vs `responses`) and which layer owns the model id.

### [Risk] A live provider switch requires rebuilding the whole session, not swapping a config field

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 11.2 — medium

**Failure Scenario**: The plan's step 4 assumes the watcher can "swap the
immutable session configuration only at a turn boundary". In this codebase
that operation is not a field assignment. `Session` owns
`client: ModelClient` (a plain, non-`Arc`, non-mutable field) constructed
once in `ModelClient::new`, and `ModelClient` binds `provider` and derives
effective reasoning effort/verbosity from it at construction time. The only
existing mid-session reconfiguration path is `Op::ConfigureSession`, and its
handler does not mutate the session in place: it clones the config, rebuilds
the `ModelClient`, re-creates the rollout recorder, rebuilds `ToolsConfig`,
and re-establishes MCP connections — aborting and replacing the old session
state. If the implementer follows the plan literally and mutates
`Session.client` in place, the client, the rollout recorder, the tool
configuration, and the MCP connections will disagree about which provider
and model are active, and a rollout resumed later will replay against the
wrong provider.

**Mitigation**:

- Pick one concrete mechanism and state it in the plan before implementing:
  either (a) extend `Op::ConfigureSession` with the fields the switch needs
  and reuse its full rebuild path, accepting that a switch restarts the
  session machinery; or (b) add a dedicated, narrowly-scoped provider-swap
  path that replaces `Session.client` *and* the model-family/tool metadata
  that depend on the provider, and document exactly which session state is
  preserved.
- Prefer (a) for the first vertical slice. Reusing the existing
  reconfiguration path avoids inventing a second, less-tested lifecycle and
  keeps the rollout recorder and MCP state consistent with the new provider.
- Do not treat `Op::ConfigureSession` as transparent: verify whether it
  aborts an in-flight turn and whether it preserves conversation history. If
  it aborts the turn, the "queued and applies after the turn reaches a
  terminal state" requirement needs an explicit queue in front of it.
- Add a test that switches provider mid-session and asserts: the next turn
  reaches the new host, the rollout recorder writes under the new provider,
  the tool configuration matches the new model's capabilities, and the
  conversation history is preserved.

### [Risk] The detached browser server becomes a persistent, always-on process with no real ownership

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 7.2 — medium

**Failure Scenario**: `config-start` launches a detached loopback server that
survives the terminal that started it. The plan gives it an idle timeout and
a `config-close` command, but the server holds the bearer token, the
credential-store handle, and a live view of the profile repository. If the
user forgets to close it, the server keeps running across logout, across
credential rotation, and across a laptop sleep; a later process that reads
the stale state file may attach to it, or the port may be reused by an
unrelated process. On a shared or CI machine, a stray server is a standing
credential-adjacent listener.

**Mitigation**:

- Bind the server lifecycle to an explicit owner: write a state file that
  records the owning pid, the started-at timestamp, and a boot-stable
  instance id, and refuse to attach to a server whose pid no longer exists.
- Make the idle timeout short by default and refresh it on authenticated
  activity, so an abandoned server self-terminates within minutes.
- Verify the pid is alive and is actually our binary before signalling or
  attaching, to avoid signalling a recycled pid.
- Add a startup check that a stale or foreign state file is discarded with a
  clear message rather than reused, and make `config-start` idempotent
  against a healthy existing instance.
- Document the process in the user-facing docs: where it runs, how to stop
  it, and what it holds.

### [Risk] Switching to a provider whose model is not in the local catalog silently degrades turn behavior

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.4
**Risk Score**: 9.6 — medium

**Failure Scenario**: Model metadata in this codebase is resolved by model
*name*, not by provider. `Config::load_*` calls
`find_family_for_model(&model)` and falls back to
`derive_default_model_family(&model)` when the name is unknown. The fallback
sets `context_window: None`, `max_output_tokens: None`,
`auto_compact_token_limit: None`,
`truncation_policy: TruncationPolicy::Bytes(10_000)`,
`supports_reasoning_summaries: false`,
`supports_parallel_tool_calls: false`, and
`supports_search_tool: false`. A third-party provider's model name
(for example `MiniMax-M2` or `gpt-4o-mini` variants) is unlikely to be in the
embedded OpenAI catalog, so the profile looks correctly configured and the
connection test passes, yet every turn runs with a 10 KB truncation policy
and a context window the code cannot infer. The symptom is not a crash; it
is that long tool outputs get cut off mid-JSON and the agent behaves as if
it were losing context.

**Mitigation**:

- Treat unknown-model metadata as a first-class condition in the profile
  validation and in the connection test: report "model metadata unknown —
  context window and truncation limits fall back to defaults" as an explicit
  warning, not a silent default.
- Resolve provider-owned model metadata the way upstream does, via the
  `RemoteModelsManager` / provider-owned `ModelInfo` path, and make the
  config-center profile carry the resolved metadata or a pointer to it.
- When the metadata is unknown, require the profile to declare the missing
  values (context window, max output tokens) explicitly, and persist them in
  the profile record so a turn is not silently downgraded.
- Add a regression test for a profile pointing at a mock provider with an
  unknown model name, asserting the effective truncation policy and context
  window are the intended ones rather than the 10 KB default.
- Include the resolved model metadata in the redacted `config status` output
  so a user can see what the agent will actually use.

### [Risk] The qcodex command name is a symlink artifact, so "qcodex config-start" is not actually reachable as designed

**Severity**: 3 | **Likelihood**: 4 | **Detectability**: 0.5
**Risk Score**: 6.0 — medium

**Failure Scenario**: The plan's step 6 says to "keep the qcodex-facing
command name in help and documentation while preserving the upstream-compatible
`code` binary behavior". In this repository the `qcodex` name exists only as
a filesystem symlink (`code-rs/bin/qcodex -> code`) over a binary whose real
cargo package name and help text are `code`. Running `qcodex --help` today
prints `Usage: code [OPTIONS] [PROMPT]` — the name in the usage string is
hardcoded, not derived from argv0. If the implementation adds the
`config-start` subcommand under the `code` name and only mentions `qcodex` in
prose, users will copy `qcodex config-start` from the docs, hit a help screen
that says `code`, and the configuration center will be documented under a
command name that the binary itself never prints. Worse, the mirror refresh
and `scripts/upstream-merge/branding_fix.py` exist precisely because upstream
renames and brand strings drift; a new command surface will be reverted or
flagged by the branding guard.

**Mitigation**:

- Decide the command surface explicitly before implementing: either ship
  `config-start` under the upstream `code` name and document it as
  `code config-start` (with `qcodex` as an alias in prose only), or make the
  help text argv0-aware so the same binary prints the invoked name.
- If argv0-aware help is chosen, check every brand-sensitive string
  (usage lines, error messages, completion scripts, `--help` epilog) and
  verify the existing `scripts/upstream-merge/branding_fix.py` still passes.
- Register shell completions for the new subcommand so
  `qcodex config-<TAB>` resolves, and confirm the completion script is
  generated from the same command tree rather than a hand-maintained list.
- Add the new command names to the branding fix allowlist if the guard
  rewrites them, so an upstream merge does not silently rename the
  configuration center.

### [Risk] The feature is delivered but never adopted because the entry point is undiscoverable

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.45
**Risk Score**: 4.95 — medium

**Failure Scenario**: The configuration center is a separate browser page
opened by a CLI command, while the TUI is the primary surface most users
live in. If the TUI shows the effective provider only in a status area the
user does not read, and the page is not reachable from the TUI, then a user
who wants to change provider has to leave the TUI, open a terminal, remember
the command, and wait for a browser. Discoverability then becomes the
binding constraint on adoption: the feature works but stays unused, and the
plan's success criteria are never met.

**Mitigation**:

- Surface the effective provider and model in the TUI's most visible
  status area, and add a TUI action that opens the configuration center (or
  at minimum shows the exact command to run).
- Make the configuration center reachable from the TUI's slash-command
  completion so it does not depend on the user remembering a terminal
  command.
- Decide the primary success metric before implementation — for example
  "a user can switch provider and see the next turn use it without leaving
  the TUI" — and verify it in the interactive verification step rather than
  only in the CLI verification step.
- Keep the first vertical slice small enough that the browser page can be
  deferred; a CLI-only `qcodex config use <name>` plus a TUI indicator may
  deliver most of the value with far less surface area.

### [Risk] The new secret store coexists with the existing plaintext account store, so the "keys are never on disk" promise is false

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.45
**Risk Score**: 8.8 — medium

**Failure Scenario**: The plan repeatedly promises that a key is never
written to `config.toml`, profile metadata, logs, or CLI output, and routes
profile secrets through a credential store. This repository already has a
separate, older mechanism: `code-rs/core/src/auth_accounts.rs` persists
accounts to `auth_accounts.json` under the code home, and
`upsert_api_key_account` writes the API key directly into that file as
`openai_api_key: Some(api_key)` — plaintext, with an `active_account_id`
already implementing multi-credential selection. If the configuration center
also writes through `AuthManager`/`auth_accounts` (the natural thing to do,
since that is the existing "multiple credentials" path), the key lands in
plaintext regardless of the new store, and the security guarantee in the
plan is quietly false. A user who reads the plan believes their key is in
the OS keyring while it sits in `~/.code/auth_accounts.json`.

**Mitigation**:

- Decide, before implementation, whether a qcodex profile secret is stored
  by the new credential store or by `auth_accounts`, and state which one
  owns the plaintext-vs-encrypted behavior in the plan and in user-facing
  docs. Do not leave it implicit.
- If profiles use the new store, ensure the resolver never routes a profile
  secret through `auth_accounts`, and add a test asserting
  `auth_accounts.json` does not contain the profile key after a profile
  switch.
- If profiles reuse `auth_accounts`, either extend that file to hold a
  keyring reference instead of the literal key, or state plainly in the
  plan and in `config status` that profile keys are stored in plaintext
  under the code home — and treat closing that gap as in scope, because the
  configuration center makes multi-key storage a first-class feature rather
  than an incidental one.
- Audit the redaction guarantees against both stores: the plan's
  "never log secrets" rule must cover `auth_accounts.json`, the new
  profile metadata file, and any debug dump of the account list.

### [Risk] The wire API contract omits the websocket variant, so a valid provider configuration is rejected or silently downgraded

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 5.4 — medium

**Failure Scenario**: The plan defines the profile wire API as `chat` or
`responses` in two places. The actual `WireApi` enum in
`code-rs/core/src/model_provider_info.rs` has three variants: `Chat`,
`Responses`, and `ResponsesWebsocket` (serialized as
`"responses_websocket"`). The websocket variant is not a cosmetic
difference: it is gated by a separate `supports_websockets` provider flag,
and `ModelClient::active_ws_version_for_prompt` selects the transport
per prompt. If the profile schema validates only `chat|responses`, a user
who already has a `responses_websocket` provider in `config.toml` cannot
represent it as a profile and gets a validation error; if the schema accepts
the stored value but the resolver always constructs a `Chat` or `Responses`
provider, the websocket transport is silently dropped and the session falls
back to HTTP streaming — which changes latency and can break providers that
require the websocket transport.

**Mitigation**:

- Define the profile wire API against the existing `WireApi` enum rather
  than a two-value string, so the profile schema cannot drift from the
  provider contract. If the first release intentionally excludes the
  websocket variant, encode that as an explicit validation error with a
  message that names the supported values, not as an absent case.
- When the websocket variant is supported, carry `supports_websockets` in
  the profile record alongside the wire API, and assert in a test that
  switching to a websocket profile selects the websocket transport.
- Add a validation test for a `responses_websocket` value arriving from an
  existing `config.toml` provider, asserting the migration path is either a
  clean import or a clear, actionable error.

### [Risk] A completed switch still sends requests to the old provider

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 16.0 — HIGH

**Failure Scenario**: The UI marks profile B active while a cached
`ModelClient`, auth manager, tool-result follow-up, side conversation, or
background run still owns profile A. The next request reaches A, potentially
using the wrong model and credential and creating an expensive or sensitive
cross-provider data leak.

**Mitigation**:

- Make the resolved profile revision part of every immutable turn snapshot and
  require every client/auth construction to be derived from that snapshot;
  prohibit process-global mutable provider state.
- At the turn boundary, drain or terminally account for all A-owned request
  work before publishing B as applied. Invalidate client, auth, model-family,
  and tool configuration caches as one transaction.
- Use two local mock providers in integration tests and assert host, model,
  wire payload, credential fingerprint, and revision for initial requests and
  every tool-result continuation. Treat a post-application A request as a
  hard regression.

### [Risk] The loopback configuration page lets another local page alter profiles or expose credentials

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75 — HIGH

**Failure Scenario**: A malicious local process or web page guesses/reuses a
configuration-server capability, bypasses origin checks, exploits a permissive
CORS policy, or reads a token from a URL/log. It replaces the active endpoint
or submits an attacker-controlled credential; a later agent turn then sends
workspace context to that endpoint.

**Mitigation**:

- Bind exclusively to `127.0.0.1`, generate a cryptographically random
  per-instance bearer token and CSRF token, hold them in server state rather
  than URLs/state files rendered to users, and reject absent/invalid Origin,
  Host, authorization, or CSRF headers on every mutation.
- Send restrictive CSP, no-store, frame-ancestors, and no-CORS headers; impose
  JSON/form size limits and strict schema validation. Never log request bodies
  or secret-bearing headers.
- Add adversarial HTTP tests for cross-origin POSTs, missing/replayed tokens,
  malformed Host/Origin values, oversized input, token absence from startup
  output/state files, and a second local process attempting to impersonate a
  stale server instance.

### [Risk] Profile metadata or fallback credential storage loses or leaks API keys

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.3
**Risk Score**: 10.5 — medium

**Failure Scenario**: Keychain access is unavailable in a packaged/sandboxed
environment, and a fallback either stores a recoverable plaintext key, uses
unsafe permissions, loses the key on upgrade, or maps the wrong secret to a
renamed/deleted profile. The feature appears to work in development but fails
or exposes credentials in a real installation.

**Mitigation**:

- Select and document one audited secret-store abstraction before UI work;
  use stable opaque profile ids as lookup keys, authenticated encryption with
  a keychain-protected key for fallback data, and owner-only permissions.
- Fail closed with a precise setup error when no secure backend is available;
  never degrade to plaintext persistence or environmental mutation.
- Test keychain-unavailable, restart, rename, delete, failed write, corrupted
  metadata, and package-sandbox cases. Verify backups/logs/profile exports
  contain only references and `has_key` booleans.

### [Risk] Provider precedence becomes ambiguous and configurations silently fight

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8 — HIGH

**Failure Scenario**: A user activates a qcodex profile while a CLI override,
`--profile`, project `.env`, custom `config.toml` provider, or persisted
ChatGPT auth remains present. Different entry points choose different winners;
the status page says B but exec/TUI/app-server sends to another endpoint.

**Mitigation**:

- Publish one exhaustive precedence matrix in code and docs, implemented by a
  single resolver shared by all entry points. Return a structured
  `configuration-conflict` error for unsupported combinations instead of
  merging fields from different sources.
- Include the effective source and profile revision in redacted status/debug
  diagnostics, and make the UI show when a profile is inactive because an
  invocation override owns routing.
- Add table-driven tests covering every pairwise and key three-way conflict
  between qcodex profiles, CLI flags, config profiles, `.env`, direct provider
  config, and persisted authentication for TUI, exec, Auto Drive, and
  app-server startup.

### [Risk] Background or resumed work crosses a provider boundary mid-task

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75 — HIGH

**Failure Scenario**: Auto Drive, subagents, review/planning calls, resumed
sessions, or tool continuations use a different lifecycle from the foreground
turn. A profile switch makes only some of their calls move to B, splitting one
task between providers and potentially leaking context or invalidating model
assumptions.

**Mitigation**:

- Define ownership explicitly: each independently executing task captures one
  profile revision at start and retains it through completion; only a newly
  scheduled task observes the new revision. Surface pending/applied state per
  task where users can act on it.
- Route all primary, planning, review, subagent, resume, and Auto Drive
  constructors through the same snapshot factory. Audit retries and deferred
  callbacks, not just initial request creation.
- Add lifecycle tests that switch profiles during each task type and assert no
  task makes mixed-provider calls; test cancellation/failure releases its old
  snapshot before the following task begins.

### [Risk] Concurrent browser/CLI instances corrupt profile state or apply a deleted profile

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 7.2 — medium

**Failure Scenario**: Two configuration pages or a script and page save edits
at once. A last writer overwrites an endpoint/key reference, a stale `use`
activates a removed id, or a partial state file survives a crash. Existing
agents then cannot resolve configuration at their next boundary.

**Mitigation**:

- Use atomic temp-file-and-rename persistence, a monotonic revision, and
  compare-and-swap on every mutation. Reject stale revisions with a response
  that asks the caller to reload rather than attempting a merge.
- Validate referential integrity of active profile and secret reference during
  every read; retain the last known-good snapshot for a live session if a new
  repository state is invalid.
- Stress test concurrent add/update/use/delete operations and interrupted
  writes; test recovery from missing, malformed, partial, and obsolete state
  files.

### [Risk] A large configuration-center feature drifts into a parallel provider system

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.35
**Risk Score**: 10.4 — medium

**Failure Scenario**: The browser server, key store, profile format, runtime
watcher, and multiple frontends are implemented independently. They duplicate
existing provider validation and auth behavior, diverge from upstream updates,
and leave an unmaintainable qcodex-only fork before a reliable minimum slice
ever ships.

**Mitigation**:

- Deliver vertical slices: repository/secret contract plus CLI first; one
  resolver and `exec` verification second; TUI safe-boundary switching third;
  browser UI and remaining frontends after those contracts are stable.
- Reuse `ModelProviderInfo`, existing config editing/loading, and auth types;
  require an architectural review for every new persistence or routing type.
- Keep browser UI a thin client over tested CLI/core APIs and record explicit
  non-goals; defer advanced provider fields, migration, and project scopes
  until the minimal multi-profile path is proven.

### [Risk] Provider protocol differences make a selected profile appear healthy but fail under tools or streaming

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.35
**Risk Score**: 7.8 — medium

**Failure Scenario**: A provider accepts a basic completion but mishandles
tool schemas, streaming, Responses vs. Chat payloads, model reasoning fields,
or model-specific features. The page reports a successful profile while an
ordinary agent turn repeatedly fails after switching.

**Mitigation**:

- Validate only stable transport fields in the UI and label capability checks
  as optional; do not falsely infer full compatibility from a TCP/HTTP probe.
- Offer an explicit read-only connection test that uses the selected wire API
  and reports redacted protocol/status diagnostics without persisting a
  partially valid profile as active.
- Cover Chat and Responses request paths, streaming, and a tool-call round
  with mock servers; preserve existing provider-aware tool/agent feature gates
  for third-party compatible endpoints.

### Convergence Review

Iteration 1 identified four HIGH risks: stale provider/client state after a
switch, local configuration-center takeover, ambiguous precedence, and mixed
provider background work. The plan now requires immutable revisioned snapshots,
transactional cache replacement, authenticated loopback APIs, a shared
precedence resolver, and lifecycle coverage for every task owner.

Iteration 2 reviewed persistence, operational, scope, and protocol risks. It
added mitigated medium risks for secret-backend failure, concurrent writes,
feature drift, and partial provider compatibility. No further unrecorded HIGH
risks were found.

Iteration 3 re-examined the plan against the actual source and found two
unrecorded HIGH risks that the plan's assumptions contradicted. The upstream
mirror has diverged from `code-rs/` by 3501 files with the last sync on
2026-09-30, and upstream has already built the credential-store and
config-module substrate this plan intends to create, so the feature risks
being rewritten on the next merge. Separately, `ModelProviderInfo` has no
field for a secret reference: its only credential paths are
`experimental_bearer_token`, an external command, and an environment
variable, so the plan's "secret reference" storage has no existing consumer
and the shortest implementation path leaks the key into `config.toml`. The
plan must resolve the upstream divergence and the credential-carrier question
before UI work begins.

Iteration 4 reviewed the session lifecycle, process ownership, model
metadata, and branding surfaces, and added six mitigated medium risks:
session rebuild semantics, the detached server's ownership, unknown-model
metadata degradation, the `qcodex` argv0 naming gap, discoverability, the
plaintext `auth_accounts.json` store, and the omitted `responses_websocket`
wire API. No further unrecorded HIGH risks were found.

**Final HIGH RISK count: 6 (4 original + 2 added in iteration 3). All 18
recorded risks have mitigations.**

---

## Post-Mortem

The feature was implemented and audited against the source. Every risk
recorded above was re-examined against the shipped code, and every defect
found during the audit was fixed in place.

### HIGH risk disposition

| Risk | Score | Disposition |
|---|---|---|
| A completed switch still sends requests to the old provider | 16.0 | **Resolved.** A switch is applied through `Op::ConfigureSession`, which rebuilds the session and its `ModelClient`. The TUI polls the store every two seconds, queues a change that arrives mid-turn, and applies it before the next turn starts. The integration test `each_profile_reaches_its_own_endpoint_with_its_own_credential` asserts that the previous provider receives no request after the switch. |
| The loopback configuration page lets another local page alter profiles | 12.75 | **Resolved.** The server binds to `127.0.0.1` on a random port. Mutating requests need a bearer token or an HttpOnly session cookie plus a matching CSRF token and an exact `Origin`/`Host`. Responses carry `no-store`, a restrictive `Content-Security-Policy`, `X-Frame-Options: DENY`, and `nosniff`. The bearer token never enters the page or stdout. |
| Provider precedence becomes ambiguous and configurations silently fight | 12.8 | **Resolved.** One resolver serves every entry point. A differing invocation override, `config.toml` provider selection, or `config.toml` model produces a `configuration-conflict` error rather than a merged configuration. Table-driven tests cover each combination. |
| Background or resumed work crosses a provider boundary mid-task | 12.75 | **Resolved.** Each task captures one profile revision for its whole run. `exec` and Auto Drive load the active profile when the run starts; Auto Drive also keeps the profile's model instead of replacing it with the built-in Auto Drive slug. |
| The feature is built against a 5-week-stale upstream | 15.0 | **Mitigated.** The implementation extends the existing `ModelProviderInfo` contract with one optional field and reuses the upstream `KeyringStore` shape rather than introducing a parallel credential or profile type. New code is confined to one `provider_profiles` module, one CLI module, and one TUI module so an upstream merge can replay it. The upstream diff still needs to be reviewed before the next merge. |
| The plan assumes a "secret reference" credential path that does not exist | 13.0 | **Resolved.** `ModelProviderInfo` gained one optional `secret_ref` field and `effective_auth` gained one branch that resolves it through the credential store. The key never reaches `config.toml`: the profile document holds only a reference. The test `the_stored_profile_document_never_contains_a_secret` and the end-to-end check both confirm no plaintext on disk. |

**Remaining HIGH risk count: 0.**

### Audit findings fixed during the post-mortem

These were not in the pre-mortem list. Each was found by auditing the
implementation and fixed in the same change.

- **The credential store blocked on an authorization prompt.** Writing a new
  platform-credential-store entry waits for user authorization, which hangs
  in a headless environment. The default backend is now an encrypted file
  under the code home, with the platform store available behind
  `CODEX_PROVIDER_SECRETS=keyring`.
- **The configuration server stopped while in use.** The idle timer was
  initialized at startup and never refreshed, so an actively used page
  vanished after the idle timeout. Every request now refreshes it.
- **Replacing a key left the old one behind.** Updating a profile stored the
  new secret without deleting the previous one, so a rotated key stayed in
  the credential store. The old secret is now removed first.
- **A failed create left an orphan secret.** The secret was stored before the
  profile was written, so a rejected write left a key with no profile. The
  secret is now removed when the write fails.
- **Concurrent writes silently overwrote each other.** The configuration
  page wrote without a revision check. Writes now pass the observed
  document revision and a conflicting write is rejected with `409`.
- **A secret could be resolved against the wrong code home.** The
  credential store was a single process-wide registration, so a process
  serving two code homes could read the wrong key. One store is now kept
  per code home, and a secret reference names the code home it belongs to.
- **The profile polling task never stopped.** Each new session started a
  background poller that was never aborted. The task is now tracked and
  aborted when the session ends, is replaced, or the UI exits.
- **A switch could interrupt a turn that had just started.** The profile
  change is now applied at the start of the next turn, before the turn is
  submitted, so no in-flight turn is interrupted.

### Verification

- `./build-fast.sh` passes with zero errors and zero warnings.
- `cargo check --workspace --all-targets` is clean.
- 54 provider-profile tests pass, including the two-provider integration
  test, the fail-closed credential test, the no-plaintext-on-disk test, the
  revision-conflict test, and the loopback-binding test.
- End-to-end checks cover profile creation, listing, activation, update,
  removal, the configuration page's create/activate/update/delete flow,
  authorization rejection, server shutdown, stale-state cleanup, and the
  absence of plaintext secrets on disk.
