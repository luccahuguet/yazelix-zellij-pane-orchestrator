use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::active_tab_session_state::{SessionAiPaneActivity, SessionAiPaneActivityState};

pub const AI_ACTIVITY_ALERT_TAB_MARKER: &str = "[!] ";
pub const AI_ACTIVITY_BUSY_TAB_MARKER: &str = "[...] ";
pub const AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL: Duration = Duration::from_secs(1);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiActivityTabNamePlan {
    pub display_name: String,
    pub base_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiActivityTabDecorationState {
    Idle,
    Busy,
    Alert,
}

impl AiActivityTabDecorationState {
    fn marker(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Busy => Some(AI_ACTIVITY_BUSY_TAB_MARKER),
            Self::Alert => Some(AI_ACTIVITY_ALERT_TAB_MARKER),
        }
    }
}

pub fn ai_activity_tab_decoration_state(
    facts: &[SessionAiPaneActivity],
) -> AiActivityTabDecorationState {
    if facts
        .iter()
        .any(|fact| fact.state == SessionAiPaneActivityState::Stale)
    {
        return AiActivityTabDecorationState::Alert;
    }

    if facts.iter().any(|fact| {
        matches!(
            fact.state,
            SessionAiPaneActivityState::Active | SessionAiPaneActivityState::Thinking
        )
    }) {
        return AiActivityTabDecorationState::Busy;
    }

    AiActivityTabDecorationState::Idle
}

pub fn terminal_title_activity_state(
    previous: Option<SessionAiPaneActivityState>,
    title: &str,
    is_focused: bool,
) -> Option<SessionAiPaneActivityState> {
    if terminal_activity_title_base(title).is_some() {
        return Some(SessionAiPaneActivityState::Active);
    }

    if is_focused {
        return None;
    }

    previous
        .filter(|state| {
            matches!(
                state,
                SessionAiPaneActivityState::Active
                    | SessionAiPaneActivityState::Thinking
                    | SessionAiPaneActivityState::Stale
            )
        })
        .map(|_| SessionAiPaneActivityState::Stale)
}

