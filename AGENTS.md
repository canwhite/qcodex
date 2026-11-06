# AGENTS.md

All experience summaries in this file are general principles. Future summaries should follow general patterns, avoiding case-by-case specifics.

> Aside from the `PRE` global prerequisites below, all content must be categorized under `Plan` / `Think` / `Do` / `Adjust`. New content goes to the end of the corresponding section.

## PRE: Global Prerequisites

- All operations default to running the `caveman` skill first.
  - **Session-start protocol (must, not optional):**
    1. Invoke the `caveman` skill as the first action of a new session, **before replying to any user message**.
    2. Do NOT wait for the skill-list system-reminder or an explicit user prompt to trigger it.
    3. Every reply in the session begins with the `🥷` prefix (skill-defined; drop caveman only for the exceptions it lists, then resume).
    4. Failure mode to avoid: invoking caveman only after the user asks "did you start caveman?" is itself a rule violation — the rule fires at session start, not at user request.
- If a skill is missing or unavailable, record the reason and continue — never block the task.
- Conflict priority: explicit user instruction > safety/build rules > this file > default habits.
- At task start: if the user asks you to execute, show and maintain a TODO list; completed items are marked `done`, unfinished items `todo`.
- Git commit messages must be written in English.

## Plan

### File Organization
- **Plans**: All implementation plans MUST be written to the `docs/` directory. Once a plan is written, run a pre-mortem and update the solutions in the plan markdown until no high-risk items remain.
- **Test Notes**: All test-related text, test cases, and test data MUST be written to the `test/` directory.
- **CLAUDE.md**: Contains general principles and constraints only, no plan content.

### Plan Content
- A plan should contain: goal, scope, steps, acceptance criteria, risks, rollback plan.
- Maintain a TODO list during execution: completed items marked `done`, unfinished items `todo`.
- After verification, update the plan, the TODO list, and the docs; if verification fails, return to `Think` / `Do` and keep debugging.

### Compact Instructions
When compressing, preserve in priority order:
1. Architecture decisions (NEVER summarize)
2. Modified files and their key changes
3. Current verification status (pass/fail)
4. Open TODOs and rollback notes
5. Tool outputs (can delete, keep pass/fail only)

## Think

### Debug Principle 1: Log Immediately at Framework Boundaries
When data crosses a framework boundary (router → handler, middleware → handler, etc.), **log immediately on the receiving side** — don't trust that the framework passed it correctly.
- Framework primitives (params, context, etc.) may return unexpected values
- Use curl to bypass the client and verify server behavior directly

### Debug Principle 2: Check Logs First, Don't Guess
When encountering issues, the first response should be to check logs, not to try things repeatedly.
- The beginning of a log often contains the root cause clue
- Use `docker inspect` to check `ExitCode`, not just `docker logs`

### Debug Principle 3: Read Framework Source, Don't Guess Behavior
When in doubt about a framework's behavior, read the source before hypothesizing. Documentation ≠ source; the source is ground truth.

### Debug Output Methodology
**Core principle: Add output upstream of the assumed problem location, not downstream**

#### Output Levels
```typescript
// Level 1: Confirm execution flow — log at function entry/exit
console.log("[DEBUG] Entering functionName", { relatedVars });

// Level 2: Confirm key variables — output actual values
console.log("[DEBUG] variableName:", actualValue);

// Level 3: Structured output
console.log("[DEBUG] result:", { field1, field2 });
```

#### Log Prefix Convention
- `grep -r "\[DEBUG-" src/` to clear all debug logs at once
- Use a consistent prefix within the same session

#### General Problem Localization Order
1. Assume "A" is the root cause
2. Add logs at A's upstream (the caller) to confirm "does execution reach A?"
3. If not reached, the problem is further upstream
4. If reached, add logs at A's output/return to confirm "is the output correct?"
5. Repeat until the root cause is found

### React Rendering Optimization
For React rendering issues (especially `useEffect` infinite loops, stale closures, and dependency array problems), invoke the `no-useeffect` skill instead of applying case-by-case fixes. If the skill is unavailable, follow the official rules on dependencies and closures.

### Read Tool Mode Preference (when `QCODE_FEATURE_READ_MODES=1`)
The Read tool supports a `mode` parameter. **The default is `mode: "summary"`** (imports + exports only, ≤8 KiB). Measured on 1470 real source files, a summary costs ~7% of the full body; on real traffic 74% of turns fire 2+ parallel Reads, so most reads are orientation rather than "I need this body".
- **Omit `mode`** for orientation — "what does X export?", "list symbols", "API surface". This is the cheap path.
- **Pass `mode: "full"`** before editing a file, quoting its contents, or reasoning about its implementation. Edit requires the full body to accept a write, so you will be sent back here if you skip it.
- An explicit `offset`/`limit` is treated as a body read (`slice`) even without a mode — "give me lines 480-491" is never a request for an outline.
- Use `mode: "slice"` when you already know the line range.

With the flag off, every read returns the full body and this section does not apply.

**Why this entry survives even though prose here has ~0 compliance:** 13 consecutive real Read calls passed no `mode` while the tool description already said "PREFER mode: summary" in bold with worked examples. The default was moved into the handler's branch precisely because asking does not work — the recovery path is carried by Edit's rejection message, not by documentation.

## Do

