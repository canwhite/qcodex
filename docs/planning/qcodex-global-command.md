# Plan: Install qcodex as a global shell command

> **User's request (restated)**: Change the current project so that, after building, I can run `qcodex` from any directory instead of using `make run` or a repository-relative path.
>
> **What this plan answers**: Make the build install a standalone `qcodex` executable into a user-writable directory already on `PATH`.
>
> Goal: `make` succeeds and a newly opened or current shell can resolve `qcodex` from any working directory.

## Context

The current Makefile only creates `code-rs/bin/qcodex` inside the repository.
That file is not a shell command outside the repository, and `make run` merely
executes that local path. A usable command must instead be placed in a directory
on `PATH`. The implementation must avoid requiring `sudo`, avoid silently
editing shell startup files, and leave the Rust binary's internal `code` name
unchanged so existing build and release tooling continues to work.

## Goal

After `make`, `command -v qcodex` resolves to an executable in a user-writable
directory on `PATH`, and `qcodex --version` succeeds regardless of the current
directory. If no safe install directory exists, the command fails with a clear
actionable message rather than installing into a system directory or pretending
the command is globally available.

## Plan

1. Inspect the active `PATH` and identify a user-writable directory. Prefer a
   conventional user bin directory (`~/.local/bin`, then `~/bin`) only when it
   is already on `PATH`; otherwise use the first safe user-owned PATH directory.
   Do not assume `~/.local/bin` is present on macOS or in every shell.
2. Replace the repository-local `qcodex` symlink workflow in `Makefile` with an
   install step after `./build-fast.sh`. Copy the built `code-rs/bin/code` binary
   atomically to `$(INSTALL_DIR)/qcodex`, preserving executable permissions.
   Default `INSTALL_DIR` to the discovered writable `PATH` directory and permit
   explicit override with `make INSTALL_DIR=/path/to/bin`.
3. Make the default `make` target perform both build and installation. Keep a
   `run` target only as a convenience that invokes the installed `qcodex`; it
   must not be the documented way to launch the application.
4. Add install-directory validation: reject an empty path, a directory absent
   from `PATH`, an unwritable target, or a target that resolves to a system
   location requiring privilege escalation. Report the selected directory and
   the final command path on success.
5. Update the development README to document `make` followed by `qcodex`, plus
   the `INSTALL_DIR` override for environments with no writable PATH directory.
   Remove the repository-relative launch instruction.

## Think — Debug Methodology

- Start at the boundary: print the resolved `INSTALL_DIR`, `PATH` membership,
  source-binary path, and destination path before copying. Use a concise
  `[qcodex-install]` prefix for any diagnostic output.
- Read the existing `build-fast.sh` artifact-copy logic before changing
  Makefile targets; it determines the reliable source binary location and cache
  behavior.
- If `qcodex` cannot be found, first compare `command -v qcodex` with the
  selected install directory, then check shell `PATH` parsing and filesystem
  permissions before investigating Rust compilation.
