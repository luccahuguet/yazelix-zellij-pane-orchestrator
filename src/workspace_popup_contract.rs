use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct WorkspacePopupRequest<'a> {
    id: &'a str,
    cwd: &'a str,
}

pub fn workspace_popup_payload(popup_id: &str, workspace_root: &str) -> Option<String> {
    let popup_id = popup_id.trim();
    let workspace_root = workspace_root.trim();
    if popup_id.is_empty() || !Path::new(workspace_root).is_absolute() {
        return None;
    }
    serde_json::to_string(&WorkspacePopupRequest {
        id: popup_id,
        cwd: workspace_root,
    })
    .ok()
}

pub fn workspace_popup_launch_matches_root(launch_cwd: &str, workspace_root: &str) -> bool {
    let launch_cwd = launch_cwd.trim();
    let workspace_root = workspace_root.trim();
    Path::new(launch_cwd).is_absolute()
        && Path::new(workspace_root).is_absolute()
        && launch_cwd.trim_end_matches('/') == workspace_root.trim_end_matches('/')
}

pub fn workspace_popup_picker_action(popup_exists: bool, receiver_is_usable: bool) -> &'static str {
    match (popup_exists, receiver_is_usable) {
        (false, _) => "toggle",
        (true, true) => "focus",
        (true, false) => "replace",
    }
}

pub fn workspace_popup_destination_id<'a>(
    expected_plugin_url: &str,
    panes: impl IntoIterator<Item = (u32, bool, Option<&'a str>)>,
) -> Option<u32> {
    let expected_plugin_url = expected_plugin_url.trim();
    if expected_plugin_url.is_empty() {
        return None;
    }
    panes
        .into_iter()
        .find(|(_, exited, plugin_url)| {
            !*exited && plugin_url.is_some_and(|url| url == expected_plugin_url)
        })
        .map(|(id, _, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_popup_request_carries_canonical_root() {
        let payload = workspace_popup_payload(" agent ", " /repo ").unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
            json!({
                "id": "agent",
                "cwd": "/repo",
            })
        );

        assert!(workspace_popup_payload("", "/repo").is_none());
        assert!(workspace_popup_payload("agent", "repo").is_none());
    }

    #[test]
    fn workspace_popup_targets_the_loaded_plugin_instance() {
        let panes = [
            (7, false, None),
            (8, true, Some("yzpp")),
            (9, false, Some("other")),
            (10, false, Some("yzpp")),
        ];
        assert_eq!(workspace_popup_destination_id(" yzpp ", panes), Some(10));
        assert_eq!(workspace_popup_destination_id("missing", panes), None);
    }

    #[test]
    fn workspace_popup_launch_must_match_the_active_workspace() {
        assert!(workspace_popup_launch_matches_root(" /repo/ ", "/repo"));
        assert!(workspace_popup_launch_matches_root(
            "/repo with spaces",
            " /repo with spaces/ "
        ));
        assert!(!workspace_popup_launch_matches_root("/old", "/repo"));
        assert!(!workspace_popup_launch_matches_root("", "/repo"));
        assert!(!workspace_popup_launch_matches_root("repo", "repo"));
    }

    #[test]
    fn picker_replaces_an_existing_popup_without_a_usable_receiver() {
        assert_eq!(workspace_popup_picker_action(false, false), "toggle");
        assert_eq!(workspace_popup_picker_action(true, true), "focus");
        assert_eq!(workspace_popup_picker_action(true, false), "replace");
    }
}
