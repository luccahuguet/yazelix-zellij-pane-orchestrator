use std::thread::sleep;
use std::time::Duration;

use yazelix_zellij_pane_orchestrator::layout_state_contract::{
    swap_step_plan, swap_step_plan_from_base, AgentState, LayoutFamily, LayoutFamilyDirection,
    LayoutVariant, SidebarState, SwapLayoutStepPlan, SwapStepDirection,
};
use yazelix_zellij_pane_orchestrator::pane_contract::FocusContextPolicy;
use yazelix_zellij_pane_orchestrator::sidebar_contract::{
    resolve_sidebar_hide, resolve_sidebar_visibility_toggle, sidebar_close_swap_steps,
    sidebar_post_layout_focus_nudges, SidebarFocusNudgeDirection, SidebarPostLayoutFocus,
    SidebarVisibilityAction,
};
use zellij_tile::prelude::*;

use crate::panes::ManagedTabPanes;
use crate::{State, RESULT_MISSING, RESULT_OK, RESULT_UNKNOWN_LAYOUT, SWAP_LAYOUT_STEP_DELAY_MS};

const BASE_LAYOUT_NAME: &str = "BASE";
const CLOSED_BASE_SIDEBAR_COLUMNS: usize = 2;

impl State {
    pub(crate) fn switch_layout_family(
        &self,
        pipe_message: &PipeMessage,
        direction: LayoutFamilyDirection,
    ) {
        let Some(active_tab_position) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if !self.can_switch_layout_family(active_tab_position) {
            self.respond(pipe_message, RESULT_OK);
            return;
        }

        let Some(layout_variant) = self.layout_variant_for_tab(active_tab_position) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let target_variant = layout_variant.with_next_family(direction);
        if target_variant == layout_variant {
            self.respond(pipe_message, RESULT_OK);
            return;
        }

        self.run_to_layout_variant_for_tab(active_tab_position, layout_variant, target_variant);
        if target_variant.agent_state == AgentState::Open {
            self.move_agent_right_after_layout_settle(active_tab_position);
        }

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn toggle_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_position) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if is_no_sidebar_mode(self.managed_panes_by_tab.get(&active_tab_position)) {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        }

        let Some(sidebar_is_closed) = self.sidebar_is_closed(active_tab_position) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let focus_context = self
            .focus_context_by_tab
            .get(&active_tab_position)
            .copied()
            .unwrap_or(crate::panes::FocusContext::Other);
        let managed_tab_panes = self.managed_panes_by_tab.get(&active_tab_position);
        let has_editor = managed_tab_panes.and_then(|tab| tab.editor).is_some();
        let has_focus_fallback = self
            .fallback_terminal_pane_by_tab
            .get(&active_tab_position)
            .is_some();

        let plan = resolve_sidebar_visibility_toggle(
            sidebar_is_closed,
            match focus_context {
                crate::panes::FocusContext::Editor => FocusContextPolicy::Editor,
                crate::panes::FocusContext::Sidebar => FocusContextPolicy::Sidebar,
                crate::panes::FocusContext::Other => FocusContextPolicy::Other,
            },
            has_editor,
            has_focus_fallback,
        );

        match plan.action {
            SidebarVisibilityAction::Open
                if self.active_layout_is_collapsed_base(active_tab_position) =>
            {
                self.run_next_swap_layout_steps(1)
            }
            SidebarVisibilityAction::Open => {
                self.set_sidebar_state(active_tab_position, SidebarState::Open)
            }
            SidebarVisibilityAction::Close => {
                self.set_sidebar_state(active_tab_position, SidebarState::Closed)
            }
        }
        self.run_sidebar_post_layout_focus(plan.post_layout_focus);

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn hide_sidebar(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_position) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        if is_no_sidebar_mode(self.managed_panes_by_tab.get(&active_tab_position)) {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        }

        let Some(sidebar_is_closed) = self.sidebar_is_closed(active_tab_position) else {
            self.respond(pipe_message, RESULT_UNKNOWN_LAYOUT);
            return;
        };

