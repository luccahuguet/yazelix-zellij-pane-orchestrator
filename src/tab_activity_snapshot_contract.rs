use serde::{Deserialize, Serialize};

use crate::active_tab_session_state::SessionAiPaneActivity;
use crate::ai_pane_activity_contract::{
    ai_activity_tab_decoration_state, AiActivityTabDecorationState,
};

pub const ALL_TAB_ACTIVITY_SNAPSHOT_SCHEMA_VERSION: i32 = 1;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabActivitySnapshotState {
    Idle,
    Busy,
    Alert,
}

impl TabActivitySnapshotState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Busy => "busy",
            Self::Alert => "alert",
        }
    }
}

impl From<AiActivityTabDecorationState> for TabActivitySnapshotState {
    fn from(state: AiActivityTabDecorationState) -> Self {
        match state {
            AiActivityTabDecorationState::Idle => Self::Idle,
            AiActivityTabDecorationState::Busy => Self::Busy,
            AiActivityTabDecorationState::Alert => Self::Alert,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct TabActivitySnapshotTab {
    pub tab_id: usize,
    pub tab_position: usize,
    pub base_name: String,
    pub activity_state: TabActivitySnapshotState,
    #[serde(default)]
    pub activity: Vec<SessionAiPaneActivity>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct AllTabActivitySnapshotV1 {
    pub schema_version: i32,
    pub tabs: Vec<TabActivitySnapshotTab>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabActivityReadState {
    pub tab_id: usize,
    pub tab_position: usize,
    pub current_name: String,
    pub base_name: Option<String>,
    pub activity: Vec<SessionAiPaneActivity>,
}

pub fn build_all_tab_activity_snapshot_v1(
    mut tabs: Vec<TabActivityReadState>,
) -> AllTabActivitySnapshotV1 {
    tabs.sort_by_key(|tab| tab.tab_position);

    AllTabActivitySnapshotV1 {
        schema_version: ALL_TAB_ACTIVITY_SNAPSHOT_SCHEMA_VERSION,
        tabs: tabs
            .into_iter()
            .map(|tab| {
                let activity = tab
                    .activity
                    .into_iter()
                    .map(|mut activity| {
                        activity.tab_position = Some(tab.tab_position);
                        activity
                    })
                    .collect::<Vec<_>>();
                let activity_state = TabActivitySnapshotState::from(
                    ai_activity_tab_decoration_state(activity.as_slice()),
                );

                TabActivitySnapshotTab {
                    tab_id: tab.tab_id,
                    tab_position: tab.tab_position,
                    base_name: tab.base_name.unwrap_or(tab.current_name),
                    activity_state,
                    activity,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    // Test lane: default
    use super::*;
    use crate::active_tab_session_state::{SessionAiPaneActivity, SessionAiPaneActivityState};
    use serde_json::json;

    // Defends: the all-tab activity snapshot is sorted by tab position and reduces activity to alert before busy before idle.
    #[test]
    fn all_tab_activity_snapshot_sorts_tabs_and_prioritizes_state() {
        let snapshot = build_all_tab_activity_snapshot_v1(vec![
            TabActivityReadState {
                tab_id: 30,
                tab_position: 2,
                current_name: "[...] agent".to_string(),
                base_name: Some("agent".to_string()),
                activity: vec![
                    SessionAiPaneActivity::tab_local(
                        99,
                        "terminal-title".to_string(),
                        "terminal:5".to_string(),
                        SessionAiPaneActivityState::Active,
                    ),
                    SessionAiPaneActivity::tab_local(
                        99,
                        "codex".to_string(),
                        "terminal:6".to_string(),
                        SessionAiPaneActivityState::Stale,
                    ),
                ],
            },
            TabActivityReadState {
                tab_id: 10,
                tab_position: 0,
                current_name: "editor".to_string(),
                base_name: None,
                activity: vec![],
            },
        ]);

        assert_eq!(
            snapshot.schema_version,
            ALL_TAB_ACTIVITY_SNAPSHOT_SCHEMA_VERSION
        );
        assert_eq!(
            snapshot
                .tabs
                .iter()
                .map(|tab| tab.tab_id)
                .collect::<Vec<_>>(),
            vec![10, 30]
        );
        assert_eq!(
            snapshot.tabs[0].activity_state,
            TabActivitySnapshotState::Idle
        );
        assert_eq!(snapshot.tabs[1].base_name, "agent");
        assert_eq!(
            snapshot.tabs[1].activity_state,
            TabActivitySnapshotState::Alert
        );
        assert_eq!(snapshot.tabs[1].activity[0].tab_position, Some(2));
        assert_eq!(snapshot.tabs[1].activity[1].tab_position, Some(2));
    }

    // Defends: the snapshot schema carries facts and clean tab names without bar or zjstatus presentation syntax.
    #[test]
    fn all_tab_activity_snapshot_serializes_without_presentation_formatting() {
        let snapshot = build_all_tab_activity_snapshot_v1(vec![TabActivityReadState {
            tab_id: 42,
            tab_position: 1,
            current_name: "agent".to_string(),
            base_name: None,
            activity: vec![SessionAiPaneActivity::tab_local(
                1,
                "terminal-title".to_string(),
                "terminal:12".to_string(),
                SessionAiPaneActivityState::Thinking,
            )],
        }]);

        let value = serde_json::to_value(&snapshot).unwrap();
        let serialized = serde_json::to_string(&snapshot).unwrap();
        let decoded: AllTabActivitySnapshotV1 = serde_json::from_str(&serialized).unwrap();

        assert_eq!(decoded, snapshot);
        assert_eq!(
            value,
            json!({
                "schema_version": ALL_TAB_ACTIVITY_SNAPSHOT_SCHEMA_VERSION,
                "tabs": [
                    {
                        "tab_id": 42,
                        "tab_position": 1,
                        "base_name": "agent",
                        "activity_state": "busy",
                        "activity": [
                            {
                                "tab_position": 1,
                                "provider": "terminal-title",
                                "pane_id": "terminal:12",
                                "activity": "thinking",
                                "state": "thinking"
                            }
                        ]
                    }
                ]
            })
        );
        assert!(!serialized.contains("#["));
        assert!(!serialized.contains("[...]"));
        assert!(!serialized.contains("[!]"));
        assert!(!serialized.contains("zjstatus"));
    }
}
