//! Resolves the active provider profile into concrete session inputs.
//!
//! Precedence, from strongest to weakest:
//!
//! 1. Explicit invocation overrides (`--model`, `--profile`, `-c` keys).
//! 2. The active qcodex provider profile.
//! 3. The normal `config.toml` provider and model selection.
//!
//! Combinations that would mix a profile's endpoint with another source's
//! credential or model are rejected with an actionable error instead of
//! being silently merged.

use super::ProviderProfile;
use super::ProviderProfileRepository;
use super::ProviderProfileStoreError;
use crate::model_provider_info::ModelProviderInfo;
use crate::model_provider_info::WireApi;
use std::fmt;

/// Describes where the effective provider settings came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSelectionSource {
    /// An explicit invocation override owns the routing.
    InvocationOverride,
    /// The active qcodex provider profile owns the routing.
    ProviderProfile {
        /// Id of the profile.
        id: String,
        /// Revision of the profile that was applied.
        revision: u64,
    },
    /// The normal `config.toml` selection owns the routing.
    ConfigFile,
}

impl ProviderSelectionSource {
    /// Returns a short, stable label for diagnostics.
    pub fn label(&self) -> &'static str {
        match self {
            Self::InvocationOverride => "invocation-override",
            Self::ProviderProfile { .. } => "provider-profile",
            Self::ConfigFile => "config-file",
        }
    }
}

/// The provider settings a session should use.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    /// The provider description.
    pub provider: ModelProviderInfo,
    /// The model to request.
    pub model: String,
    /// Id of the provider, used by subsystems that key off it.
    pub provider_id: String,
    /// Where these settings came from.
    pub source: ProviderSelectionSource,
    /// The profile that produced these settings, if any.
    pub profile: Option<ProviderProfile>,
}

/// Errors produced while resolving the active profile.
#[derive(Debug)]
pub enum ProviderResolutionError {
    /// The profile store could not be read.
    Store(ProviderProfileStoreError),
    /// The active profile id does not resolve to a profile.
    ActiveProfileMissing(String),
    /// An invocation override conflicts with the active profile.
    Conflict(String),
    /// The profile could not be turned into a provider.
    InvalidProfile(String),
}

impl fmt::Display for ProviderResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(err) => write!(f, "cannot read the provider profile store: {err}"),
            Self::ActiveProfileMissing(id) => write!(
                f,
                "the active provider profile `{id}` no longer exists; select another profile or deactivate it"
            ),
            Self::Conflict(message) => write!(f, "configuration conflict: {message}"),
            Self::InvalidProfile(message) => write!(f, "the active provider profile is invalid: {message}"),
        }
    }
}

impl std::error::Error for ProviderResolutionError {}

impl From<ProviderProfileStoreError> for ProviderResolutionError {
    fn from(err: ProviderProfileStoreError) -> Self {
        Self::Store(err)
    }
}

/// Inputs the resolver needs to detect conflicts.
#[derive(Debug, Clone, Default)]
pub struct ProviderResolutionInputs {
    /// Provider id requested by an invocation override.
    pub override_provider_id: Option<String>,
    /// Model requested by an invocation override.
    pub override_model: Option<String>,
    /// Provider id selected by `config.toml`.
    pub config_provider_id: Option<String>,
    /// Model selected by `config.toml`.
    pub config_model: Option<String>,
}

/// Resolves the active provider profile for a session.
#[derive(Debug)]
pub struct ProviderProfileResolver {
    repository: ProviderProfileRepository,
}

impl ProviderProfileResolver {
    /// Creates a resolver over the profile store in `code_home`.
    pub fn new(code_home: &std::path::Path) -> Self {
        Self {
            repository: ProviderProfileRepository::new(code_home),
        }
    }

    /// Creates a resolver over an existing repository.
    pub fn with_repository(repository: ProviderProfileRepository) -> Self {
        Self { repository }
    }

    /// Returns the underlying repository.
    pub fn repository(&self) -> &ProviderProfileRepository {
        &self.repository
    }

    /// Returns the active profile, if any.
    pub fn active_profile(&self) -> Result<Option<ProviderProfile>, ProviderResolutionError> {
        let document = self.repository.read()?;
        let Some(active_id) = document.active_profile_id.as_ref() else {
            return Ok(None);
        };
        let profile = document.profiles.get(active_id).cloned().ok_or_else(|| {
            ProviderResolutionError::ActiveProfileMissing(active_id.clone())
        })?;
        Ok(Some(profile))
    }

