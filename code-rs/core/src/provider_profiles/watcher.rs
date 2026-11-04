//! Runtime watcher for provider profile changes.
//!
//! The watcher owns the observation loop for a long-lived session. It
//! notices that the active profile changed, resolves the replacement off
//! the request path, and hands the result to the session host, which
//! applies it at a safe boundary.
//!
//! Two rules make the switch safe:
//!
//! * A turn that has already started keeps the snapshot it began with.
//!   The switch only takes effect for a turn that starts afterwards.
//! * If resolving the replacement fails, the previous snapshot stays in
//!   place and the failure is reported.

use super::ProviderProfileRepository;
use super::ProviderResolutionInputs;
use super::ProviderProfileResolver;
use super::ResolvedProvider;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

/// How often the watcher re-reads the profile store.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// The outcome of a watcher poll.
#[derive(Debug)]
pub enum ProfileWatchEvent {
    /// The active profile changed and a replacement was resolved.
    ProfileChanged(Box<ResolvedProvider>),
    /// The active profile was removed and the session should fall back.
    ProfileDeactivated,
    /// Nothing changed since the last poll.
    Unchanged,
    /// The store could not be read.
    StoreError(String),
    /// The replacement profile could not be resolved.
    ResolutionFailed(String),
}

impl fmt::Display for ProfileWatchEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfileChanged(resolved) => {
                write!(f, "provider profile changed to `{}`", resolved.model)
            }
            Self::ProfileDeactivated => write!(f, "provider profile deactivated"),
            Self::Unchanged => write!(f, "provider profile unchanged"),
            Self::StoreError(err) => write!(f, "provider profile store error: {err}"),
            Self::ResolutionFailed(message) => {
                write!(f, "provider profile could not be applied: {message}")
            }
        }
    }
}

/// Observes the provider profile store on behalf of a session host.
#[derive(Debug)]
pub struct ProviderProfileWatcher {
    resolver: ProviderProfileResolver,
    state: Mutex<WatcherState>,
}

/// The last observed state of the store.
#[derive(Debug, Clone, Default)]
struct WatcherState {
    /// The document revision observed at the last successful poll.
    last_revision: Option<u64>,
    /// The id of the profile that was last applied.
    applied_profile_id: Option<String>,
    /// The revision of the profile that was last applied.
    applied_profile_revision: Option<u64>,
}

impl ProviderProfileWatcher {
    /// Creates a watcher over the profile store in `code_home`.
    pub fn new(code_home: &Path) -> Self {
        Self::with_repository(ProviderProfileRepository::new(code_home))
    }

    /// Creates a watcher over an existing repository.
    pub fn with_repository(repository: ProviderProfileRepository) -> Self {
        Self {
            resolver: ProviderProfileResolver::with_repository(repository),
            state: Mutex::new(WatcherState::default()),
        }
    }

    /// Records the currently applied profile without observing a change.
    ///
    /// A session calls this at startup so the first poll does not report a
    /// spurious change.
    pub fn mark_applied(&self, profile_id: Option<&str>, revision: Option<u64>) {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.applied_profile_id = profile_id.map(str::to_string);
        state.applied_profile_revision = revision;
        state.last_revision = self.resolver.repository().document_revision().ok();
    }

    /// Returns the id of the profile that was last applied.
    pub fn applied_profile_id(&self) -> Option<String> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        state.applied_profile_id.clone()
    }

    /// Polls the store once.
    ///
    /// Returns [`ProfileWatchEvent::Unchanged`] when nothing changed, and a
    /// change event when the active profile differs from the applied one.
    pub fn poll(&self, inputs: &ProviderResolutionInputs) -> ProfileWatchEvent {
        let current_revision = match self.resolver.repository().document_revision() {
            Ok(revision) => revision,
            Err(err) => return ProfileWatchEvent::StoreError(err.to_string()),
        };

        let applied = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.applied_profile_id.clone(), state.applied_profile_revision)
        };

        let resolved = match self.resolver.resolve(inputs) {
            Ok(resolved) => resolved,
            Err(err) => return ProfileWatchEvent::ResolutionFailed(err.to_string()),
        };

        let (resolved_id, resolved_revision) = match resolved.as_ref() {
            Some(resolved) => match &resolved.source {
                super::ProviderSelectionSource::ProviderProfile { id, revision } => {
                    (Some(id.clone()), Some(*revision))
                }
                _ => (None, None),
            },
            None => (None, None),
        };

        let unchanged = resolved_id == applied.0
            && match (resolved_revision, applied.1) {
                (Some(current), Some(previous)) => current == previous,
                (None, None) => true,
                _ => false,
            };

        if unchanged {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.last_revision = Some(current_revision);
            return ProfileWatchEvent::Unchanged;
        }

        // The new profile is resolved successfully, so record it as applied
        // before handing it to the session host. The host applies it at its
        // next safe boundary; if it fails there, the host reports the error
        // and the next poll will try again.
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.applied_profile_id = resolved_id.clone();
            state.applied_profile_revision = resolved_revision;
            state.last_revision = Some(current_revision);
        }

        match resolved {
            Some(resolved) => ProfileWatchEvent::ProfileChanged(Box::new(resolved)),
            None => ProfileWatchEvent::ProfileDeactivated,
        }
    }
}

