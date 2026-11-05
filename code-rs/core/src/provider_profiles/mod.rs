//! Versioned, user-managed OpenAI-compatible provider profiles.
//!
//! A *provider profile* describes one OpenAI-compatible endpoint: where it
//! lives, which wire protocol it speaks, which model to use, and how to
//! authenticate against it. Profiles are stored outside `config.toml` so
//! that editing them never rewrites unrelated user settings.
//!
//! Secrets never live in this module. A profile only carries a
//! [`ProviderSecretRef`], which points at an entry in the credential store.

mod crypto;
pub mod config_center;
#[cfg(test)]
mod integration_tests;
mod repository;
mod resolver;
mod secret_store;
mod watcher;

pub use repository::ProviderProfileRepository;
pub use repository::ProviderProfileStoreError;
pub use repository::now_secs;
pub use repository::unique_profile_id;
pub use resolver::ProviderProfileResolver;
pub use resolver::ProviderResolutionError;
pub use resolver::ProviderResolutionInputs;
pub use resolver::ProviderSelectionSource;
pub use resolver::PROJECT_PROVIDER_OVERRIDE_ENV_VAR;
pub use resolver::project_provider_override_requested;
pub use resolver::ResolvedProvider;
pub use resolver::parse_wire_api;
pub use secret_store::ProviderSecretRef;
pub use secret_store::ProviderSecretStore;
pub use secret_store::ProviderSecretStoreError;
pub use secret_store::KeyringStore;
pub use watcher::DEFAULT_POLL_INTERVAL;
pub use watcher::ProfileWatchEvent;
pub use watcher::ProviderProfileWatcher;
pub use watcher::SharedProviderProfileWatcher;

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::RwLock;

use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::WireApi;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::SystemTime;

/// Schema version for the persisted profile document.
pub const PROFILE_STORE_VERSION: u32 = 1;

/// Identifier of a single managed provider profile.
pub type ProviderProfileId = String;

/// A user-managed OpenAI-compatible provider profile.
///
/// Only non-secret material is stored here. The API key (if any) lives in
/// the credential store and is addressed by [`Self::credential`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderProfile {
    /// Stable identifier, unique within the store.
    pub id: ProviderProfileId,

    /// Human-facing name shown in the configuration center.
    pub display_name: String,

    /// Base URL of the OpenAI-compatible endpoint.
    pub base_url: String,

    /// Wire protocol the endpoint speaks.
    pub wire_api: WireApi,

    /// Model to request from this endpoint.
    pub model: String,

    /// Reference to the credential used for this profile, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<ProviderSecretRef>,

    /// Optional provider id mapping for this profile.
    ///
    /// Some Codex subsystems (for example built-in agent defaults) key
    /// behavior off a provider id. Keeping the id on the profile lets those
    /// subsystems make the same decision they would have made for an
    /// equivalent `config.toml` provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,

    /// Optional non-secret provider settings.
    ///
    /// Kept as a free-form map so new `ModelProviderInfo` fields can be
    /// surfaced without changing this schema.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub provider_options: BTreeMap<String, String>,

    /// Monotonic revision, bumped on every mutation.
    pub revision: u64,

    /// Creation timestamp, seconds since the Unix epoch.
    pub created_at: u64,

    /// Last update timestamp, seconds since the Unix epoch.
    pub updated_at: u64,
}

impl ProviderProfile {
    /// Creates a new profile with a fresh revision and timestamps.
    pub fn new(
        id: impl Into<ProviderProfileId>,
        display_name: impl Into<String>,
        base_url: impl Into<String>,
        wire_api: WireApi,
        model: impl Into<String>,
    ) -> Self {
        let now = unix_now();
        Self {
            id: id.into(),
            display_name: display_name.into(),
            base_url: base_url.into(),
            wire_api,
            model: model.into(),
            credential: None,
            provider_id: None,
            provider_options: BTreeMap::new(),
            revision: 1,
            created_at: now,
            updated_at: now,
        }
    }

    /// Validates the profile and returns a human-readable error, if any.
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() {
            return Err("profile id must not be empty".to_string());
        }
        if self.display_name.trim().is_empty() {
            return Err("profile display name must not be empty".to_string());
        }
        if self.model.trim().is_empty() {
            return Err("profile model must not be empty".to_string());
        }
        validate_base_url(&self.base_url)?;
        Ok(())
    }

    /// Builds the provider description used to configure a session.
    ///
    /// The returned provider carries the profile's credential reference so
    /// the secret is resolved from the credential store at request time
    /// rather than being materialized into `config.toml`.
    pub fn to_model_provider_info(&self) -> ModelProviderInfo {
        let mut provider = ModelProviderInfo {
            name: self.display_name.clone(),
            base_url: Some(self.base_url.clone()),
            env_key: None,
            env_key_instructions: None,
            experimental_bearer_token: None,
            auth: None,
            wire_api: self.wire_api,
            query_params: None,
            http_headers: None,
            env_http_headers: None,
            request_max_retries: None,
            stream_max_retries: None,
            stream_idle_timeout_ms: None,
            websocket_connect_timeout_ms: None,
            supports_websockets: self.wire_api == WireApi::ResponsesWebsocket,
            requires_openai_auth: false,
            openrouter: None,
            secret_ref: None,
        };
        provider.secret_ref = self.credential.clone();
        provider
    }
}

