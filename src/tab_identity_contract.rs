use std::collections::{HashMap, HashSet};

use zellij_tile::prelude::TabInfo;

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

// Test lane: default
#[cfg(test)]
mod tests {
    use super::{
        active_tab_id, active_tab_position, current_tab_ids, retain_current_tab_state,
        tab_id_by_position, tab_position_by_id,
    };
    use std::collections::HashMap;
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
}
