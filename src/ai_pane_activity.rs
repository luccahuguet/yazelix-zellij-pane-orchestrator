use std::collections::HashSet;
use std::time::{Duration, Instant};

use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivity;
use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivityState;
use yazelix_zellij_pane_orchestrator::ai_pane_activity_contract::{
    ai_activity_tab_decoration_state, ai_activity_tab_decoration_write_deadline,
    normalized_ai_activity_state, plan_ai_activity_tab_name, remove_ai_pane_activity_fact,
    terminal_title_activity_poll_needed, terminal_title_activity_state,
    upsert_ai_pane_activity_fact, AiActivityTabNamePlan, AiPaneActivityRegistration,
    AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL, TERMINAL_TITLE_ACTIVITY_PROVIDER,
};
use yazelix_zellij_pane_orchestrator::tab_activity_snapshot_contract::{
    build_all_tab_activity_snapshot_v1, AllTabActivitySnapshotV1, TabActivityReadState,
};
use zellij_tile::prelude::*;

use crate::panes::pane_id_to_string;
use crate::{State, RESULT_DENIED, RESULT_INVALID_PAYLOAD, RESULT_MISSING, RESULT_OK};

struct AiActivityTabDecorationPlan {
    tab_id: usize,
    current_name: String,
    name_plan: AiActivityTabNamePlan,
}

const TERMINAL_TITLE_ACTIVITY_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);

impl State {
    pub(crate) fn reconcile_ai_pane_activity_tabs(&mut self, tabs: &[TabInfo]) {
        let current_tab_ids = tabs.iter().map(|tab| tab.tab_id).collect::<HashSet<_>>();
        self.ai_pane_activity_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        self.ai_activity_tab_base_name_by_tab
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        self.reconcile_terminal_title_ai_activity();
        self.sync_ai_activity_tab_decorations_for_tabs(tabs);
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
        self.sync_ai_activity_tab_decorations_for_known_tabs();
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
        self.sync_ai_activity_tab_decoration_for_tab(tab_id);
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
                    base_name: self.ai_activity_tab_base_name_by_tab.get(tab_id).cloned(),
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
        let observations = self
            .tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .flat_map(|(tab_id, panes)| {
                panes
                    .iter()
                    .map(move |pane| (*tab_id, pane.pane_id, pane.title.clone(), pane.is_focused))
            })
            .collect::<Vec<_>>();

        for (tab_id, pane_id, title, is_focused) in observations {
            self.reconcile_terminal_title_activity_observation(tab_id, pane_id, title, is_focused);
        }

        self.retain_nonempty_ai_activity_tabs();
        self.schedule_terminal_title_activity_reconcile_if_needed();
    }

    fn reconcile_terminal_title_ai_activity_from_host_pane_info(&mut self) {
        let poll_candidates = self.terminal_title_activity_poll_candidates();
        for (tab_id, pane_id) in poll_candidates {
            let Some(pane_info) = get_pane_info(pane_id) else {
                self.remove_terminal_title_activity_fact_for_pane(pane_id);
                continue;
            };
            if pane_info.is_plugin {
                self.remove_terminal_title_activity_fact_for_pane(pane_id);
                continue;
            }

            self.update_cached_terminal_pane_info(tab_id, pane_id, &pane_info);
            self.reconcile_terminal_title_activity_observation(
                tab_id,
                pane_id,
                pane_info.title,
                pane_info.is_focused,
            );
        }

        self.retain_nonempty_ai_activity_tabs();
        self.schedule_terminal_title_activity_reconcile_if_needed();
    }