        let focus_context = self
            .focus_context_by_tab
            .get(&active_tab_position)
            .copied()
            .unwrap_or(crate::panes::FocusContext::Other);
        let managed_tab_panes = self.managed_panes_by_tab.get(&active_tab_position);
        let has_editor = managed_tab_panes.and_then(|tab| tab.editor).is_some();
        let has_focus_fallback = self
            .fallback_terminal_pane_by_tab
            .get(&active_tab_position)
            .is_some();

        if let Some(post_layout_focus) = resolve_sidebar_hide(
            sidebar_is_closed,
            match focus_context {
                crate::panes::FocusContext::Editor => FocusContextPolicy::Editor,
                crate::panes::FocusContext::Sidebar => FocusContextPolicy::Sidebar,
                crate::panes::FocusContext::Other => FocusContextPolicy::Other,
            },
            has_editor,
            has_focus_fallback,
        ) {
            self.set_sidebar_state(active_tab_position, SidebarState::Closed);
            self.run_sidebar_post_layout_focus(post_layout_focus);
        }

        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn get_active_layout_variant(
        &self,
        active_tab_position: usize,
    ) -> Option<LayoutVariant> {
        let active_swap_layout_name = self
            .active_swap_layout_name_by_tab
            .get(&active_tab_position)
            .cloned()
            .flatten();

        active_swap_layout_name
            .as_deref()
            .and_then(LayoutVariant::from_layout_name)
    }

    pub(crate) fn layout_variant_for_tab(
        &self,
        active_tab_position: usize,
    ) -> Option<LayoutVariant> {
        self.get_active_layout_variant(active_tab_position)
            .or_else(|| self.base_layout_variant(active_tab_position))
    }

    pub(crate) fn sidebar_is_closed(&self, active_tab_position: usize) -> Option<bool> {
        self.layout_variant_for_tab(active_tab_position)
            .map(|variant| variant.is_sidebar_closed())
    }

    pub(crate) fn agent_is_closed(&self, active_tab_position: usize) -> Option<bool> {
        self.layout_variant_for_tab(active_tab_position)
            .and_then(|variant| variant.agent_is_closed())
    }

    fn base_layout_sidebar_is_closed(&self, active_tab_position: usize) -> Option<bool> {
        if !self.active_layout_is_base(active_tab_position) {
            return None;
        }
        self.managed_panes_by_tab
            .get(&active_tab_position)
            .and_then(|tab| tab.sidebar)
            .map(|sidebar| sidebar.pane_columns <= CLOSED_BASE_SIDEBAR_COLUMNS)
    }

    fn base_layout_variant(&self, active_tab_position: usize) -> Option<LayoutVariant> {
        if !self.active_layout_is_base(active_tab_position) {
            return None;
        }

        let sidebar_state = if self.base_layout_sidebar_is_closed(active_tab_position)? {
            SidebarState::Closed
        } else {
            SidebarState::Open
        };
        let agent_state = self
            .managed_panes_by_tab
            .get(&active_tab_position)
            .and_then(|tab| tab.agent)
            .map(|_| AgentState::Open)
            .unwrap_or(AgentState::Absent);

        Some(LayoutVariant::new(
            LayoutFamily::Single,
            sidebar_state,
            agent_state,
        ))
    }

    fn active_layout_is_base(&self, active_tab_position: usize) -> bool {
        self.active_swap_layout_name_by_tab
            .get(&active_tab_position)
            .and_then(|layout| layout.as_deref())
            .is_some_and(|layout| layout == BASE_LAYOUT_NAME)
    }

    fn active_layout_is_collapsed_base(&self, active_tab_position: usize) -> bool {
        self.active_layout_is_base(active_tab_position)
            && self.base_layout_sidebar_is_closed(active_tab_position) == Some(true)
    }

    fn can_switch_layout_family(&self, active_tab_position: usize) -> bool {
        let user_pane_count = self
            .user_pane_count_by_tab
            .get(&active_tab_position)
            .copied()
            .unwrap_or(0);

        let managed_tab_panes = self.managed_panes_by_tab.get(&active_tab_position);
        if is_no_sidebar_mode(managed_tab_panes) {
            user_pane_count >= 2
        } else if managed_tab_panes.and_then(|tab| tab.agent).is_some() {
            user_pane_count >= 4
        } else {
            user_pane_count >= 3
        }
    }

