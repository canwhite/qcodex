//! CLI commands for managing OpenAI-compatible provider profiles.
//!
//! The configuration center has two halves: a set of `config` subcommands
//! that work headlessly, and `config-start` / `config-close`, which manage
//! the local configuration page.
//!
//! Secrets are only ever read from standard input. They are never taken
//! from a command-line argument, echoed to the terminal, or written to
//! `config.toml`.

use code_core::provider_profiles::parse_wire_api;
use code_core::provider_profiles::ProviderProfile;
use code_core::provider_profiles::ProviderProfileRepository;
use code_core::provider_profiles::ProviderProfileResolver;
use code_core::provider_profiles::ProviderSecretStore;
use code_core::provider_profiles::ProviderStoreState;
use clap::Parser;
use clap::Subcommand;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::io::Read;
use std::path::PathBuf;

/// Manage OpenAI-compatible provider profiles.
#[derive(Debug, Parser)]
pub struct ConfigCli {
    #[clap(subcommand)]
    pub command: ConfigCommand,
}

/// The available `config` subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// List the configured provider profiles.
    List {
        /// Emit JSON instead of a table.
        #[clap(long)]
        json: bool,
    },

    /// Show one provider profile.
    Show {
        /// Id of the profile.
        id: String,

        /// Emit JSON instead of text.
        #[clap(long)]
        json: bool,
    },

    /// Add a new provider profile.
    Add {
        /// Id of the profile. Generated when omitted.
        #[clap(long)]
        id: Option<String>,

        /// Display name shown in the configuration center.
        #[clap(long)]
        name: Option<String>,

        /// Base URL of the OpenAI-compatible endpoint.
        #[clap(long)]
        base_url: String,

        /// Wire protocol: `chat`, `responses`, or `responses_websocket`.
        #[clap(long, default_value = "chat")]
        wire_api: String,

        /// Model to request from this endpoint.
        #[clap(long)]
        model: String,

        /// Provider id recorded on the profile.
        #[clap(long)]
        provider_id: Option<String>,

        /// Read the API key from standard input.
        #[clap(long)]
        api_key_stdin: bool,

        /// Activate the profile after creating it.
        #[clap(long)]
        activate: bool,
    },

    /// Update an existing provider profile.
    Update {
        /// Id of the profile.
        id: String,

        /// New display name.
        #[clap(long)]
        name: Option<String>,

        /// New base URL.
        #[clap(long)]
        base_url: Option<String>,

        /// New wire protocol.
        #[clap(long)]
        wire_api: Option<String>,

        /// New model.
        #[clap(long)]
        model: Option<String>,

        /// New provider id.
        #[clap(long)]
        provider_id: Option<String>,

        /// Read a replacement API key from standard input.
        #[clap(long)]
        api_key_stdin: bool,

        /// Remove the stored API key.
        #[clap(long)]
        clear_api_key: bool,
    },

    /// Remove a provider profile.
    Remove {
        /// Id of the profile.
        id: String,

        /// Activate this profile after the removal.
        #[clap(long)]
        replacement: Option<String>,
    },

    /// Activate a provider profile.
    Use {
        /// Id of the profile.
        id: String,
    },

    /// Stop using the global provider profile and return to config/dotenv fallback.
    Deactivate,

    /// Import one compatible-provider .env file into the global profile store.
    ImportDotenv {
        /// Dotenv file to import. Defaults to .env in the current directory.
        #[clap(long)]
        path: Option<PathBuf>,

        /// Id for the imported profile. Generated when omitted.
        #[clap(long)]
        id: Option<String>,

        /// Display name for the imported profile.
        #[clap(long)]
        name: Option<String>,

        /// Required acknowledgement before importing an API key into global credential storage.
        #[clap(long)]
        confirm: bool,
    },

    /// Show the effective provider configuration.
    Status {
        /// Emit JSON instead of text.
        #[clap(long)]
        json: bool,
    },
}

