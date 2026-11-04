//! Reviewed MCP launcher presets for opt-in browser automation.

use std::time::Duration;

use crate::config_types::McpServerConfig;
use crate::config_types::McpServerTransportConfig;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpPreset {
    Playwright,
    ComputerUse,
}

impl McpPreset {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "playwright" => Some(Self::Playwright),
            "computer-use" => Some(Self::ComputerUse),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Playwright => "playwright",
            Self::ComputerUse => "computer-use",
        }
    }
}

pub struct McpPresetDefinition {
    pub config: McpServerConfig,
    pub notice: &'static str,
}

/// Returns a pinned, opt-in launcher configuration. Neither preset shares a
/// browser session with qcodex's internal browser.
pub fn mcp_preset_definition(preset: McpPreset) -> McpPresetDefinition {
    match preset {
        McpPreset::Playwright => McpPresetDefinition {
            config: McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "npx".to_string(),
                    args: vec![
                        "--yes".to_string(),
                        "@playwright/mcp@0.0.80".to_string(),
                        "--isolated".to_string(),
                    ],
                    env: None,
                },
                startup_timeout_sec: Some(Duration::from_secs(20)),
                tool_timeout_sec: Some(Duration::from_secs(60)),
                enabled_tools: None,
                disabled_tools: None,
            },
            notice: "Playwright uses a temporary isolated browser profile. It does not attach to qcodex's internal browser.",
        },
        McpPreset::ComputerUse => McpPresetDefinition {
            config: McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "npx".to_string(),
                    args: vec![
                        "--yes".to_string(),
                        "computer-use-mcp@1.8.0".to_string(),
                    ],
                    env: None,
                },
                startup_timeout_sec: Some(Duration::from_secs(20)),
                tool_timeout_sec: Some(Duration::from_secs(60)),
                enabled_tools: None,
                disabled_tools: None,
            },
            notice: "Computer Use can control your interactive desktop. Review its package and use a dedicated account or disposable desktop for untrusted tasks.",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playwright_preset_is_pinned_and_isolated() {
        let definition = mcp_preset_definition(McpPreset::Playwright);
        assert_eq!(definition.config.startup_timeout_sec, Some(Duration::from_secs(20)));
        match definition.config.transport {
            McpServerTransportConfig::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args, ["--yes", "@playwright/mcp@0.0.80", "--isolated"]);
                assert!(env.is_none());
            }
            _ => panic!("expected stdio transport"),
        }
    }

    #[test]
    fn computer_use_preset_is_pinned() {
        let definition = mcp_preset_definition(McpPreset::ComputerUse);
        match definition.config.transport {
            McpServerTransportConfig::Stdio { command, args, env } => {
                assert_eq!(command, "npx");
                assert_eq!(args, ["--yes", "computer-use-mcp@1.8.0"]);
                assert!(env.is_none());
            }
            _ => panic!("expected stdio transport"),
        }
    }
}
