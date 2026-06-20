mod agent;
mod ai_pane_activity;
mod editor;
mod heartbeat;
mod layout;
mod panes;
mod runtime_config;
mod screen_saver;
mod sidebar_yazi;
mod status_bar_cache;
mod workspace;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use std::time::{Duration, Instant};
use workspace::{bootstrap_workspace_root, WorkspaceState};
use yazelix_zellij_pane_orchestrator::active_tab_session_state::SessionAiPaneActivity;
use yazelix_zellij_pane_orchestrator::horizontal_focus_contract::HorizontalDirection;
use yazelix_zellij_pane_orchestrator::layout_state_contract::{
    LayoutFamilyDirection, LayoutVariant,
};
use yazelix_zellij_pane_orchestrator::right_sidebar_command_contract::RightSidebarCommandConfig;
use yazelix_zellij_pane_orchestrator::screen_saver_contract::ScreenSaverConfig;
use yazelix_zellij_pane_orchestrator::status_bar_cache_contract::StatusBarCacheRuntime;
use yazelix_zellij_pane_orchestrator::status_usage_provider_contract::StatusUsageProviderConfig;
use yazelix_zellij_pane_orchestrator::tab_identity_contract::{
    retain_current_tab_state, TabIdentityState,
};
use yazelix_zellij_pane_orchestrator::timer_schedule_contract::next_timer_delay;
use zellij_tile::prelude::*;

pub(crate) const RESULT_OK: &str = "ok";
pub(crate) const RESULT_FOCUSED_EDITOR: &str = "focused_editor";
pub(crate) const RESULT_FOCUSED_SIDEBAR: &str = "focused_sidebar";
pub(crate) const RESULT_FOCUSED_AGENT: &str = "focused_agent";
pub(crate) const RESULT_OPENED_SIDEBAR: &str = "opened_sidebar";
pub(crate) const RESULT_MISSING: &str = "missing";
pub(crate) const RESULT_NOT_READY: &str = "not_ready";
pub(crate) const RESULT_DENIED: &str = "permissions_denied";
pub(crate) const RESULT_INVALID_PAYLOAD: &str = "invalid_payload";
pub(crate) const RESULT_UNKNOWN_LAYOUT: &str = "unknown_layout";
pub(crate) const RESULT_UNSUPPORTED_EDITOR: &str = "unsupported_editor";
pub(crate) const RESULT_STALE_GENERATION: &str = "stale_generation";
pub(crate) const RESULT_VERSION_MISMATCH: &str = "version_mismatch";
pub(crate) const COMMAND_STEP_DELAY_MS: u64 = 35;
pub(crate) const SWAP_LAYOUT_STEP_DELAY_MS: u64 = 1;
const TAB_LOCAL_PANE_RECONCILE_DELAY: Duration = Duration::from_millis(500);

#[derive(Default)]
struct State {
    tab_identity: TabIdentityState,
    active_swap_layout_name_by_tab: HashMap<usize, Option<String>>,
    last_known_layout_variant_by_tab: RefCell<HashMap<usize, LayoutVariant>>,
    tab_pane_caches: panes::TabPaneCaches,
    last_pane_manifest: Option<PaneManifest>,
    tab_local_pane_reconcile_next_flush: Option<Instant>,
    tab_name_by_tab_id: HashMap<usize, String>,
    tab_fullscreen_active_by_tab: HashMap<usize, bool>,
    tab_sync_panes_active_by_tab: HashMap<usize, bool>,
    workspace_state_by_tab: HashMap<usize, WorkspaceState>,
    sidebar_yazi_state_by_tab: HashMap<usize, sidebar_yazi::SidebarYaziState>,
    ai_pane_activity_by_tab: HashMap<usize, Vec<SessionAiPaneActivity>>,
    ai_activity_tab_base_name_by_tab: HashMap<usize, String>,
    ai_activity_tab_decoration_last_write: Option<Instant>,
    ai_activity_tab_decoration_next_flush: Option<Instant>,
    seen_tab_ids: HashSet<usize>,
    initial_workspace_state: Option<WorkspaceState>,
    runtime_dir: PathBuf,
    screen_saver_config: ScreenSaverConfig,
    right_sidebar_command: Option<RightSidebarCommandConfig>,
    screen_saver_last_input: Option<Instant>,
    screen_saver_next_timeout: Option<Instant>,
    screen_saver_pane_id: Option<PaneId>,
    screen_saver_restore_floating_layer: bool,
    status_bar_cache_runtime: Option<StatusBarCacheRuntime>,
    status_bar_cache_last_payload: Option<String>,
    workspace_status_pipe_payload_by_plugin: HashMap<u32, String>,
    status_bar_claude_usage_next_refresh: Option<Instant>,
    status_bar_codex_usage_next_refresh: Option<Instant>,
    status_bar_opencode_go_usage_next_refresh: Option<Instant>,
    status_usage_provider_config: StatusUsageProviderConfig,
    orchestrator_heartbeat: heartbeat::OrchestratorHeartbeat,
    timer_armed_for: Option<Instant>,
    runtime_config_generation: String,
    permissions_granted: bool,
}

