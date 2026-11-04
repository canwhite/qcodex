# Plan: Apply the DESIGN-NEW visual language to a darker TUI theme

> **User's request (restated)**: Change the TUI design theme to follow `DESIGN-NEW.md`, with an overall darker tone.
>
> **What this plan answers**: How to translate the supplied blue-purple dashboard tokens into a cohesive, selectable dark terminal theme without breaking theme selection, terminal fallback, or readable TUI states.
>
> Goal: Add a dark, blue-purple TUI theme derived from `DESIGN-NEW.md` and validate it across normal and reduced terminal color palettes.

## Context

`DESIGN-NEW.md` supplies a blue-purple dashboard design system, but its declared canvas and card surfaces are light. The TUI already centralizes predefined palettes in `code-rs/tui/src/theme.rs`, exposes named themes through `ThemeName` in `code-rs/core/src/config_types.rs`, and presents them in the `/theme` picker and chat-widget display labels. The implementation must therefore create a deliberately dark interpretation of the design rather than copy its white surfaces verbatim.

Assumption: this is a new built-in, truecolor dark theme, not a replacement for every existing TUI theme or an attempt to reproduce dashboard-only structures such as sidebars and card shadows in a terminal.

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
- Should the new design become the default dark palette, replace an existing dark palette, or remain an additional opt-in theme? This plan assumes an additional opt-in theme.
- Should users be able to select a dedicated ANSI-16 approximation, or is the shared dark ANSI-16 fallback the desired compatibility behavior?

## Out of Scope

- Rebuilding the terminal UI into the dashboard layout described by `DESIGN-NEW.md` (sidebar, cards, gradients, shadows, and web typography).
- Changing terminal emulator colors, fonts, or user shell configuration.
- Redesigning non-TUI clients or changing all existing built-in themes.

---

> Next step: run `/pre-mortem docs/planning/tui-dark-design-new-theme.md` to identify failure modes and harden this plan.