/// Runs a `config` subcommand.
pub fn run_config_cli(code_home: PathBuf, cli: ConfigCli) -> std::io::Result<()> {
    let repository = ProviderProfileRepository::new(&code_home);
    let secret_store = ProviderSecretStore::new(&code_home);

    match cli.command {
        ConfigCommand::List { json } => list_profiles(&repository, &secret_store, json),
        ConfigCommand::Show { id, json } => show_profile(&repository, &secret_store, &id, json),
        ConfigCommand::Add {
            id,
            name,
            base_url,
            wire_api,
            model,
            provider_id,
            api_key_stdin,
            activate,
        } => add_profile(
            &repository,
            &secret_store,
            id,
            name,
            base_url,
            wire_api,
            model,
            provider_id,
            api_key_stdin,
            activate,
        ),
        ConfigCommand::Update {
            id,
            name,
            base_url,
            wire_api,
            model,
            provider_id,
            api_key_stdin,
            clear_api_key,
        } => update_profile(
            &repository,
            &secret_store,
            &id,
            name,
            base_url,
            wire_api,
            model,
            provider_id,
            api_key_stdin,
            clear_api_key,
        ),
        ConfigCommand::Remove { id, replacement } => {
            remove_profile(&repository, &secret_store, &id, replacement.as_deref())
        }
        ConfigCommand::Use { id } => use_profile(&repository, &id),
        ConfigCommand::Deactivate => deactivate_profile(&repository),
        ConfigCommand::ImportDotenv {
            path,
            id,
            name,
            confirm,
        } => import_dotenv_profile(&repository, &secret_store, path, id, name, confirm),
        ConfigCommand::Status { json } => show_status(&repository, &secret_store, json),
    }
}

fn import_dotenv_profile(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    path: Option<PathBuf>,
    id: Option<String>,
    name: Option<String>,
    confirm: bool,
) -> std::io::Result<()> {
    if !confirm {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to import a dotenv file without --confirm",
        ));
    }

    let path = path.unwrap_or_else(|| PathBuf::from(".env"));
    let values = read_dotenv_provider_settings(&path)?;
    let base_url = required_dotenv_value(&values, "OPENAI_BASE_URL", &path)?;
    let model = required_dotenv_value(&values, "OPENAI_MODEL", &path)?;
    let wire_api = values
        .get("OPENAI_WIRE_API")
        .map(String::as_str)
        .unwrap_or("chat");
    let wire_api = parse_wire_api(wire_api).map_err(std::io::Error::other)?;
    let api_key = values
        .get("OPENAI_API_KEY")
        .filter(|value| !value.trim().is_empty())
        .cloned();

    let existing = repository.list().map_err(std::io::Error::other)?;
    let id = id.unwrap_or_else(|| {
        code_core::provider_profiles::unique_profile_id(
            &existing
                .iter()
                .map(|profile| (profile.id.clone(), profile.clone()))
                .collect(),
        )
    });
    if existing.iter().any(|profile| profile.id == id) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("provider profile `{id}` already exists; use `config update` instead"),
        ));
    }
    let display_name = name.unwrap_or_else(|| id.clone());
    let mut profile = ProviderProfile::new(id.clone(), display_name, base_url, wire_api, model);

    if let Some(api_key) = api_key {
        let secret_id = fresh_secret_id(&id);
        let secret_ref = secret_store
            .store_secret(&secret_id, &api_key)
            .map_err(std::io::Error::other)?;
        profile.credential = Some(secret_ref);
    }

    let stored_secret = profile.credential.clone();
    let stored = match repository.add_and_activate(profile, None) {
        Ok(stored) => stored,
        Err(err) => {
            if let Some(secret_ref) = stored_secret.as_ref() {
                let _ = secret_store.delete_secret(secret_ref);
            }
            return Err(std::io::Error::other(err));
        }
    };

    println!(
        "Imported and activated provider profile `{}` ({}; {}).",
        stored.id,
        stored.model,
        stored.wire_api_label()
    );
    Ok(())
}

fn read_dotenv_provider_settings(path: &std::path::Path) -> std::io::Result<BTreeMap<String, String>> {
    let values = dotenvy::from_path_iter(path)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
        .into_iter()
        .filter(|(key, _)| {
            matches!(
                key.as_str(),
                "OPENAI_BASE_URL" | "OPENAI_MODEL" | "OPENAI_WIRE_API" | "OPENAI_API_KEY"
            )
        })
        .collect();
    Ok(values)
}

