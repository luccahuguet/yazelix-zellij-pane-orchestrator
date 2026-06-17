#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalDirection {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalPaneRole {
    Sidebar,
    Agent,
    Other,
}

pub fn horizontal_role_for_pane<Id: PartialEq>(
    pane_id: &Id,
    pane_title: &str,
    sidebar_pane_id: Option<&Id>,
    agent_pane_id: Option<&Id>,
) -> HorizontalPaneRole {
    if sidebar_pane_id == Some(pane_id) {
        return HorizontalPaneRole::Sidebar;
    }
    if agent_pane_id == Some(pane_id) {
        return HorizontalPaneRole::Agent;
    }

    match pane_title.trim() {
        "sidebar" => HorizontalPaneRole::Sidebar,
        "agent" => HorizontalPaneRole::Agent,
        _ => HorizontalPaneRole::Other,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizontalPaneSnapshot {
    pub role: HorizontalPaneRole,
    pub is_plugin: bool,
    pub exited: bool,
    pub is_focused: bool,
    pub pane_x: usize,
    pub pane_y: usize,
    pub pane_columns: usize,
    pub pane_rows: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizontalFocusPlan {
    FocusPane(usize),
    PreviousTab,
    NextTab,
    MissingFocusedPane,
}

pub fn resolve_horizontal_focus(
    panes: &[HorizontalPaneSnapshot],
    direction: HorizontalDirection,
    sidebar_is_closed: bool,
    agent_is_closed: bool,
) -> HorizontalFocusPlan {
    let Some((focused_index, focused_pane)) = panes
        .iter()
        .enumerate()
        .find(|(_, pane)| !pane.is_plugin && !pane.exited && pane.is_focused)
    else {
        return HorizontalFocusPlan::MissingFocusedPane;
    };

    let current_left = focused_pane.pane_x;
    let current_right = focused_pane.pane_x + focused_pane.pane_columns;
    let current_top = focused_pane.pane_y;
    let current_bottom = focused_pane.pane_y + focused_pane.pane_rows;

    let candidates = panes
        .iter()
        .enumerate()
        .filter(|(index, _pane)| *index != focused_index)
        .filter(|(_, pane)| !pane.is_plugin && !pane.exited)
        .filter(|(_, pane)| !(sidebar_is_closed && pane.role == HorizontalPaneRole::Sidebar))
        .filter(|(_, pane)| !(agent_is_closed && pane.role == HorizontalPaneRole::Agent))
        .filter_map(|(index, pane)| {
            let candidate_left = pane.pane_x;
            let candidate_right = pane.pane_x + pane.pane_columns;
            let overlap_top = current_top.max(pane.pane_y);
            let overlap_bottom = current_bottom.min(pane.pane_y + pane.pane_rows);
            let vertical_overlap = overlap_bottom.saturating_sub(overlap_top);

            if vertical_overlap == 0 {
                return None;
            }

            let edge_distance = match direction {
                HorizontalDirection::Left if candidate_right <= current_left => {
                    Some(current_left - candidate_right)
                }
                HorizontalDirection::Right if candidate_left >= current_right => {
                    Some(candidate_left - current_right)
                }
                _ => None,
            }?;

            Some((index, edge_distance, vertical_overlap))
        });

    let best = match direction {
        HorizontalDirection::Left => {
            candidates.min_by_key(|(_, edge_distance, vertical_overlap)| {
                (*edge_distance, usize::MAX - *vertical_overlap)
            })
        }
        HorizontalDirection::Right => {
            candidates.min_by_key(|(_, edge_distance, vertical_overlap)| {
                (*edge_distance, usize::MAX - *vertical_overlap)
            })
        }
    };

    match (direction, best.map(|(index, _, _)| index)) {
        (_, Some(index)) => HorizontalFocusPlan::FocusPane(index),
        (HorizontalDirection::Left, None) => HorizontalFocusPlan::PreviousTab,
        (HorizontalDirection::Right, None) => HorizontalFocusPlan::NextTab,
    }
}

// Test lane: maintainer
#[cfg(test)]
mod tests {
    use super::{
        horizontal_role_for_pane, resolve_horizontal_focus, HorizontalDirection,
        HorizontalFocusPlan, HorizontalPaneRole, HorizontalPaneSnapshot,
    };

    // Defends: leftward focus skips a closed sidebar instead of treating it as a real target.
    #[test]
    fn closed_sidebar_is_skipped_when_walking_left() {
        let panes = [
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Sidebar,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 0,
                pane_y: 0,
                pane_columns: 1,
                pane_rows: 40,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: true,
                pane_x: 1,
                pane_y: 0,
                pane_columns: 80,
                pane_rows: 40,
            },
        ];

        assert_eq!(
            resolve_horizontal_focus(&panes, HorizontalDirection::Left, true, false),
            HorizontalFocusPlan::PreviousTab
        );
    }

    // Defends: an open sidebar remains a valid leftward focus target.
    #[test]
    fn open_sidebar_is_still_a_valid_left_target() {
        let panes = [
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Sidebar,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 0,
                pane_y: 0,
                pane_columns: 24,
                pane_rows: 40,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: true,
                pane_x: 24,
                pane_y: 0,
                pane_columns: 80,
                pane_rows: 40,
            },
        ];

        assert_eq!(
            resolve_horizontal_focus(&panes, HorizontalDirection::Left, false, false),
            HorizontalFocusPlan::FocusPane(0)
        );
    }

    // Defends: a collapsed right-side agent pane is hidden from horizontal focus walking.
    #[test]
    fn closed_agent_is_skipped_when_walking_right() {
        let panes = [
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: true,
                pane_x: 0,
                pane_y: 0,
                pane_columns: 80,
                pane_rows: 40,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Agent,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 80,
                pane_y: 0,
                pane_columns: 1,
                pane_rows: 40,
            },
        ];

        assert_eq!(
            resolve_horizontal_focus(&panes, HorizontalDirection::Right, false, true),
            HorizontalFocusPlan::NextTab
        );
    }

    // Defends: the nearest visible left pane wins even when a hidden sidebar exists farther left.
    #[test]
    fn nearest_visible_left_pane_wins_over_hidden_sidebar() {
        let panes = [
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Sidebar,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 0,
                pane_y: 0,
                pane_columns: 1,
                pane_rows: 40,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 1,
                pane_y: 0,
                pane_columns: 60,
                pane_rows: 40,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: true,
                pane_x: 61,
                pane_y: 0,
                pane_columns: 40,
                pane_rows: 40,
            },
        ];

        assert_eq!(
            resolve_horizontal_focus(&panes, HorizontalDirection::Left, true, false),
            HorizontalFocusPlan::FocusPane(1)
        );
    }

    // Defends: panes without horizontal overlap do not count as left or right focus targets.
    #[test]
    fn panes_without_horizontal_overlap_do_not_count_as_left_or_right_targets() {
        let panes = [
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: true,
                pane_x: 1,
                pane_y: 0,
                pane_columns: 80,
                pane_rows: 20,
            },
            HorizontalPaneSnapshot {
                role: HorizontalPaneRole::Other,
                is_plugin: false,
                exited: false,
                is_focused: false,
                pane_x: 1,
                pane_y: 20,
                pane_columns: 80,
                pane_rows: 20,
            },
        ];

        assert_eq!(
            resolve_horizontal_focus(&panes, HorizontalDirection::Right, true, false),
            HorizontalFocusPlan::NextTab
        );
    }

    // Regression: activity-driven terminal titles must not make a managed agent pane look like
    // an ordinary pane while horizontal navigation is deciding whether to leave the tab.
    #[test]
    fn managed_agent_identity_wins_over_activity_mutated_title() {
        assert_eq!(
            horizontal_role_for_pane(&7, "codex ·", None, Some(&7)),
            HorizontalPaneRole::Agent
        );
    }
}
