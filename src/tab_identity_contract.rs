use std::collections::{HashMap, HashSet};

use zellij_tile::prelude::TabInfo;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabIdentityState {
    active_tab_position: Option<usize>,
    active_tab_id: Option<usize>,
    tab_id_by_position: HashMap<usize, usize>,
    tab_position_by_id: HashMap<usize, usize>,
}

impl TabIdentityState {
    pub fn from_tabs(tabs: &[TabInfo]) -> Self {
        Self {
            active_tab_position: active_tab_position(tabs),
            active_tab_id: active_tab_id(tabs),
            tab_id_by_position: tab_id_by_position(tabs),
            tab_position_by_id: tab_position_by_id(tabs),
        }
    }

    pub fn active_tab_position(&self) -> Option<usize> {
        self.active_tab_position
    }

    pub fn active_tab_id(&self) -> Option<usize> {
        self.active_tab_id
    }

    pub fn position_for_tab_id(&self, tab_id: usize) -> Option<usize> {
        self.tab_position_by_id.get(&tab_id).copied()
    }

    pub fn tab_id_by_position(&self) -> &HashMap<usize, usize> {
        &self.tab_id_by_position
    }

    pub fn current_tab_ids(&self) -> HashSet<usize> {
        self.tab_position_by_id.keys().copied().collect()
    }

    pub fn has_complete_position_map<I>(&self, tab_positions: I, position_count: usize) -> bool
    where
        I: IntoIterator<Item = usize>,
    {
        !self.tab_id_by_position.is_empty()
            && position_count == self.tab_id_by_position.len()
            && tab_positions
                .into_iter()
                .all(|position| self.tab_id_by_position.contains_key(&position))
    }
}

pub fn active_tab_id(tabs: &[TabInfo]) -> Option<usize> {
    tabs.iter().find(|tab| tab.active).map(|tab| tab.tab_id)
}

pub fn active_tab_position(tabs: &[TabInfo]) -> Option<usize> {
    tabs.iter().find(|tab| tab.active).map(|tab| tab.position)
}

pub fn current_tab_ids(tabs: &[TabInfo]) -> HashSet<usize> {
    tabs.iter().map(|tab| tab.tab_id).collect()
}

pub fn tab_id_by_position(tabs: &[TabInfo]) -> HashMap<usize, usize> {
    tabs.iter().map(|tab| (tab.position, tab.tab_id)).collect()
}

pub fn tab_position_by_id(tabs: &[TabInfo]) -> HashMap<usize, usize> {
    tabs.iter().map(|tab| (tab.tab_id, tab.position)).collect()
}

pub fn retain_current_tab_state<T>(
    state_by_tab_id: &mut HashMap<usize, T>,
    current_tab_ids: &HashSet<usize>,
) {
    state_by_tab_id.retain(|tab_id, _| current_tab_ids.contains(tab_id));
}

