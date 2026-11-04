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

**Final HIGH RISK count: 4. All recorded risks have mitigations.**