/// Validates a provider base URL.
///
/// Only `http` and `https` are accepted; anything else would produce a
/// confusing failure much later, at request time.
pub fn validate_base_url(base_url: &str) -> Result<(), String> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err("base URL must not be empty".to_string());
    }
    let parsed =
        url::Url::parse(trimmed).map_err(|err| format!("base URL is not a valid URL: {err}"))?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(format!(
                "base URL must use http or https, got `{other}`"
            ));
        }
    }
    if parsed.host_str().is_none() {
        return Err("base URL must include a host".to_string());
    }
    Ok(())
}

/// Current wall-clock time as seconds since the Unix epoch.
pub(crate) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// The persisted profile document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderProfileDocument {
    /// Schema version of this document.
    pub version: u32,

    /// Id of the active profile, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile_id: Option<ProviderProfileId>,

    /// All known profiles, keyed by id.
    #[serde(default)]
    pub profiles: BTreeMap<ProviderProfileId, ProviderProfile>,
}

impl Default for ProviderProfileDocument {
    fn default() -> Self {
        Self {
            version: PROFILE_STORE_VERSION,
            active_profile_id: None,
            profiles: BTreeMap::new(),
        }
    }
}

/// Returns the on-disk path of the provider profile document.
pub fn provider_profiles_path(code_home: &std::path::Path) -> PathBuf {
    code_home.join("provider_profiles.json")
}

/// Returns a human-readable label for a wire API.
pub fn wire_api_label(wire_api: WireApi) -> &'static str {
    match wire_api {
        WireApi::Chat => "chat",
        WireApi::Responses => "responses",
        WireApi::ResponsesWebsocket => "responses_websocket",
    }
}

/// A redacted view of the profile store, safe to serialize for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderStoreState {
    /// Id of the active profile, if any.
    pub active_profile_id: Option<ProviderProfileId>,
    /// Redacted summaries of every profile.
    pub profiles: Vec<ProviderProfileSummary>,
}

/// A redacted summary of one profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderProfileSummary {
    /// Id of the profile.
    pub id: ProviderProfileId,
    /// Display name.
    pub display_name: String,
    /// Base URL.
    pub base_url: String,
    /// Wire protocol.
    pub wire_api: String,
    /// Model.
    pub model: String,
    /// Provider id, if any.
    pub provider_id: Option<String>,
    /// Whether the profile is active.
    pub active: bool,
    /// Whether a credential is stored for the profile.
    pub has_api_key: bool,
    /// Whether Codex knows the model's context window and limits.
    ///
    /// A model name that is not in the local catalog falls back to
    /// conservative defaults, which can truncate long tool output.
    pub model_metadata_known: bool,
    /// Revision of the profile.
    pub revision: u64,
}

impl ProviderProfile {
    /// Returns the human-readable label of the profile's wire API.
    pub fn wire_api_label(&self) -> &'static str {
        wire_api_label(self.wire_api)
    }

    /// Builds a redacted summary of this profile.
    pub fn to_summary(&self, has_api_key: bool) -> ProviderProfileSummary {
        ProviderProfileSummary {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            base_url: self.base_url.clone(),
            wire_api: self.wire_api_label().to_string(),
            model: self.model.clone(),
            provider_id: self.provider_id.clone(),
            active: false,
            has_api_key,
            model_metadata_known: model_metadata_is_known(&self.model),
            revision: self.revision,
        }
    }
}

/// Reports whether Codex knows the model's metadata.
///
/// An unknown model name means the context window, output limits, and
/// truncation policy fall back to conservative defaults.
pub fn model_metadata_is_known(model: &str) -> bool {
    crate::model_family::find_family_for_model(model).is_some()
}

impl ProviderStoreState {
    /// Builds a redacted view of the store.
    pub fn new(
        profiles: &[ProviderProfile],
        active_profile_id: Option<&str>,
        secret_store: &ProviderSecretStore,
    ) -> Result<Self, ProviderSecretStoreError> {
        let mut summaries = Vec::with_capacity(profiles.len());
        for profile in profiles {
            let has_api_key = match profile.credential.as_ref() {
                Some(secret_ref) => secret_store.has_secret(secret_ref)?,
                None => false,
            };
            let mut summary = profile.to_summary(has_api_key);
            summary.active = active_profile_id == Some(profile.id.as_str());
            summaries.push(summary);
        }
        Ok(Self {
            active_profile_id: active_profile_id.map(str::to_string),
            profiles: summaries,
        })
    }
}

