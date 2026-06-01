#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCommandSequence {
    pub change_directory_command: String,
    pub open_file_commands: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCommandSequenceError {
    EmptyTargets,
    UnsupportedEditor,
}

pub fn normalize_open_file_targets<'a>(
    file_path: Option<&'a str>,
    file_paths: &'a [String],
) -> Vec<&'a str> {
    if file_paths.is_empty() {
        return file_path
            .into_iter()
            .filter(|path| !path.is_empty())
            .collect();
    }

    file_paths
        .iter()
        .map(String::as_str)
        .filter(|path| !path.is_empty())
        .collect()
}

pub fn build_editor_command_sequence(
    editor: &str,
    working_dir: &str,
    file_paths: &[&str],
) -> Result<EditorCommandSequence, EditorCommandSequenceError> {
    let change_directory_command = build_editor_change_directory_command(editor, working_dir)
        .ok_or(EditorCommandSequenceError::UnsupportedEditor)?;
    let file_paths = file_paths
        .iter()
        .copied()
        .filter(|path| !path.is_empty())
        .collect::<Vec<_>>();
    if file_paths.is_empty() {
        return Err(EditorCommandSequenceError::EmptyTargets);
    }

    match editor {
        "neovim" => Ok(EditorCommandSequence {
            change_directory_command,
            open_file_commands: file_paths
                .iter()
                .map(|file_path| {
                    format!(
                        ":execute 'edit ' . fnameescape('{}')",
                        escape_vim_single_quoted_string(file_path)
                    )
                })
                .collect(),
        }),
        _ => Err(EditorCommandSequenceError::UnsupportedEditor),
    }
}

pub fn build_editor_change_directory_command(editor: &str, working_dir: &str) -> Option<String> {
    match editor {
        "neovim" => Some(format!(
            ":execute 'cd ' . fnameescape('{}')",
            escape_vim_single_quoted_string(working_dir)
        )),
        _ => None,
    }
}

fn escape_vim_single_quoted_string(path: &str) -> String {
    path.replace('\'', "''")
}

// Test lane: default
#[cfg(test)]
mod tests {
    use super::*;

    // Defends: new multi-file payloads are preferred while legacy single-file payloads remain accepted during core/plugin rollouts.
    #[test]
    fn normalize_targets_prefers_multi_file_payload_with_legacy_fallback() {
        let file_paths = vec!["/tmp/one.txt".to_string(), "/tmp/two.txt".to_string()];
        assert_eq!(
            normalize_open_file_targets(Some("/tmp/legacy.txt"), &file_paths),
            vec!["/tmp/one.txt", "/tmp/two.txt"]
        );
        assert_eq!(
            normalize_open_file_targets(Some("/tmp/legacy.txt"), &[]),
            vec!["/tmp/legacy.txt"]
        );
    }

    // Defends: Helix file opens are editor-bridge owned and must not fall back to command-mode text injection.
    #[test]
    fn helix_open_file_command_sequence_is_rejected() {
        assert_eq!(
            build_editor_command_sequence(
                "helix",
                "/tmp/project",
                &["/tmp/project/one.txt", "/tmp/project/two words.txt"],
            ),
            Err(EditorCommandSequenceError::UnsupportedEditor)
        );
    }

    // Defends: Helix cwd sync is editor-bridge owned and must not fall back to command-mode text injection.
    #[test]
    fn helix_change_directory_command_is_rejected() {
        assert_eq!(
            build_editor_change_directory_command("helix", "/tmp/project"),
            None
        );
    }

    // Defends: managed Neovim pane reuse escapes each selected file path before sending edit commands.
    #[test]
    fn neovim_sequence_escapes_each_selected_file() {
        let sequence = build_editor_command_sequence(
            "neovim",
            "/tmp/project",
            &["/tmp/project/one.txt", "/tmp/project/it'really.txt"],
        )
        .unwrap();

        assert_eq!(
            sequence.change_directory_command,
            ":execute 'cd ' . fnameescape('/tmp/project')"
        );
        assert_eq!(
            sequence.open_file_commands,
            vec![
                ":execute 'edit ' . fnameescape('/tmp/project/one.txt')",
                ":execute 'edit ' . fnameescape('/tmp/project/it''really.txt')"
            ]
        );
    }

    // Defends: malformed open-file payloads without any selected target are rejected before editor-specific command generation.
    #[test]
    fn command_sequence_rejects_empty_target_payload() {
        assert_eq!(
            build_editor_command_sequence("neovim", "/tmp/project", &[]),
            Err(EditorCommandSequenceError::EmptyTargets)
        );
    }
}