pub fn decorate_ai_activity_tab_name(
    tab_name: &str,
    state: AiActivityTabDecorationState,
) -> String {
    match state.marker() {
        Some(marker) => format!("{marker}{tab_name}"),
        None => tab_name.to_string(),
    }
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

fn ai_activity_marker_base(tab_name: &str) -> Option<&str> {
    [AI_ACTIVITY_BUSY_TAB_MARKER, AI_ACTIVITY_ALERT_TAB_MARKER]
        .iter()
        .find_map(|marker| tab_name.strip_prefix(marker))
        .map(str::trim)
        .filter(|base_name| !base_name.is_empty())
}

pub fn plan_ai_activity_tab_name(
    current_name: &str,
    previous_base_name: Option<&str>,
    state: AiActivityTabDecorationState,
) -> AiActivityTabNamePlan {
    if state != AiActivityTabDecorationState::Idle {
        let base_name = previous_base_name
            .filter(|base_name| tab_name_is_activity_decoration_for_base(current_name, base_name))
            .unwrap_or_else(|| {
                terminal_activity_title_base(current_name)
                    .or_else(|| ai_activity_marker_base(current_name))
                    .unwrap_or(current_name)
            })
            .to_string();
        return AiActivityTabNamePlan {
            display_name: decorate_ai_activity_tab_name(&base_name, state),
            base_name: Some(base_name),
        };
    }

    let display_name = previous_base_name
        .filter(|base_name| tab_name_is_activity_decoration_for_base(current_name, base_name))
        .unwrap_or(current_name)
        .to_string();
    AiActivityTabNamePlan {
        display_name,
        base_name: None,
    }
}

fn tab_name_is_activity_decoration_for_base(tab_name: &str, base_name: &str) -> bool {
    tab_name == base_name
        || ai_activity_marker_base(tab_name) == Some(base_name)
        || terminal_activity_title_base(tab_name) == Some(base_name)
}

pub fn ai_activity_tab_decoration_write_deadline(
    now: Instant,
    last_write: Option<Instant>,
) -> Option<Instant> {
    let next_allowed = last_write? + AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL;
    (now < next_allowed).then_some(next_allowed)
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

    // Defends: tab decoration reduces pane activity to alert before busy before idle.
    #[test]
    fn ai_activity_tab_decoration_state_prioritizes_alert_then_busy_then_idle() {
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
            ai_activity_tab_decoration_state(&[active.clone()]),
            AiActivityTabDecorationState::Busy
        );
        assert_eq!(
            ai_activity_tab_decoration_state(&[thinking]),
            AiActivityTabDecorationState::Busy
        );
        assert_eq!(
            ai_activity_tab_decoration_state(&[active, stale]),
            AiActivityTabDecorationState::Alert
        );
        assert_eq!(
            ai_activity_tab_decoration_state(&[inactive]),
            AiActivityTabDecorationState::Idle
        );
        assert_eq!(
            ai_activity_tab_decoration_state(&[]),
            AiActivityTabDecorationState::Idle
        );
    }

    // Defends: terminal title activity persists as an alert until the user focuses the producing pane.
    #[test]
    fn terminal_title_activity_becomes_stale_until_producing_pane_focus_acknowledges_it() {
        assert_eq!(
            terminal_title_activity_state(None, "⠋ yazelix", false),
            Some(SessionAiPaneActivityState::Active)
        );
        assert_eq!(
            terminal_title_activity_state(
                Some(SessionAiPaneActivityState::Active),
                "yazelix",
                false
            ),
            Some(SessionAiPaneActivityState::Stale)
        );
        assert_eq!(
            terminal_title_activity_state(
                Some(SessionAiPaneActivityState::Stale),
                "yazelix",
                false
            ),
            Some(SessionAiPaneActivityState::Stale)
        );
        assert_eq!(
            terminal_title_activity_state(Some(SessionAiPaneActivityState::Stale), "yazelix", true),
            None
        );
        assert_eq!(
            terminal_title_activity_state(
                Some(SessionAiPaneActivityState::Active),
                "yazelix",
                true
            ),
            None
        );
    }

    // Defends: tab-level activity decoration preserves the user's base tab name and clears only Yazelix's own marker.
    #[test]
    fn tab_name_plan_decorates_and_restores_activity_marker() {
        let busy =
            plan_ai_activity_tab_name("yazelix-terminal", None, AiActivityTabDecorationState::Busy);
        assert_eq!(
            busy,
            AiActivityTabNamePlan {
                display_name: "[...] yazelix-terminal".into(),
                base_name: Some("yazelix-terminal".into()),
            }
        );

        let still_busy = plan_ai_activity_tab_name(
            "[...] yazelix-terminal",
            Some("yazelix-terminal"),
            AiActivityTabDecorationState::Busy,
        );
        assert_eq!(still_busy, busy);

        let inactive = plan_ai_activity_tab_name(
            "[...] yazelix-terminal",
            Some("yazelix-terminal"),
            AiActivityTabDecorationState::Idle,
        );
        assert_eq!(
            inactive,
            AiActivityTabNamePlan {
                display_name: "yazelix-terminal".into(),
                base_name: None,
            }
        );

        let alert = plan_ai_activity_tab_name(
            "[...] yazelix-terminal",
            Some("yazelix-terminal"),
            AiActivityTabDecorationState::Alert,
        );
        assert_eq!(
            alert,
            AiActivityTabNamePlan {
                display_name: "[!] yazelix-terminal".into(),
                base_name: Some("yazelix-terminal".into()),
            }
        );

        let renamed_while_busy = plan_ai_activity_tab_name(
            "scratch",
            Some("yazelix-terminal"),
            AiActivityTabDecorationState::Busy,
        );
        assert_eq!(
            renamed_while_busy,
            AiActivityTabNamePlan {
                display_name: "[...] scratch".into(),
                base_name: Some("scratch".into()),
            }
        );

        let inactive_user_marker =
            plan_ai_activity_tab_name("[...] scratch", None, AiActivityTabDecorationState::Idle);
        assert_eq!(
            inactive_user_marker,
            AiActivityTabNamePlan {
                display_name: "[...] scratch".into(),
                base_name: None,
            }
        );

        let busy_user_marker =
            plan_ai_activity_tab_name("[...] scratch", None, AiActivityTabDecorationState::Busy);
        assert_eq!(
            busy_user_marker,
            AiActivityTabNamePlan {
                display_name: "[...] scratch".into(),
                base_name: Some("scratch".into()),
            }
        );

        let restored_user_marker = plan_ai_activity_tab_name(
            "[...] scratch",
            Some("scratch"),
            AiActivityTabDecorationState::Idle,
        );
        assert_eq!(
            restored_user_marker,
            AiActivityTabNamePlan {
                display_name: "scratch".into(),
                base_name: None,
            }
        );
    }

    // Defends: terminal titles that already expose live activity produce a stable tab marker instead of rename churn.
    #[test]
    fn tab_name_plan_uses_terminal_activity_title_as_stable_busy_signal() {
        assert_eq!(terminal_activity_title_base("⠋ yazelix"), Some("yazelix"));
        assert_eq!(terminal_activity_title_base("x yazelix"), None);

        let active_title =
            plan_ai_activity_tab_name("yazelix", None, AiActivityTabDecorationState::Busy);
        assert_eq!(
            active_title,
            AiActivityTabNamePlan {
                display_name: "[...] yazelix".into(),
                base_name: Some("yazelix".into()),
            }
        );

        let next_frame = plan_ai_activity_tab_name(
            "[...] yazelix",
            Some("yazelix"),
            AiActivityTabDecorationState::Busy,
        );
        assert_eq!(
            next_frame,
            AiActivityTabNamePlan {
                display_name: "[...] yazelix".into(),
                base_name: Some("yazelix".into()),
            }
        );

        let restored = plan_ai_activity_tab_name(
            "[...] yazelix",
            Some("yazelix"),
            AiActivityTabDecorationState::Idle,
        );
        assert_eq!(
            restored,
            AiActivityTabNamePlan {
                display_name: "yazelix".into(),
                base_name: None,
            }
        );

        let unrelated_title =
            plan_ai_activity_tab_name("project", None, AiActivityTabDecorationState::Busy);
        assert_eq!(
            unrelated_title,
            AiActivityTabNamePlan {
                display_name: "[...] project".into(),
                base_name: Some("project".into()),
            }
        );

        let restored_project = plan_ai_activity_tab_name(
            "[...] project",
            Some("project"),
            AiActivityTabDecorationState::Idle,
        );
        assert_eq!(
            restored_project,
            AiActivityTabNamePlan {
                display_name: "project".into(),
                base_name: None,
            }
        );

        let old_spinner_tab =
            plan_ai_activity_tab_name("⠋ yazelix", None, AiActivityTabDecorationState::Busy);
        assert_eq!(
            old_spinner_tab,
            AiActivityTabNamePlan {
                display_name: "[...] yazelix".into(),
                base_name: Some("yazelix".into()),
            }
        );
    }

    // Defends: native tab rename side effects have a one-second safety floor.
    #[test]
    fn ai_activity_tab_decoration_write_deadline_rate_limits_rapid_writes() {
        let now = Instant::now();

        assert_eq!(ai_activity_tab_decoration_write_deadline(now, None), None);
        assert_eq!(
            ai_activity_tab_decoration_write_deadline(now, Some(now)),
            Some(now + AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL)
        );
        assert_eq!(
            ai_activity_tab_decoration_write_deadline(
                now + AI_ACTIVITY_TAB_DECORATION_MIN_WRITE_INTERVAL,
                Some(now)
            ),
            None
        );
    }
}
