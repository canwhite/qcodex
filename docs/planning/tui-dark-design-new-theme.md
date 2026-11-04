# Plan: Apply the DESIGN-NEW visual language to a darker TUI theme

> **User's request (restated)**: Change the TUI design theme to follow `DESIGN-NEW.md`, with an overall darker tone.
>
> **What this plan answers**: How to translate the supplied blue-purple dashboard tokens into a cohesive, selectable dark terminal theme without breaking theme selection, terminal fallback, or readable TUI states.
>
> Goal: Add a dark, blue-purple TUI theme derived from `DESIGN-NEW.md` and validate it across normal and reduced terminal color palettes.

## Context

`DESIGN-NEW.md` supplies a blue-purple dashboard design system, but its declared canvas and card surfaces are light. The TUI already centralizes predefined palettes in `code-rs/tui/src/theme.rs`, exposes named themes through `ThemeName` in `code-rs/core/src/config_types.rs`, and presents them in the `/theme` picker and chat-widget display labels. The implementation must therefore create a deliberately dark interpretation of the design rather than copy its white surfaces verbatim.

Decision: this is the default built-in, truecolor dark theme. Existing themes remain available as explicit selections; this does not attempt to reproduce dashboard-only structures such as sidebars and card shadows in a terminal.

## Goal

Users can choose a named dark blue-purple theme whose background, text hierarchy, focus/selection, semantic states, syntax colors, cursor, spinner, and progress indicator follow the intent of `DESIGN-NEW.md`, remain legible in the TUI, and degrade safely on ANSI-only terminals.

## Plan

1. Audit all theme consumers before editing: trace `ThemeName` serialization, palette mapping and ANSI fallback in `code-rs/tui/src/theme.rs`, the `/theme` picker in `code-rs/tui/src/bottom_pane/theme_selection_view.rs`, persisted config helpers in `code-rs/core/src/config*`, and user-facing labels in `code-rs/tui/src/chatwidget.rs`. Record every exhaustive `ThemeName` match so the new variant compiles and is selectable everywhere.
2. Define the dark adaptation of `DESIGN-NEW.md` as explicit TUI token mappings. Preserve its indigo primary, blue-purple secondary/progress accent, and success/error/warning intent; replace its cold-white canvas/card surfaces with layered near-black blue-grey backgrounds; derive high-, normal-, and dim-text colors with adequate contrast; and select distinct border, focused-border, selection, cursor, syntax, spinner, and info colors. Keep gradients, rounded corners, shadows, and web-layout-only tokens out of the terminal palette.
3. Add the built-in theme variant to `ThemeName`, its config-name serialization/deserialization path, and the truecolor theme factory in `code-rs/tui/src/theme.rs`. Update all mapping and brightness-classification helpers so initialization, live switching, syntax highlighting, custom-theme behavior, and terminal-background autodetection continue to choose the correct palette.
4. Expose the theme consistently in the `/theme` selector and every display-name/status surface. Give it a concise name and description that make its dark blue-purple character clear; retain existing ordering (light choices first, dark choices after) and do not alter users' saved selections of existing theme names.
5. Decide and implement reduced-palette behavior: either map the new theme to the existing generic dark ANSI-16 palette or add a dedicated ANSI-16 counterpart only if its distinct identity remains meaningful with 16 colors. Verify that `PaletteMode::Ansi16` never offers an unavailable theme name and that ANSI-256 quantization produces readable selection and text contrast.
6. Add focused regression coverage at the appropriate existing theme/config and TUI rendering seams. Cover serialized config round-trip or parsing, initialization and live selection of the new variant, dark/light classification used by syntax rendering, and representative TUI states (normal text, dim metadata, focused control, selection, success/warning/error, and code highlighting). Update only intentionally affected deterministic snapshots.
7. Manually inspect the theme in a truecolor terminal and a forced ANSI fallback, including `/theme` preview/apply/revert, composer focus and cursor, streamed answer/reasoning, code blocks/diffs, command approval/status, errors, and Auto Drive surfaces. Tune only centrally defined theme tokens when a surface is weak rather than adding ad-hoc component colors.

## Think — Debug Methodology

- Start at theme boundaries: log the resolved `ThemeName`, palette mode, terminal-detection decision, and whether quantization ran in `init_theme`, `switch_theme`, and configuration persistence paths. Use an existing tracing convention; do not log individual rendered cells.
- Read the source in this order: `ThemeName` and config serialization, `theme.rs` mapping/factory functions, theme picker event flow, then component consumers. Do not infer completeness from one visible picker list.
- Localize failures upstream first: distinguish a missing enum/config mapping from a wrong resolved token, and a wrong token from a rendering component that bypasses the shared palette.
- Search globally for the new and existing dark theme variants plus exhaustive `ThemeName` matches, and search hard-coded `Color::` values in TUI rendering paths when a visual surface ignores the active theme. Use a recognizable temporary trace prefix such as `[DEBUG-theme]`, then remove it before completion.