    fn reconcile_terminal_title_activity_observation(
        &mut self,
        tab_id: usize,
        pane_id: PaneId,
        title: String,
        is_focused: bool,
    ) {
        let Some(pane_id) = pane_id_to_string(Some(pane_id)) else {
            return;
        };
        let previous = self.ai_pane_activity_by_tab.get(&tab_id).and_then(|facts| {
            facts
                .iter()
                .find(|fact| {
                    fact.provider == TERMINAL_TITLE_ACTIVITY_PROVIDER && fact.pane_id == pane_id
                })
                .map(|fact| fact.state)
        });
        let is_active_tab_focus = is_focused && self.tab_identity.active_tab_id() == Some(tab_id);
        let Some(state) = terminal_title_activity_state(previous, &title, is_active_tab_focus)
        else {
            self.remove_terminal_title_activity_fact(tab_id, &pane_id);
            return;
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
    }

    fn terminal_title_activity_poll_candidates(&self) -> Vec<(usize, PaneId)> {
        let active_tab_id = self.tab_identity.active_tab_id();
        let active_title_pane_ids = self.terminal_title_activity_fact_pane_ids();
        self.tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .flat_map(|(tab_id, panes)| {
                panes.iter().filter_map(|pane| {
                    let pane_id = pane_id_to_string(Some(pane.pane_id))?;
                    (Some(*tab_id) != active_tab_id || active_title_pane_ids.contains(&pane_id))
                        .then_some((*tab_id, pane.pane_id))
                })
            })
            .collect()
    }

    fn terminal_title_activity_fact_tab_ids(&self) -> Vec<usize> {
        self.ai_pane_activity_by_tab
            .iter()
            .filter(|(_, facts)| {
                facts
                    .iter()
                    .any(|fact| fact.provider == TERMINAL_TITLE_ACTIVITY_PROVIDER)
            })
            .map(|(tab_id, _)| *tab_id)
            .collect()
    }

    fn terminal_title_activity_fact_pane_ids(&self) -> HashSet<String> {
        self.ai_pane_activity_by_tab
            .values()
            .flat_map(|facts| {
                facts
                    .iter()
                    .filter(|fact| fact.provider == TERMINAL_TITLE_ACTIVITY_PROVIDER)
                    .map(|fact| fact.pane_id.clone())
            })
            .collect()
    }

    pub(crate) fn schedule_terminal_title_activity_reconcile_if_needed(&mut self) {
        if !self.permissions_granted {
            self.terminal_title_activity_next_reconcile = None;
            return;
        }

        let terminal_pane_tab_ids = self
            .tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .flat_map(|(tab_id, panes)| panes.iter().map(move |_| *tab_id))
            .collect::<Vec<_>>();
        let terminal_title_activity_fact_tab_ids = self.terminal_title_activity_fact_tab_ids();
        if !terminal_title_activity_poll_needed(
            self.tab_identity.active_tab_id(),
            &terminal_pane_tab_ids,
            &terminal_title_activity_fact_tab_ids,
        ) {
            self.terminal_title_activity_next_reconcile = None;
            return;
        }

        let deadline = Instant::now() + TERMINAL_TITLE_ACTIVITY_RECONCILE_INTERVAL;
        if self
            .terminal_title_activity_next_reconcile
            .map(|existing| deadline < existing)
            .unwrap_or(true)
        {
            self.terminal_title_activity_next_reconcile = Some(deadline);
            self.arm_next_timer();
        }
    }

    pub(crate) fn handle_terminal_title_activity_reconcile_timer(&mut self) {
        let Some(deadline) = self.terminal_title_activity_next_reconcile else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }

        self.terminal_title_activity_next_reconcile = None;
        self.reconcile_terminal_title_ai_activity_from_host_pane_info();
        self.sync_ai_activity_tab_decorations_for_known_tabs();
    }

    pub(crate) fn handle_terminal_title_activity_pane_closed(&mut self, pane_id: PaneId) {
        if self.remove_terminal_title_activity_fact_for_pane(pane_id) {
            self.sync_ai_activity_tab_decorations_for_known_tabs();
        }
        self.schedule_terminal_title_activity_reconcile_if_needed();
    }

    pub(crate) fn handle_terminal_title_activity_command_pane_exited(&mut self, terminal_id: u32) {
        let pane_id = PaneId::Terminal(terminal_id);
        let Some(tab_id) = self.find_tab_id_for_terminal_pane(pane_id) else {
            self.remove_terminal_title_activity_fact_for_pane(pane_id);
            self.schedule_terminal_title_activity_reconcile_if_needed();
            return;
        };

        if let Some(pane_info) = get_pane_info(pane_id) {
            self.update_cached_terminal_pane_info(tab_id, pane_id, &pane_info);
            self.reconcile_terminal_title_activity_observation(
                tab_id,
                pane_id,
                pane_info.title,
                pane_info.is_focused,
            );
        } else {
            self.remove_terminal_title_activity_fact_for_pane(pane_id);
        }
        self.retain_nonempty_ai_activity_tabs();
        self.sync_ai_activity_tab_decorations_for_known_tabs();
        self.schedule_terminal_title_activity_reconcile_if_needed();
    }

    fn find_tab_id_for_terminal_pane(&self, pane_id: PaneId) -> Option<usize> {
        self.tab_pane_caches
            .terminal_panes_by_tab
            .iter()
            .find(|(_, panes)| panes.iter().any(|pane| pane.pane_id == pane_id))
            .map(|(tab_id, _)| *tab_id)
    }

