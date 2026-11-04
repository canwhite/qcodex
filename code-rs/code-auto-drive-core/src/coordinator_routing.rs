//! Auto Drive CLI model-routing preparation.
//!
//! This module is deliberately independent of the coordinator run loop: it
//! turns persisted settings plus runtime availability into the routing entries
//! the coordinator exposes to the model.

use code_core::config_types::{
    AutoDriveModelRoutingEntry,
    AutoDriveSettings,
    ReasoningEffort,
    normalize_auto_drive_model_routing_entries,
};

const PRIMARY_MODEL: &str = "gpt-5.5";
const FAST_MODEL: &str = "gpt-5.6-luna";
const PRIMARY_DESCRIPTION: &str = "Hard planning and complex problem solving";
const FAST_DESCRIPTION: &str = "Fast implementation loops and failing-test iteration";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliRoutingEntry {
    pub(crate) model: String,
    pub(crate) reasoning_levels: Vec<ReasoningEffort>,
    pub(crate) description: String,
}

fn reasoning_priority(level: ReasoningEffort) -> u8 {
    match level {
        ReasoningEffort::Minimal => 0,
        ReasoningEffort::Low => 1,
        ReasoningEffort::Medium => 2,
        ReasoningEffort::High => 3,
        ReasoningEffort::XHigh => 4,
        ReasoningEffort::None => 5,
    }
}

pub(crate) fn reasoning_effort_label(level: ReasoningEffort) -> &'static str {
    match level {
        ReasoningEffort::Minimal | ReasoningEffort::None => "minimal",
        ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::XHigh => "xhigh",
    }
}

pub(crate) fn format_reasoning_levels(levels: &[ReasoningEffort]) -> String {
    levels
        .iter()
        .map(|level| reasoning_effort_label(*level))
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn default_entries() -> Vec<CliRoutingEntry> {
    vec![
        CliRoutingEntry {
            model: PRIMARY_MODEL.to_string(),
            reasoning_levels: vec![ReasoningEffort::High, ReasoningEffort::XHigh],
            description: PRIMARY_DESCRIPTION.to_string(),
        },
        CliRoutingEntry {
            model: FAST_MODEL.to_string(),
            reasoning_levels: vec![ReasoningEffort::High],
            description: FAST_DESCRIPTION.to_string(),
        },
    ]
}

fn defaults_for_auth(
    _auth_mode: Option<code_app_server_protocol::AuthMode>,
    _supports_pro_only_models: bool,
) -> Vec<CliRoutingEntry> {
    default_entries()
}

#[cfg(test)]
pub(crate) fn models_for_auth(
    auth_mode: Option<code_app_server_protocol::AuthMode>,
    supports_pro_only_models: bool,
) -> Vec<String> {
    defaults_for_auth(auth_mode, supports_pro_only_models)
        .into_iter()
        .map(|entry| entry.model)
        .collect()
}

fn normalize_entries(entries: &[AutoDriveModelRoutingEntry]) -> Vec<CliRoutingEntry> {
    let mut normalized: Vec<CliRoutingEntry> = Vec::new();
    for entry in normalize_auto_drive_model_routing_entries(entries.iter().cloned()) {
        if !entry.enabled {
            continue;
        }

        if let Some(existing) = normalized
            .iter_mut()
            .find(|candidate| candidate.model.eq_ignore_ascii_case(&entry.model))
        {
            for level in entry.reasoning_levels {
                if !existing.reasoning_levels.contains(&level) {
                    existing.reasoning_levels.push(level);
                }
            }
            existing.reasoning_levels.sort_by_key(|level| reasoning_priority(*level));
            if existing.description.is_empty() && !entry.description.is_empty() {
                existing.description = entry.description;
            }
            continue;
        }

        normalized.push(CliRoutingEntry {
            model: entry.model,
            reasoning_levels: entry.reasoning_levels,
            description: entry.description,
        });
    }
    normalized
}

pub(crate) fn resolve_entries(
    settings: &AutoDriveSettings,
    auth_mode: Option<code_app_server_protocol::AuthMode>,
    supports_pro_only_models: bool,
    available_models: &[String],
) -> Vec<CliRoutingEntry> {
    let mut entries = normalize_entries(&settings.model_routing_entries);
    entries.retain(|entry| {
        available_models
            .iter()
            .any(|model| model.eq_ignore_ascii_case(&entry.model))
    });

    if entries.is_empty() {
        return defaults_for_auth(auth_mode, supports_pro_only_models)
            .into_iter()
            .filter(|entry| {
                available_models
                    .iter()
                    .any(|model| model.eq_ignore_ascii_case(&entry.model))
            })
            .collect();
    }

    entries
}
