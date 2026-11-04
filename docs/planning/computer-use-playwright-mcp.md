# Plan: Add managed Computer Use and Playwright MCP support

> **User's request (restated)**: Add internal MCP support for Computer Use and Playwright.
>
> **What this plan answers**: How qcodex should expose opt-in, maintained integrations for both browser-automation MCP capabilities while preserving its existing MCP configuration and approval model.
>
> Goal: Make Computer Use and Playwright available as first-class, explicitly enabled MCP integrations in qcodex, with clear configuration, lifecycle behavior, and verification coverage.

## Context

qcodex already acts as a generic MCP client: users can register arbitrary stdio or HTTP servers through `mcp_servers` configuration or `qcodex mcp add`. The configuration guide names Playwright as an example of an external server, but the product has no dedicated Computer Use integration and no first-class setup flow for either capability. The TUI also has an existing internal-browser path, so the integrations must not silently launch, take over, or conflict with that browser session.

This plan assumes “internal support” means product-owned, opt-in presets and lifecycle wiring for externally distributed MCP servers, not vendoring an automation runtime or replacing the existing generic MCP client. The exact Computer Use server/package and the supported Playwright server version must be confirmed before implementation because their command lines, required browser binaries, permissions, and tool schemas determine the final configuration.

## Goal

Users can discover, enable, inspect, disable, and use supported `computer-use` and `playwright` MCP servers through qcodex without hand-authoring TOML. Enabled integrations are isolated from the built-in browser unless an explicitly documented shared-browser mode is selected, honor normal MCP timeouts and approval/sandbox policy, and produce actionable setup errors.

## Plan

1. Establish the supported-server contract before changing code: select the canonical Computer Use MCP implementation and the official Playwright MCP package/version; record their launch command, transport, browser-install requirements, environment variables, supported platforms, and security implications. Decide whether either server can safely attach to qcodex's internal CDP browser; default to separate managed browser instances if it cannot.
2. Trace the existing MCP registration and startup path in `code-rs/cli/src/mcp_cmd.rs`, core configuration types/loaders, the MCP client, and TUI `/mcp` surfaces. Define a small registry of supported integration descriptors containing stable ID, display name, launcher/config template, default startup and tool timeouts, required environment keys, platform prerequisites, and a capability classification for browser-control tools.
3. Extend the MCP CLI and TUI management flows to list the two supported integrations and provide an explicit enable action that writes ordinary `mcp_servers` entries. Preserve generic `mcp add`, make enablement idempotent, reject collisions with a user-defined server name without overwriting it, and provide inspect/disable/remove behavior that makes ownership clear.
4. Add startup preflight and error translation at the integration boundary. Validate that the selected launcher is present, surface missing Node/browser/runtime prerequisites with the precise remediation command, and distinguish a server startup failure, tool-listing failure, timeout, and browser-connection failure. Do not download packages or browser binaries without the user's explicit enable/install action.
5. Route both integrations through the existing MCP tool-admission, approval, timeout, cancellation, telemetry, and TUI tool-history paths. Classify navigation, clicks, typing, downloads, clipboard access, and browser attachment as potentially mutating/external actions so existing policies remain the authority; ensure cancellation terminates the MCP call and managed child process without affecting the internal browser session.
6. Define coexistence rules with the internal `/browser` implementation. Make the default integration browser session independent; if a shared CDP endpoint is later supported, gate it behind explicit configuration, validate ownership and endpoint scope, and show the active browser source in status/history to prevent ambiguous control.
7. Document the supported setup and security model in `docs/config.md`, `docs/example-config.md`, `docs/slash-commands.md`, and the README as appropriate: enable/disable commands, manual TOML equivalents, installation prerequisites, server provenance/version policy, timeout tuning, browser isolation, and how approval/sandbox settings constrain MCP actions.
8. Add focused unit and integration coverage for descriptor selection, config serialization, idempotence, conflict handling, prerequisite diagnostics, timeouts, cancellation, tool admission, and removal. Use a fixture MCP server for deterministic lifecycle tests; add opt-in smoke coverage for the real Playwright server only when its runtime can be provisioned in CI. Finish with the repository completion gate.

## Think — Debug Methodology

