use std::collections::HashSet;

use serde::Deserialize;
use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivity;
use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivityState;
use yazelix_zellij_pane_orchestrator::ai_pane_activity_contract::{
    managed_terminal_title_activity_state, normalized_ai_activity_state,
    remove_ai_pane_activity_fact, upsert_ai_pane_activity_fact, AiPaneActivityRegistration,
    TERMINAL_TITLE_ACTIVITY_PROVIDER,
};
use yazelix_zellij_pane_orchestrator::tab_activity_snapshot_contract::{
    build_all_tab_activity_snapshot_v1, AllTabActivitySnapshotV1, TabActivityReadState,
};
use zellij_tile::prelude::*;

use crate::panes::pane_id_to_string;
use crate::{State, RESULT_DENIED, RESULT_INVALID_PAYLOAD, RESULT_MISSING, RESULT_OK};

#[derive(Deserialize)]
struct TerminalTitleActivitySnapshotObservation {
    tab_id: usize,
    pane_id: u32,
    #[serde(default)]
    title: String,
}

impl State {
    pub(crate) fn reconcile_ai_pane_activity_tabs(&mut self, tabs: &[TabInfo]) {
        let current_tab_ids = tabs.iter().map(|tab| tab.tab_id).collect::<HashSet<_>>();
        self.ai_pane_activity_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        self.reconcile_terminal_title_ai_activity();
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
        self.reconcile_terminal_title_ai_activity();
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
        self.refresh_status_bar_cache();
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn reconcile_terminal_title_activity_snapshot(
        &mut self,
        pipe_message: &PipeMessage,
    ) {
        if !self.permissions_granted {
            self.respond(pipe_message, RESULT_DENIED);
            return;
        }

        let Some(payload) = pipe_message.payload.as_deref() else {
            self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
            return;
        };

        let observations: Vec<TerminalTitleActivitySnapshotObservation> =
            match serde_json::from_str(payload) {
                Ok(observations) => observations,
                Err(_) => {
                    self.respond(pipe_message, RESULT_INVALID_PAYLOAD);
                    return;
                }
            };

        let mut observed_terminal_panes = HashSet::new();
        for observation in observations {
            if self
                .tab_identity
                .position_for_tab_id(observation.tab_id)
                .is_none()
            {
                continue;
            }
            let pane_id = PaneId::Terminal(observation.pane_id);
            if let Some(pane_key) = pane_id_to_string(Some(pane_id)) {
                observed_terminal_panes.insert((observation.tab_id, pane_key));
            }
            self.reconcile_terminal_title_activity_observation(
                observation.tab_id,
                pane_id,
                observation.title,
                None,
                None,
            );
        }

        self.retain_terminal_title_activity_snapshot_facts(&observed_terminal_panes);
        self.retain_nonempty_ai_activity_tabs();
        self.refresh_status_bar_cache();
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

    pub(crate) fn get_all_tab_activity_state(&self, pipe_message: &PipeMessage) {
        let Some(_) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        let Some(snapshot) = self.all_tab_activity_snapshot() else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        match serde_json::to_string(&snapshot) {
            Ok(serialized) => self.respond(pipe_message, &serialized),
            Err(_) => self.respond(pipe_message, RESULT_INVALID_PAYLOAD),
        }
    }

    pub(crate) fn all_tab_activity_snapshot(&self) -> Option<AllTabActivitySnapshotV1> {
        let tabs = self
            .tab_identity
            .tab_id_by_position()
            .iter()
            .map(|(tab_position, tab_id)| {
                let current_name = self.tab_name_by_tab_id.get(tab_id)?.clone();
                Some(TabActivityReadState {
                    tab_id: *tab_id,
                    tab_position: *tab_position,
                    current_name,
                    base_name: None,
                    active: self.tab_identity.active_tab_id() == Some(*tab_id),
                    is_fullscreen_active: self
                        .tab_fullscreen_active_by_tab
                        .get(tab_id)
                        .copied()
                        .unwrap_or(false),
                    is_sync_panes_active: self
                        .tab_sync_panes_active_by_tab
                        .get(tab_id)
                        .copied()
                        .unwrap_or(false),
                    has_floating_panes: self
                        .tab_pane_caches
                        .terminal_panes_by_tab
                        .get(tab_id)
                        .map(|panes| panes.iter().any(|pane| pane.is_floating))
                        .unwrap_or(false),
                    activity: self
                        .ai_pane_activity_by_tab
                        .get(tab_id)
                        .cloned()
                        .unwrap_or_default(),
                })
            })
            .collect::<Option<Vec<_>>>()?;

        Some(build_all_tab_activity_snapshot_v1(tabs))
    }

    fn reconcile_terminal_title_ai_activity(&mut self) {
        let command_marker = self.managed_agent_command_marker.clone();
        let observations = self
            .tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .flat_map(|(tab_id, panes)| {
                panes.iter().map(move |pane| {
                    (
                        *tab_id,
                        pane.pane_id,
                        pane.title.clone(),
                        pane.terminal_command.clone(),
                    )
                })
            })
            .collect::<Vec<_>>();

        for (tab_id, pane_id, title, terminal_command) in observations {
            self.reconcile_terminal_title_activity_observation(
                tab_id,
                pane_id,
                title,
                terminal_command.as_deref(),
                command_marker.as_deref(),
            );
        }

        self.retain_nonempty_ai_activity_tabs();
    }

    fn reconcile_terminal_title_activity_observation(
        &mut self,
        tab_id: usize,
        pane_id: PaneId,
        title: String,
        terminal_command: Option<&str>,
        command_marker: Option<&str>,
    ) -> bool {
        let Some(pane_id) = pane_id_to_string(Some(pane_id)) else {
            return false;
        };
        let previous = self.ai_pane_activity_by_tab.get(&tab_id).and_then(|facts| {
            facts
                .iter()
                .find(|fact| {
                    fact.provider == TERMINAL_TITLE_ACTIVITY_PROVIDER && fact.pane_id == pane_id
                })
                .map(|fact| fact.state)
        });
        let Some(state) =
            managed_terminal_title_activity_state(&title, terminal_command, command_marker)
        else {
            return self.remove_terminal_title_activity_fact(tab_id, &pane_id);
        };

        let tab_position = self
            .tab_identity
            .position_for_tab_id(tab_id)
            .unwrap_or(tab_id);
        upsert_ai_pane_activity_fact(
            self.ai_pane_activity_by_tab.entry(tab_id).or_default(),
            SessionAiPaneActivity::tab_local(
                tab_position,
                TERMINAL_TITLE_ACTIVITY_PROVIDER.to_string(),
                pane_id,
                state,
            ),
        );
        previous != Some(state)
    }

    pub(crate) fn handle_terminal_title_activity_pane_closed(&mut self, pane_id: PaneId) {
        if self.remove_terminal_title_activity_fact_for_pane(pane_id) {
            self.refresh_status_bar_cache();
        }
    }

    pub(crate) fn handle_terminal_title_activity_command_pane_exited(&mut self, terminal_id: u32) {
        if self.remove_terminal_title_activity_fact_for_pane(PaneId::Terminal(terminal_id)) {
            self.refresh_status_bar_cache();
        }
    }

    fn remove_terminal_title_activity_fact(&mut self, tab_id: usize, pane_id: &str) -> bool {
        let Some(facts) = self.ai_pane_activity_by_tab.get_mut(&tab_id) else {
            return false;
        };

        let previous_len = facts.len();
        remove_ai_pane_activity_fact(facts, TERMINAL_TITLE_ACTIVITY_PROVIDER, pane_id);
        let changed = facts.len() != previous_len;
        let is_empty = facts.is_empty();
        if is_empty {
            self.ai_pane_activity_by_tab.remove(&tab_id);
        }
        changed
    }

    fn remove_terminal_title_activity_fact_for_pane(&mut self, pane_id: PaneId) -> bool {
        let Some(pane_id) = pane_id_to_string(Some(pane_id)) else {
            return false;
        };
        let mut changed = false;
        for facts in self.ai_pane_activity_by_tab.values_mut() {
            let previous_len = facts.len();
            remove_ai_pane_activity_fact(facts, TERMINAL_TITLE_ACTIVITY_PROVIDER, &pane_id);
            changed |= facts.len() != previous_len;
        }
        self.retain_nonempty_ai_activity_tabs();
        changed
    }

    fn retain_nonempty_ai_activity_tabs(&mut self) {
        self.ai_pane_activity_by_tab
            .retain(|_, facts| !facts.is_empty());
    }

    fn retain_terminal_title_activity_snapshot_facts(
        &mut self,
        observed_terminal_panes: &HashSet<(usize, String)>,
    ) {
        for (tab_id, facts) in self.ai_pane_activity_by_tab.iter_mut() {
            facts.retain(|fact| {
                fact.provider != TERMINAL_TITLE_ACTIVITY_PROVIDER
                    || observed_terminal_panes.contains(&(*tab_id, fact.pane_id.clone()))
            });
        }
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
