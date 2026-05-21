use std::collections::BTreeMap;
use std::thread::sleep;
use std::time::Duration;

use yazelix_zellij_pane_orchestrator::layout_state_contract::AgentState;
use zellij_tile::prelude::*;

use crate::panes::AGENT_TITLE;
use crate::{State, COMMAND_STEP_DELAY_MS, RESULT_MISSING, RESULT_OK, RESULT_UNKNOWN_LAYOUT};

impl State {
    pub(crate) fn toggle_agent_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_position) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if let Some(agent_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_position)
            .and_then(|managed_tab_panes| managed_tab_panes.agent)
        {
            let agent_is_focused = self
                .focused_terminal_pane_by_tab
                .get(&active_tab_position)
                .copied()
                == Some(agent_pane.pane_id);

            if agent_is_focused {
                if self
                    .set_agent_state(active_tab_position, AgentState::Closed)
                    .is_none()
                {
                    self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
                    return;
                }
                self.focus_non_agent_after_hide(active_tab_position);
            } else {
                if self
                    .set_agent_state(active_tab_position, AgentState::Open)
                    .is_none()
                {
                    self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
                    return;
                }
                sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
                focus_pane_with_id(agent_pane.pane_id, false, false);
            }

            self.respond(pipe_message, RESULT_OK);
            return;
        }

        let command_to_run = CommandToRun::new_with_args("yzx", vec!["agent"]);
        let Some(agent_pane_id) = open_command_pane(command_to_run, BTreeMap::new()) else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        rename_pane_with_id(agent_pane_id, AGENT_TITLE);
        if self
            .set_agent_state(active_tab_position, AgentState::Open)
            .is_none()
        {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        }
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        focus_pane_with_id(agent_pane_id, false, false);
        self.respond(pipe_message, RESULT_OK);
    }

    fn focus_non_agent_after_hide(&self, active_tab_position: usize) {
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        if let Some(editor_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_position)
            .and_then(|managed_tab_panes| managed_tab_panes.editor)
        {
            focus_pane_with_id(editor_pane.pane_id, false, false);
            return;
        }

        if let Some(fallback_pane) = self
            .fallback_terminal_pane_by_tab
            .get(&active_tab_position)
            .copied()
        {
            focus_pane_with_id(fallback_pane, false, false);
        }
    }

    pub(crate) fn move_agent_right_after_layout_settle(&self, active_tab_position: usize) {
        sleep(Duration::from_millis(COMMAND_STEP_DELAY_MS));
        if let Some(agent_pane) = self
            .managed_panes_by_tab
            .get(&active_tab_position)
            .and_then(|managed_tab_panes| managed_tab_panes.agent)
        {
            move_pane_with_pane_id_in_direction(agent_pane.pane_id, Direction::Right);
        }
    }
}
