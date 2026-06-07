# Agent Guidelines

Shared Yazelix agent workflow and release policy live in the main repo:

- https://github.com/luccahuguet/yazelix/blob/main/AGENTS.md
- In sibling local checkouts, read `../yazelix/AGENTS.md` first

Only pane-orchestrator-specific guidance belongs here.

## Local Scope

- This repo owns the `yazelix_zellij_pane_orchestrator.wasm` plugin source and standalone pane-orchestration API.
- Keep core standalone behavior free of `YAZELIX_RUNTIME_DIR`, `yzx_control`, and generated main-repo paths.
- Yazelix-specific commands may exist as integration extensions, but main Yazelix owns generated layouts and runtime packaging.

## Local Commands

- `cargo fmt --all -- --check`
- `cargo test --lib`
- `cargo build --target wasm32-wasip1 --profile release`
- `nix build .#yazelix_zellij_pane_orchestrator --no-link`

## Integration Notes

The package artifact is `share/yazelix_zellij_pane_orchestrator/yazelix_pane_orchestrator.wasm`. For coupled runtime changes, publish this child commit before updating the main repo lock.
