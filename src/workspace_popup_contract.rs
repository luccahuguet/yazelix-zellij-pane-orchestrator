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

pub fn workspace_popup_show_action(popup_exists: bool) -> &'static str {
    if popup_exists {
        "focus"
    } else {
        "toggle"
    }
}

pub fn yazi_emit_to_args(
    receiver: &str,
    name: &str,
    args: impl IntoIterator<Item = impl Into<String>>,
) -> Option<Vec<String>> {
    let receiver = receiver.trim();
    let name = name.trim();
    if receiver.is_empty() || name.is_empty() {
        return None;
    }

    Some(
        [
            "emit-to".to_string(),
            receiver.to_string(),
            name.to_string(),
        ]
        .into_iter()
        .chain(args.into_iter().map(Into::into))
        .collect(),
    )
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
    fn workspace_picker_focuses_a_live_popup_and_toggles_only_to_create_one() {
        assert_eq!(workspace_popup_show_action(true), "focus");
        assert_eq!(workspace_popup_show_action(false), "toggle");
    }

    #[test]
    fn yazi_events_keep_receiver_and_path_arguments_structured() {
        assert_eq!(
            yazi_emit_to_args(" yazi-7 ", "cd", ["/workspace with spaces"]),
            Some(vec![
                "emit-to".to_string(),
                "yazi-7".to_string(),
                "cd".to_string(),
                "/workspace with spaces".to_string(),
            ])
        );
        assert_eq!(
            yazi_emit_to_args("yazi-7", "plugin", ["zoxide-editor"]),
            Some(vec![
                "emit-to".to_string(),
                "yazi-7".to_string(),
                "plugin".to_string(),
                "zoxide-editor".to_string(),
            ])
        );
        assert_eq!(yazi_emit_to_args("", "plugin", ["picker"]), None);
    }
}