- Search the repository for `code-rs/bin/qcodex`, `make run`, and launch
  examples so documentation and automation do not retain the local-only model.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` — must pass cleanly with no warnings or errors.
- **Make entry point**: `make` — must build, choose a safe install directory,
  and report the installed `qcodex` path.
- **Runtime verification**: from a temporary directory outside the repository,
  run `command -v qcodex` and `qcodex --version`; both must succeed without a
  `./` prefix or repository-relative path.
- **Logic correctness**: manually exercise a valid writable PATH directory, an
  explicit `INSTALL_DIR` override, an empty/non-PATH override, and a collision
  with a pre-existing `qcodex`. Confirm replacement is atomic and failures do
  not leave a partial executable.

## Adjust — Rollback and Global Scan

- **Rollback plan**: remove the installed `qcodex` at the Makefile-reported
  destination, then revert the Makefile and README changes. No system-wide
  package state or shell configuration is modified.
- **Global scan**: search release scripts, npm wrappers, documentation, and CI
  for assumptions that the primary developer executable is only named `code`.
  Keep upstream compatibility paths intact while ensuring end-user examples use
  `qcodex`.
- **Backwards compatibility**: retain `code-rs/bin/code` and all existing
  release-wrapper names; `qcodex` is an installed developer-facing command,
  not a Cargo bin rename.

## Open Questions

- Assumption: a user-writable directory already on `PATH` is available. If it
  is not, the implementation will require an explicit `INSTALL_DIR` rather than
  editing `.zshrc`, `.bashrc`, or system directories automatically.
- The exact overwrite policy for a pre-existing unrelated `qcodex` should be
  confirmed during implementation; the safe default is to require an explicit
  opt-in before replacing it.

## Out of Scope

- Renaming the Rust Cargo binary, package names, release archives, or npm
  wrappers from `code` to `qcodex`.
- Installing via Homebrew, npm, a system package manager, or `sudo`.
- Automatically modifying shell configuration files.

---

> Next step: run `/pre-mortem docs/planning/qcodex-global-command.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] The installed command is absent in the shell that users actually open

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Initial Risk Score**: 5 × 4 × (1 - 0.3) = 14 **(HIGH)**
**Residual Risk Score after mitigation**: 5 × 1 × (1 - 0.9) = 0.5

**Failure Scenario**: `make` inherits one PATH, installs successfully, and
reports success, but a newly opened login shell has a different PATH. Users
still receive `command not found: qcodex` outside the repository.

**Mitigation**:

- Treat both the invoking shell and the user's login shell as acceptance
  boundaries: before installation, validate `INSTALL_DIR` against the invoked
  PATH; after installation, verify `qcodex --version` from a temporary working
  directory using the same PATH and, where available, the configured login
  shell.
- If the destination is not in the relevant PATH, fail before copying and print
  the exact `INSTALL_DIR` override and PATH-export command. Do not claim a
  global command was installed.
- Document that the user must choose a directory shared by their normal and
  login-shell PATHs; the plan deliberately does not silently mutate shell rc
  files.

### [Risk] Installation overwrites another program named qcodex

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Initial Risk Score**: 4 × 4 × (1 - 0.2) = 12.8 **(HIGH)**
**Residual Risk Score after mitigation**: 4 × 1 × (1 - 0.95) = 0.2

**Failure Scenario**: A pre-existing `qcodex` belongs to another project or
package. A routine build silently replaces it, breaking unrelated work.

**Mitigation**:

- Before replacement, resolve any existing `qcodex` and compare it with the
  planned destination. Refuse to replace an existing destination unless it is a
  previously recorded qcodex-managed artifact or the user supplies an explicit
  force variable.
- Write a small adjacent ownership marker containing the source repository and
  build identity only after the binary copy succeeds; never use a filename match
  alone as ownership proof.
- Use a temporary file in the destination directory followed by atomic rename,
  so a failed update cannot corrupt either the existing command or its marker.

### [Risk] A copied binary loses files it needs at runtime

**Severity**: 5 | **Likelihood**: 3 | **Detectability**: 0.3
**Risk Score**: 5 × 3 × (1 - 0.3) = 10.5

**Failure Scenario**: `qcodex --version` works, but a command that expects
relative resources, an update helper, or a companion binary fails once the main
binary has been copied outside the repository.

**Mitigation**:

- Prefer installing a stable launcher that resolves the project-built binary
  only if runtime inspection proves the executable requires repository-relative
  assets; otherwise install the standalone binary. Select the mechanism based
  on runtime verification, not convenience.
- Verify at least the CLI version path, normal interactive startup, and one
  subcommand that exercises executable/resource discovery from a temporary
  directory.
- Keep the install implementation focused on the primary CLI and do not imply
  companion executables are globally installed unless they are explicitly
  handled and tested.

### [Risk] Two builds leave qcodex partially written or mismatched

**Severity**: 4 | **Likelihood**: 3 | **Detectability**: 0.4
**Risk Score**: 4 × 3 × (1 - 0.4) = 7.2

**Failure Scenario**: Concurrent `make` processes copy different builds over
the same destination, yielding a truncated binary or a binary/marker mismatch.

