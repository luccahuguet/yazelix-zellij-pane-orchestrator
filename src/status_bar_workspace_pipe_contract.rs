// Test lane: default

pub const ZJSTATUS_WORKSPACE_PIPE_MESSAGE: &str = "yazelix_workspace_status";
pub const ZJSTATUS_WORKSPACE_PIPE_NAME: &str = "pipe_workspace";

pub fn workspace_pipe_protocol_payload(content: &str) -> String {
    format!(
        "zjstatus::pipe::{ZJSTATUS_WORKSPACE_PIPE_NAME}::{}",
        sanitize_zjstatus_pipe_content(content)
    )
}

fn sanitize_zjstatus_pipe_content(content: &str) -> String {
    content.replace("::", ":").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::workspace_pipe_protocol_payload;

    // Regression: workspace labels sent through zjstatus pipe protocol cannot contain protocol delimiters or extra lines.
    #[test]
    fn workspace_pipe_payload_sanitizes_protocol_content() {
        let payload = workspace_pipe_protocol_payload(" [foo::bar\nbaz]");

        assert_eq!(payload, "zjstatus::pipe::pipe_workspace:: [foo:bar baz]");
    }

    // Defends: an empty workspace label clears the active bar widget instead of preserving a stale tab name.
    #[test]
    fn empty_workspace_pipe_payload_clears_widget() {
        let payload = workspace_pipe_protocol_payload("");

        assert_eq!(payload, "zjstatus::pipe::pipe_workspace::");
    }
}
