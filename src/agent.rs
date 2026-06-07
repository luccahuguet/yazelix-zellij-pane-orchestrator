use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use yazelix_zellij_pane_orchestrator::agent_focus_contract::{
    resolve_agent_focus_toggle, AgentFocusTogglePlan,
};
use yazelix_zellij_pane_orchestrator::layout_state_contract::AgentState;
use zellij_tile::prelude::*;

use crate::panes::AGENT_TITLE;
use crate::{
    State, COMMAND_STEP_DELAY_MS, RESULT_FOCUSED_AGENT, RESULT_FOCUSED_EDITOR, RESULT_MISSING,
    RESULT_OK, RESULT_UNKNOWN_LAYOUT,
};

impl State {
    pub(crate) fn toggle_agent_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if let Some(agent_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|managed_tab_panes| managed_tab_panes.agent)
        {
            let agent_is_focused = self
                .focused_terminal_pane_by_tab
                .get(&active_tab_id)
                .copied()
                == Some(agent_pane.pane_id);

            if agent_is_focused {
                if self
                    .set_agent_state(active_tab_id, AgentState::Closed)
                    .is_none()
                {
                    self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
                    return;
                }
                self.focus_non_agent_after_hide(active_tab_id);
            } else {
                if let Err(result) =
                    self.open_existing_agent_sidebar(active_tab_id, agent_pane.pane_id)
                {
                    self.respond(pipe_message, result);
                    return;
                }
            }

            self.respond(pipe_message, RESULT_OK);
            return;
        }

        if let Err(result) = self.create_agent_sidebar(active_tab_id) {
            self.respond(pipe_message, result);
            return;
        }
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn toggle_editor_right_sidebar_focus(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let Some(managed_tab_panes) = self.managed_panes_by_tab.get(&active_tab_id) else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        let agent_is_focused = managed_tab_panes
            .agent
            .map(|agent_pane| {
                self.focused_terminal_pane_by_tab
                    .get(&active_tab_id)
                    .copied()
                    == Some(agent_pane.pane_id)
            })
            .unwrap_or(false);
        let agent_is_closed = self.agent_is_closed(active_tab_id).unwrap_or(false);
        let has_focus_fallback = self
            .fallback_terminal_pane_by_tab
            .get(&active_tab_id)
            .is_some();

        match resolve_agent_focus_toggle(
            agent_is_focused,
            managed_tab_panes.agent.is_some(),
            agent_is_closed,
            managed_tab_panes.editor.is_some(),
            has_focus_fallback,
        ) {
            AgentFocusTogglePlan::FocusEditor => {
                if let Some(editor_pane) = managed_tab_panes.editor {
                    focus_pane_with_id(editor_pane.pane_id, false, false);
                    self.respond(pipe_message, RESULT_FOCUSED_EDITOR);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            AgentFocusTogglePlan::FocusFallback => {
                if let Some(fallback_pane) = self
                    .fallback_terminal_pane_by_tab
                    .get(&active_tab_id)
                    .copied()
                {
                    focus_pane_with_id(fallback_pane, false, false);
                    self.respond(pipe_message, RESULT_OK);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            AgentFocusTogglePlan::FocusAgent => {
                if let Some(agent_pane) = managed_tab_panes.agent {
                    focus_pane_with_id(agent_pane.pane_id, false, false);
                    self.respond(pipe_message, RESULT_FOCUSED_AGENT);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            AgentFocusTogglePlan::OpenAndFocusAgent => {
                if let Some(agent_pane) = managed_tab_panes.agent {
                    match self.open_existing_agent_sidebar(active_tab_id, agent_pane.pane_id) {
                        Ok(()) => self.respond(pipe_message, RESULT_FOCUSED_AGENT),
                        Err(result) => self.respond(pipe_message, result),
                    }
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            AgentFocusTogglePlan::CreateAndFocusAgent => {
                match self.create_agent_sidebar(active_tab_id) {
                    Ok(()) => self.respond(pipe_message, RESULT_FOCUSED_AGENT),
                    Err(result) => self.respond(pipe_message, result),
                }
            }
            AgentFocusTogglePlan::MissingTarget => self.respond(pipe_message, RESULT_MISSING),
        }
    }

    fn create_agent_sidebar(&self, active_tab_id: usize) -> Result<(), &'static str> {
        let Some(right_sidebar_command) = self.right_sidebar_command.as_ref() else {
            return Err(RESULT_MISSING);
        };
        let command_to_run = CommandToRun {
            path: PathBuf::from(&right_sidebar_command.command),
            args: right_sidebar_command.args.clone(),
            cwd: None,
        };
        let Some(agent_pane_id) = open_command_pane(command_to_run, BTreeMap::new()) else {
            return Err(RESULT_MISSING);
        };

        rename_pane_with_id(agent_pane_id, AGENT_TITLE);
        self.open_existing_agent_sidebar(active_tab_id, agent_pane_id)
    }

    fn open_existing_agent_sidebar(
        &self,
        active_tab_id: usize,
        agent_pane_id: PaneId,
    ) -> Result<(), &'static str> {
        if self
            .set_agent_state(active_tab_id, AgentState::Open)
            .is_none()
        {
            return Err(RESULT_UNKNOWN_LAYOUT);
        }
        self.move_agent_pane_right_after_layout_settle(agent_pane_id);
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        focus_pane_with_id(agent_pane_id, false, false);
        Ok(())
    }

    fn focus_non_agent_after_hide(&self, active_tab_id: usize) {
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        if let Some(editor_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|managed_tab_panes| managed_tab_panes.editor)
        {
            focus_pane_with_id(editor_pane.pane_id, false, false);
            return;
        }

        if let Some(fallback_pane) = self
            .fallback_terminal_pane_by_tab
            .get(&active_tab_id)
            .copied()
        {
            focus_pane_with_id(fallback_pane, false, false);
        }
    }

    pub(crate) fn move_agent_right_after_layout_settle(&self, active_tab_id: usize) {
        if let Some(agent_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|managed_tab_panes| managed_tab_panes.agent)
        {
            self.move_agent_pane_right_after_layout_settle(agent_pane.pane_id);
        }
    }

    fn move_agent_pane_right_after_layout_settle(&self, agent_pane_id: PaneId) {
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        move_pane_with_pane_id_in_direction(agent_pane_id, Direction::Right);
    }
}
