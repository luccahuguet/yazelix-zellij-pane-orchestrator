#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFamilyDirection {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwapStepDirection {
    Next,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwapLayoutStepPlan {
    pub direction: SwapStepDirection,
    pub steps: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutFamily {
    Single,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarState {
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Absent,
    Open,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutVariant {
    pub family: LayoutFamily,
    pub sidebar_state: SidebarState,
    pub agent_state: AgentState,
}

const LAYOUT_ORDER: &[LayoutVariant] = &[
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Absent),
    LayoutVariant::new(
        LayoutFamily::Single,
        SidebarState::Closed,
        AgentState::Absent,
    ),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Open),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Closed),
    LayoutVariant::new(LayoutFamily::Single, SidebarState::Closed, AgentState::Open),
    LayoutVariant::new(
        LayoutFamily::Single,
        SidebarState::Closed,
        AgentState::Closed,
    ),
];

impl LayoutVariant {
    pub const fn new(
        family: LayoutFamily,
        sidebar_state: SidebarState,
        agent_state: AgentState,
    ) -> Self {
        Self {
            family,
            sidebar_state,
            agent_state,
        }
    }

    pub fn layout_name(self) -> &'static str {
        match (self.family, self.sidebar_state, self.agent_state) {
            (LayoutFamily::Single, SidebarState::Open, AgentState::Absent) => "single_open",
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Absent) => "single_closed",
            (LayoutFamily::Single, SidebarState::Open, AgentState::Open) => {
                "single_open_agent_open"
            }
            (LayoutFamily::Single, SidebarState::Open, AgentState::Closed) => {
                "single_open_agent_closed"
            }
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Open) => {
                "single_closed_agent_open"
            }
            (LayoutFamily::Single, SidebarState::Closed, AgentState::Closed) => {
                "single_closed_agent_closed"
            }
        }
    }

    pub fn from_layout_name(layout_name: &str) -> Option<Self> {
        LAYOUT_ORDER
            .iter()
            .copied()
            .find(|variant| variant.layout_name() == layout_name)
    }

    pub fn is_sidebar_closed(self) -> bool {
        self.sidebar_state == SidebarState::Closed
    }

    pub fn agent_is_closed(self) -> Option<bool> {
        match self.agent_state {
            AgentState::Absent => None,
            AgentState::Open => Some(false),
            AgentState::Closed => Some(true),
        }
    }

    pub fn with_sidebar_state(self, sidebar_state: SidebarState) -> Self {
        Self {
            sidebar_state,
            ..self
        }
    }

    pub fn with_agent_state(self, agent_state: AgentState) -> Self {
        Self {
            agent_state,
            ..self
        }
    }

    pub fn with_next_family(self, direction: LayoutFamilyDirection) -> Self {
        Self {
            family: self.family.next(direction),
            ..self
        }
    }
}

impl LayoutFamily {
    fn next(self, _direction: LayoutFamilyDirection) -> Self {
        self
    }
}

pub fn swap_step_plan(current: LayoutVariant, target: LayoutVariant) -> Option<SwapLayoutStepPlan> {
    let current_index = layout_order_index(current)?;
    let target_index = layout_order_index(target)?;
    let layout_count = LAYOUT_ORDER.len();
    let next_steps = if target_index >= current_index {
        target_index - current_index
    } else {
        layout_count - current_index + target_index
    };
    let previous_steps = if current_index >= target_index {
        current_index - target_index
    } else {
        current_index + layout_count - target_index
    };

    if next_steps <= previous_steps {
        Some(SwapLayoutStepPlan {
            direction: SwapStepDirection::Next,
            steps: next_steps,
        })
    } else {
        Some(SwapLayoutStepPlan {
            direction: SwapStepDirection::Previous,
            steps: previous_steps,
        })
    }
}

pub fn swap_step_plan_from_base(target: LayoutVariant) -> Option<SwapLayoutStepPlan> {
    Some(SwapLayoutStepPlan {
        direction: SwapStepDirection::Next,
        steps: layout_order_index(target)? + 1,
    })
}

fn layout_order_index(variant: LayoutVariant) -> Option<usize> {
    LAYOUT_ORDER
        .iter()
        .position(|candidate| *candidate == variant)
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

    // Defends: existing layout names continue to parse as no-agent variants for current sessions.
    #[test]
    fn parses_existing_no_agent_layout_names() {
        assert_eq!(
            LayoutVariant::from_layout_name("single_closed"),
            Some(LayoutVariant::new(
                LayoutFamily::Single,
                SidebarState::Closed,
                AgentState::Absent
            ))
        );
    }

    // Defends: managed-agent layout names carry independent left-sidebar and right-agent state.
    #[test]
    fn parses_agent_layout_names() {
        assert_eq!(
            LayoutVariant::from_layout_name("single_closed_agent_open"),
            Some(LayoutVariant::new(
                LayoutFamily::Single,
                SidebarState::Closed,
                AgentState::Open
            ))
        );
    }

    // Defends: old no-agent sidebar toggles stay inside the no-agent layout block.
    #[test]
    fn no_agent_sidebar_toggle_is_adjacent() {
        let plan = swap_step_plan(
            LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Absent),
            LayoutVariant::new(
                LayoutFamily::Single,
                SidebarState::Closed,
                AgentState::Absent,
            ),
        )
        .unwrap();

        assert_eq!(
            plan,
            SwapLayoutStepPlan {
                direction: SwapStepDirection::Next,
                steps: 1
            }
        );
    }

    // Defends: hiding or showing the right agent sidebar is a one-step swap after the agent exists.
    #[test]
    fn agent_visibility_toggle_is_adjacent() {
        let plan = swap_step_plan(
            LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Open),
            LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Closed),
        )
        .unwrap();

        assert_eq!(
            plan,
            SwapLayoutStepPlan {
                direction: SwapStepDirection::Next,
                steps: 1
            }
        );
    }

    // Regression: BASE sits before the first swap layout, so reaching an agent layout needs one extra next-swap step.
    #[test]
    fn base_to_agent_layout_counts_from_before_first_swap_layout() {
        let target = LayoutVariant::new(LayoutFamily::Single, SidebarState::Open, AgentState::Open);

        assert_eq!(
            swap_step_plan_from_base(target),
            Some(SwapLayoutStepPlan {
                direction: SwapStepDirection::Next,
                steps: 3
            })
        );
    }

    // Defends: layout-family changes are no-ops after removing the bottom-terminal family.
    #[test]
    fn layout_family_switch_is_noop() {
        let current =
            LayoutVariant::new(LayoutFamily::Single, SidebarState::Closed, AgentState::Open);
        let target = current.with_next_family(LayoutFamilyDirection::Next);

        assert_eq!(target, current);
    }
}