pub fn position_pane_identity_conflicts_with_cached_tabs(
    current_pane_ids_by_position: &HashMap<usize, HashSet<u32>>,
    tab_id_by_position: &HashMap<usize, usize>,
    previous_pane_ids_by_tab: &HashMap<usize, HashSet<u32>>,
) -> bool {
    if previous_pane_ids_by_tab.len() < 2 {
        return false;
    }

    current_pane_ids_by_position
        .iter()
        .any(|(tab_position, current_pane_ids)| {
            if current_pane_ids.is_empty() {
                return false;
            }

            let Some(expected_tab_id) = tab_id_by_position.get(tab_position) else {
                return false;
            };
            let Some(expected_previous_pane_ids) = previous_pane_ids_by_tab.get(expected_tab_id)
            else {
                return false;
            };
            if expected_previous_pane_ids.is_empty()
                || !expected_previous_pane_ids.is_disjoint(current_pane_ids)
            {
                return false;
            }

            previous_pane_ids_by_tab
                .iter()
                .any(|(tab_id, previous_pane_ids)| {
                    tab_id != expected_tab_id && !previous_pane_ids.is_disjoint(current_pane_ids)
                })
        })
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::{
        active_tab_id, active_tab_position, current_tab_ids,
        position_pane_identity_conflicts_with_cached_tabs, retain_current_tab_state,
        tab_id_by_position, tab_position_by_id, TabIdentityState,
    };
    use std::collections::{HashMap, HashSet};
    use zellij_tile::prelude::TabInfo;

    fn tab(position: usize, tab_id: usize, active: bool) -> TabInfo {
        TabInfo {
            position,
            tab_id,
            active,
            ..TabInfo::default()
        }
    }

    // Regression: closing an earlier Zellij tab compacts positions, but ownership must stay attached to stable tab IDs.
    #[test]
    fn tab_identity_survives_position_compaction() {
        let tabs_after_first_tab_closes = [tab(0, 20, true), tab(1, 30, false)];

        assert_eq!(active_tab_id(&tabs_after_first_tab_closes), Some(20));
        assert_eq!(active_tab_position(&tabs_after_first_tab_closes), Some(0));
        assert_eq!(
            tab_id_by_position(&tabs_after_first_tab_closes),
            HashMap::from([(0, 20), (1, 30)])
        );
        assert_eq!(
            tab_position_by_id(&tabs_after_first_tab_closes),
            HashMap::from([(20, 0), (30, 1)])
        );

        let identity = TabIdentityState::from_tabs(&tabs_after_first_tab_closes);
        assert_eq!(identity.active_tab_id(), Some(20));
        assert_eq!(identity.active_tab_position(), Some(0));
        assert_eq!(identity.position_for_tab_id(30), Some(1));
        assert!(identity.has_complete_position_map([0, 1], 2));
    }

    // Regression: retaining tab-local state after compaction must delete closed IDs without shifting surviving state left.
    #[test]
    fn retaining_tab_id_state_does_not_reassign_surviving_tabs() {
        let mut workspace_root_by_tab_id =
            HashMap::from([(10, "/ferox"), (20, "/yazelix"), (30, "/yazelix-terminal")]);
        let current_tab_ids = current_tab_ids(&[tab(0, 20, true), tab(1, 30, false)]);

        retain_current_tab_state(&mut workspace_root_by_tab_id, &current_tab_ids);

        assert_eq!(
            workspace_root_by_tab_id,
            HashMap::from([(20, "/yazelix"), (30, "/yazelix-terminal")])
        );
    }

    // Regression: a PaneUpdate can arrive after a tab move but before the matching TabUpdate; in that window
    // the old position map must not reassign pane-local activity to the tab that moved into the old slot.
    #[test]
    fn detects_moved_tab_pane_identity_against_stale_position_map() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let stale_tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let moved_panes_by_position =
            HashMap::from([(0, HashSet::from([2])), (1, HashSet::from([1]))]);

        assert!(position_pane_identity_conflicts_with_cached_tabs(
            &moved_panes_by_position,
            &stale_tab_id_by_position,
            &previous_panes,
        ));
    }

    // Defends: ordinary pane additions within the same tab order still rebuild immediately.
    #[test]
    fn accepts_same_order_pane_identity_with_added_pane() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let same_order_panes_by_position =
            HashMap::from([(0, HashSet::from([1, 3])), (1, HashSet::from([2]))]);

        assert!(!position_pane_identity_conflicts_with_cached_tabs(
            &same_order_panes_by_position,
            &tab_id_by_position,
            &previous_panes,
        ));
    }

    // Defends: replacing panes inside a tab is not treated as tab reordering unless the pane ids match another tab.
    #[test]
    fn accepts_replaced_pane_identity_without_cross_tab_match() {
        let previous_panes = HashMap::from([(10, HashSet::from([1])), (20, HashSet::from([2]))]);
        let tab_id_by_position = HashMap::from([(0, 10), (1, 20)]);
        let replaced_panes_by_position =
            HashMap::from([(0, HashSet::from([3])), (1, HashSet::from([2]))]);

        assert!(!position_pane_identity_conflicts_with_cached_tabs(
            &replaced_panes_by_position,
            &tab_id_by_position,
            &previous_panes,
        ));
    }
}
