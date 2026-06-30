use std::collections::{HashMap, HashSet};

use serde::Serialize;
use yazelix_zellij_pane_orchestrator::active_tab_session_state::{
    build_active_tab_session_state_v1, ActiveTabReadState, ActiveTabSessionStateV1,
    SessionSidebarYazi, SessionStatusExtensions, SessionTransientPane, SessionTransientPanes,
    SessionWorkspace,
};
use yazelix_zellij_pane_orchestrator::horizontal_focus_contract::{
    horizontal_role_for_pane, is_visible_popup_pane, resolve_horizontal_focus, HorizontalDirection,
    HorizontalFocusPlan, HorizontalPaneSnapshot,
};
use yazelix_zellij_pane_orchestrator::pane_contract::{
    resolve_focus_context, select_managed_pane_index, FocusContextPolicy, PaneSnapshot,
};
use yazelix_zellij_pane_orchestrator::sidebar_contract::{
    resolve_sidebar_focus_toggle, SidebarFocusTogglePlan,
};
use yazelix_zellij_pane_orchestrator::tab_identity_contract::{
    position_pane_identity_conflicts_with_cached_tabs, retain_current_tab_state,
};
use yazelix_zellij_pane_orchestrator::transient_pane_contract::{
    select_transient_pane, transient_pane_identity, TransientPaneKind, TransientPaneSnapshot,
};
use zellij_tile::prelude::*;

use crate::workspace::WorkspaceStateSource;
use crate::{
    State, RESULT_FOCUSED_EDITOR, RESULT_FOCUSED_SIDEBAR, RESULT_INVALID_PAYLOAD, RESULT_MISSING,
    RESULT_OK, RESULT_OPENED_SIDEBAR,
};