register_plugin!(State);

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        set_selectable(false);
        let plugin_ids = get_plugin_ids();
        let bootstrap_root = bootstrap_workspace_root(&plugin_ids.initial_cwd);
        self.initial_workspace_state = Some(WorkspaceState::from_bootstrap_root(bootstrap_root));
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::OpenTerminalsOrPlugins,
            PermissionType::RunCommands,
            PermissionType::WriteToStdin,
            PermissionType::ReadCliPipes,
            PermissionType::MessageAndLaunchOtherPlugins,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
        self.runtime_dir = configuration
            .get("runtime_dir")
            .map(|value| PathBuf::from(value.trim()))
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or(plugin_ids.initial_cwd);
        self.screen_saver_config = ScreenSaverConfig::from_plugin_configuration(&configuration);
        self.right_sidebar_command =
            RightSidebarCommandConfig::from_plugin_configuration(&configuration);
        self.status_usage_provider_config =
            StatusUsageProviderConfig::from_plugin_configuration(&configuration);
        self.runtime_config_generation = configuration
            .get("runtime_config_generation")
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        if self.screen_saver_config.enabled {
            self.screen_saver_last_input = Some(Instant::now());
        }
        self.initialize_orchestrator_heartbeat();
        let mut subscriptions = vec![
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::PaneClosed,
            EventType::CommandPaneExited,
            EventType::PermissionRequestResult,
            EventType::Timer,
        ];
        if self.screen_saver_config.enabled {
            subscriptions.push(EventType::InputReceived);
        }
        subscribe(&subscriptions);
        self.schedule_initial_screen_saver_timeout();
        self.schedule_initial_status_bar_claude_usage_refresh();
        self.schedule_initial_status_bar_codex_usage_refresh();
        self.schedule_initial_status_bar_opencode_go_usage_refresh();
        self.arm_next_timer();
    }

    fn update(&mut self, event: Event) -> bool {
        self.record_orchestrator_event(heartbeat::event_kind(&event));
        match event {
            Event::TabUpdate(tabs) => {
                self.tab_identity = TabIdentityState::from_tabs(&tabs);
                self.tab_name_by_tab_id = tabs
                    .iter()
                    .map(|tab| (tab.tab_id, tab.name.clone()))
                    .collect();
                self.tab_fullscreen_active_by_tab = tabs
                    .iter()
                    .map(|tab| (tab.tab_id, tab.is_fullscreen_active))
                    .collect();
                self.tab_sync_panes_active_by_tab = tabs
                    .iter()
                    .map(|tab| (tab.tab_id, tab.is_sync_panes_active))
                    .collect();
                self.reconcile_workspace_state(&tabs);
                self.reconcile_ai_pane_activity_tabs(&tabs);
                {
                    let mut last_known_layout_variant_by_tab =
                        self.last_known_layout_variant_by_tab.borrow_mut();
                    for tab in &tabs {
                        if let Some(layout_variant) = tab
                            .active_swap_layout_name
                            .as_deref()
                            .and_then(LayoutVariant::from_layout_name)
                        {
                            last_known_layout_variant_by_tab.insert(tab.tab_id, layout_variant);
                        }
                    }
                }
                self.active_swap_layout_name_by_tab = tabs
                    .into_iter()
                    .map(|tab| (tab.tab_id, tab.active_swap_layout_name))
                    .collect();
                self.retain_tab_local_pane_state_for_current_tabs();
                if let Some(pane_manifest) = self.last_pane_manifest.clone() {
                    self.rebuild_tab_local_pane_state_or_defer(&pane_manifest);
                }
            }
            Event::PaneUpdate(pane_manifest) => {
                self.last_pane_manifest = Some(pane_manifest.clone());
                self.rebuild_tab_local_pane_state_or_defer(&pane_manifest);
            }
            Event::PermissionRequestResult(status) => {
                self.permissions_granted = status == PermissionStatus::Granted;
                if self.permissions_granted {
                    self.sync_ai_activity_tab_decorations_for_known_tabs();
                }
            }
            Event::InputReceived => self.record_screen_saver_input(),
            Event::Timer(_) => {
                self.timer_armed_for = None;
                self.record_orchestrator_timer();
                self.handle_tab_local_pane_reconcile_timer();
                self.handle_ai_activity_tab_decoration_timer();
                self.handle_screen_saver_timer();
                self.handle_status_bar_claude_usage_timer();
                self.handle_status_bar_codex_usage_timer();
                self.handle_status_bar_opencode_go_usage_timer();
                self.handle_orchestrator_heartbeat_timer();
            }
            Event::PaneClosed(pane_id) => {
                self.handle_terminal_title_activity_pane_closed(pane_id);
                self.handle_screen_saver_pane_closed(pane_id);
            }
            Event::CommandPaneExited(terminal_id, _, _) => {
                self.handle_terminal_title_activity_command_pane_exited(terminal_id);
                self.handle_screen_saver_command_exit(terminal_id);
            }
            _ => {}
        }
        self.refresh_status_bar_cache();
        self.arm_next_timer();
        false
    }

    fn pipe(&mut self, pipe_message: PipeMessage) -> bool {
        self.record_orchestrator_pipe(pipe_message.name.as_str());
        match pipe_message.name.as_str() {
            "focus_editor" => {
                self.focus_managed_pane(&pipe_message, panes::ManagedPaneKind::Editor);
                false
            }
            "focus_sidebar" => {
                self.focus_managed_pane(&pipe_message, panes::ManagedPaneKind::Sidebar);
                false
            }
            "toggle_editor_sidebar_focus" => {
                self.toggle_editor_sidebar_focus(&pipe_message);
                false
            }
            "toggle_editor_right_sidebar_focus" => {
                self.toggle_editor_right_sidebar_focus(&pipe_message);
                false
            }
            "move_focus_left_or_tab" => {
                self.move_horizontal_focus_or_tab(&pipe_message, HorizontalDirection::Left);
                false
            }
            "move_focus_right_or_tab" => {
                self.move_horizontal_focus_or_tab(&pipe_message, HorizontalDirection::Right);
                false
            }
            "smart_reveal" => {
                self.smart_reveal(&pipe_message);
                false
            }
            "open_file" => {
                self.open_file_in_managed_editor(&pipe_message);
                false
            }
            "set_managed_editor_cwd" => {
                self.set_managed_editor_cwd(&pipe_message);
                false
            }
            "next_family" => {
                self.switch_layout_family(&pipe_message, LayoutFamilyDirection::Next);
                false
            }
            "previous_family" => {
                self.switch_layout_family(&pipe_message, LayoutFamilyDirection::Previous);
                false
            }
            "toggle_sidebar" => {
                self.toggle_sidebar(&pipe_message);
                false
            }
            "toggle_agent_sidebar" => {
                self.toggle_agent_sidebar(&pipe_message);
                false
            }
            "hide_sidebar" => {
                self.hide_sidebar(&pipe_message);
                false
            }
            "register_sidebar_yazi_state" => {
                self.register_sidebar_yazi_state(&pipe_message);
                false
            }
            "register_ai_pane_activity" => {
                self.register_ai_pane_activity(&pipe_message);
                false
            }
            "reconcile_terminal_title_activity_snapshot" => {
                self.reconcile_terminal_title_activity_snapshot(&pipe_message);
                false
            }
            "get_active_tab_session_state" => {
                self.get_active_tab_session_state(&pipe_message);
                false
            }
            "get_all_tab_activity_state" => {
                self.get_all_tab_activity_state(&pipe_message);
                false
            }
            "retarget_workspace" => {
                self.retarget_workspace(&pipe_message);
                false
            }
            "open_terminal_in_cwd" => {
                self.open_terminal_in_cwd(&pipe_message);
                false
            }
            "open_workspace_terminal" => {
                self.open_workspace_terminal(&pipe_message);
                false
            }
            "reload_runtime_config" => {
                self.reload_runtime_config(&pipe_message);
                false
            }
            "maintainer_debug_editor_state" => {
                self.maintainer_debug_editor_state(&pipe_message);
                false
            }
            "debug_write_literal" => {
                self.debug_write_literal(&pipe_message);
                false
            }
            "debug_send_escape" => {
                self.debug_send_escape(&pipe_message);
                false
            }
            _ => false,
        }
    }

    fn render(&mut self, _rows: usize, _cols: usize) {}
}

