use std::collections::HashSet;

use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivity;
use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivityState;
use yazelix_zellij_pane_orchestrator::ai_pane_activity_contract::{
    normalized_ai_activity_state, upsert_ai_pane_activity_fact, AiPaneActivityRegistration,
};
use zellij_tile::prelude::*;

use crate::panes::pane_id_to_string;
use crate::{State, RESULT_DENIED, RESULT_INVALID_PAYLOAD, RESULT_MISSING, RESULT_OK};

impl State {
    pub(crate) fn reconcile_ai_pane_activity_tabs(&mut self, tabs: &[TabInfo]) {
        let current_tab_ids = tabs.iter().map(|tab| tab.tab_id).collect::<HashSet<_>>();
        self.ai_pane_activity_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
    }

    pub(crate) fn reconcile_ai_pane_activity_panes(&mut self) {
        let pane_ids_by_tab = self
            .tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .map(|(tab_id, panes)| {
                let pane_ids = panes
                    .iter()
                    .filter_map(|pane| pane_id_to_string(Some(pane.pane_id)))
                    .collect::<HashSet<_>>();
                (*tab_id, pane_ids)
            })
            .collect::<std::collections::HashMap<_, _>>();

        self.ai_pane_activity_by_tab
            .retain(|tab_id, activity_facts| {
                let Some(tab_pane_ids) = pane_ids_by_tab.get(tab_id) else {
                    return false;
                };
                activity_facts.retain(|fact| {
                    fact.pane_id.trim().is_empty() || tab_pane_ids.contains(&fact.pane_id)
                });
                true
            });
    }

    pub(crate) fn register_ai_pane_activity(&mut self, pipe_message: &PipeMessage) {
        if !self.permissions_granted {
            self.respond(pipe_message, RESULT_DENIED);
            return;
        }

        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };

        let registration: AiPaneActivityRegistration = match serde_json::from_str(payload) {
            Ok(registration) => registration,
            Err(_) => {
                self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                return;
            }
        };

        let Some(state) = normalized_ai_activity_state(&registration) else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };

        let provider = registration.provider.trim().to_string();
        let pane_id = registration.pane_id.trim().to_string();
        if pane_id.is_empty()
            && matches!(
                state,
                SessionAiPaneActivityState::Active
                    | SessionAiPaneActivityState::Thinking
                    | SessionAiPaneActivityState::Stale
            )
        {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        }

        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        let tab_id = if pane_id.is_empty() {
            active_tab_id
        } else {
            match self.find_tab_id_for_terminal_pane_id(&pane_id) {
                Some(tab_id) => tab_id,
                None => {
                    self.respond(pipe_message, RESULT_MISSING);
                    return;
                }
            }
        };

        let tab_position = self
            .tab_identity
            .position_for_tab_id(tab_id)
            .unwrap_or(tab_id);
        let fact = SessionAiPaneActivity::tab_local(tab_position, provider, pane_id, state);
        upsert_ai_pane_activity_fact(
            self.ai_pane_activity_by_tab.entry(tab_id).or_default(),
            fact,
        );
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn get_active_ai_pane_activity_snapshot(
        &self,
        active_tab_id: usize,
    ) -> Vec<SessionAiPaneActivity> {
        let active_tab_position = self
            .tab_identity
            .position_for_tab_id(active_tab_id)
            .unwrap_or(active_tab_id);
        self.ai_pane_activity_by_tab
            .get(&active_tab_id)
            .cloned()
            .map(|activity_facts| {
                activity_facts
                    .into_iter()
                    .map(|mut fact| {
                        fact.tab_position = Some(active_tab_position);
                        fact
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn find_tab_id_for_terminal_pane_id(&self, pane_id: &str) -> Option<usize> {
        self.tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .find(|(_, panes)| {
                panes
                    .iter()
                    .filter_map(|pane| pane_id_to_string(Some(pane.pane_id)))
                    .any(|candidate| candidate == pane_id)
            })
            .map(|(tab_id, _)| *tab_id)
    }
}