## Do — Verification Strategy

- **Build**: `./build-fast.sh` from the repository root — must pass cleanly with zero errors and warnings.
- **Static analysis / type check**: N/A as a separate command; the required build is the repository's mandated completion gate. Resolve every compiler warning it reports.
- **Focused automated coverage during implementation**: run only the directly relevant existing Rust tests while iterating if needed, then rely on the required build as the final gate; do not run formatting tools and never run `rustfmt`.
- **Runtime verification**: launch the built TUI in a truecolor terminal, select and preview the new theme through `/theme`, restart to confirm persisted configuration reloads it, then repeat with `CODE_FORCE_ANSI256=1` (and, if applicable, ANSI-16 mode) to confirm safe fallback.
- **Logic correctness**: walk startup with default/autodetected configuration, startup with the new saved theme, live preview/apply/revert, syntax-theme selection, and custom-theme selection after using the new theme. Confirm every path resolves a valid palette and never leaves stale custom colors or an unrecognized label.

## Adjust — Rollback and Global Scan

- **Rollback plan**: remove the new `ThemeName` variant and its associated config, factory, picker, label, and test entries as one atomic revert. Existing persisted configurations require a compatibility decision before release: retain a deprecated alias/fallback if users may already have saved the new name.
- **Global scan**: search all Rust sources, config documentation, fixtures, snapshots, selector lists, and exhaustive matches for `ThemeName`, `DarkCarbonNight`, and hard-coded colors that should instead consume the shared palette. Review the same paths for every existing predefined theme to avoid introducing a one-off selection or persistence gap.
- **Backwards compatibility**: do not rename or reorder serialized values for existing themes. The new config value must be accepted by current config loading; document its user-facing name/config spelling if theme settings are documented.

## Open Questions

- `DESIGN-NEW.md` is explicitly light-surface despite the request for a darker theme. The implementation should preserve its blue-purple identity while choosing dark neutral surfaces, unless a separate dark token sheet is provided.
- Resolved: the new design is the default palette; existing themes remain available as explicit selections.
- Should users be able to select a dedicated ANSI-16 approximation, or is the shared dark ANSI-16 fallback the desired compatibility behavior?

## Out of Scope

- Rebuilding the terminal UI into the dashboard layout described by `DESIGN-NEW.md` (sidebar, cards, gradients, shadows, and web typography).
- Changing terminal emulator colors, fonts, or user shell configuration.
- Redesigning non-TUI clients or changing all existing built-in themes.

---

> Next step: run `/pre-mortem docs/planning/tui-dark-design-new-theme.md` to identify failure modes and harden this plan.

---

## Pre-Mortem Risks

<!-- AUTO-GENERATED: New risks will be appended below -->

### [Risk] The delivered palette is a generic dark theme, not a dark DESIGN-NEW interpretation

**Severity**: 5 | **Likelihood**: 4 | **Detectability**: 0.3
**Risk Score**: 14.0

**Failure Scenario**: The source design's light surfaces conflict with the request for a darker tone, so implementation makes unreviewed substitutions. The result loses the supplied blue-purple hierarchy or is rejected as visually unrelated after code and snapshots have already changed.

**Mitigation**:
- Before coding, add a reviewed token table to the implementation change that maps each applicable `DESIGN-NEW.md` token to its dark TUI equivalent and explicitly marks web-only tokens as excluded.
- Review representative terminal renders before changing the palette tokens that define the default visual language.
- Capture representative truecolor renders for approval before tuning peripheral components or changing the default-theme policy.

### [Risk] Theme integration is incomplete, leaving selection, persistence, or live switching broken

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: A new `ThemeName` is added to the palette factory but omitted from a config mapping, selector, display-name match, fallback map, or cache/update path. The TUI compiles but a saved setting is rejected, a label is wrong, or switching leaves stale colors.

**Mitigation**:
- Create a checklist from every `ThemeName` match found by the global audit and complete it before marking the enum change done.
- Add automated coverage for parse/serialize, startup from persisted configuration, live preview/apply/revert, and switching back to a custom theme.
- Make the compiler identify exhaustive matches; avoid wildcard arms that could conceal a newly unsupported variant.

### [Risk] Reduced-color terminals make focus, selection, and text unreadable

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: The blue-purple truecolor palette quantizes into similar ANSI-256 colors or is incorrectly offered in ANSI-16 mode. Focus rings, selections, dim metadata, and status messages then merge with the dark background on common terminal configurations.

**Mitigation**:
- Make the reduced-palette decision explicit: route the new theme to the established dark ANSI-16 palette unless a dedicated variant passes contrast review.
- Test with `CODE_FORCE_ANSI256=1` and an ANSI-16 environment; inspect normal, dim, selected, focused, and semantic-status states rather than only the theme picker.
- Add an automated token-level contrast or distinct-color assertion for foreground/background, focused-border/background, and selection/text pairs where the test harness supports it.

