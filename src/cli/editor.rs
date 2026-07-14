//! Preparation for interactive editing using `$EDITOR`.
//!
//! Export a template to a temporary file, launch an external editor, and read the edited text back.
//! Keep process and temporary-file side effects here; validation and persistence belong to
//! [`pinto::service::apply_item_edit`].

use anyhow::Context;
use pinto::error::Error;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tempfile::{Builder, NamedTempFile};

/// Resolve the command string used to launch the editor.
///
/// Prefer `$VISUAL` over `$EDITOR`. Return `None` when both variables are unset or blank, matching
/// the convention used by tools such as Git.
pub(super) fn resolve_editor() -> Option<String> {
    let visual = std::env::var("VISUAL").ok();
    let editor = std::env::var("EDITOR").ok();
    select_editor(visual.as_deref(), editor.as_deref())
}

/// Select the startup target from the environment variable candidates, excluding blank values.
fn select_editor(visual: Option<&str>, editor: Option<&str>) -> Option<String> {
    [visual, editor]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// Write the template to a secure temporary file and open it with the configured editor.
///
/// `slug` is used only as a sanitized filename prefix. [`NamedTempFile`] owns the file for the
/// complete editor session, so it is removed when this function returns or unwinds. Callers from
/// asynchronous contexts run this blocking operation through `spawn_blocking`.
pub(super) fn edit_in_editor(template: &str, slug: &str) -> anyhow::Result<String> {
    let editor = resolve_editor().ok_or(Error::EditorNotSet)?;
    let buffer = create_edit_buffer(template, slug)?;
    launch_and_read(&editor, buffer.path())
}

/// Create the editor buffer in the system temporary directory.
fn create_edit_buffer(template: &str, slug: &str) -> anyhow::Result<EditBuffer> {
    create_edit_buffer_in(std::env::temp_dir(), template, slug, None)
}

/// Own a named editor buffer until the editor operation finishes.
struct EditBuffer {
    file: NamedTempFile,
}

impl EditBuffer {
    fn path(&self) -> &Path {
        self.file.path()
    }
}

/// Create and initialize an editor buffer using tempfile's exclusive-create primitive.
///
/// `random_bytes` is only used by deterministic unit tests; production creation keeps tempfile's
/// collision-resistant default.
fn create_edit_buffer_in<P: AsRef<Path>>(
    directory: P,
    template: &str,
    slug: &str,
    random_bytes: Option<usize>,
) -> anyhow::Result<EditBuffer> {
    let safe_slug = sanitize_slug(slug);
    let prefix = format!("pinto-{safe_slug}-");
    let mut builder = Builder::new();
    if let Some(random_bytes) = random_bytes {
        builder.rand_bytes(random_bytes);
    }
    let mut file = builder
        .prefix(&prefix)
        .suffix(".md")
        .tempfile_in(directory)
        .with_context(|| format!("failed to create edit buffer for slug {slug:?}"))?;
    file.write_all(template.as_bytes())
        .and_then(|_| file.flush())
        .with_context(|| format!("failed to write edit buffer at {}", file.path().display()))?;

    Ok(EditBuffer { file })
}

/// Keep the user-controlled slug within a single safe filename component.
fn sanitize_slug(slug: &str) -> String {
    let safe: String = slug
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    safe
}

/// Start the editor and read the file contents after it exits.
///
/// Editor commands use the same quote-aware tokenizer as the interactive shell. The first part is
/// the executable, remaining parts are arguments, and the target file path is appended last.
/// Standard input and output are inherited from the parent so interactive editors retain the
/// terminal.
fn launch_and_read(editor: &str, path: &Path) -> anyhow::Result<String> {
    let mut parts = parse_editor_command(editor)?.into_iter();
    let program = parts.next().ok_or(Error::EditorNotSet)?;
    let status = Command::new(program)
        .args(parts)
        .arg(path)
        .status()
        .map_err(|e| Error::EditorLaunch {
            editor: editor.to_string(),
            message: e.to_string(),
        })?;
    if !status.success() {
        return Err(Error::EditorLaunch {
            editor: editor.to_string(),
            message: format!("exited with a non-zero status ({status})"),
        }
        .into());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read edit buffer at {}", path.display()))?;
    Ok(text)
}

/// Parse an editor command into an executable and its arguments.
fn parse_editor_command(editor: &str) -> anyhow::Result<Vec<String>> {
    let parts = super::shell::split_args(editor).map_err(|error| Error::EditorLaunch {
        editor: editor.to_string(),
        message: error.to_string(),
    })?;
    if parts.is_empty() {
        Err(Error::EditorNotSet.into())
    } else {
        Ok(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use tempfile::TempDir;

    #[test]
    fn visual_takes_priority_over_editor() {
        assert_eq!(
            select_editor(Some("visual"), Some("editor")),
            Some("visual".to_string())
        );
        assert_eq!(
            select_editor(Some("  "), Some(" editor ")),
            Some("editor".to_string())
        );
    }

    #[test]
    fn editor_command_parser_preserves_quoted_executable_paths() {
        assert_eq!(
            parse_editor_command(r#""/Applications/My Editor/editor" --wait"#)
                .expect("quoted editor command parses"),
            vec![
                "/Applications/My Editor/editor".to_string(),
                "--wait".to_string()
            ]
        );
    }

    #[test]
    fn edit_buffer_sanitizes_slug_writes_template_and_cleans_on_drop() {
        let dir = TempDir::new().expect("temp dir");
        let buffer = create_edit_buffer_in(dir.path(), "initial template", "T-1/../x", Some(0))
            .expect("create edit buffer");
        let path = buffer.path().to_owned();
        let name = path.file_name().unwrap().to_string_lossy();
        // The four characters `/ . . /` fall into `_`.
        assert!(name.starts_with("pinto-T-1____x-"), "sanitized: {name}");
        assert!(name.ends_with(".md"));
        assert_eq!(path.parent(), Some(dir.path()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "initial template");

        drop(buffer);
        assert!(!path.exists());
    }

    #[test]
    fn edit_buffer_does_not_truncate_a_preexisting_path() {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("pinto-T-1____x-.md");
        std::fs::write(&path, "sentinel").expect("write sentinel");

        let result = create_edit_buffer_in(dir.path(), "replacement", "T-1/../x", Some(0));

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn edit_buffer_does_not_follow_a_preexisting_symlink() {
        use std::os::unix::fs::symlink;

        let dir = TempDir::new().expect("temp dir");
        let sentinel = dir.path().join("sentinel");
        let path = dir.path().join("pinto-T-1____x-.md");
        std::fs::write(&sentinel, "sentinel").expect("write sentinel");
        symlink(&sentinel, &path).expect("create symlink");

        let result = create_edit_buffer_in(dir.path(), "replacement", "T-1/../x", Some(0));

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "sentinel");
    }

    #[cfg(unix)]
    #[test]
    fn edit_buffer_has_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().expect("temp dir");
        let buffer = create_edit_buffer_in(dir.path(), "template", "T-1", Some(0))
            .expect("create edit buffer");

        assert_eq!(
            std::fs::metadata(buffer.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    #[test]
    fn edit_buffer_is_removed_during_unwind() {
        let dir = TempDir::new().expect("temp dir");
        let buffer = create_edit_buffer_in(dir.path(), "template", "T-1", Some(0))
            .expect("create edit buffer");
        let path = buffer.path().to_owned();

        let result = catch_unwind(AssertUnwindSafe(move || {
            let _buffer = buffer;
            panic!("simulate an unwind while editing");
        }));

        assert!(result.is_err());
        assert!(!path.exists());
    }
}
