use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use yazelix_zellij_pane_orchestrator::tab_identity_contract::{
    active_tab_id as select_active_tab_id, current_tab_ids as collect_current_tab_ids,
};
use zellij_tile::prelude::*;

use crate::panes::pane_id_to_string;
use crate::sidebar_yazi::SidebarYaziState;
use crate::{State, COMMAND_STEP_DELAY_MS, RESULT_INVALID_PAYLOAD, RESULT_MISSING, RESULT_OK};
use yazelix_zellij_pane_orchestrator::editor_open_contract::build_editor_change_directory_command;
use yazelix_zellij_pane_orchestrator::workspace_popup_contract::{
    workspace_popup_destination_id, workspace_popup_launch_matches_root, workspace_popup_payload,
    workspace_popup_picker_action,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceState {
    pub(crate) root: String,
    pub(crate) source: WorkspaceStateSource,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspacePopupYaziState {
    pane_id: String,
    yazi_id: String,
    launch_cwd: String,
}

pub(crate) fn bootstrap_workspace_root(initial_cwd: &Path) -> String {
    initial_cwd.display().to_string()
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkspaceStateSource {
    Bootstrap,
    Explicit,
}

#[derive(Deserialize)]
struct WorkspaceRetargetRequest {
    workspace_root: String,
    #[serde(default)]
    workspace_source: Option<WorkspaceStateSource>,
    cd_focused_pane: bool,
    editor: Option<String>,
    sidebar_yazi: Option<WorkspaceSidebarYaziRegistration>,
}

#[derive(Deserialize)]
struct WorkspaceSidebarYaziRegistration {
    pane_id: String,
    yazi_id: String,
    cwd: String,
}

#[derive(Serialize)]
struct WorkspaceRetargetResponse {
    status: String,
    editor_status: String,
    sidebar_yazi_id: Option<String>,
    sidebar_yazi_cwd: Option<String>,
}

#[derive(Deserialize)]
struct OpenTerminalRequest {
    cwd: String,
}

#[derive(Deserialize)]
struct WorkspacePopupYaziRegistration {
    pane_id: String,
    yazi_id: String,
    cwd: String,
}

impl State {
    pub(crate) fn reconcile_workspace_state(&mut self, tabs: &[TabInfo]) {
        let current_tab_ids = collect_current_tab_ids(tabs);
        self.workspace_state_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        self.seen_tab_ids
            .retain(|tab_id| current_tab_ids.contains(tab_id));

        let active_tab_id = select_active_tab_id(tabs);

        if let Some(active_tab_id) = active_tab_id {
            let is_new_tab = !self.seen_tab_ids.contains(&active_tab_id);
            if !self.workspace_state_by_tab.contains_key(&active_tab_id) {
                let inherited_workspace_state = if is_new_tab {
                    self.initial_workspace_state.clone()
                } else if self.workspace_state_by_tab.is_empty() {
                    self.initial_workspace_state.clone()
                } else {
                    None
                };

                if let Some(workspace_state) = inherited_workspace_state {
                    self.workspace_state_by_tab
                        .insert(active_tab_id, workspace_state);
                }
            }
        }

        self.seen_tab_ids = current_tab_ids;
    }

    pub(crate) fn reconcile_workspace_popup_yazi_state(&mut self) {
        let pane_id_by_tab = self.workspace_yazi_pane_id_by_tab();
        self.workspace_popup_yazi_state_by_tab
            .retain(|tab_id, state| pane_id_by_tab.get(tab_id) == Some(&state.pane_id));
    }

    pub(crate) fn register_workspace_popup_yazi_state(&mut self, pipe_message: &PipeMessage) {
        if !self.permissions_granted {
            self.respond(pipe_message, crate::RESULT_DENIED);
            return;
        }
        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };
        let registration: WorkspacePopupYaziRegistration = match serde_json::from_str(payload) {
            Ok(registration) => registration,
            Err(_) => {
                self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                return;
            }
        };
        let pane_id = registration.pane_id.trim().to_string();
        let yazi_id = registration.yazi_id.trim().to_string();
        let launch_cwd = registration.cwd.trim().to_string();
        if pane_id.is_empty() || yazi_id.is_empty() || !Path::new(&launch_cwd).is_absolute() {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        }
        let Some(tab_id) = self
            .workspace_yazi_pane_id_by_tab()
            .into_iter()
            .find_map(|(tab_id, candidate)| (candidate == pane_id).then_some(tab_id))
        else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        self.workspace_popup_yazi_state_by_tab.insert(
            tab_id,
            WorkspacePopupYaziState {
                pane_id,
                yazi_id: yazi_id.clone(),
                launch_cwd: launch_cwd.clone(),
            },
        );
        let launch_matches_workspace = self
            .workspace_root_for_tab(tab_id)
            .is_some_and(|root| workspace_popup_launch_matches_root(&launch_cwd, root));
        if launch_matches_workspace && self.pending_workspace_zoxide_picker_by_tab.remove(&tab_id) {
            self.launch_zoxide_picker(&yazi_id);
        }
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn retarget_workspace(&mut self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        let Some(active_tab_position) = self.tab_identity.active_tab_position() else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };

        let workspace_retarget_request: WorkspaceRetargetRequest =
            match serde_json::from_str(payload) {
                Ok(request) => request,
                Err(_) => {
                    self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                    return;
                }
            };

        let workspace_root = workspace_retarget_request.workspace_root.trim().to_string();
        if workspace_root.is_empty() {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        }

        let workspace_state = WorkspaceState {
            root: workspace_root,
            source: workspace_retarget_request
                .workspace_source
                .unwrap_or(WorkspaceStateSource::Explicit),
        };
        rename_tab(
            tab_index_from_position(active_tab_position),
            &tab_name_from_workspace_root(&workspace_state.root),
        );
        self.workspace_state_by_tab
            .insert(active_tab_id, workspace_state.clone());
        self.pending_workspace_zoxide_picker_by_tab
            .remove(&active_tab_id);

        if let Some(registration) = workspace_retarget_request.sidebar_yazi {
            self.register_inline_sidebar_yazi_state(active_tab_id, registration);
        }

        if workspace_retarget_request.cd_focused_pane {
            let Some(focused_pane_id) = self.get_focused_terminal_pane(pipe_message) else {
                return;
            };

            write_chars_to_pane_id(
                &change_directory_command(&workspace_state.root),
                focused_pane_id,
            );
            sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
            write_to_pane_id(vec![13], focused_pane_id);
        }

        let editor_status = workspace_retarget_request
            .editor
            .as_deref()
            .map(str::trim)
            .filter(|editor| !editor.is_empty())
            .map(|editor| {
                let Some(change_directory_command) =
                    build_editor_change_directory_command(editor, &workspace_state.root)
                else {
                    return "unsupported_editor".to_string();
                };

                let Some(editor_pane) = self
                    .tab_pane_caches
                    .managed_panes_by_tab
                    .get(&active_tab_id)
                    .and_then(|managed_tab_panes| managed_tab_panes.editor)
                else {
                    return "missing".to_string();
                };

                write_to_pane_id(vec![27], editor_pane.pane_id);
                sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
                write_chars_to_pane_id(&change_directory_command, editor_pane.pane_id);
                sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
                write_to_pane_id(vec![13], editor_pane.pane_id);
                "ok".to_string()
            })
            .unwrap_or_else(|| "skipped".to_string());

        let sidebar_yazi_state = self.get_active_sidebar_yazi_state_snapshot(active_tab_id);
        let response = WorkspaceRetargetResponse {
            status: RESULT_OK.to_string(),
            editor_status,
            sidebar_yazi_id: sidebar_yazi_state.map(|state| state.yazi_id.clone()),
            sidebar_yazi_cwd: sidebar_yazi_state.map(|state| state.cwd.clone()),
        };

        match serde_json::to_string(&response) {
            Ok(serialized_response) => self.respond(pipe_message, &serialized_response),
            Err(_) => self.respond(pipe_message, RESULT_INVALID_PAYLOAD),
        }
    }

    fn register_inline_sidebar_yazi_state(
        &mut self,
        active_tab_id: usize,
        registration: WorkspaceSidebarYaziRegistration,
    ) {
        let pane_id = registration.pane_id.trim().to_string();
        let yazi_id = registration.yazi_id.trim().to_string();
        let cwd = registration.cwd.trim().to_string();
        if pane_id.is_empty() || yazi_id.is_empty() || cwd.is_empty() {
            return;
        }

        let expected_sidebar_pane_id = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|managed_tab_panes| managed_tab_panes.sidebar)
            .and_then(|sidebar| pane_id_to_string(Some(sidebar.pane_id)));
        if expected_sidebar_pane_id.as_deref() != Some(pane_id.as_str()) {
            return;
        }

        self.sidebar_yazi_state_by_tab.insert(
            active_tab_id,
            SidebarYaziState {
                pane_id,
                yazi_id,
                cwd,
            },
        );
    }

    pub(crate) fn workspace_state_for_tab(&self, active_tab_id: usize) -> Option<WorkspaceState> {
        self.workspace_state_by_tab
            .get(&active_tab_id)
            .cloned()
            .or_else(|| self.initial_workspace_state.clone())
    }

    pub(crate) fn open_terminal_in_cwd(&self, pipe_message: &PipeMessage) {
        let Some(_active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };

        let open_terminal_request: OpenTerminalRequest = match serde_json::from_str(payload) {
            Ok(request) => request,
            Err(_) => {
                self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                return;
            }
        };

        open_terminal(&open_terminal_request.cwd);
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn open_workspace_terminal(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let Some(workspace_state) = self.workspace_state_for_tab(active_tab_id) else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        open_terminal(&workspace_state.root);
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn toggle_workspace_popup(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        match self.workspace_popup_action_message(active_tab_id, pipe_message, "toggle") {
            Ok(message) => {
                pipe_message_to_plugin(message);
                self.respond(pipe_message, RESULT_OK);
            }
            Err(result) => self.respond(pipe_message, result),
        }
    }

    pub(crate) fn open_workspace_zoxide_picker(&mut self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        if self.yazi_cli.is_none() || self.workspace_yazi_pane_title.is_none() {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        }
        let popup_pane_id_by_tab = self.workspace_yazi_pane_id_by_tab();
        let Some(workspace_root) = self.workspace_root_for_tab(active_tab_id) else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };
        let picker_receiver = self
            .workspace_popup_yazi_state_by_tab
            .get(&active_tab_id)
            .filter(|state| popup_pane_id_by_tab.get(&active_tab_id) == Some(&state.pane_id))
            .filter(|state| workspace_popup_launch_matches_root(&state.launch_cwd, workspace_root))
            .map(|state| state.yazi_id.clone());
        let action = workspace_popup_picker_action(
            popup_pane_id_by_tab.contains_key(&active_tab_id),
            picker_receiver.is_some(),
        );
        let popup_message =
            match self.workspace_popup_action_message(active_tab_id, pipe_message, action) {
                Ok(message) => message,
                Err(result) => {
                    self.respond(pipe_message, result);
                    return;
                }
            };

        if picker_receiver.is_none() {
            self.pending_workspace_zoxide_picker_by_tab
                .insert(active_tab_id);
        }
        pipe_message_to_plugin(popup_message);

        if let Some(receiver) = picker_receiver {
            self.pending_workspace_zoxide_picker_by_tab
                .remove(&active_tab_id);
            self.launch_zoxide_picker(&receiver);
        }
        self.respond(pipe_message, RESULT_OK);
    }

    fn workspace_popup_action_message(
        &self,
        active_tab_id: usize,
        pipe_message: &PipeMessage,
        action: &str,
    ) -> Result<MessageToPlugin, &'static str> {
        let workspace_root = self
            .workspace_root_for_tab(active_tab_id)
            .ok_or(RESULT_MISSING)?;
        let popup_id = pipe_message
            .payload
            .as_deref()
            .ok_or(RESULT_INVALID_PAYLOAD)?;
        let payload =
            workspace_popup_payload(popup_id, workspace_root).ok_or(RESULT_INVALID_PAYLOAD)?;
        let plugin_url = self.popup_plugin_url.as_deref().ok_or(RESULT_MISSING)?;
        let destination_plugin_id = self
            .last_pane_manifest
            .as_ref()
            .and_then(|manifest| {
                workspace_popup_destination_id(
                    plugin_url,
                    manifest
                        .panes
                        .values()
                        .flatten()
                        .map(|pane| (pane.id, pane.exited, pane.plugin_url.as_deref())),
                )
            })
            .ok_or(RESULT_MISSING)?;

        Ok(MessageToPlugin::new(action)
            .with_destination_plugin_id(destination_plugin_id)
            .with_payload(payload))
    }

    fn workspace_yazi_pane_id_by_tab(&self) -> HashMap<usize, String> {
        let Some(expected_title) = self.workspace_yazi_pane_title.as_deref() else {
            return HashMap::new();
        };
        self.tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .filter_map(|(tab_id, panes)| {
                panes
                    .iter()
                    .find(|pane| pane.is_floating && pane.title.trim() == expected_title)
                    .and_then(|pane| pane_id_to_string(Some(pane.pane_id)))
                    .map(|pane_id| (*tab_id, pane_id))
            })
            .collect()
    }

    fn workspace_root_for_tab(&self, tab_id: usize) -> Option<&str> {
        self.workspace_state_by_tab
            .get(&tab_id)
            .or(self.initial_workspace_state.as_ref())
            .map(|workspace| workspace.root.as_str())
    }

    fn launch_zoxide_picker(&self, receiver: &str) {
        let Some(yazi_cli) = self.yazi_cli.as_deref() else {
            return;
        };
        let command = [yazi_cli, "emit-to", receiver, "plugin", "zoxide-editor"];
        run_command_with_env_variables_and_cwd(
            &command,
            get_session_environment_variables(),
            self.runtime_dir.clone(),
            BTreeMap::new(),
        );
    }
}

impl WorkspaceState {
    pub(crate) fn from_bootstrap_root(root: String) -> Self {
        Self {
            root,
            source: WorkspaceStateSource::Bootstrap,
        }
    }
}

pub(crate) fn tab_name_from_workspace_root(workspace_root: &str) -> String {
    let trimmed = workspace_root.trim_end_matches(std::path::MAIN_SEPARATOR);
    let candidate = if trimmed.is_empty() {
        workspace_root
    } else {
        trimmed
    };

    Path::new(candidate)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("unnamed")
        .to_string()
}

pub(crate) fn tab_index_from_position(tab_position: usize) -> u32 {
    // Zellij reports tabs to plugins by 0-based position, but rename_tab targets the 1-based tab index.
    u32::try_from(tab_position + 1).expect("tab position should fit in u32")
}

fn change_directory_command(path: &str) -> String {
    format!("cd \"{}\"", escape_double_quoted_path(path))
}

fn escape_double_quoted_path(path: &str) -> String {
    path.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::bootstrap_workspace_root;
    use std::path::PathBuf;

    // Defends: bootstrap workspace state starts from the initial cwd instead of probing plugin-local filesystem state.
    #[test]
    fn bootstrap_workspace_root_uses_initial_cwd() {
        let initial_cwd = PathBuf::from("/tmp/restarted-project");

        let result = bootstrap_workspace_root(&initial_cwd);

        assert_eq!(result, "/tmp/restarted-project");
    }
}