fn required_dotenv_value(
    values: &BTreeMap<String, String>,
    key: &str,
    path: &std::path::Path,
) -> std::io::Result<String> {
    values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} must contain a non-empty {key}", path.display()),
            )
        })
}

fn fresh_secret_id(profile_id: &str) -> String {
    format!("{profile_id}-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_import_requires_the_provider_endpoint_and_model() {
        let path = tempfile::NamedTempFile::new().expect("dotenv file");
        std::fs::write(path.path(), "OPENAI_BASE_URL=https://example.com/v1\n")
            .expect("write dotenv");

        let values = read_dotenv_provider_settings(path.path()).expect("read dotenv");
        assert_eq!(
            required_dotenv_value(&values, "OPENAI_BASE_URL", path.path()).expect("base URL"),
            "https://example.com/v1"
        );
        assert!(required_dotenv_value(&values, "OPENAI_MODEL", path.path()).is_err());
    }

    #[test]
    fn dotenv_import_ignores_unrelated_values() {
        let path = tempfile::NamedTempFile::new().expect("dotenv file");
        std::fs::write(
            path.path(),
            "OPENAI_BASE_URL=https://example.com/v1\nOPENAI_MODEL=m\nSECRET=value\n",
        )
        .expect("write dotenv");

        let values = read_dotenv_provider_settings(path.path()).expect("read dotenv");
        assert_eq!(values.len(), 2);
        assert!(!values.contains_key("SECRET"));
    }

    #[test]
    fn dotenv_import_rejects_invalid_dotenv_syntax() {
        let path = tempfile::NamedTempFile::new().expect("dotenv file");
        std::fs::write(path.path(), "OPENAI_BASE_URL=https://example.com/v1\n=invalid\n")
            .expect("write dotenv");

        assert!(read_dotenv_provider_settings(path.path()).is_err());
    }

    #[test]
    fn dotenv_import_rejects_an_existing_profile_before_writing_a_secret() {
        let home = tempfile::tempdir().expect("profile home");
        let repository = ProviderProfileRepository::new(home.path());
        repository
            .add(
                ProviderProfile::new(
                    "existing",
                    "Existing",
                    "https://existing.example/v1",
                    code_core::WireApi::Chat,
                    "existing-model",
                ),
                None,
            )
            .expect("existing profile");
        let dotenv = tempfile::NamedTempFile::new().expect("dotenv file");
        std::fs::write(
            dotenv.path(),
            "OPENAI_BASE_URL=https://import.example/v1\nOPENAI_MODEL=import-model\nOPENAI_API_KEY=test-key\n",
        )
        .expect("write dotenv");
        let secret_store = ProviderSecretStore::new(home.path());

        let error = import_dotenv_profile(
            &repository,
            &secret_store,
            Some(dotenv.path().to_path_buf()),
            Some("existing".to_string()),
            None,
            true,
        )
        .expect_err("duplicate id must fail");

        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(
            repository
                .get("existing")
                .expect("read profile")
                .expect("profile")
                .model,
            "existing-model"
        );
    }

    #[test]
    fn status_labels_an_explicit_project_override() {
        let status = ProviderStatus::new(None, None, 1, true, Some("/tmp/home".to_string()));

        assert_eq!(status.source.as_deref(), Some("config-file"));
        assert_eq!(status.profile_state, "project-override");
        assert_eq!(status.config_home.as_deref(), Some("/tmp/home"));
    }

    #[test]
    fn fresh_secret_ids_do_not_reuse_a_profile_namespace() {
        let first = fresh_secret_id("profile");
        let second = fresh_secret_id("profile");

        assert_ne!(first, "profile");
        assert_ne!(first, second);
    }
}