impl State {
    fn rebuild_tab_local_pane_state(&mut self, pane_manifest: &PaneManifest) {
        if !self.tab_identity.has_complete_position_map(
            pane_manifest.panes.keys().copied(),
            pane_manifest.panes.len(),
        ) {
            return;
        }

        self.tab_pane_caches = panes::TabPaneCaches::rebuild(
            pane_manifest,
            self.tab_identity.tab_id_by_position(),
            &self.tab_pane_caches.focus_context_by_tab,
        );
        self.workspace_status_pipe_payload_by_plugin
            .retain(|plugin_id, _| self.tab_pane_caches.has_zjstatus_plugin_id(*plugin_id));
        self.reconcile_sidebar_yazi_state();
        self.reconcile_ai_pane_activity_panes();
    }

    fn rebuild_tab_local_pane_state_or_defer(&mut self, pane_manifest: &PaneManifest) {
        if self
            .tab_pane_caches
            .pane_manifest_conflicts_with_cached_tab_positions(
                pane_manifest,
                self.tab_identity.tab_id_by_position(),
            )
        {
            self.schedule_tab_local_pane_reconcile_flush();
            return;
        }

        self.tab_local_pane_reconcile_next_flush = None;
        self.rebuild_tab_local_pane_state(pane_manifest);
    }

