use serde::Deserialize;

use crate::active_tab_session_state::{SessionAiPaneActivity, SessionAiPaneActivityState};

pub const TERMINAL_TITLE_ACTIVITY_PROVIDER: &str = "terminal-title";
const ACTIVITY_TITLE_SPINNER_CHARS: &str = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AiPaneActivityRegistration {
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub pane_id: String,
    #[serde(default)]
    pub activity: String,
    #[serde(default)]
    pub state: Option<SessionAiPaneActivityState>,
}

pub fn normalized_ai_activity_state(
    registration: &AiPaneActivityRegistration,
) -> Option<SessionAiPaneActivityState> {
    if let Some(state) = registration.state {
        return Some(state);
    }
    let activity = registration.activity.trim();
    if activity.is_empty() {
        return Some(SessionAiPaneActivityState::Unknown);
    }
    SessionAiPaneActivityState::from_activity(activity)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiActivityTabState {
    Idle,
    Busy,
    Alert,
}

pub fn ai_activity_tab_state(facts: &[SessionAiPaneActivity]) -> AiActivityTabState {
    if facts
        .iter()
        .any(|fact| fact.state == SessionAiPaneActivityState::Stale)
    {
        return AiActivityTabState::Alert;
    }

    if facts.iter().any(|fact| {
        matches!(
            fact.state,
            SessionAiPaneActivityState::Active | SessionAiPaneActivityState::Thinking
        )
    }) {
        return AiActivityTabState::Busy;
    }

    AiActivityTabState::Idle
}

pub fn terminal_title_activity_state(title: &str) -> Option<SessionAiPaneActivityState> {
    if terminal_activity_title_base(title).is_some() {
        return Some(SessionAiPaneActivityState::Active);
    }

    None
}

pub fn managed_terminal_title_activity_state(
    title: &str,
    terminal_command: Option<&str>,
    command_marker: Option<&str>,
) -> Option<SessionAiPaneActivityState> {
    if command_marker.is_some()
        && !terminal_command_matches_marker(terminal_command, command_marker)
    {
        return None;
    }
    terminal_title_activity_state(title)
}

pub fn terminal_command_matches_marker(
    terminal_command: Option<&str>,
    command_marker: Option<&str>,
) -> bool {
    command_marker
        .map(str::trim)
        .filter(|marker| !marker.is_empty())
        .is_some_and(|marker| {
            terminal_command
                .map(str::trim)
                .is_some_and(|command| command.contains(marker))
        })
}

pub fn terminal_activity_title_base(title: &str) -> Option<&str> {
    let title = title.trim();
    let (spinner, base_name) = title.split_once(' ')?;
    let mut chars = spinner.chars();
    let spinner = chars.next()?;
    if chars.next().is_some() || !ACTIVITY_TITLE_SPINNER_CHARS.contains(spinner) {
        return None;
    }
    let base_name = base_name.trim();
    (!base_name.is_empty()).then_some(base_name)
}

pub fn upsert_ai_pane_activity_fact(
    facts: &mut Vec<SessionAiPaneActivity>,
    fact: SessionAiPaneActivity,
) {
    if fact.pane_id.trim().is_empty() {
        facts.retain(|existing| !existing.pane_id.trim().is_empty());
        facts.push(fact);
        return;
    }

    if let Some(existing) = facts
        .iter_mut()
        .find(|existing| existing.pane_id == fact.pane_id && existing.provider == fact.provider)
    {
        *existing = fact;
    } else {
        facts.push(fact);
    }
}

pub fn remove_ai_pane_activity_fact(
    facts: &mut Vec<SessionAiPaneActivity>,
    provider: &str,
    pane_id: &str,
) {
    facts.retain(|existing| existing.provider != provider || existing.pane_id != pane_id);
}

#[cfg(test)]
mod tests {
    // Test lane: default
    use super::*;

    fn registration(activity: &str) -> AiPaneActivityRegistration {
        AiPaneActivityRegistration {
            provider: "codex".into(),
            pane_id: "terminal:5".into(),
            activity: activity.into(),
            state: None,
        }
    }

    // Defends: legacy activity tokens map into the normalized status-bus state taxonomy.
    #[test]
    fn normalizes_legacy_ai_activity_tokens_to_status_states() {
        assert_eq!(
            normalized_ai_activity_state(&registration("streaming")),
            Some(SessionAiPaneActivityState::Active)
        );
        assert_eq!(
            normalized_ai_activity_state(&registration("thinking")),
            Some(SessionAiPaneActivityState::Thinking)
        );
        assert_eq!(normalized_ai_activity_state(&registration("busy")), None);
    }

    // Defends: repeated activity observations update a tab-local provider/pane fact instead of duplicating it.
    #[test]
    fn upserts_ai_activity_by_provider_and_pane_identity() {
        let mut facts = vec![SessionAiPaneActivity::tab_local(
            1,
            "codex".into(),
            "terminal:5".into(),
            SessionAiPaneActivityState::Active,
        )];

        upsert_ai_pane_activity_fact(
            &mut facts,
            SessionAiPaneActivity::tab_local(
                1,
                "codex".into(),
                "terminal:5".into(),
                SessionAiPaneActivityState::Thinking,
            ),
        );

        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].state, SessionAiPaneActivityState::Thinking);
    }

    // Defends: tab activity reduces pane activity to alert before busy before idle.
    #[test]
    fn ai_activity_tab_state_prioritizes_alert_then_busy_then_idle() {
        let active = SessionAiPaneActivity::tab_local(
            1,
            "codex".into(),
            "terminal:5".into(),
            SessionAiPaneActivityState::Active,
        );
        let thinking = SessionAiPaneActivity::tab_local(
            1,
            "codex".into(),
            "terminal:5".into(),
            SessionAiPaneActivityState::Thinking,
        );
        let inactive = SessionAiPaneActivity::tab_local(
            1,
            "codex".into(),
            "terminal:5".into(),
            SessionAiPaneActivityState::Inactive,
        );
        let stale = SessionAiPaneActivity::tab_local(
            1,
            "codex".into(),
            "terminal:5".into(),
            SessionAiPaneActivityState::Stale,
        );

        assert_eq!(
            ai_activity_tab_state(&[active.clone()]),
            AiActivityTabState::Busy
        );
        assert_eq!(ai_activity_tab_state(&[thinking]), AiActivityTabState::Busy);
        assert_eq!(
            ai_activity_tab_state(&[active, stale]),
            AiActivityTabState::Alert
        );
        assert_eq!(ai_activity_tab_state(&[inactive]), AiActivityTabState::Idle);
        assert_eq!(ai_activity_tab_state(&[]), AiActivityTabState::Idle);
    }

    // Regression: terminal-title activity clears when the spinner title disappears, even if the pane is unfocused.
    #[test]
    fn terminal_title_activity_clears_when_spinner_title_disappears() {
        assert_eq!(
            terminal_title_activity_state("⠋ yazelix"),
            Some(SessionAiPaneActivityState::Active)
        );
        assert_eq!(terminal_title_activity_state("yazelix"), None);
    }

    #[test]
    fn managed_terminal_title_activity_requires_the_configured_command() {
        let command = "/nix/store/agent/bin/yzx-agent";

        assert_eq!(
            managed_terminal_title_activity_state("⠋ Codex", Some(command), Some(command)),
            Some(SessionAiPaneActivityState::Active)
        );
        assert_eq!(
            managed_terminal_title_activity_state("⠋ Codex", Some("/bin/codex"), Some(command),),
            None
        );
        assert_eq!(
            managed_terminal_title_activity_state("Codex", Some(command), Some(command)),
            None
        );
    }
}
