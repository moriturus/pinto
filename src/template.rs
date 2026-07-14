//! Plain text template to apply during creation.

use crate::error::{Error, Result};
use std::fmt;
use std::str::FromStr;

/// Safe name for the template (file name without extension).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TemplateName(String);

impl TemplateName {
    /// Create a name consisting only of ASCII alphanumeric characters, `-`, and `_`.
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(Error::InvalidTemplateName(value));
        }
        Ok(Self(value))
    }

    /// Template name without extension.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TemplateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for TemplateName {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::new(value)
    }
}

/// The creation target to which the template is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    /// The body of the PBI.
    Item,
    /// Sprint goal body.
    Sprint,
}

impl TemplateKind {
    /// Directory name under `.pinto/templates/`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Sprint => "sprint",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn template_name_accepts_safe_file_stems_and_rejects_paths() {
        assert_eq!(TemplateName::new("story-1").unwrap().as_str(), "story-1");
        for invalid in ["", "../secret", "feature/one", "with space", "日本語"] {
            assert_eq!(
                TemplateName::new(invalid),
                Err(Error::InvalidTemplateName(invalid.to_string()))
            );
        }
    }
}