    fn schedule_tab_local_pane_reconcile_flush(&mut self) {
        self.tab_local_pane_reconcile_next_flush =
            Some(Instant::now() + TAB_LOCAL_PANE_RECONCILE_DELAY);
        self.arm_next_timer();
    }

    fn handle_tab_local_pane_reconcile_timer(&mut self) {
        let Some(deadline) = self.tab_local_pane_reconcile_next_flush else {
            return;
        };
        if Instant::now() < deadline {
            return;
        }

        self.tab_local_pane_reconcile_next_flush = None;
        let Some(pane_manifest) = self.last_pane_manifest.clone() else {
            return;
        };
        if self
            .tab_pane_caches
            .pane_manifest_conflicts_with_cached_tab_positions(
                &pane_manifest,
                self.tab_identity.tab_id_by_position(),
            )
        {
            return;
        }

        self.rebuild_tab_local_pane_state(&pane_manifest);
    }

    fn retain_tab_local_pane_state_for_current_tabs(&mut self) {
        let current_tab_ids = self.tab_identity.current_tab_ids();
        if current_tab_ids.is_empty() {
            self.last_pane_manifest = None;
        }
        self.last_known_layout_variant_by_tab
            .borrow_mut()
            .retain(|tab_id, _| current_tab_ids.contains(tab_id));
        self.tab_pane_caches.retain_current_tabs(&current_tab_ids);
        retain_current_tab_state(&mut self.sidebar_yazi_state_by_tab, &current_tab_ids);
        retain_current_tab_state(&mut self.tab_name_by_tab_id, &current_tab_ids);
        retain_current_tab_state(&mut self.tab_fullscreen_active_by_tab, &current_tab_ids);
        retain_current_tab_state(&mut self.tab_sync_panes_active_by_tab, &current_tab_ids);
        retain_current_tab_state(&mut self.ai_activity_tab_base_name_by_tab, &current_tab_ids);
    }

    pub(crate) fn ensure_action_ready(&self, pipe_message: &PipeMessage) -> Option<usize> {
        if !self.permissions_granted {
            self.respond(pipe_message, RESULT_DENIED);
            return None;
        }

        let Some(active_tab_id) = self.tab_identity.active_tab_id() else {
            self.respond(pipe_message, RESULT_NOT_READY);
            return None;
        };

        Some(active_tab_id)
    }

    pub(crate) fn respond(&self, pipe_message: &PipeMessage, result: &str) {
        if let PipeSource::Cli(pipe_id) = &pipe_message.source {
            cli_pipe_output(pipe_id, result);
        }
    }
}

impl State {
    fn arm_next_timer(&mut self) {
        let now = Instant::now();
        let Some((deadline, delay)) = next_timer_delay(
            now,
            [
                self.screen_saver_next_timeout,
                self.status_bar_claude_usage_next_refresh,
                self.status_bar_codex_usage_next_refresh,
                self.status_bar_opencode_go_usage_next_refresh,
                self.tab_local_pane_reconcile_next_flush,
                self.ai_activity_tab_decoration_next_flush,
                self.orchestrator_heartbeat.next_flush,
            ],
            self.timer_armed_for,
        ) else {
            return;
        };

        set_timeout(delay.as_secs_f64());
        self.timer_armed_for = Some(deadline);
    }
}
