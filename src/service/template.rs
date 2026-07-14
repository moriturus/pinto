//! Load templates used when creating backlog items and sprints.

use super::open_board;
use crate::error::{Error, Result};
use crate::template::{TemplateKind, TemplateName};
use std::path::Path;
use tokio::fs;

/// Read `.pinto/templates/<kind>/<name>.md` as plain text.
///
/// Templates are user-editable plain-text files; creation commands use the body unchanged. If the
/// file is absent, return [`Error::TemplateNotFound`] with the path where it should be created.
pub async fn template_body(
    project_dir: &Path,
    kind: TemplateKind,
    name: &TemplateName,
) -> Result<String> {
    let (board_dir, _repo, _config) = open_board(project_dir).await?;
    let path = board_dir
        .join("templates")
        .join(kind.as_str())
        .join(format!("{name}.md"));
    if !fs::try_exists(&path)
        .await
        .map_err(|error| Error::io(&path, &error))?
    {
        return Err(Error::TemplateNotFound {
            kind: kind.as_str(),
            name: name.clone(),
            path,
        });
    }
    fs::read_to_string(&path)
        .await
        .map_err(|error| Error::TemplateUnreadable {
            path,
            message: error.to_string(),
        })
}