### [Risk] Hard-coded component colors bypass the new shared palette

**Severity**: 4 | **Likelihood**: 4 | **Detectability**: 0.2
**Risk Score**: 12.8

**Failure Scenario**: Most screens adopt the palette, but infrequently used composer, diff, approval, Auto Drive, or history states retain hard-coded light/cyan colors. The mixed UI becomes illegible or visually inconsistent only during real workflows.

**Mitigation**:
- Globally inventory hard-coded `Color::` values in TUI rendering code and classify each as intentional terminal capability handling, syntax behavior, or a palette bypass requiring replacement.
- Use representative snapshot/manual scenarios covering composer, streamed history, markdown/code, diffs, approval/status, errors, and Auto Drive before completion.
- Fix recurring colors in shared token helpers, not through per-screen overrides, and add a regression case whenever a bypass is discovered.

### [Risk] Scope expands from a palette change into a terminal dashboard redesign

**Severity**: 3 | **Likelihood**: 3 | **Detectability**: 0.5
**Risk Score**: 4.5

**Failure Scenario**: Dashboard concepts such as gradients, cards, shadows, navigation rails, and web typography are copied into terminal components, causing a large visual refactor that obscures the theme change and introduces unrelated regressions.

**Mitigation**:
- Preserve the plan's scope boundary: translate color semantics and hierarchy only, and reject layout or interaction changes unless separately approved.
- Keep implementation diffs centered on theme/config/selector code and require a separate plan for component layout redesign.

### [Risk] The new serialized name creates a compatibility trap after release

**Severity**: 3 | **Likelihood**: 2 | **Detectability**: 0.6
**Risk Score**: 2.4

**Failure Scenario**: A name is changed late or removed after users persist it in their configuration, causing silent fallback or config-loading failures on upgrade/downgrade.

**Mitigation**:
- Choose the serialized kebab-case name once, document it, and never rename it in the same release cycle.
- If removal becomes necessary, preserve a parsing alias or intentionally migrate the saved setting with a tested fallback.

---

## Post-Mortem

**Date**: 2026-11-04

### Survey

The completed path is `ThemeName` (configuration default and persisted name) → palette-mode mapping → predefined palette → `/theme` picker and chat label → TUI rendering consumers. `DarkGeoBluePurple` is now the default at both configuration and runtime initialization; existing explicit selections remain unchanged.

### Issues Fixed

| # | Severity | Type | Fix | Verification |
|---|---|---|---|---|
| 1 | High | Default-policy gap | Made `DarkGeoBluePurple` the enum and runtime default, so an unset config starts dark and terminal autodetection does not replace it. | Core/TUI default tests; `./build-fast.sh` |
| 2 | High | Untested fallback contract | Split palette mapping by supplied mode and added a deterministic test that maps GEO to the established dark ANSI-16 fallback. | TUI fallback test; `./build-fast.sh` |
| 3 | High | Persistence/integration gap | Added the stable `dark-geo-blue-purple` config spelling, picker entry, display label, and persistence round-trip test. | Core persistence test; `./build-fast.sh` |
| 4 | Medium | Specification drift | Updated the plan from “opt-in” to the user-approved default-dark policy and added contrast regression coverage for text, selection, primary, and focus tokens. | Five focused TUI tests; `./build-fast.sh` |

### High-Risk Closure

| Original risk | Residual score | Evidence |
|---|---:|---|
| Design drift from `DESIGN-NEW.md` | 3.0 | The palette directly carries dark blue-grey surfaces, indigo primary/progress, blue-purple secondary, and separated semantic and syntax tokens; contrast targets for text, selection, primary, and focus are now enforced by regression tests. |
| Incomplete selection or persistence integration | 0.4 | Exhaustive compile-time matches, picker and display-name entries, plus config persistence coverage. |
| Unreadable reduced-color fallback | 0.1 | Deterministic ANSI-16 mapping test routes the theme to the existing high-contrast dark palette. |
| Palette bypass by unrelated component colors | 6.0 | The change is centralized in the shared theme path; final audit found no new component-level color bypass. Existing component colors remain outside this palette-only change. |

**High-risk constraints remaining**: 0. No residual risk score exceeds 12.

### Root Causes and Prevention

- The original plan treated dark mode as opt-in, while the product requirement is a dark default. The default is now covered at both config and runtime seams.
- Palette fallback was coupled to runtime terminal detection, which made it hard to prove in a test. The pure mode-specific helper now makes the fallback contract testable.
- Future built-in themes must update the enum, persistence mapping, palette factory, picker, display label, and tests together; the compiler and focused test names make missing steps visible.
- Keep the approved default-policy decision in the plan and code tests together, so an implementation update cannot leave stale guidance behind.