    fn update_cached_terminal_pane_info(
        &mut self,
        tab_id: usize,
        pane_id: PaneId,
        pane_info: &PaneInfo,
    ) {
        let Some(panes) = self.tab_pane_caches.terminal_panes_by_tab.get_mut(&tab_id) else {
            return;
        };
        let Some(pane) = panes.iter_mut().find(|pane| pane.pane_id == pane_id) else {
            return;
        };

        pane.title.clone_from(&pane_info.title);
        pane.terminal_command
            .clone_from(&pane_info.terminal_command);
        pane.is_focused = pane_info.is_focused;
        pane.is_floating = pane_info.is_floating;
        pane.pane_x = pane_info.pane_x;
        pane.pane_y = pane_info.pane_y;
        pane.pane_columns = pane_info.pane_columns;
        pane.pane_rows = pane_info.pane_rows;
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

    pub(crate) fn sync_ai_activity_tab_decorations_for_tabs(&mut self, tabs: &[TabInfo]) {
        let tab_entries = tabs
            .iter()
            .map(|tab| (tab.tab_id, tab.name.clone()))
            .collect::<Vec<_>>();
        self.sync_ai_activity_tab_decoration_entries(tab_entries);
    }

    pub(crate) fn sync_ai_activity_tab_decorations_for_known_tabs(&mut self) {
        let tab_entries = self
            .tab_name_by_tab_id
            .iter()
            .filter_map(|(tab_id, name)| {
                self.tab_identity
                    .position_for_tab_id(*tab_id)
                    .map(|_| (*tab_id, name.clone()))
            })
            .collect::<Vec<_>>();
        self.sync_ai_activity_tab_decoration_entries(tab_entries);
    }

    fn sync_ai_activity_tab_decoration_for_tab(&mut self, tab_id: usize) {
        if self.tab_identity.position_for_tab_id(tab_id).is_none() {
            return;
        }
        let Some(current_name) = self.tab_name_by_tab_id.get(&tab_id).cloned() else {
            return;
        };
        self.sync_ai_activity_tab_decoration_entries(vec![(tab_id, current_name)]);
    }

    fn sync_ai_activity_tab_decoration_entries(&mut self, tab_entries: Vec<(usize, String)>) {
        if !self.permissions_granted {
            return;
        }

        let plans = tab_entries
            .into_iter()
            .map(|(tab_id, current_name)| {
                self.ai_activity_tab_decoration_plan(tab_id, current_name)
            })
            .collect::<Vec<_>>();

        let needs_native_rename = plans
            .iter()
            .any(|plan| plan.name_plan.display_name != plan.current_name);
        if !needs_native_rename {
            self.apply_ai_activity_tab_decoration_plans(plans);
            return;
        }

        let now = Instant::now();
        if let Some(deadline) = ai_activity_tab_decoration_write_deadline(
            now,
            self.ai_activity_tab_decoration_last_write,
        ) {
            self.schedule_ai_activity_tab_decoration_flush(deadline);
            return;
        }

        self.ai_activity_tab_decoration_next_flush = None;
        let issued_native_rename = self.apply_ai_activity_tab_decoration_plans(plans);
        self.ai_activity_tab_decoration_last_write = Some(now);
        if issued_native_rename {
            self.schedule_ai_activity_tab_decoration_flush(
                now + AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL,
            );
        }
    }

    fn ai_activity_tab_decoration_plan(
        &self,
        tab_id: usize,
        current_name: String,
    ) -> AiActivityTabDecorationPlan {
        let facts = self
            .ai_pane_activity_by_tab
            .get(&tab_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let state = ai_activity_tab_decoration_state(facts);
        let previous_base_name = self
            .ai_activity_tab_base_name_by_tab
            .get(&tab_id)
            .map(String::as_str);
        let name_plan = plan_ai_activity_tab_name(&current_name, previous_base_name, state);

        AiActivityTabDecorationPlan {
            tab_id,
            current_name,
            name_plan,
        }
    }

    fn apply_ai_activity_tab_decoration_plans(
        &mut self,
        plans: Vec<AiActivityTabDecorationPlan>,
    ) -> bool {
        let mut issued_native_rename = false;
        for plan in plans {
            if plan.name_plan.display_name != plan.current_name {
                rename_tab_with_id(plan.tab_id as u64, &plan.name_plan.display_name);
                issued_native_rename = true;
            }
            self.tab_name_by_tab_id
                .insert(plan.tab_id, plan.name_plan.display_name.clone());

            if let Some(base_name) = plan.name_plan.base_name {
                self.ai_activity_tab_base_name_by_tab
                    .insert(plan.tab_id, base_name);
            } else {
                self.ai_activity_tab_base_name_by_tab.remove(&plan.tab_id);
            }
        }
        issued_native_rename
    }

    fn schedule_ai_activity_tab_decoration_flush(&mut self, deadline: Instant) {
        if self
            .ai_activity_tab_decoration_next_flush
            .map(|existing| deadline < existing)
            .unwrap_or(true)
        {
            self.ai_activity_tab_decoration_next_flush = Some(deadline);
            self.arm_next_timer();
        }
    }

    pub(crate) fn handle_ai_activity_tab_decoration_timer(&mut self) {
        let Some(deadline) = self.ai_activity_tab_decoration_next_flush else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }

        self.ai_activity_tab_decoration_next_flush = None;
        self.sync_ai_activity_tab_decorations_for_known_tabs();
    }
}