pub(crate) const EDITOR_TITLE: &str = "editor";
pub(crate) const SIDEBAR_TITLE: &str = "sidebar";
pub(crate) const AGENT_TITLE: &str = "agent";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManagedTerminalPane {
    pub(crate) pane_id: PaneId,
    pub(crate) pane_columns: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalPaneLayout {
    pub(crate) pane_id: PaneId,
    pub(crate) title: String,
    pub(crate) terminal_command: Option<String>,
    pub(crate) is_focused: bool,
    pub(crate) is_floating: bool,
    pub(crate) is_suppressed: bool,
    pub(crate) pane_x: usize,
    pub(crate) pane_y: usize,
    pub(crate) pane_columns: usize,
    pub(crate) pane_rows: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ManagedTabPanes {
    pub(crate) editor: Option<ManagedTerminalPane>,
    pub(crate) sidebar: Option<ManagedTerminalPane>,
    pub(crate) agent: Option<ManagedTerminalPane>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ManagedPaneKind {
    Editor,
    Sidebar,
}

pub(crate) type FocusContext = FocusContextPolicy;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TabPaneCaches {
    pub(crate) focus_context_by_tab: HashMap<usize, FocusContext>,
    pub(crate) focused_terminal_pane_by_tab: HashMap<usize, PaneId>,
    pub(crate) fallback_terminal_pane_by_tab: HashMap<usize, PaneId>,
    pub(crate) managed_panes_by_tab: HashMap<usize, ManagedTabPanes>,
    pub(crate) terminal_panes_by_tab: HashMap<usize, Vec<TerminalPaneLayout>>,
    pub(crate) zjstatus_plugin_id_by_tab: HashMap<usize, u32>,
    pub(crate) user_pane_count_by_tab: HashMap<usize, usize>,
}

impl TabPaneCaches {
    pub(crate) fn rebuild(
        pane_manifest: &PaneManifest,
        tab_id_by_position: &HashMap<usize, usize>,
        previous_focus_context_by_tab: &HashMap<usize, FocusContext>,
    ) -> Self {
        Self {
            focus_context_by_tab: build_focus_context_by_tab(
                pane_manifest,
                tab_id_by_position,
                previous_focus_context_by_tab,
            ),
            focused_terminal_pane_by_tab: build_focused_terminal_pane_by_tab(
                pane_manifest,
                tab_id_by_position,
            ),
            fallback_terminal_pane_by_tab: build_fallback_terminal_pane_by_tab(
                pane_manifest,
                tab_id_by_position,
            ),
            managed_panes_by_tab: build_managed_panes_by_tab(pane_manifest, tab_id_by_position),
            terminal_panes_by_tab: build_terminal_panes_by_tab(pane_manifest, tab_id_by_position),
            zjstatus_plugin_id_by_tab: build_zjstatus_plugin_id_by_tab(
                pane_manifest,
                tab_id_by_position,
            ),
            user_pane_count_by_tab: build_user_pane_count_by_tab(pane_manifest, tab_id_by_position),
        }
    }

    pub(crate) fn retain_current_tabs(&mut self, current_tab_ids: &HashSet<usize>) {
        retain_current_tab_state(&mut self.focus_context_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.focused_terminal_pane_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.fallback_terminal_pane_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.managed_panes_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.terminal_panes_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.zjstatus_plugin_id_by_tab, current_tab_ids);
        retain_current_tab_state(&mut self.user_pane_count_by_tab, current_tab_ids);
    }

    pub(crate) fn has_zjstatus_plugin_id(&self, plugin_id: u32) -> bool {
        self.zjstatus_plugin_id_by_tab
            .values()
            .any(|id| *id == plugin_id)
    }

    pub(crate) fn pane_manifest_conflicts_with_cached_tab_positions(
        &self,
        pane_manifest: &PaneManifest,
        tab_id_by_position: &HashMap<usize, usize>,
    ) -> bool {
        pane_manifest_conflicts_with_cached_tab_positions(
            pane_manifest,
            tab_id_by_position,
            &self.terminal_panes_by_tab,
        )
    }
}

pub(crate) fn pane_manifest_conflicts_with_cached_tab_positions(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
    previous_terminal_panes_by_tab: &HashMap<usize, Vec<TerminalPaneLayout>>,
) -> bool {
    let previous_pane_ids_by_tab = previous_terminal_panes_by_tab
        .iter()
        .map(|(tab_id, panes)| {
            let pane_ids = panes
                .iter()
                .filter_map(|pane| match pane.pane_id {
                    PaneId::Terminal(id) => Some(id),
                    PaneId::Plugin(_) => None,
                })
                .collect::<HashSet<_>>();
            (*tab_id, pane_ids)
        })
        .collect::<HashMap<_, _>>();

    let current_pane_ids_by_position = pane_manifest
        .panes
        .iter()
        .map(|(tab_position, panes)| {
            let current_pane_ids = panes
                .iter()
                .filter(|pane| !pane.is_plugin && !pane.exited)
                .map(|pane| pane.id)
                .collect::<HashSet<_>>();
            (*tab_position, current_pane_ids)
        })
        .collect::<HashMap<_, _>>();

    position_pane_identity_conflicts_with_cached_tabs(
        &current_pane_ids_by_position,
        tab_id_by_position,
        &previous_pane_ids_by_tab,
    )
}
#[derive(Serialize)]
struct MaintainerDebugEditorState {
    permissions_granted: bool,
    active_tab_position: Option<usize>,
    active_swap_layout_name: Option<String>,
    workspace_root: Option<String>,
    workspace_root_source: Option<String>,
    editor_pane_id: Option<String>,
    sidebar_pane_id: Option<String>,
    agent_pane_id: Option<String>,
    sidebar_yazi_id: Option<String>,
    sidebar_yazi_cwd: Option<String>,
    sidebar_is_collapsed: Option<bool>,
    agent_is_collapsed: Option<bool>,
}

fn build_managed_panes_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, ManagedTabPanes> {
    let mut managed_panes_by_tab = HashMap::new();

    for (tab_position, panes) in &pane_manifest.panes {
        let Some(tab_id) = tab_id_by_position.get(tab_position).copied() else {
            continue;
        };
        managed_panes_by_tab.insert(
            tab_id,
            ManagedTabPanes {
                editor: select_managed_terminal_pane(panes, EDITOR_TITLE),
                sidebar: select_managed_terminal_pane(panes, SIDEBAR_TITLE),
                agent: select_managed_terminal_pane(panes, AGENT_TITLE),
            },
        );
    }

    managed_panes_by_tab
}

fn build_user_pane_count_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, usize> {
    pane_manifest
        .panes
        .iter()
        .filter_map(|(tab_position, panes)| {
            let tab_id = tab_id_by_position.get(tab_position).copied()?;
            let user_pane_count = panes
                .iter()
                .filter(|pane| !pane.is_plugin)
                .filter(|pane| !pane.exited)
                .count();
            Some((tab_id, user_pane_count))
        })
        .collect()
}

fn build_focus_context_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
    previous_focus_context_by_tab: &HashMap<usize, FocusContext>,
) -> HashMap<usize, FocusContext> {
    let mut focus_context_by_tab = HashMap::new();

    for (tab_position, panes) in &pane_manifest.panes {
        let Some(tab_id) = tab_id_by_position.get(tab_position).copied() else {
            continue;
        };
        let focused_pane = panes.iter().find(|pane| pane.is_focused && !pane.is_plugin);
        let previous_focus_context = previous_focus_context_by_tab
            .get(&tab_id)
            .copied()
            .unwrap_or(FocusContext::Other);
        let focus_context = resolve_focus_context(
            focused_pane.map(|pane| pane.title.as_str()),
            previous_focus_context,
        );
        focus_context_by_tab.insert(tab_id, focus_context);
    }

    focus_context_by_tab
}

fn build_focused_terminal_pane_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, PaneId> {
    pane_manifest
        .panes
        .iter()
        .filter_map(|(tab_position, panes)| {
            let tab_id = tab_id_by_position.get(tab_position).copied()?;
            panes
                .iter()
                .find(|pane| pane.is_focused && !pane.is_plugin && !pane.exited)
                .map(|pane| (tab_id, PaneId::Terminal(pane.id)))
        })
        .collect()
}

fn build_fallback_terminal_pane_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, PaneId> {
    pane_manifest
        .panes
        .iter()
        .filter_map(|(tab_position, panes)| {
            let tab_id = tab_id_by_position.get(tab_position).copied()?;
            let editor_pane = select_managed_terminal_pane(panes, EDITOR_TITLE);
            editor_pane.map(|pane| (tab_id, pane.pane_id)).or_else(|| {
                panes
                    .iter()
                    .find(|pane| {
                        !pane.is_plugin
                            && !pane.exited
                            && !matches!(pane.title.trim(), SIDEBAR_TITLE | AGENT_TITLE)
                    })
                    .map(|pane| (tab_id, PaneId::Terminal(pane.id)))
            })
        })
        .collect()
}

fn build_terminal_panes_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, Vec<TerminalPaneLayout>> {
    pane_manifest
        .panes
        .iter()
        .filter_map(|(tab_position, panes)| {
            let tab_id = tab_id_by_position.get(tab_position).copied()?;
            let terminal_panes = panes
                .iter()
                .filter(|pane| !pane.is_plugin && !pane.exited)
                .map(|pane| TerminalPaneLayout {
                    pane_id: PaneId::Terminal(pane.id),
                    title: pane.title.clone(),
                    terminal_command: pane.terminal_command.clone(),
                    is_focused: pane.is_focused,
                    is_floating: pane.is_floating,
                    is_suppressed: pane.is_suppressed,
                    pane_x: pane.pane_x,
                    pane_y: pane.pane_y,
                    pane_columns: pane.pane_columns,
                    pane_rows: pane.pane_rows,
                })
                .collect();
            Some((tab_id, terminal_panes))
        })
        .collect()
}

fn build_zjstatus_plugin_id_by_tab(
    pane_manifest: &PaneManifest,
    tab_id_by_position: &HashMap<usize, usize>,
) -> HashMap<usize, u32> {
    pane_manifest
        .panes
        .iter()
        .filter_map(|(tab_position, panes)| {
            let tab_id = tab_id_by_position.get(tab_position).copied()?;
            panes
                .iter()
                .find(|pane| {
                    pane.is_plugin
                        && !pane.exited
                        && pane
                            .plugin_url
                            .as_deref()
                            .map(is_zjstatus_plugin_url)
                            .unwrap_or(false)
                })
                .map(|pane| (tab_id, pane.id))
        })
        .collect()
}

fn is_zjstatus_plugin_url(plugin_url: &str) -> bool {
    let plugin_url = plugin_url.trim();
    plugin_url == "zjstatus.wasm" || plugin_url.ends_with("/zjstatus.wasm")
}

impl State {
    fn collect_active_tab_read_state(&self, active_tab_id: Option<usize>) -> ActiveTabReadState {
        let active_swap_layout_name = active_tab_id
            .and_then(|tab_id| self.active_swap_layout_name_by_tab.get(&tab_id))
            .cloned()
            .flatten();
        let workspace_root = active_tab_id
            .and_then(|tab_id| self.workspace_state_by_tab.get(&tab_id))
            .map(|workspace_state| workspace_state.root.clone())
            .or_else(|| self.initial_workspace_state.clone().map(|state| state.root));
        let workspace_source = active_tab_id
            .and_then(|tab_id| self.workspace_state_by_tab.get(&tab_id))
            .map(|workspace_state| match workspace_state.source {
                WorkspaceStateSource::Bootstrap => "bootstrap".to_string(),
                WorkspaceStateSource::Explicit => "explicit".to_string(),
            })
            .or_else(|| {
                self.initial_workspace_state
                    .as_ref()
                    .map(|workspace_state| match workspace_state.source {
                        WorkspaceStateSource::Bootstrap => "bootstrap".to_string(),
                        WorkspaceStateSource::Explicit => "explicit".to_string(),
                    })
            });
        let explicit_workspace = match (
            active_tab_id,
            workspace_root.clone(),
            workspace_source.clone(),
        ) {
            (Some(tab_id), Some(root), Some(source))
                if matches!(
                    self.workspace_state_by_tab
                        .get(&tab_id)
                        .map(|workspace_state| workspace_state.source),
                    Some(WorkspaceStateSource::Explicit)
                ) =>
            {
                Some(SessionWorkspace { root, source })
            }
            _ => None,
        };
        let bootstrap_workspace = match (workspace_root, workspace_source) {
            (Some(root), Some(source)) if source == "bootstrap" => {
                Some(SessionWorkspace { root, source })
            }
            _ => None,
        };
        let editor_pane = active_tab_id
            .and_then(|tab_id| self.tab_pane_caches.managed_panes_by_tab.get(&tab_id))
            .and_then(|managed_tab_panes| managed_tab_panes.editor);
        let sidebar_pane = active_tab_id
            .and_then(|tab_id| self.tab_pane_caches.managed_panes_by_tab.get(&tab_id))
            .and_then(|managed_tab_panes| managed_tab_panes.sidebar);
        let agent_pane = active_tab_id
            .and_then(|tab_id| self.tab_pane_caches.managed_panes_by_tab.get(&tab_id))
            .and_then(|managed_tab_panes| managed_tab_panes.agent);
        let sidebar_yazi_state =
            active_tab_id.and_then(|tab_id| self.get_active_sidebar_yazi_state_snapshot(tab_id));
        let transient_panes = build_session_transient_panes(
            active_tab_id
                .and_then(|tab_id| self.tab_pane_caches.terminal_panes_by_tab.get(&tab_id))
                .map(Vec::as_slice),
        );
        let ai_pane_activity = active_tab_id
            .map(|tab_id| self.get_active_ai_pane_activity_snapshot(tab_id))
            .unwrap_or_default();
        let focus_context = match active_tab_id
            .and_then(|tab_id| {
                self.tab_pane_caches
                    .focus_context_by_tab
                    .get(&tab_id)
                    .copied()
            })
            .unwrap_or(FocusContext::Other)
        {
            FocusContext::Editor => "editor",
            FocusContext::Sidebar => "sidebar",
            FocusContext::Other => "other",
        };

        ActiveTabReadState {
            active_swap_layout_name,
            explicit_workspace,
            bootstrap_workspace,
            editor_pane_id: pane_id_to_string(editor_pane.map(|pane| pane.pane_id)),
            sidebar_pane_id: pane_id_to_string(sidebar_pane.map(|pane| pane.pane_id)),
            agent_pane_id: pane_id_to_string(agent_pane.map(|pane| pane.pane_id)),
            sidebar_yazi: sidebar_yazi_state.map(|state| SessionSidebarYazi {
                yazi_id: state.yazi_id.clone(),
                cwd: state.cwd.clone(),
            }),
            sidebar_collapsed: active_tab_id.and_then(|tab_id| self.sidebar_is_closed(tab_id)),
            agent_collapsed: active_tab_id.and_then(|tab_id| self.agent_is_closed(tab_id)),
            focus_context: focus_context.to_string(),
            transient_panes,
            extensions: SessionStatusExtensions { ai_pane_activity },
        }
    }

    pub(crate) fn active_tab_session_state_snapshot(
        &self,
        active_tab_id: usize,
    ) -> ActiveTabSessionStateV1 {
        let active_tab_position = self
            .tab_identity
            .position_for_tab_id(active_tab_id)
            .or(self.tab_identity.active_tab_position())
            .unwrap_or(active_tab_id);
        let read_state = self.collect_active_tab_read_state(Some(active_tab_id));
        build_active_tab_session_state_v1(active_tab_position, read_state)
    }

    pub(crate) fn smart_reveal(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let focus_context = self
            .tab_pane_caches
            .focus_context_by_tab
            .get(&active_tab_id)
            .copied()
            .unwrap_or(FocusContext::Other);

        if focus_context == FocusContext::Editor {
            let Some(editor_pane) = self.get_managed_pane(pipe_message, ManagedPaneKind::Editor)
            else {
                return;
            };

            write_to_pane_id(vec![27, b'r'], editor_pane.pane_id);
            self.respond(pipe_message, RESULT_OK);
            return;
        }

        self.toggle_editor_sidebar_focus(pipe_message);
    }

    pub(crate) fn focus_managed_pane(
        &self,
        pipe_message: &PipeMessage,
        pane_kind: ManagedPaneKind,
    ) {
        let Some(managed_pane) = self.get_managed_pane(pipe_message, pane_kind) else {
            return;
        };

        let sidebar_is_closed = if matches!(pane_kind, ManagedPaneKind::Sidebar) {
            self.tab_identity
                .active_tab_id()
                .and_then(|tab_id| self.sidebar_is_closed(tab_id))
                .unwrap_or(false)
        } else {
            false
        };

        if sidebar_is_closed {
            if let Some(active_tab_id) = self.tab_identity.active_tab_id() {
                self.open_sidebar_and_focus_after_layout_settle_for_tab(active_tab_id);
            } else {
                self.open_sidebar_and_focus_after_layout_settle();
            }
        } else {
            focus_pane_with_id(managed_pane.pane_id, false, false);
        }
        self.respond(pipe_message, RESULT_OK);
    }

    pub(crate) fn toggle_editor_sidebar_focus(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let Some(managed_tab_panes) = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
        else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        let focus_context = self
            .tab_pane_caches
            .focus_context_by_tab
            .get(&active_tab_id)
            .copied()
            .unwrap_or(FocusContext::Other);
        let sidebar_is_closed = self.sidebar_is_closed(active_tab_id).unwrap_or(false);
        let plan = resolve_sidebar_focus_toggle(
            focus_context,
            managed_tab_panes.sidebar.is_some(),
            sidebar_is_closed,
            managed_tab_panes.editor.is_some(),
        );

        match plan {
            SidebarFocusTogglePlan::FocusEditor => {
                if let Some(target_pane) = managed_tab_panes.editor {
                    focus_pane_with_id(target_pane.pane_id, false, false);
                    self.respond(pipe_message, RESULT_FOCUSED_EDITOR);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            SidebarFocusTogglePlan::FocusSidebar => {
                if let Some(target_pane) = managed_tab_panes.sidebar {
                    focus_pane_with_id(target_pane.pane_id, false, false);
                    self.respond(pipe_message, RESULT_FOCUSED_SIDEBAR);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            SidebarFocusTogglePlan::OpenAndFocusSidebar => {
                if managed_tab_panes.sidebar.is_some() {
                    self.open_sidebar_and_focus_after_layout_settle_for_tab(active_tab_id);
                    self.respond(pipe_message, RESULT_OPENED_SIDEBAR);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            SidebarFocusTogglePlan::MissingTarget => self.respond(pipe_message, RESULT_MISSING),
        }
    }

    pub(crate) fn move_horizontal_focus_or_tab(
        &self,
        pipe_message: &PipeMessage,
        direction: HorizontalDirection,
    ) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };

        let Some(terminal_panes) = self
            .tab_pane_caches
            .terminal_panes_by_tab
            .get(&active_tab_id)
        else {
            self.respond(pipe_message, RESULT_MISSING);
            return;
        };

        let sidebar_is_closed = self.sidebar_is_closed(active_tab_id).unwrap_or(false);
        let agent_is_closed = self.agent_is_closed(active_tab_id).unwrap_or(false);
        let managed_tab_panes = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
            .cloned()
            .unwrap_or_default();
        let visible_popup_is_open = terminal_panes
            .iter()
            .any(|pane| is_visible_popup_pane(&pane.title, pane.is_floating, pane.is_suppressed));
        let managed_sidebar_pane_id = managed_tab_panes.sidebar.map(|managed| managed.pane_id);
        let managed_agent_pane_id = managed_tab_panes.agent.map(|managed| managed.pane_id);
        let pane_snapshots: Vec<HorizontalPaneSnapshot> = terminal_panes
            .iter()
            .map(|pane| HorizontalPaneSnapshot {
                role: horizontal_role_for_pane(
                    &pane.pane_id,
                    &pane.title,
                    managed_sidebar_pane_id.as_ref(),
                    managed_agent_pane_id.as_ref(),
                ),
                is_plugin: false,
                exited: false,
                is_focused: pane.is_focused,
                pane_x: pane.pane_x,
                pane_y: pane.pane_y,
                pane_columns: pane.pane_columns,
                pane_rows: pane.pane_rows,
            })
            .collect();

        match resolve_horizontal_focus(
            &pane_snapshots,
            direction,
            visible_popup_is_open,
            sidebar_is_closed,
            agent_is_closed,
        ) {
            HorizontalFocusPlan::FocusPane(index) => {
                if let Some(target_pane) = terminal_panes.get(index) {
                    focus_pane_with_id(target_pane.pane_id, false, false);
                    self.respond(pipe_message, RESULT_OK);
                } else {
                    self.respond(pipe_message, RESULT_MISSING);
                }
            }
            HorizontalFocusPlan::PreviousTab => {
                go_to_previous_tab();
                self.respond(pipe_message, RESULT_OK);
            }
            HorizontalFocusPlan::NextTab => {
                go_to_next_tab();
                self.respond(pipe_message, RESULT_OK);
            }
            HorizontalFocusPlan::MissingFocusedPane => self.respond(pipe_message, RESULT_MISSING),
        }
    }

    pub(crate) fn maintainer_debug_editor_state(&self, pipe_message: &PipeMessage) {
        let active_tab_position = self.tab_identity.active_tab_position();
        let read_state = self.collect_active_tab_read_state(self.tab_identity.active_tab_id());

        let state = MaintainerDebugEditorState {
            permissions_granted: self.permissions_granted,
            active_tab_position,
            active_swap_layout_name: read_state.active_swap_layout_name,
            workspace_root: read_state
                .explicit_workspace
                .as_ref()
                .or(read_state.bootstrap_workspace.as_ref())
                .map(|workspace| workspace.root.clone()),
            workspace_root_source: read_state
                .explicit_workspace
                .as_ref()
                .or(read_state.bootstrap_workspace.as_ref())
                .map(|workspace| workspace.source.clone()),
            editor_pane_id: read_state.editor_pane_id,
            sidebar_pane_id: read_state.sidebar_pane_id,
            agent_pane_id: read_state.agent_pane_id,
            sidebar_yazi_id: read_state
                .sidebar_yazi
                .as_ref()
                .map(|state| state.yazi_id.clone()),
            sidebar_yazi_cwd: read_state
                .sidebar_yazi
                .as_ref()
                .map(|state| state.cwd.clone()),
            sidebar_is_collapsed: read_state.sidebar_collapsed,
            agent_is_collapsed: read_state.agent_collapsed,
        };

        match serde_json::to_string(&state) {
            Ok(serialized_state) => self.respond(pipe_message, &serialized_state),
            Err(_) => self.respond(pipe_message, RESULT_INVALID_PAYLOAD),
        }
    }

    pub(crate) fn get_active_tab_session_state(&self, pipe_message: &PipeMessage) {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return;
        };
        let snapshot = self.active_tab_session_state_snapshot(active_tab_id);

        match serde_json::to_string(&snapshot) {
            Ok(serialized) => self.respond(pipe_message, &serialized),
            Err(_) => self.respond(pipe_message, RESULT_INVALID_PAYLOAD),
        }
    }

    pub(crate) fn get_managed_pane(
        &self,
        pipe_message: &PipeMessage,
        pane_kind: ManagedPaneKind,
    ) -> Option<ManagedTerminalPane> {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return None;
        };

        let managed_pane = self
            .tab_pane_caches
            .managed_panes_by_tab
            .get(&active_tab_id)
            .and_then(|managed_tab_panes| match pane_kind {
                ManagedPaneKind::Editor => managed_tab_panes.editor,
                ManagedPaneKind::Sidebar => managed_tab_panes.sidebar,
            });

        match managed_pane {
            Some(managed_pane) => Some(managed_pane),
            None => {
                self.respond(pipe_message, RESULT_MISSING);
                None
            }
        }
    }

    pub(crate) fn get_focused_terminal_pane(&self, pipe_message: &PipeMessage) -> Option<PaneId> {
        let Some(active_tab_id) = self.ensure_action_ready(pipe_message) else {
            return None;
        };

        match self
            .tab_pane_caches
            .focused_terminal_pane_by_tab
            .get(&active_tab_id)
            .copied()
        {
            Some(pane_id) => Some(pane_id),
            None => {
                self.respond(pipe_message, RESULT_MISSING);
                None
            }
        }
    }
}

impl TerminalPaneLayout {
    pub(crate) fn transient_snapshot(&self) -> TransientPaneSnapshot<'_, PaneId> {
        TransientPaneSnapshot {
            pane_id: self.pane_id,
            title: self.title.as_str(),
            terminal_command: self.terminal_command.as_deref(),
            is_plugin: false,
            exited: false,
            is_floating: self.is_floating,
            is_focused: self.is_focused,
        }
    }
}

fn build_session_transient_panes(
    terminal_panes: Option<&[TerminalPaneLayout]>,
) -> SessionTransientPanes {
    let Some(terminal_panes) = terminal_panes else {
        return SessionTransientPanes::default();
    };
    let snapshots: Vec<TransientPaneSnapshot<'_, PaneId>> = terminal_panes
        .iter()
        .map(TerminalPaneLayout::transient_snapshot)
        .collect();

    SessionTransientPanes {
        popup: build_session_transient_pane(&snapshots, TransientPaneKind::Popup),
        menu: build_session_transient_pane(&snapshots, TransientPaneKind::Menu),
    }
}

fn build_session_transient_pane(
    snapshots: &[TransientPaneSnapshot<'_, PaneId>],
    kind: TransientPaneKind,
) -> Option<SessionTransientPane> {
    let transient_pane = select_transient_pane(snapshots, transient_pane_identity(kind))?;
    pane_id_to_string(Some(transient_pane.pane_id)).map(|pane_id| SessionTransientPane {
        pane_id,
        is_focused: transient_pane.is_focused,
    })
}

fn select_managed_terminal_pane(
    panes: &[PaneInfo],
    expected_title: &str,
) -> Option<ManagedTerminalPane> {
    let pane_snapshots: Vec<PaneSnapshot<'_>> = panes
        .iter()
        .map(|pane| PaneSnapshot {
            title: pane.title.as_str(),
            is_plugin: pane.is_plugin,
            exited: pane.exited,
            is_focused: pane.is_focused,
            is_suppressed: pane.is_suppressed,
        })
        .collect();

    let selected_pane = select_managed_pane_index(&pane_snapshots, expected_title)
        .and_then(|index| panes.get(index));

    selected_pane.map(|pane| ManagedTerminalPane {
        pane_id: PaneId::Terminal(pane.id),
        pane_columns: pane.pane_columns,
    })
}

pub(crate) fn pane_id_to_string(pane_id: Option<PaneId>) -> Option<String> {
    match pane_id {
        Some(PaneId::Terminal(id)) => Some(format!("terminal:{id}")),
        Some(PaneId::Plugin(id)) => Some(format!("plugin:{id}")),
        None => None,
    }
}