**Mitigation**:

- Copy to a uniquely named temporary file in the final destination directory,
  set permissions, validate the temporary executable, then rename it atomically.
- Serialize installation with a destination-local lock and always release it on
  shell exit; allow compilation to remain concurrent.
- On startup, reject a mismatched ownership marker and instruct the user to run
  the explicit repair/install target.

### [Risk] The Makefile selects a privileged or unsafe destination

**Severity**: 4 | **Likelihood**: 2 | **Detectability**: 0.8
**Risk Score**: 4 × 2 × (1 - 0.8) = 1.6

**Failure Scenario**: PATH order points first at a system directory, causing a
privilege error or encouraging users to rerun the build with `sudo`.

**Mitigation**:

- Limit automatic selection to existing directories owned and writable by the
  current user; never choose a root-owned directory, even if it is writable
  through unusual ACLs.
- Reject `sudo make` and provide an explicit user-owned `INSTALL_DIR` example.
- Print the selected destination before any filesystem mutation.

### [Risk] Documentation reintroduces a local-only invocation

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.8
**Risk Score**: 3 × 3 × (1 - 0.8) = 1.8

**Failure Scenario**: README examples continue to recommend `make run` or a
repository path, so users do not discover or validate the global command.

**Mitigation**:

- Replace the development launch example with `qcodex` and add the exact
  failure path for an unavailable writable PATH directory.
- Add a repository-wide documentation scan for `make run` and
  `code-rs/bin/qcodex` to the completion checklist.

**Convergence review**: The first sweep found two high risks. After adding the
mandatory PATH-boundary, ownership, atomic-install, and no-privilege gates,
the residual score of every recorded risk is at most 10.5. The second sweep
(runtime packaging, concurrency, permissions, documentation, and external
tooling) found no additional risk above 12. **Unmitigated high risks: 0.**

---

## Post-Mortem: Global qcodex installation

**Date**: 2026-11-03

The implemented path is `make` → `build-fast.sh` → `code-rs/bin/code` → atomic
copy to the selected user-owned PATH directory as `qcodex`. The ownership marker
protects future installs and `uninstall` protects unknown commands.

### Issues Fixed

| # | Severity | Type | Location |
|---|---|---|---|
| 1 | High | Make parsing error | `Makefile` install-directory discovery |
| 2 | Medium | Plan/implementation gap | `Makefile` default destination selection |

1. The initial directory-discovery expression embedded a shell `case` pattern
   (`*)`) inside `$(shell ...)`; GNU Make interpreted the closing parenthesis
   as the end of its own function, yielding an invalid empty install directory.
   It now uses PATH splitting and `grep` without Make-sensitive shell syntax.
2. The first implementation only used `~/.local/bin`. It now prioritizes that
   conventional directory (then `~/bin`) when available on PATH and otherwise
   discovers a writable, user-owned PATH directory.

### Verification

- `./build-fast.sh` passed through `make` with no compiler warnings or errors.
- In actual install permissions, Make resolved `INSTALL_DIR` to
  `/Users/doing/.local/bin` and installed `qcodex` successfully.
- From a temporary directory outside the repository, `qcodex --help` and
  `qcodex --version` succeeded.
- `make uninstall` removed only the marked command, and a subsequent `make`
  restored it.
- A temporary unowned `qcodex` was rejected and remained unchanged.

### Residual Risk Review

All high-risk scenarios in the pre-mortem have executable prevention gates:
PATH membership and runtime resolution, ownership-marker validation, atomic
replacement, no-sudo enforcement, and collision rejection. **Unmitigated high
risks: 0.**

### Prevention Tasks

### Prevention Task: Validate Make expansion before installation

**Type**: AFK
**Root Cause**: Make and shell syntax were combined in a computed variable
without checking the expanded result.
**Prevention**: Keep a dry-run/variable-expansion check in the install change
checklist before running a real installation.

**Acceptance Criteria**:

- [x] `make -pn` resolves a non-empty `INSTALL_DIR` under actual install
  permissions.
- [x] `make -n` expands an install destination without syntax errors.
