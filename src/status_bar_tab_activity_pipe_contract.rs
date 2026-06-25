// Test lane: default

pub const ZJSTATUS_TAB_ACTIVITY_PIPE_MESSAGE: &str = "yazelix_tab_activity";
pub const ZJSTATUS_TAB_ACTIVITY_PIPE_NAME: &str = "pipe_tab_activity";

pub fn tab_activity_pipe_protocol_payload(snapshot_json: &str) -> String {
    format!(
        "zjstatus::pipe::{ZJSTATUS_TAB_ACTIVITY_PIPE_NAME}::{}",
        sanitize_zjstatus_pipe_content(snapshot_json)
    )
}

fn sanitize_zjstatus_pipe_content(content: &str) -> String {
    content.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::tab_activity_pipe_protocol_payload;

    // Defends: activity snapshots can contain JSON punctuation while still using zjstatus pipe protocol framing.
    #[test]
    fn tab_activity_pipe_payload_preserves_json_content() {
        let payload = tab_activity_pipe_protocol_payload(r#"{"base_name":"agent::plan"}"#);

        assert_eq!(
            payload,
            r#"zjstatus::pipe::pipe_tab_activity::{"base_name":"agent::plan"}"#
        );
    }

    // Regression: raw line breaks must not split one snapshot into multiple zjstatus pipe protocol messages.
    #[test]
    fn tab_activity_pipe_payload_removes_raw_line_breaks() {
        let payload = tab_activity_pipe_protocol_payload("{\n}");

        assert_eq!(payload, "zjstatus::pipe::pipe_tab_activity::{ }");
    }
}