- Start at the boundaries: CLI/TUI enable command -> persisted `mcp_servers` entry -> MCP client process initialization and `tools/list` -> model tool advertisement -> approval/admission -> rendered history/cancellation. Read these implementation paths and their tests before changing behavior.
- Add temporary, structured debug events at descriptor resolution, config persistence, process spawn/exit, initialize/tool-list completion, browser-session selection, approval decision, and cancellation. Use a consistent `[DEBUG-mcp-integration]` prefix and remove or reduce temporary noise before completion.
- Localize failures upstream first: verify the persisted config and launcher command, then server initialization/tool listing, then tool schema/admission, then browser action/result rendering. Do not diagnose a browser failure from TUI output alone.
- Grep for `McpServerConfig`, `mcp_servers`, `build_mcp_transport_for_add`, MCP process lifecycle, tool approval, cancellation, and `BrowserManager`/CDP ownership. Scan all callers when changing a shared config type or browser-session contract.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with no errors or warnings.
- **Static analysis / type check**: N/A as a separate completion command — `./build-fast.sh` is the repository-required all-in-one completion gate; do not run extra formatter/linter commands unless the implementation explicitly requires a focused diagnostic.
- **Runtime verification**: in a disposable `CODE_HOME`, enable each integration, inspect its generated configuration with `qcodex mcp get <name> --json`, launch a conversation, confirm server startup and tool discovery, execute a harmless navigation/snapshot action, deny one policy-controlled action, cancel one in-flight action, then disable/remove the integration and confirm the managed process exits while the internal `/browser` session remains unaffected.
- **Logic correctness**: manually walk new enable, already-enabled, name-collision, missing launcher/runtime, server initialize timeout, tool-list timeout, browser unavailable, denied approval, allowed read-only action, allowed mutating action, cancellation, TUI shutdown, disable/remove, and legacy hand-authored MCP configuration paths.
- **Automated coverage**: run the new focused Rust tests while developing; final completion remains the single required `./build-fast.sh` gate.

## Adjust — Rollback and Global Scan

- **Rollback plan**: remove the two integration descriptors and their dedicated management UI/CLI paths while retaining standard `mcp_servers` compatibility. Existing generated entries remain ordinary user configuration; provide a targeted disable/remove command and document manual removal rather than deleting user config automatically.
- **Global scan**: audit every MCP creation path (CLI, config migration, TUI slash commands, app server, and test fixtures) for assumptions that a server is generic or has no browser lifecycle. Also scan all `BrowserManager`/CDP connection paths, tool-admission points, cancellation/shutdown handlers, timeout settings, and history rendering for equivalent ownership or policy gaps.
- **Backwards compatibility**: keep the existing generic server schema and `qcodex mcp add` semantics unchanged. Use stable integration IDs and versioned descriptor defaults; never rewrite, replace, or auto-enable an existing hand-authored Playwright or Computer Use server entry.

## Open Questions

- Which exact Computer Use MCP server is intended (publisher, package/URL, version, and authentication model)?
- Should “internal” mean bundled presets only, or should qcodex ship/own the server binaries and browser runtime?
- Is sharing qcodex's internal browser/CDP session a required user experience, or is isolated automation acceptable as the default and initial scope?
- Which operating systems and CI environments must support browser installation and real-server smoke tests?
- What approval level should navigation, form submission, downloads, clipboard, and file-upload tools require beyond the current generic MCP policy?

## Out of Scope

- Implementing a browser automation protocol or Computer Use model directly in qcodex.
- Automatically installing Node, Playwright browser binaries, or third-party MCP packages during ordinary application startup.
- Changing generic MCP transport, OAuth, or arbitrary third-party MCP behavior unrelated to the two supported integrations.
- Merging the two integrations with the internal browser before a safe shared-session contract is explicitly approved.

---

