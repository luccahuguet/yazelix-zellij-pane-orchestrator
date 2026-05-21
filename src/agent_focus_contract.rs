#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentFocusTogglePlan {
    FocusEditor,
    FocusFallback,
    FocusAgent,
    OpenAndFocusAgent,
    CreateAndFocusAgent,
    MissingTarget,
}

pub fn resolve_agent_focus_toggle(
    agent_is_focused: bool,
    has_agent: bool,
    agent_is_closed: bool,
    has_editor: bool,
    has_focus_fallback: bool,
) -> AgentFocusTogglePlan {
    if agent_is_focused && has_editor {
        AgentFocusTogglePlan::FocusEditor
    } else if agent_is_focused && has_focus_fallback {
        AgentFocusTogglePlan::FocusFallback
    } else if agent_is_focused {
        AgentFocusTogglePlan::MissingTarget
    } else if has_agent && agent_is_closed {
        AgentFocusTogglePlan::OpenAndFocusAgent
    } else if has_agent {
        AgentFocusTogglePlan::FocusAgent
    } else {
        AgentFocusTogglePlan::CreateAndFocusAgent
    }
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{resolve_agent_focus_toggle, AgentFocusTogglePlan};

    // Defends: the editor/right-sidebar focus switch does not hide the agent when returning to editor.
    #[test]
    fn focused_agent_returns_to_editor() {
        assert_eq!(
            resolve_agent_focus_toggle(true, true, false, true, true),
            AgentFocusTogglePlan::FocusEditor
        );
    }

    // Defends: a collapsed existing right sidebar is opened and focused rather than recreated.
    #[test]
    fn collapsed_existing_agent_is_opened_and_focused() {
        assert_eq!(
            resolve_agent_focus_toggle(false, true, true, true, true),
            AgentFocusTogglePlan::OpenAndFocusAgent
        );
    }

    // Defends: the focus switch can bootstrap the first right-sidebar agent pane.
    #[test]
    fn missing_agent_is_created_and_focused() {
        assert_eq!(
            resolve_agent_focus_toggle(false, false, false, true, true),
            AgentFocusTogglePlan::CreateAndFocusAgent
        );
    }
}