/// Writes `contents` to `path` atomically, with owner-only permissions.
///
/// The file is written to a temporary sibling and then renamed, so a crash
/// mid-write can never leave a partially written file behind.
pub(crate) fn write_private_file(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent: {}", path.display()),
        )
    })?;
    std::fs::create_dir_all(parent)?;

    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(contents)?;
    file.flush()?;
    file.as_file_mut().sync_all()?;
    // `NamedTempFile::persist` refuses to overwrite an existing file, so
    // replace the target explicitly and persist into its place. The
    // replacement is a rename, which is atomic on the same filesystem.
    let temp_path = file.path().to_path_buf();
    if path.exists() {
        std::fs::rename(&temp_path, path)?;
    } else {
        file.persist(path).map_err(|err| err.error)?;
    }
    restrict_permissions(path)?;
    Ok(())
}

/// Restricts a file to owner-only access where the platform supports it.
fn restrict_permissions(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// Process-wide registry of the active secret store.
///
/// [`crate::model_provider_info::ModelProviderInfo::effective_auth`] resolves
/// a secret reference without access to a session handle, so the store is
/// registered here when a configuration is loaded. Registration is
/// idempotent and only ever points at the code home the process was started
/// with.
type SecretStoreRegistry = Vec<(PathBuf, Arc<ProviderSecretStore>)>;

static SECRET_STORES: OnceLock<RwLock<SecretStoreRegistry>> = OnceLock::new();

fn secret_store_slot() -> &'static RwLock<SecretStoreRegistry> {
    SECRET_STORES.get_or_init(|| RwLock::new(Vec::new()))
}

/// Registers the secret store for this process.
///
/// One store is kept per code home, so a process that serves several code
/// homes resolves each profile's secret from the right store.
pub fn register_secret_store(code_home: &std::path::Path, store: Arc<ProviderSecretStore>) {
    let mut slot = secret_store_slot()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let key = code_home.to_path_buf();
    if let Some(existing) = slot.iter_mut().find(|(path, _)| *path == key) {
        existing.1 = store;
        return;
    }
    slot.push((key, store));
}

/// Removes the registered secret store.
pub fn clear_secret_store(code_home: &std::path::Path) {
    let mut slot = secret_store_slot()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    slot.retain(|(path, _)| path != code_home);
}

/// Returns the secret store for `code_home`, if any.
pub fn registered_secret_store(code_home: &std::path::Path) -> Option<Arc<ProviderSecretStore>> {
    secret_store_slot()
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .find(|(path, _)| path == code_home)
        .map(|(_, store)| Arc::clone(store))
}

/// Resolves the secret addressed by `secret_ref`.
///
/// Fails closed: when no secret store is registered, or the referenced
/// secret is missing, the caller receives an error rather than an
/// unauthenticated request.
pub fn resolve_secret(secret_ref: &ProviderSecretRef) -> crate::error::Result<String> {
    let store = match secret_ref.code_home.as_ref() {
        Some(code_home) => registered_secret_store(code_home),
        None => registered_secret_store(std::path::Path::new("")).or_else(|| {
            // A reference without a code home predates multi-home support.
            // Fall back to the only registered store, if there is exactly one.
            let slot = secret_store_slot()
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if slot.len() == 1 {
                Some(Arc::clone(&slot[0].1))
            } else {
                None
            }
        }),
    }
    .ok_or_else(|| {
        crate::error::CodexErr::UnsupportedOperation(format!(
            "provider secret `{}` cannot be resolved: no credential store is registered",
            secret_ref.profile_id
        ))
    })?;
    let secret = store
        .load_secret(secret_ref)
        .map_err(|err| crate::error::CodexErr::UnsupportedOperation(err.to_string()))?
        .ok_or_else(|| {
            crate::error::CodexErr::UnsupportedOperation(format!(
                "provider secret `{}` is missing from the credential store",
                secret_ref.profile_id
            ))
        })?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_profile_starts_at_revision_one() {
        let profile =
            ProviderProfile::new("id", "Name", "https://example.com/v1", WireApi::Chat, "m");
        assert_eq!(profile.revision, 1);
        assert_eq!(profile.created_at, profile.updated_at);
    }

    #[test]
    fn validate_rejects_empty_required_fields() {
        let mut profile =
            ProviderProfile::new("id", "Name", "https://example.com/v1", WireApi::Chat, "m");
        assert!(profile.validate().is_ok());

        profile.id = String::new();
        assert!(profile.validate().is_err());

        profile.id = "id".to_string();
        profile.model = String::new();
        assert!(profile.validate().is_err());
    }

    #[test]
    fn validate_rejects_non_http_schemes() {
        let profile = ProviderProfile::new("id", "Name", "ftp://example.com", WireApi::Chat, "m");
        assert!(profile.validate().is_err());

        let profile =
            ProviderProfile::new("id", "Name", "https://example.com/v1", WireApi::Chat, "m");
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn to_model_provider_info_carries_secret_ref() {
        let mut profile =
            ProviderProfile::new("id", "Name", "https://example.com/v1", WireApi::Chat, "m");
        profile.credential = Some(ProviderSecretRef::new("id"));

        let provider = profile.to_model_provider_info();
        assert_eq!(provider.secret_ref, profile.credential);
        assert_eq!(provider.base_url.as_deref(), Some("https://example.com/v1"));
        assert!(provider.experimental_bearer_token.is_none());
        assert!(provider.env_key.is_none());
    }
}