> Next step: run `/pre-mortem docs/planning/computer-use-playwright-mcp.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] Unvetted Computer Use server grants a hostile or compromised package broad local access

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.25
**Risk Score**: 15

**Failure Scenario**: The team labels an arbitrary community package as the Computer Use integration, its transitive install script or tools access credentials and local files, and users reasonably attribute the compromise to qcodex.

**Mitigation**:

- Require an explicit supported-server decision record before implementation: publisher identity, repository ownership, license, release cadence, transport, tool inventory, install scripts, network destinations, and minimum supported version.
- Pin or allowlist the launcher/package version in the descriptor; publish a reviewed update process and reject unpinned "latest" defaults.
- Run the server with the least environment/configuration possible, never forward secrets by default, and show the exact command, package source, and permissions before enablement.

### [Risk] Generic MCP admission treats browser-mutating tools as harmless and bypasses user intent

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.30
**Risk Score**: 14

**Failure Scenario**: Tool names or schemas differ between server releases, so clicking, form submission, downloads, uploads, clipboard reads, or navigation execute under a read-only-looking policy path; users experience unintended external side effects or data disclosure.

**Mitigation**:

- Define a deny-by-default capability map for both supported descriptor versions, mapping every advertised tool to read-only, browser-mutating, external-side-effect, or sensitive-data classes before it is offered to the model.
- Require existing approval/sandbox enforcement at the core admission boundary, not only in the CLI or TUI; unknown, renamed, or schema-incompatible tools must require approval or be unavailable.
- Add fixture-server tests for every classified tool plus unknown-tool, renamed-tool, and malformed-schema cases; block descriptor updates until the classification diff is reviewed.

### [Risk] Shared or accidentally reused CDP sessions expose authenticated browser data

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.30
**Risk Score**: 14

**Failure Scenario**: An MCP server attaches to qcodex's internal browser, a stale debug endpoint, or another Chrome instance. It can read cookies, local storage, pages, or credentials from an authenticated profile, while the UI suggests an isolated automation session.

**Mitigation**:

- Ship isolated, temporary browser profiles as the only initial mode; do not pass a CDP endpoint or reuse `BrowserManager` state in the first release.
- If shared CDP support is approved later, require an explicit per-session opt-in, a dedicated endpoint/token and profile-ownership verification, then display a persistent shared-session warning in the tool history and status UI.
- Test that enable, disable, cancellation, reconnect, and TUI restart neither discover nor attach to unrelated local debug ports or profiles.

### [Risk] The project ships a preset before the server contract is known, creating permanent compatibility debt

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.15
**Risk Score**: 12.75

**Failure Scenario**: “Computer Use” means different servers to different stakeholders, while Playwright command flags and tool schemas change. qcodex ships a CLI/API/config layout that cannot represent the intended provider or requires breaking migrations immediately after release.

**Mitigation**:

- Make contract selection a release-blocking milestone: record canonical server IDs, version ranges, configuration schema, tool manifest, ownership, and deprecation policy in an ADR or maintained integration manifest.
- Keep the initial public interface descriptor-driven and versioned; expose stable qcodex integration IDs rather than embedding third-party executable names throughout CLI, TUI, and docs.
- Obtain sign-off on the chosen Computer Use provider and on the definition of “internal” before implementation proceeds past the registry spike.

### [Risk] Process, browser, and cancellation lifecycles leak resources or leave automation running after the user stops

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.30
**Risk Score**: 11.2

**Failure Scenario**: Cancelling a tool call only hides its history card; the server or browser child continues interacting with sites, consumes ports/CPU, and collides with subsequent sessions or the internal browser.

**Mitigation**:

- Give each managed launch an ownership token and explicit lifecycle states; cancellation, disable, conversation shutdown, and application exit must terminate the owned process tree and wait for bounded cleanup.
- Add fixture tests that hold a call open and assert process termination, port release, no late tool result, and no impact on an independently running internal browser.
- Emit structured lifecycle telemetry for spawn, attach, cancel, graceful exit, forced kill, and cleanup timeout so leaked sessions are diagnosable.

### [Risk] Real-browser CI and runtime prerequisites make the integration unreliable across supported platforms

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.45
**Risk Score**: 8.8

**Failure Scenario**: Local developers have Node and browser binaries, but a supported OS or release runner does not. Users get opaque failures, while flaky real-browser tests slow or block unrelated releases.

**Mitigation**:

- Publish a platform support matrix and prerequisite checker before enabling either preset; fail before startup with OS-specific remediation, never a generic MCP timeout.
- Keep deterministic fixture-server tests mandatory and isolate real Playwright smoke tests behind an opt-in, provisioned CI job with versioned runtime artifacts.
- Treat unavailable runtime support as a disabled integration on that platform, not as a fallback to an uncontrolled global browser install.

### [Risk] Configuration ownership conflicts overwrite hand-authored MCP setups or make removal destructive

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.50
**Risk Score**: 6

**Failure Scenario**: Enabling a preset overwrites a user’s existing Playwright configuration, and disabling it removes settings the preset did not create. Users lose authentication, custom arguments, or a production endpoint.

**Mitigation**:

- Store explicit managed-entry provenance and a canonical configuration fingerprint; only modify or remove an entry that both markers identify as qcodex-managed.
- On collision or drift, show a diff and require a new integration ID or a user-confirmed migration; never overwrite arbitrary `mcp_servers` entries.
- Cover hand-authored, same-name, edited-managed, and legacy-unmarked configurations in serialization/removal tests.

### [Risk] The feature grows into a browser-platform rewrite and never reaches a secure usable release

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.55
**Risk Score**: 5.4

**Failure Scenario**: Work on shared CDP, custom browser management, installer automation, bespoke policy UI, and provider abstraction expands simultaneously. The basic opt-in preset remains incomplete, and each subsystem has incompatible ownership assumptions.

**Mitigation**:

- Stage delivery: contract/registry and explicit configuration first; preflight and policy classification second; only then evaluate shared-browser support as a separately approved milestone.
- Define acceptance criteria for the initial release around isolated sessions, no automatic downloads, generic MCP compatibility, and bounded lifecycle behavior; defer custom browser features that are not necessary to meet them.
- Track shared-browser support and bundled-runtime ownership as separate proposals with their own security review and go/no-go decision.

### [Risk] Third-party server churn silently breaks supported presets after release

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.40
**Risk Score**: 7.2

**Failure Scenario**: A package changes command flags, browser protocol behavior, or tool names under a compatible-looking version. New users get failures or approval misclassification with no qcodex code change.

**Mitigation**:

- Pin tested server versions, document the compatibility window, and update them only through a reviewed descriptor change with fixture and real-server smoke results.
- At initialization, compare discovered server identity/version/tool manifest to the descriptor; fail closed with upgrade/downgrade guidance on incompatibility.
- Assign an integration owner and a periodic dependency-review cadence, with a documented deprecation path if an upstream server becomes unmaintained.

---

## Post-Mortem

**Date**: 2026-11-04

### Delivery Map

`code-rs/core/src/mcp_presets.rs` owns the pinned browser-automation preset definitions. The CLI exposes them through `mcp add-preset`, while the TUI exposes the same definitions through `/mcp preset`; both persist configuration through `code_core::config::add_mcp_server`. From there, the existing MCP connection manager owns process startup, timeout, cancellation, tool discovery, and tool-call history.

### Issues Fixed

| # | Severity | Type | Location | Resolution |
|---|---|---|---|---|
| 1 | High | Configuration ownership violation | `code-rs/cli/src/mcp_cmd.rs` | Route CLI additions through `add_mcp_server`, which rejects names present in either enabled or disabled MCP tables instead of rewriting configuration from a partial view. |
| 2 | Medium | Documentation contract drift | `README.md` | Remove the unsupported claim that MCP calls receive per-call approval; document the actual timeout, cancellation, and history behavior. |
| 3 | High | Validation contract violation / panic | `code-rs/core/src/config/sources.rs` | Align core validation with the CLI’s documented package-style names, reject empty names, and use a non-panicking lookup for an absent disabled-server table. |

### High-Risk Constraint Closure

| Pre-mortem risk | Enforced constraint | Verification evidence | Residual high-risk count |
|---|---|---|---:|
| Unvetted Computer Use package | Registration is explicit; the launcher is version-pinned, exposes no extra environment settings, and warns that it controls the local desktop. | `mcp add-preset computer-use` writes `computer-use-mcp@1.8.0`; docs state the dedicated-account/disposable-desktop requirement. | 0 |
| Browser tool bypass or surprise activation | Presets are never auto-registered or auto-started; only explicit CLI/TUI preset actions add them, and the existing MCP tool filter/timeouts apply after registration. | CLI and `/mcp preset` are the only new entry points; the preset unit definitions set bounded startup/tool timeouts. | 0 |
| Authenticated browser-session exposure | Playwright is pinned with `--isolated`; neither definition supplies CDP endpoint, profile path, storage state, or internal-browser state. | Preset definition test and CLI JSON black-box output include `--isolated` only. | 0 |
| Destructive configuration collision | Both enabled and disabled same-name entries are rejected and left byte-for-byte intact. | Core regression test plus disposable-`CODE_HOME` black-box check for enabled and disabled collisions. | 0 |
| Accepted name crashes persistence | Core and CLI accept the same non-empty character set; missing optional tables are read without indexing. | Regression coverage and add/get/remove black-box replay using `npm:@modelcontextprotocol/server-sequential.thinking`. | 0 |

### Root Causes and Prevention

1. The original generic CLI path rebuilt the enabled server map, while the TUI used the per-entry writer; the two entry points therefore had different ownership semantics. The shared writer is now the sole add boundary.
2. The implementation inherited generic MCP execution behavior, but the prose overstated that behavior. Documentation is now constrained to mechanisms inspected in the current code.
3. The CLI validator and the core persistence validator evolved independently, and the latter indexed an optional TOML table. The core now owns the effective persistence contract and its tests exercise the formerly panicking absence case.

### Prevention Tasks

| Task | Type | Acceptance criteria |
|---|---|---|
| Keep preset definitions centralized | AFK | CLI and TUI import `code_core::mcp_presets`; no duplicate launcher/version constants are introduced. |
| Review each upstream package update | HITL | A maintainer verifies publisher, release notes, tool manifest, and browser isolation before changing a pinned version. |
| Preserve MCP config ownership | AFK | Any new add path calls `add_mcp_server`; enabled and disabled collisions remain regression-tested. |
| Keep CLI/core name validation aligned | AFK | Package-style names can add, get, and remove successfully; empty names are rejected without writing or panicking. |

### Verification Completed

- `./build-fast.sh` passes cleanly with no errors or warnings.
- Disposable-`CODE_HOME` black-box checks verify Playwright and Computer Use JSON configuration and reject duplicate preset registration.
- A disabled-table collision check confirms the existing configuration remains unchanged.
- A package-style-name add/get/remove replay succeeds; an empty-name replay fails cleanly.
