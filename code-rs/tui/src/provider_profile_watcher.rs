//! Runtime observation of the qcodex provider profile store.
//!
//! The TUI polls the profile store on a background task. When the active
//! profile changes, the watcher emits an [`AppEvent::ProviderProfileChanged`]
//! so the chat widget can apply the new provider at a turn boundary.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use code_core::config::Config;
use code_core::provider_profiles::ProfileWatchEvent;
use code_core::provider_profiles::ProviderProfileWatcher;
use code_core::provider_profiles::ProviderResolutionInputs;
use std::sync::Arc;
use std::time::Duration;

/// How often the watcher polls the profile store.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Creates a watcher for the profile store of a session's code home.
///
/// Returns `None` when no profile is active, so the TUI does not pay for a
/// background task that can never report a change.
pub(crate) fn new_provider_profile_watcher(
    config: &Config,
) -> Option<Arc<ProviderProfileWatcher>> {
    let watcher = Arc::new(ProviderProfileWatcher::new(&config.code_home));
    watcher.mark_applied(
        config.active_provider_profile_id.as_deref(),
        config.active_provider_profile_revision,
    );
    if config.active_provider_profile_id.is_none() {
        // No profile is active yet, but one may be activated later. Keep the
        // watcher alive so the switch is noticed.
        return Some(watcher);
    }
    Some(watcher)
}

/// Starts the background polling task.
///
/// The returned handle is aborted when the session stops.
pub(crate) fn spawn_provider_profile_poll(
    watcher: Arc<ProviderProfileWatcher>,
    app_event_tx: AppEventSender,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(POLL_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick completes immediately; skip it so the watcher does
        // not report a change for the profile the session already uses.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let event = watcher.poll(&ProviderResolutionInputs::default());
            let message = match event {
                ProfileWatchEvent::Unchanged => continue,
                ProfileWatchEvent::ProfileChanged(resolved) => {
                    let profile_id = resolved.profile.as_ref().map(|profile| profile.id.clone());
                    let model = resolved.profile.as_ref().map(|profile| profile.model.clone());
                    app_event_tx.send(AppEvent::ProviderProfileChanged {
                        profile_id,
                        model,
                        message: format!(
                            "Provider profile changed to `{}` ({}).",
                            resolved.provider_id, resolved.model
                        ),
                    });
                    continue;
                }
                ProfileWatchEvent::ProfileDeactivated => {
                    app_event_tx.send(AppEvent::ProviderProfileChanged {
                        profile_id: None,
                        model: None,
                        message: "Provider profile deactivated; using the config.toml provider."
                            .to_string(),
                    });
                    continue;
                }
                ProfileWatchEvent::StoreError(err) => {
                    format!("Could not read the provider profile store: {err}")
                }
                ProfileWatchEvent::ResolutionFailed(message) => {
                    format!("Could not apply the provider profile: {message}")
                }
            };
            app_event_tx.send(AppEvent::ProviderProfileChanged {
                profile_id: None,
                model: None,
                message,
            });
        }
    })
}