    /// Resolves the provider settings for a session.
    ///
    /// When no profile is active, `Ok(None)` is returned and the caller
    /// keeps its normal `config.toml` behavior.
    pub fn resolve(
        &self,
        inputs: &ProviderResolutionInputs,
    ) -> Result<Option<ResolvedProvider>, ProviderResolutionError> {
        let Some(profile) = self.active_profile()? else {
            return Ok(None);
        };

        profile
            .validate()
            .map_err(ProviderResolutionError::InvalidProfile)?;

        check_for_conflicts(&profile, inputs)?;


        let provider = profile.to_model_provider_info();
        let provider_id = profile
            .provider_id
            .clone()
            .unwrap_or_else(|| format!("qcodex-profile-{}", profile.id));
        let model = profile.model.clone();

        Ok(Some(ResolvedProvider {
            provider,
            model,
            provider_id,
            source: ProviderSelectionSource::ProviderProfile {
                id: profile.id.clone(),
                revision: profile.revision,
            },
            profile: Some(profile),
        }))
    }
}

/// Returns the wire API a profile should use given a textual value.
pub fn parse_wire_api(value: &str) -> Result<WireApi, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok(WireApi::Chat),
        "responses" => Ok(WireApi::Responses),
        "responses_websocket" | "responses-websocket" => Ok(WireApi::ResponsesWebsocket),
        other => Err(format!(
            "unsupported wire API `{other}`; expected one of: chat, responses, responses_websocket"
        )),
    }
}

