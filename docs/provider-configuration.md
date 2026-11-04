# Provider Configuration

Codex can talk to any OpenAI-compatible HTTP endpoint. The provider
configuration center lets you manage several such endpoints ("provider
profiles") and pick which one a session uses, without editing
`config.toml` by hand.

## Quick start

```bash
# Create a profile. The API key is read from a pipe so it never appears in
# your shell history or in the process list.
printf '%s' "$MY_KEY" | code config add \
  --name "My provider" \
  --base-url https://api.example.com/v1 \
  --wire-api chat \
  --model gpt-4o-mini \
  --api-key-stdin \
  --activate

# Open the configuration page in a browser.
code config-start

# Inspect the effective configuration (never prints the key).
code config status
```

`code config-start` also works under the `qcodex` name:

```bash
qcodex config-start
```

If you already have a compatible `.env` in a project, explicitly migrate it
into the portable global store instead of copying it between repositories:

```bash
qcodex config import-dotenv --path .env --confirm
qcodex config status --json
```

Import requires `--confirm` because it may place `OPENAI_API_KEY` into secure
global credential storage. It never prints the key.

## What a profile contains

| Field | Meaning |
|---|---|
| `id` | Stable identifier. Generated when you do not supply one. |
| `display name` | Label shown in the configuration page. |
| `base URL` | Root of the OpenAI-compatible API, for example `https://api.example.com/v1`. |
| `wire API` | `chat`, `responses`, or `responses_websocket`. |
| `model` | Model id sent to the endpoint. |
| `API key` | Optional. Stored in your system credential store. |
| `provider id` | Optional label used by provider-aware Codex features. |

The wire API cannot be detected automatically: `chat` posts to
`/v1/chat/completions`, while `responses` posts to `/v1/responses`. They use
different payload shapes, so pick the one your endpoint implements.

## Where things are stored

| What | Where |
|---|---|
| Profile metadata | `~/.code/provider_profiles.json` |
| API key (default) | `~/.code/provider_secrets.enc`, encrypted with a key in `~/.code/provider_secrets.key` |
| API key (optional) | Your system credential store, when `CODEX_PROVIDER_SECRETS=keyring` is set |
| Running server state | `~/.code/config_center.json` |

The API key is **never** written to `config.toml`, to the profile document,
to logs, or to command output. `code config list` and `code config status`
report only whether a key is present.

By default the secret lives in an encrypted file under the code home. The
encryption key is a separate random file with owner-only permissions, so
the encrypted file is useless on its own. This backend never blocks on an
authorization prompt, which makes it the right choice for headless installs
and CI.

Set `CODEX_PROVIDER_SECRETS=keyring` to keep secrets in the platform
credential store instead (Keychain, Credential Manager, Secret Service).
Writing a new entry there may prompt you for authorization the first time.

Both backends fail closed: if the secret cannot be stored securely, the
command reports an error instead of writing a plaintext secret.

## Precedence

When more than one thing wants to choose the provider, the winner is decided
in this order:

1. **Invocation overrides** — `--model`, `--profile`, and `-c key=value`.
2. **The active qcodex provider profile**.
3. **Normal `config.toml`** — `model_provider`, `model`, and `[profiles.*]`.

When a qcodex provider profile is active, qcodex does not import provider
settings from the current project's `.env`. This makes the selected source
portable: starting qcodex in another repository cannot silently change its
endpoint, model, or wire API. `qcodex config status --json` reports
`source: "provider-profile"` and `profile_state: "active-profile"` when this
is in effect.

For one deliberate project-scoped invocation, set
`QCODEX_PROJECT_PROVIDER_OVERRIDE=1` in the shell before launching qcodex.
That switch allows the current project's `.env` and config to win for that
process only. qcodex deliberately ignores this variable when it appears in a
project `.env`, so a repository cannot silently override your global profile.
Use `qcodex config deactivate` to stop using the global profile until another
profile is activated.

An invocation override or a `config.toml` provider selection that disagrees
with the active profile is reported as a configuration conflict. Codex does
not merge an endpoint from one source with a credential from another.

Note that `config.toml`'s `[profiles.*]` tables are a different concept:
they bundle model and reasoning settings, and they refer to a provider by
id. A qcodex provider profile owns the endpoint, the wire API, the model,
and the credential.

## When a switch takes effect

A turn that has already started keeps the provider it began with. A switch
that happens mid-turn is applied at the turn's end, so every request in that
turn still reaches the original provider.

The interactive TUI notices the change and shows the new provider in the
transcript. `code exec` and Auto Drive load the active profile when the run
starts and keep it for the whole run.

## Command reference

```bash
code config list [--json]
code config show <id> [--json]
code config add --base-url <url> --model <model> [--name <name>]
                [--wire-api chat|responses|responses_websocket]
                [--provider-id <id>] [--api-key-stdin] [--activate]
code config update <id> [--name <name>] [--base-url <url>]
                     [--wire-api <api>] [--model <model>]
                     [--provider-id <id>] [--api-key-stdin] [--clear-api-key]
code config remove <id> [--replacement <id>]
code config use <id>
code config deactivate
code config import-dotenv [--path <file>] [--id <id>] [--name <name>] --confirm
code config status [--json]

code config-start [--no-browser] [--json] [--idle-timeout <seconds>]
code config-close [--json]
```

Removing the active profile clears the active selection. Pass
`--replacement <id>` to activate another profile in the same step. Codex
never selects a different source silently.

## Diagnosing problems

**`provider secret '<id>' is missing from the credential store`**
The key was removed or the profile was restored from a backup without the
credential store. Re-add it:
`printf '%s' "$KEY" | code config update <id> --api-key-stdin`.

**`configuration conflict: ... cannot be combined with the active provider profile`**
An invocation override or a `config.toml` selection points at a different
provider. Remove the override, or deactivate the profile with
`code config` (edit `provider_profiles.json` to clear `active_profile_id`).

**`model metadata unknown`**
Codex derives context window and truncation limits from the model name.
A model name that is not in the local catalog falls back to conservative
defaults, which can truncate long tool output. Check `code config status` to
see the model in effect.

**The page does not open**
`code config-start --no-browser --json` prints the URL. Use
`code config-close` to stop a server that is still running.

**A request fails after switching**
Confirm the wire API matches the endpoint. A `chat` profile pointed at a
Responses-only endpoint fails on the first request.
