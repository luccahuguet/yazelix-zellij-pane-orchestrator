# yazelix-zellij-pane-orchestrator

Standalone Zellij plugin for tab-local pane orchestration. The plugin originated in Yazelix, but core pane behavior is usable without installing Yazelix.

## Build

```bash
cargo test --lib
cargo build --target wasm32-wasip1 --profile release
```

The public artifact is:

```text
target/wasm32-wasip1/release/yazelix_zellij_pane_orchestrator.wasm
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
- `reload_runtime_config`

Debug commands are maintainer-only and not part of the ordinary standalone API:

- `maintainer_debug_editor_state`
- `debug_write_literal`
- `debug_send_escape`

## Standalone contract

Core behavior must not require `YAZELIX_RUNTIME_DIR`, `YAZELIX_SESSION_CONFIG_PATH`, `yzx_control`, or Yazelix-managed config paths. Yazelix consumes this plugin as a first-party integration, but those integration paths are extensions on top of the standalone Zellij plugin contract.
