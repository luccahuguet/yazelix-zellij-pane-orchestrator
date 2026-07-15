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
}