### New Dependencies / SDK Migration: Always Read Source First
**Before introducing a new library or migrating versions:**
1. Read the key source implementation (e.g. `node_modules/{package}/dist/`, or follow `package.json` `main` / `module` / `exports` to find the entry point)
2. Verify exports, function signatures, calling conventions, and data formats
3. Documentation ≠ source code — docs may be wrong or outdated

### Verification (Required After Any Change, Tiered by Change Type)
- **Documentation changes**: check spelling, links, and formatting.
- **Code changes**:
  1. Run the project's **build command** — must pass (e.g. `bun run build`, `go build`, `cargo build`, `gradle build`)
  2. Run the project's **static analysis / type check** — zero errors (e.g. `bunx tsc --noEmit`, `golangci-lint run`, `cargo check`, `mypy`)
  3. Runtime verification: the service starts + API responses are correct
- **API changes**: test the normal, unauthenticated, unauthorized, and empty-data paths with `curl`
- **Frontend changes**: must be verified in a browser, not with `curl` alone. Steps: build → restart the service → walk through the affected user flows in a browser → check the DevTools Console for errors

**Definition of done:**
- Build passes
- Static analysis / type check zero errors across the entire project
- Runtime verification passed
- Logic correctness check passed
- Fix Protocol followed (if this change is a bug fix): regression test red-then-green, blast radius checked, adversarial re-review done
- Prevention step taken, if this bug class is likely to recur
- Frontend changes verified in a browser
- No untracked TODO left behind; if any remain, they must be explicitly recorded
- TODO list shown and kept updated with `done` / `todo`

### Logic Correctness Check
**Any code change must verify logic correctness in addition to static analysis / type checking.**

#### Verification Method
1. List all execution paths affected by the change (normal + edge cases)
2. Confirm each path's return value matches expectations
3. Manually test critical paths with curl (unauthenticated / unauthorized / empty data)
4. Find a logic issue → fix it → re-verify all paths until complete

#### Self-Check Checklist
- [ ] What execution paths does this change have?
- [ ] What is the return value of each path?
- [ ] Are edge cases (empty data, unauthenticated, unauthorized) handled correctly?
- [ ] Does anything else depend on this logic that needs updating?

### Frontend Change Verification
**Must use browser verification, not just curl.** curl doesn't trigger CORS and may miss cross-origin issues.

#### Verification Steps
1. `bun run build`
2. Restart the server
3. Open a browser and walk through the affected user flows
4. Check the DevTools Console for errors

### Integration / Acceptance Test (Effects Test)
Main test: Has the modified new entry been integrated into the system? Are the new capabilities available and ready for integration? Is the result in line with expectations? It needs evidence.

### Fix Protocol
**Need:** bug fixes are themselves a frequent source of new bugs. Being careful does not prevent this — make the error visible and let the visible thing fail loudly.

1. **Test first.** Before changing code, pin the old behavior with a test/assertion that fails because of the bug (red first, then green). If the bug cannot be reproduced automatically, record the manual repro steps and walk through them again after the fix.
2. **Never edit expectations to pass.** Fixing a test by changing its expected value, or deleting/weakening the test, is a self-granted amnesty — treat it as a new bug. The one exception: the assertion itself was wrong. Then prove it (it never held, or it encoded an unintended behavior) and say so out loud in the report. "The test was wrong" is a claim requiring the same evidence as any other.
3. **Blast radius check.** Before adding any new rejection / interception / validation, list the normal paths it would also block, and get the user's ruling first. Never decide the scope of a block yourself. Exception: the scope was already decided in a plan, ADRs, or a prior ruling in this session — then follow it and record that you did. A block of this kind is never an AFK decision; it is always HITL.
4. **Adversarial re-review after the fix.** Review once from the fixer's view ("is the change right?") and once from the caller's view ("did the external behavior change?"). Only pass when both agree — the same eyes cannot see their own blind spot.

**Why:** "changed wrong but stayed green" is the only dangerous failure; a wrong change that turns the suite red is not fatal.

### Untrusted Claims
- Claims in comments or docs — including your own — are never evidence; see `Think` Principle 3. Only a reproducible input plus observed output counts.
- When a comment contradicts behavior, behavior wins — fix the comment.
- Self-assessment ("done", "works") is the most common false positive: hold it to the same evidence you would demand of anyone else.

### Prevention for Recurring Bugs
A bug class likely to recur needs a prevention machine, not just a fix — a probe that fails on purpose, a contract test, an invariant checker. Run `post-mortem`; it requires the prevention step and classifies it AFK / HITL.

## Adjust

### Global Scan After Fix
After a fix, do not apply single-case patches. Zoom out and scan globally:
1. Does this root cause exist elsewhere?
2. Do similar patterns also have latent defects?
3. Does the fix introduce consistency risk (A changed, but the analogous B did not)?
4. If the problem is hard to localize, use the `diagnose` skill on its own. If the skill is unavailable, continue with a systematic investigation.

**Why:** Local fixes accumulate technical debt; similar issues recur.
**How to apply:** After every fix, ask "where else does this problem exist?" — stop only when you can confidently answer "this one place only".

### Closed-Loop Update
- Update the plan, TODO list, and verification status in `docs/`.
- If verification fails, return to `Think` / `Do` and keep debugging.
- If new risks are found, update the pre-mortem and the plan.