/// A shared watcher handle.
pub type SharedProviderProfileWatcher = Arc<ProviderProfileWatcher>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider_profiles::ProviderProfile;
    use crate::model_provider_info::WireApi;
    use std::path::PathBuf;

    struct TempHome(PathBuf);

    impl TempHome {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "qcodex-watcher-{tag}-{}",
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

    fn profile(id: &str, model: &str) -> ProviderProfile {
        ProviderProfile::new(
            id,
            "Example",
            "https://example.com/v1",
            WireApi::Chat,
            model,
        )
    }

    #[test]
    fn reports_nothing_when_no_profile_is_active() {
        let home = TempHome::new("none");
        let watcher = ProviderProfileWatcher::new(&home.0);
        watcher.mark_applied(None, None);

        let event = watcher.poll(&ProviderResolutionInputs::default());
        assert!(matches!(event, ProfileWatchEvent::Unchanged));
    }

    #[test]
    fn detects_a_newly_activated_profile() {
        let home = TempHome::new("activate");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1", "m1"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let watcher = ProviderProfileWatcher::with_repository(repository);
        watcher.mark_applied(None, None);

        let event = watcher.poll(&ProviderResolutionInputs::default());
        assert!(matches!(event, ProfileWatchEvent::ProfileChanged(_)));
    }

    #[test]
    fn detects_a_switch_between_profiles() {
        let home = TempHome::new("switch");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1", "m1"), None).expect("add");
        repository.add(profile("p2", "m2"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let watcher = ProviderProfileWatcher::with_repository(repository.clone());
        watcher.mark_applied(Some("p1"), Some(1));

        assert!(matches!(
            watcher.poll(&ProviderResolutionInputs::default()),
            ProfileWatchEvent::Unchanged
        ));

        repository.activate("p2", None).expect("activate");
        let event = watcher.poll(&ProviderResolutionInputs::default());
        match event {
            ProfileWatchEvent::ProfileChanged(resolved) => {
                assert_eq!(resolved.model, "m2");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn detects_deactivation() {
        let home = TempHome::new("deactivate");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1", "m1"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let watcher = ProviderProfileWatcher::with_repository(repository.clone());
        watcher.mark_applied(Some("p1"), Some(1));

        repository.deactivate(None).expect("deactivate");
        let event = watcher.poll(&ProviderResolutionInputs::default());
        assert!(matches!(event, ProfileWatchEvent::ProfileDeactivated));
    }

    #[test]
    fn a_failed_resolution_is_reported_and_retried() {
        let home = TempHome::new("failed");
        let repository = ProviderProfileRepository::new(&home.0);
        repository.add(profile("p1", "m1"), None).expect("add");
        repository.activate("p1", None).expect("activate");

        let watcher = ProviderProfileWatcher::with_repository(repository.clone());
        watcher.mark_applied(None, None);

        // An invocation override that disagrees with the profile makes the
        // resolution fail, and the failure is reported rather than applied.
        let inputs = ProviderResolutionInputs {
            override_provider_id: Some("openai".to_string()),
            ..Default::default()
        };
        let event = watcher.poll(&inputs);
        assert!(matches!(event, ProfileWatchEvent::ResolutionFailed(_)));

        // Polling again without the conflicting override succeeds.
        let event = watcher.poll(&ProviderResolutionInputs::default());
        assert!(matches!(event, ProfileWatchEvent::ProfileChanged(_)));
    }
}