    pub(crate) fn run_next_swap_layout_steps(&self, steps: usize) {
        for _ in 0..steps {
            next_swap_layout();
            sleep(Duration::from_millis(SWAP_LAYOUT_STEP_DELAY_MS));
        }
    }

    pub(crate) fn run_previous_swap_layout_steps(&self, steps: usize) {
        for _ in 0..steps {
            previous_swap_layout();
            sleep(Duration::from_millis(SWAP_LAYOUT_STEP_DELAY_MS));
        }
    }

    pub(crate) fn run_to_layout_variant(
        &self,
        current_variant: LayoutVariant,
        target_variant: LayoutVariant,
    ) {
        if let Some(plan) = swap_step_plan(current_variant, target_variant) {
            self.run_swap_step_plan(plan);
        }
    }

    fn run_to_layout_variant_for_tab(
        &self,
        active_tab_position: usize,
        current_variant: LayoutVariant,
        target_variant: LayoutVariant,
    ) {
        if self.active_layout_is_base(active_tab_position) {
            if let Some(plan) = swap_step_plan_from_base(target_variant) {
                self.run_swap_step_plan(plan);
            }
            return;
        }

        self.run_to_layout_variant(current_variant, target_variant);
    }

    pub(crate) fn set_agent_state(
        &self,
        active_tab_position: usize,
        agent_state: AgentState,
    ) -> Option<()> {
        let current_variant = self.layout_variant_for_tab(active_tab_position)?;
        let target_variant = current_variant.with_agent_state(agent_state);
        self.run_to_layout_variant_for_tab(active_tab_position, current_variant, target_variant);
        Some(())
    }

    fn set_sidebar_state(&self, active_tab_position: usize, sidebar_state: SidebarState) {
        if self.active_layout_is_base(active_tab_position) && sidebar_state == SidebarState::Closed
        {
            self.run_next_swap_layout_steps(sidebar_close_swap_steps(true));
            return;
        }

        if let Some(current_variant) = self.layout_variant_for_tab(active_tab_position) {
            let target_variant = current_variant.with_sidebar_state(sidebar_state);
            self.run_to_layout_variant_for_tab(
                active_tab_position,
                current_variant,
                target_variant,
            );
        }
    }

    fn run_swap_step_plan(&self, plan: SwapLayoutStepPlan) {
        match plan.direction {
            SwapStepDirection::Next => self.run_next_swap_layout_steps(plan.steps),
            SwapStepDirection::Previous => self.run_previous_swap_layout_steps(plan.steps),
        }
    }

    pub(crate) fn open_sidebar_and_focus_after_layout_settle(&self) {
        if let Some(active_tab_position) = self.active_tab_position {
            self.open_sidebar_and_focus_after_layout_settle_for_tab(active_tab_position);
            return;
        }
        self.run_previous_swap_layout_steps(1);
        self.run_sidebar_post_layout_focus(SidebarPostLayoutFocus::MoveLeftToSidebar);
    }

    pub(crate) fn open_sidebar_and_focus_after_layout_settle_for_tab(
        &self,
        active_tab_position: usize,
    ) {
        if self.active_layout_is_collapsed_base(active_tab_position) {
            self.run_next_swap_layout_steps(1);
        } else {
            self.set_sidebar_state(active_tab_position, SidebarState::Open);
        }
        self.run_sidebar_post_layout_focus(SidebarPostLayoutFocus::MoveLeftToSidebar);
    }

    fn run_sidebar_post_layout_focus(&self, post_layout_focus: SidebarPostLayoutFocus) {
        for nudge in sidebar_post_layout_focus_nudges(post_layout_focus) {
            sleep(Duration::from_millis(nudge.delay_ms));
            move_focus(match nudge.direction {
                SidebarFocusNudgeDirection::Left => Direction::Left,
                SidebarFocusNudgeDirection::Right => Direction::Right,
            });
        }
    }
}

fn is_no_sidebar_mode(managed_tab_panes: Option<&ManagedTabPanes>) -> bool {
    managed_tab_panes.and_then(|tab| tab.sidebar).is_none()
}