fn list_profiles(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    json: bool,
) -> std::io::Result<()> {
    let profiles = repository.list().map_err(std::io::Error::other)?;
    let active_id = repository.active_profile_id().map_err(std::io::Error::other)?;

    if json {
        let state = ProviderStoreState::new(&profiles, active_id.as_deref(), secret_store)
            .map_err(std::io::Error::other)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&state).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    if profiles.is_empty() {
        println!("No provider profiles configured.");
        println!("Run `code config add --base-url <url> --model <model>` to create one.");
        return Ok(());
    }

    println!("{:<24} {:<20} {:<12} {}", "ID", "MODEL", "WIRE API", "KEY");
    for profile in &profiles {
        let marker = if active_id.as_deref() == Some(profile.id.as_str()) {
            "*"
        } else {
            " "
        };
        let has_key = profile
            .credential
            .as_ref()
            .map(|secret_ref| secret_store.has_secret(secret_ref))
            .transpose()
            .map_err(std::io::Error::other)?
            .unwrap_or(false);
        println!(
            "{marker}{:<24} {:<20} {:<12} {}",
            profile.id,
            profile.model,
            profile.wire_api_label(),
            if has_key { "set" } else { "-" }
        );
    }
    Ok(())
}

fn show_profile(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    id: &str,
    json: bool,
) -> std::io::Result<()> {
    let profile = repository
        .get(id)
        .map_err(std::io::Error::other)?
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("profile `{id}` does not exist"),
            )
        })?;
    let active_id = repository.active_profile_id().map_err(std::io::Error::other)?;
    let has_key = profile
        .credential
        .as_ref()
        .map(|secret_ref| secret_store.has_secret(secret_ref))
        .transpose()
        .map_err(std::io::Error::other)?
        .unwrap_or(false);

    if json {
        let mut summary = profile.to_summary(has_key);
        summary.active = active_id.as_deref() == Some(id);
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    println!("Id:        {}", profile.id);
    println!("Name:      {}", profile.display_name);
    println!("Base URL:  {}", profile.base_url);
    println!("Wire API:  {}", profile.wire_api_label());
            println!("Model:     {}", profile.model);
            if !code_core::provider_profiles::model_metadata_is_known(&profile.model) {
                println!(
                    "           (model metadata is unknown; context window and truncation limits fall back to defaults)"
                );
            }
    if let Some(provider_id) = profile.provider_id.as_ref() {
        println!("Provider:  {provider_id}");
    }
    println!("API key:   {}", if has_key { "set" } else { "not set" });
    println!("Revision:  {}", profile.revision);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_profile(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    id: Option<String>,
    name: Option<String>,
    base_url: String,
    wire_api: String,
    model: String,
    provider_id: Option<String>,
    api_key_stdin: bool,
    activate: bool,
) -> std::io::Result<()> {
    let wire_api = parse_wire_api(&wire_api).map_err(std::io::Error::other)?;
    let existing = repository.list().map_err(std::io::Error::other)?;
    let id = id.unwrap_or_else(|| {
        code_core::provider_profiles::unique_profile_id(
            &existing
                .iter()
                .map(|profile| (profile.id.clone(), profile.clone()))
                .collect(),
        )
    });
    let display_name = name.unwrap_or_else(|| id.clone());

    let mut profile = ProviderProfile::new(id.clone(), display_name, base_url, wire_api, model);
    profile.provider_id = provider_id;

    if api_key_stdin {
        let secret = read_secret_from_stdin()?;
        let secret_id = fresh_secret_id(&id);
        let secret_ref = secret_store
            .store_secret(&secret_id, &secret)
            .map_err(std::io::Error::other)?;
        profile.credential = Some(secret_ref);
    }

    let stored_secret = profile.credential.clone();
    let stored = match if activate {
        repository.add_and_activate(profile, None)
    } else {
        repository.add(profile, None)
    } {
        Ok(stored) => stored,
        Err(err) => {
            // The profile was not created, so the secret that was just
            // stored would be an orphan. Remove it.
            if let Some(secret_ref) = stored_secret.as_ref() {
                let _ = secret_store.delete_secret(secret_ref);
            }
            return Err(std::io::Error::other(err));
        }
    };

    println!("Created provider profile `{}`.", stored.id);
    if activate {
        println!("Activated provider profile `{}`.", stored.id);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn update_profile(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    id: &str,
    name: Option<String>,
    base_url: Option<String>,
    wire_api: Option<String>,
    model: Option<String>,
    provider_id: Option<String>,
    api_key_stdin: bool,
    clear_api_key: bool,
) -> std::io::Result<()> {
    let mut profile = repository
        .get(id)
        .map_err(std::io::Error::other)?
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("profile `{id}` does not exist"),
            )
        })?;

    if let Some(name) = name {
        profile.display_name = name;
    }
    if let Some(base_url) = base_url {
        profile.base_url = base_url;
    }
    if let Some(wire_api) = wire_api {
        profile.wire_api = parse_wire_api(&wire_api).map_err(std::io::Error::other)?;
    }
    if let Some(model) = model {
        profile.model = model;
    }
    if let Some(provider_id) = provider_id {
        profile.provider_id = Some(provider_id);
    }
    let previous_secret = profile.credential.clone();
    let mut replacement_secret = None;
    if clear_api_key {
        profile.credential = None;
    }
    if api_key_stdin {
        let secret = read_secret_from_stdin()?;
        let secret_id = fresh_secret_id(id);
        let secret_ref = secret_store
            .store_secret(&secret_id, &secret)
            .map_err(std::io::Error::other)?;
        replacement_secret = Some(secret_ref.clone());
        profile.credential = Some(secret_ref);
    }

    profile.revision += 1;
    profile.updated_at = code_core::provider_profiles::now_secs();

    if let Err(err) = repository.update(profile, None) {
        if let Some(secret_ref) = replacement_secret.as_ref() {
            let _ = secret_store.delete_secret(secret_ref);
        }
        return Err(std::io::Error::other(err));
    }
    if (clear_api_key || api_key_stdin)
        && let Some(previous_secret) = previous_secret.as_ref()
        && Some(previous_secret) != replacement_secret.as_ref()
    {
            let _ = secret_store.delete_secret(previous_secret);
    }
    println!("Updated provider profile `{id}`.");
    Ok(())
}