/// Rejects combinations that would mix a profile with another source.
///
/// The active profile owns its endpoint, its credential, and its model. An
/// invocation override or a `config.toml` selection that points somewhere
/// else cannot be combined with it, because the two sources would disagree
/// about where a request goes and which credential signs it.
fn check_for_conflicts(
    profile: &ProviderProfile,
    inputs: &ProviderResolutionInputs,
) -> Result<(), ProviderResolutionError> {
    let profile_provider_id = profile
        .provider_id
        .clone()
        .unwrap_or_else(|| format!("qcodex-profile-{}", profile.id));

    if let Some(override_provider_id) = inputs.override_provider_id.as_ref()
        && override_provider_id != &profile_provider_id
    {
        return Err(ProviderResolutionError::Conflict(format!(
            "the invocation selects provider `{override_provider_id}` but the active provider profile `{}` selects a different endpoint; deactivate the profile or drop the override",
            profile.id
        )));
    }

    if let Some(config_provider_id) = inputs.config_provider_id.as_ref()
        && config_provider_id != &profile_provider_id
    {
        return Err(ProviderResolutionError::Conflict(format!(
            "config.toml selects provider `{config_provider_id}` but the active provider profile `{}` selects a different endpoint; deactivate the profile or remove the config.toml selection",
            profile.id
        )));
    }

    if let Some(override_model) = inputs.override_model.as_ref()
        && override_model != &profile.model
    {
        return Err(ProviderResolutionError::Conflict(format!(
            "the invocation selects model `{override_model}` but the active provider profile `{}` requests `{}`; deactivate the profile or drop the override",
            profile.id, profile.model
        )));
    }

    if let Some(config_model) = inputs.config_model.as_ref()
        && config_model != &profile.model
    {
        return Err(ProviderResolutionError::Conflict(format!(
            "config.toml selects model `{config_model}` but the active provider profile `{}` requests `{}`; deactivate the profile or remove the config.toml selection",
            profile.id, profile.model
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profiles::ProviderProfile;
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-resolver-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(id: &str) -> ProviderProfile {
        ProviderProfile::new(
            id,
            "Example",
            "https://example.com/v1",
            WireApi::Chat,
            "gpt-test",
        )
    }

    #[test]
    fn no_active_profile_resolves_to_none() {
        let home = TempHome::new("none");
        let resolver = ProviderProfileResolver::new(&home.0);
        let resolved = resolver
            .resolve(&ProviderResolutionInputs::default())
            .expect("resolve");
        assert!(resolved.is_none());
    }

    #[test]
    fn active_profile_supplies_provider_and_model() {
        let home = TempHome::new("active");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let resolver = ProviderProfileResolver::with_repository(repository);
        let resolved = resolver
            .resolve(&ProviderResolutionInputs::default())
            .expect("resolve")
            .expect("resolved");

        assert_eq!(resolved.model, "gpt-test");
        assert_eq!(resolved.provider.base_url.as_deref(), Some("https://example.com/v1"));
        assert!(matches!(
            resolved.source,
            ProviderSelectionSource::ProviderProfile { .. }
        ));
    }

    #[test]
    fn conflicting_provider_override_is_rejected() {
        let home = TempHome::new("conflict");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let resolver = ProviderProfileResolver::with_repository(repository);
        let inputs = ProviderResolutionInputs {
            override_provider_id: Some("openai".to_string()),
            ..Default::default()
        };
        let error = resolver.resolve(&inputs).expect_err("conflict expected");
        assert!(matches!(error, ProviderResolutionError::Conflict(_)));
    }

    #[test]
    fn stale_active_profile_id_is_reported() {
        let home = TempHome::new("stale");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1"), None).expect("add");
        repository.activate("p1", None).expect("activate");
        repository.remove("p1", None, None).expect("remove");

        // Removing the active profile clears the selection, so re-add an
        // id that does not exist to simulate a stale pointer.
        let mut document = repository.read().expect("read");
        document.active_profile_id = Some("missing".to_string());
        repository.write(&document).expect("write");

        let resolver = ProviderProfileResolver::with_repository(repository);
        let error = resolver
            .resolve(&ProviderResolutionInputs::default())
            .expect_err("missing expected");
        assert!(matches!(
            error,
            ProviderResolutionError::ActiveProfileMissing(_)
        ));
    }

    #[test]
    fn parse_wire_api_accepts_known_values() {
        assert_eq!(parse_wire_api("chat").expect("chat"), WireApi::Chat);
        assert_eq!(parse_wire_api("responses").expect("responses"), WireApi::Responses);
        assert_eq!(
            parse_wire_api("responses_websocket").expect("ws"),
            WireApi::ResponsesWebsocket
        );
        assert!(parse_wire_api("grpc").is_err());
    }
}

#[cfg(test)]
mod precedence_tests {
    use super::*;
    use crate::provider_profiles::ProviderProfileRepository;
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-precedence-{tag}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn profile(id: &str) -> ProviderProfile {
        ProviderProfile::new(
            id,
            "Example",
            "https://example.com/v1",
            WireApi::Chat,
            "gpt-test",
        )
    }

    fn resolver_with_active_profile(home: &TempHome) -> ProviderProfileResolver {
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1"), None).expect("add");
        repository.activate("p1", None).expect("activate");
        ProviderProfileResolver::with_repository(repository)
    }

    /// A `config.toml` provider selection that names the profile's own
    /// provider id is compatible, not a conflict.
    #[test]
    fn a_matching_config_provider_id_is_accepted() {
        let home = TempHome::new("matching");
        let resolver = resolver_with_active_profile(&home);
        let inputs = ProviderResolutionInputs {
            config_provider_id: Some("qcodex-profile-p1".to_string()),
            ..Default::default()
        };
        assert!(resolver.resolve(&inputs).expect("resolve").is_some());
    }

    /// A `config.toml` provider selection that names a different provider is
    /// a conflict, because the two would disagree about the endpoint.
    #[test]
    fn a_differing_config_provider_id_is_a_conflict() {
        let home = TempHome::new("differing");
        let resolver = resolver_with_active_profile(&home);
        let inputs = ProviderResolutionInputs {
            config_provider_id: Some("openai".to_string()),
            ..Default::default()
        };
        let error = resolver.resolve(&inputs).expect_err("conflict expected");
        assert!(matches!(error, ProviderResolutionError::Conflict(_)));
    }

    /// A `config.toml` selection that names the same provider id but a
    /// different model is also a conflict: the profile owns the model.
    #[test]
    fn a_differing_config_model_is_a_conflict() {
        let home = TempHome::new("model-conflict");
        let resolver = resolver_with_active_profile(&home);
        let inputs = ProviderResolutionInputs {
            config_model: Some("some-other-model".to_string()),
            ..Default::default()
        };
        let error = resolver.resolve(&inputs).expect_err("conflict expected");
        assert!(matches!(error, ProviderResolutionError::Conflict(_)));
    }

    /// An invocation override that names the profile's own provider id is
    /// compatible.
    #[test]
    fn a_matching_override_provider_id_is_accepted() {
        let home = TempHome::new("override-matching");
        let resolver = resolver_with_active_profile(&home);
        let inputs = ProviderResolutionInputs {
            override_provider_id: Some("qcodex-profile-p1".to_string()),
            ..Default::default()
        };
        assert!(resolver.resolve(&inputs).expect("resolve").is_some());
    }

    /// An invocation override that names a different provider conflicts.
    #[test]
    fn a_differing_override_provider_id_is_a_conflict() {
        let home = TempHome::new("override-differing");
        let resolver = resolver_with_active_profile(&home);
        let inputs = ProviderResolutionInputs {
            override_provider_id: Some("openai".to_string()),
            ..Default::default()
        };
        let error = resolver.resolve(&inputs).expect_err("conflict expected");
        assert!(matches!(error, ProviderResolutionError::Conflict(_)));
    }

    /// Without an active profile the resolver defers to `config.toml`.
    #[test]
    fn no_active_profile_defers_to_config() {
        let home = TempHome::new("defer");
        let resolver = ProviderProfileResolver::new(&home.0);
        let resolved = resolver
            .resolve(&ProviderResolutionInputs {
                config_provider_id: Some("openai".to_string()),
                ..Default::default()
            })
            .expect("resolve");
        assert!(resolved.is_none());
    }
}
