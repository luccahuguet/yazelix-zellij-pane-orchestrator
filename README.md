# yazelix-zellij-pane-orchestrator

Standalone Zellij plugin for tab-local pane orchestration. The plugin originated in Yazelix, but core pane behavior is usable without installing Yazelix.

## Build

```bash
cargo test --lib
cargo build --target wasm32-wasip1 --profile release
nix build .#yazelix_zellij_pane_orchestrator
```

The public artifact is:

```text
target/wasm32-wasip1/release/yazelix_zellij_pane_orchestrator.wasm
```

The Nix package artifact for Yazelix runtime integration is:

```text
share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm
```

## Minimal Zellij config

```kdl
plugins {
    yazelix-zellij-pane-orchestrator location="file:/absolute/path/to/yazelix_zellij_pane_orchestrator.wasm" {
        screen_saver_enabled false
    }
}

keybinds {
    normal {
        bind "Alt y" {
            MessagePlugin "yazelix-zellij-pane-orchestrator" {
                name "toggle_sidebar"
            }
        }
    }
}
```

## Standalone pipe API

These commands are intended to work without Yazelix runtime paths:

- `focus_editor`
- `focus_sidebar`
- `toggle_editor_sidebar_focus`
- `move_focus_left_or_tab`
- `move_focus_right_or_tab`
- `next_family`
- `previous_family`
- `toggle_sidebar`
- `hide_sidebar`
- `get_active_tab_session_state`
- `open_terminal_in_cwd`
- `open_workspace_terminal`

Yazelix integration commands depend on Yazelix-managed editor/sidebar/workspace conventions:

- `smart_reveal`
- `open_file`
- `set_managed_editor_cwd`
- `register_sidebar_yazi_state`
- `register_ai_pane_activity`
- `retarget_workspace`
- `toggle_workspace_popup`
- `reload_runtime_config`

`retarget_workspace` accepts an optional `workspace_source` of `explicit` or
`bootstrap`; callers normally omit it, while coordinators can preserve the
previous provenance when rolling back a failed multi-step retarget.
`toggle_workspace_popup` requires a configured `popup_plugin_url`, accepts a
popup id as its payload, and forwards that id with the active tab's canonical
workspace root to the loaded popup instance matching that URL.

`register_ai_pane_activity` records tab-local AI activity facts. Live
spinner-prefixed terminal titles such as Codex's activity title provide the same
fact when their pane command matches the optional
`managed_agent_command_marker`. The fact is removed when the spinner title
disappears or the pane exits. The plugin reduces all facts by stable tab id and
broadcasts the complete snapshot through `pipe_tab_activity`; compatible status
bars consume it and unrelated plugins ignore it. It never writes activity into
native Zellij tab names.

Editor command-mode integration is Neovim-only. Helix buffer opens and cwd sync are owned by the Yazelix Helix action bridge; direct Helix `open_file`, `set_managed_editor_cwd`, or `retarget_workspace` editor requests are rejected instead of sending `:open` or `:cd` text into the terminal.

Debug commands are maintainer-only and not part of the ordinary standalone API:

- `maintainer_debug_editor_state`
- `debug_write_literal`
- `debug_send_escape`

## Standalone contract

Core behavior must not require `YAZELIX_RUNTIME_DIR`, `YAZELIX_SESSION_CONFIG_PATH`, `yzx_control`, or Yazelix-managed config paths. Yazelix consumes this plugin as a first-party integration, but those integration paths are extensions on top of the standalone Zellij plugin contract.