fn remove_profile(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    id: &str,
    replacement: Option<&str>,
) -> std::io::Result<()> {
    let profile = repository
        .get(id)
        .map_err(std::io::Error::other)?
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("profile `{id}` does not exist"),
            )
        })?;

    if let Some(secret_ref) = profile.credential.as_ref() {
        let _ = secret_store.delete_secret(secret_ref);
    }

    let removed = repository
        .remove(id, replacement, None)
        .map_err(std::io::Error::other)?;
    if removed {
        println!("Removed provider profile `{id}`.");
    }
    Ok(())
}

fn use_profile(repository: &ProviderProfileRepository, id: &str) -> std::io::Result<()> {
    let profile = repository
        .activate(id, None)
        .map_err(std::io::Error::other)?;
    println!("Activated provider profile `{}` ({}).", profile.id, profile.model);
    Ok(())
}

fn deactivate_profile(repository: &ProviderProfileRepository) -> std::io::Result<()> {
    repository.deactivate(None).map_err(std::io::Error::other)?;
    println!("Deactivated the global provider profile.");
    Ok(())
}

fn show_status(
    repository: &ProviderProfileRepository,
    secret_store: &ProviderSecretStore,
    json: bool,
) -> std::io::Result<()> {
    let resolver = ProviderProfileResolver::with_repository(repository.clone());
    let project_provider_override =
        code_core::provider_profiles::project_provider_override_requested();
    let resolved = resolver
        .resolve(&code_core::provider_profiles::ProviderResolutionInputs {
            project_provider_override,
            ..Default::default()
        })
        .map_err(std::io::Error::other)?;

    let active_id = repository.active_profile_id().map_err(std::io::Error::other)?;
    let profile_count = repository.list().map_err(std::io::Error::other)?.len();
    let config_home = repository
        .path()
        .parent()
        .map(|path| path.display().to_string());

    if json {
        let status = ProviderStatus::new(
            active_id.as_deref(),
            resolved.as_ref(),
            profile_count,
            project_provider_override,
            config_home.clone(),
        );
        println!(
            "{}",
            serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".to_string())
        );
        return Ok(());
    }

    match resolved {
        Some(resolved) => {
            let profile = resolved.profile.as_ref();
            let has_key = profile
                .and_then(|profile| profile.credential.as_ref())
                .map(|secret_ref| secret_store.has_secret(secret_ref))
                .transpose()
                .map_err(std::io::Error::other)?
                .unwrap_or(false);
            println!("Active profile:  {}", active_id.unwrap_or_else(|| "-".to_string()));
            println!("Provider:       {}", resolved.provider_id);
            println!("Base URL:       {}", resolved.provider.base_url.as_deref().unwrap_or("-"));
            println!("Wire API:       {}", resolved.provider.wire_api_label());
            println!("Model:          {}", resolved.model);
            if !code_core::provider_profiles::model_metadata_is_known(&resolved.model) {
                println!(
                    "               (model metadata is unknown; context window and truncation limits fall back to defaults)"
                );
            }
            println!("API key:        {}", if has_key { "set" } else { "not set" });
            println!("Revision:       {}", profile.map(|p| p.revision).unwrap_or_default());
            println!("Source:         {}", resolved.source.label());
            if let Some(config_home) = config_home.as_deref() {
                println!("Config home:    {config_home}");
            }
        }
        None => {
            println!("Active profile:  none");
            if project_provider_override {
                println!("Provider profiles: project override requested");
            } else if profile_count == 0 {
                println!("Provider profiles: none configured");
            } else {
                println!("Provider profiles: {profile_count} saved, none activated");
            }
            println!("Provider:       (from config.toml or dotenv fallback)");
            println!("Source:         config-file");
            if let Some(config_home) = config_home.as_deref() {
                println!("Config home:    {config_home}");
            }
        }
    }
    Ok(())
}

