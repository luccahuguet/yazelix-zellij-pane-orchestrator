use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightSidebarCommandConfig {
    pub command: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RightSidebarCommandLaunchSpec {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
}

impl RightSidebarCommandConfig {
    pub fn from_plugin_configuration(configuration: &BTreeMap<String, String>) -> Option<Self> {
        let command = configuration
            .get("right_sidebar_command")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())?;

        let mut indexed_args = configuration
            .iter()
            .filter_map(|(key, value)| {
                key.strip_prefix("right_sidebar_arg_")
                    .and_then(|index| index.parse::<usize>().ok())
                    .filter(|index| *index > 0)
                    .map(|index| (index, value.clone()))
            })
            .collect::<Vec<_>>();
        indexed_args.sort_by_key(|(index, _)| *index);

        Some(Self {
            command,
            args: indexed_args.into_iter().map(|(_, value)| value).collect(),
        })
    }

    pub fn launch_spec(&self, cwd: &str) -> RightSidebarCommandLaunchSpec {
        RightSidebarCommandLaunchSpec {
            command: self.command.clone(),
            args: self.args.clone(),
            cwd: cwd.to_string(),
        }
    }
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

    // Defends: generated plugin configuration owns the right-sidebar command surface.
    #[test]
    fn reads_command_and_numbered_args() {
        let config = BTreeMap::from([
            ("right_sidebar_command".to_string(), "codex".to_string()),
            ("right_sidebar_arg_2".to_string(), "gpt-5.5".to_string()),
            ("right_sidebar_arg_1".to_string(), "--model".to_string()),
        ]);

        assert_eq!(
            RightSidebarCommandConfig::from_plugin_configuration(&config),
            Some(RightSidebarCommandConfig {
                command: "codex".to_string(),
                args: vec!["--model".to_string(), "gpt-5.5".to_string()],
            })
        );
    }

    // Defends: missing generated command config fails closed instead of falling back to a hidden launcher.
    #[test]
    fn empty_command_is_missing() {
        let config = BTreeMap::from([("right_sidebar_command".to_string(), "   ".to_string())]);

        assert_eq!(
            RightSidebarCommandConfig::from_plugin_configuration(&config),
            None
        );
    }

    // Regression: the managed right agent pane must launch in the workspace root so Codex resume history uses the same cwd filter as a direct terminal launch.
    #[test]
    fn launch_spec_sets_workspace_cwd() {
        let right_sidebar_command = RightSidebarCommandConfig {
            command: "codex".to_string(),
            args: vec!["--model".to_string(), "gpt-5".to_string()],
        };

        assert_eq!(
            right_sidebar_command.launch_spec("/tmp/project"),
            RightSidebarCommandLaunchSpec {
                command: "codex".to_string(),
                args: vec!["--model".to_string(), "gpt-5".to_string()],
                cwd: "/tmp/project".to_string(),
            }
        );
    }
}
