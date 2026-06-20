use std::collections::BTreeMap;

pub const STATUS_USAGE_PROVIDER_CLAUDE_ENABLED_KEY: &str = "status_usage_provider_claude_enabled";
pub const STATUS_USAGE_PROVIDER_CODEX_ENABLED_KEY: &str = "status_usage_provider_codex_enabled";
pub const STATUS_USAGE_PROVIDER_OPENCODE_GO_ENABLED_KEY: &str =
    "status_usage_provider_opencode_go_enabled";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusUsageProviderConfig {
    pub claude_usage: bool,
    pub codex_usage: bool,
    pub opencode_go_usage: bool,
}

impl Default for StatusUsageProviderConfig {
    fn default() -> Self {
        Self {
            claude_usage: true,
            codex_usage: true,
            opencode_go_usage: true,
        }
    }
}

impl StatusUsageProviderConfig {
    pub fn from_plugin_configuration(configuration: &BTreeMap<String, String>) -> Self {
        let default = Self::default();

        Self {
            claude_usage: bool_config(
                configuration,
                STATUS_USAGE_PROVIDER_CLAUDE_ENABLED_KEY,
                default.claude_usage,
            ),
            codex_usage: bool_config(
                configuration,
                STATUS_USAGE_PROVIDER_CODEX_ENABLED_KEY,
                default.codex_usage,
            ),
            opencode_go_usage: bool_config(
                configuration,
                STATUS_USAGE_PROVIDER_OPENCODE_GO_ENABLED_KEY,
                default.opencode_go_usage,
            ),
        }
    }
}

fn bool_config(configuration: &BTreeMap<String, String>, key: &str, default: bool) -> bool {
    match configuration.get(key).map(|raw| raw.trim()) {
        Some("true" | "1" | "yes" | "on") => true,
        Some("false" | "0" | "no" | "off") => false,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test lane: default
    // Defends: standalone plugin invocations keep the historical all-provider refresh behavior.
    #[test]
    fn missing_provider_config_preserves_all_enabled_default() {
        let configuration = BTreeMap::new();

        assert_eq!(
            StatusUsageProviderConfig::from_plugin_configuration(&configuration),
            StatusUsageProviderConfig {
                claude_usage: true,
                codex_usage: true,
                opencode_go_usage: true,
            }
        );
    }

    // Test lane: default
    // Defends: generated Yazelix plugin config can narrow usage refreshes to the enabled provider.
    #[test]
    fn explicit_provider_config_can_enable_only_codex() {
        let configuration = BTreeMap::from([
            (
                STATUS_USAGE_PROVIDER_CLAUDE_ENABLED_KEY.to_string(),
                "false".to_string(),
            ),
            (
                STATUS_USAGE_PROVIDER_CODEX_ENABLED_KEY.to_string(),
                "true".to_string(),
            ),
            (
                STATUS_USAGE_PROVIDER_OPENCODE_GO_ENABLED_KEY.to_string(),
                "false".to_string(),
            ),
        ]);

        assert_eq!(
            StatusUsageProviderConfig::from_plugin_configuration(&configuration),
            StatusUsageProviderConfig {
                claude_usage: false,
                codex_usage: true,
                opencode_go_usage: false,
            }
        );
    }
}