/// Reads a secret from standard input.
///
/// The value is trimmed of a trailing newline and never echoed.
pub fn read_secret_from_stdin() -> std::io::Result<String> {
    if std::io::stdin().is_terminal() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "reading an API key from standard input requires a pipe; \
             for example: printf '%s' \"$KEY\" | code config add --api-key-stdin ...",
        ));
    }
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    let trimmed = buffer.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the API key read from standard input was empty",
        ));
    }
    Ok(trimmed.to_string())
}

/// Redacted view of the effective provider configuration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderStatus {
    /// Id of the active profile.
    pub active_profile_id: Option<String>,
    /// Provider id in effect.
    pub provider_id: Option<String>,
    /// Base URL in effect.
    pub base_url: Option<String>,
    /// Wire protocol in effect.
    pub wire_api: Option<String>,
    /// Model in effect.
    pub model: Option<String>,
    /// Whether Codex knows the model's context window and limits.
    pub model_metadata_known: Option<bool>,
    /// Where the settings came from.
    pub source: Option<String>,
    /// Number of saved global profiles.
    pub profile_count: usize,
    /// Whether the global profile store has an active selection.
    pub profile_state: String,
    /// Directory containing the portable profile store.
    pub config_home: Option<String>,
}

impl ProviderStatus {
    /// Builds a redacted status view.
    pub fn new(
        active_profile_id: Option<&str>,
        resolved: Option<&code_core::provider_profiles::ResolvedProvider>,
        profile_count: usize,
        project_provider_override: bool,
        config_home: Option<String>,
    ) -> Self {
        let profile_state = match (project_provider_override, profile_count, active_profile_id) {
            (true, _, _) => "project-override",
            (false, 0, _) => "no-profiles",
            (false, _, Some(_)) => "active-profile",
            _ => "profiles-not-activated",
        }
        .to_string();
        match resolved {
            Some(resolved) => Self {
                active_profile_id: active_profile_id.map(str::to_string),
                provider_id: Some(resolved.provider_id.clone()),
                base_url: resolved.provider.base_url.clone(),
                wire_api: Some(resolved.provider.wire_api_label().to_string()),
                model: Some(resolved.model.clone()),
                model_metadata_known: Some(
                    code_core::provider_profiles::model_metadata_is_known(&resolved.model),
                ),
                source: Some(resolved.source.label().to_string()),
                profile_count,
                profile_state,
                config_home,
            },
            None => Self {
                active_profile_id: active_profile_id.map(str::to_string),
                provider_id: None,
                base_url: None,
                wire_api: None,
                model: None,
                model_metadata_known: None,
                source: Some("config-file".to_string()),
                profile_count,
                profile_state,
                config_home,
            },
        }
    }
}

/// Options for `config-start`.
#[derive(Debug, Parser)]
pub struct ConfigStartArgs {
    /// Do not open a browser window.
    #[clap(long)]
    pub no_browser: bool,

    /// Emit JSON describing the started server.
    #[clap(long)]
    pub json: bool,

    /// How long the server stays alive without activity.
    #[clap(long, value_name = "SECONDS", default_value_t = 900)]
    pub idle_timeout: u64,
}

/// Options for `config-close`.
#[derive(Debug, Parser)]
pub struct ConfigCloseArgs {
    /// Emit JSON describing the result.
    #[clap(long)]
    pub json: bool,
}

/// Starts the configuration center and opens it in a browser.
///
/// The server runs in this process, so the command stays in the foreground
/// until the user closes it or it goes idle.
pub fn run_config_start(code_home: PathBuf, args: ConfigStartArgs) -> std::io::Result<()> {
    let center = code_core::provider_profiles::config_center::ConfigCenter::start(
        &code_home,
        std::time::Duration::from_secs(args.idle_timeout),
    )
    .map_err(std::io::Error::other)?;

    let state_path = code_core::provider_profiles::config_center::ConfigCenterStatePath::new(
        &code_home,
    );
    let state = code_core::provider_profiles::config_center::ConfigCenterState::new(
        std::process::id(),
        center.port(),
        center.bearer_token().to_string(),
        random_instance_id(),
    );
    state_path.write(&state).map_err(std::io::Error::other)?;

    let url = center.url();
    if args.json {
        let payload = serde_json::json!({
            "url": url,
            "port": center.port(),
            "pid": std::process::id(),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
        );
    } else {
        println!("Provider configuration center is running at {url}");
        println!("Press Ctrl-C to stop it.");
    }

    if !args.no_browser && std::io::stdout().is_terminal() {
        let _ = open_browser(&url);
    }

    let result = center.serve_until_idle().map_err(std::io::Error::other);
    let _ = state_path.remove();
    result
}

/// Stops a running configuration center.
pub fn run_config_close(code_home: PathBuf, args: ConfigCloseArgs) -> std::io::Result<()> {
    let state_path = code_core::provider_profiles::config_center::ConfigCenterStatePath::new(
        &code_home,
    );
    let instance =
        code_core::provider_profiles::config_center::ServerInstance::load(&code_home)
            .map_err(std::io::Error::other)?;

    let Some(instance) = instance else {
        if args.json {
            println!("{}", serde_json::json!({ "stopped": false, "reason": "not-running" }));
        } else {
            println!("No provider configuration center is running.");
        }
        return Ok(());
    };

    if !instance.owner_alive {
        // The owner is gone; discard the stale state file.
        let _ = state_path.remove();
        if args.json {
            println!("{}", serde_json::json!({ "stopped": false, "reason": "stale" }));
        } else {
            println!("Removed a stale configuration center state file.");
        }
        return Ok(());
    }

    let stopped = request_shutdown(instance.state.port, &instance.state.bearer_token)?;
    let _ = state_path.remove();

    if args.json {
        println!(
            "{}",
            serde_json::json!({ "stopped": stopped, "pid": instance.state.pid })
        );
    } else if stopped {
        println!("Stopped the provider configuration center.");
    } else {
        println!("The provider configuration center did not respond to the shutdown request.");
    }
    Ok(())
}

/// Asks a running configuration center to shut down.
fn request_shutdown(port: u16, bearer_token: &str) -> std::io::Result<bool> {
    use std::io::Write;
    use std::net::TcpStream;

    let address = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&address)?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(2)))?;
    let request = format!(
        "POST /api/shutdown HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {bearer_token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    let mut buffer = [0u8; 64];
    let _ = std::io::Read::read(&mut stream, &mut buffer);
    Ok(true)
}

/// Opens the given URL in the default browser.
fn open_browser(url: &str) -> std::io::Result<()> {
    let command = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(command)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
}

/// Generates a random instance id.
fn random_instance_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 8];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
