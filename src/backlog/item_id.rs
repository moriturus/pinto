//! PBI ID.

use crate::error::{Error, Result};
use std::fmt;
use std::str::FromStr;

/// PBI stable identifier (`<PREFIX>-<NUMBER>`, e.g. `T-1`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemId {
    prefix: String,
    number: u32,
}

impl ItemId {
    /// Construct an ID from a validated prefix and number.
    ///
    /// Prefixes are ASCII letters only. Use this fallible constructor at public boundaries; the
    /// crate-private constructor below is reserved for known-safe fixtures and values produced by
    /// already-validated domain data.
    ///
    /// # Examples
    ///
    /// ```
    /// use pinto::backlog::ItemId;
    ///
    /// let id = ItemId::try_new("T", 42).expect("safe prefix");
    /// assert_eq!(id.prefix(), "T");
    /// assert_eq!(id.number(), 42);
    /// assert_eq!(id.to_string(), "T-42");
    /// ```
    pub fn try_new(prefix: impl Into<String>, number: u32) -> Result<Self> {
        let prefix = prefix.into();
        if !is_safe_prefix(&prefix) {
            return Err(Error::InvalidItemId(format!("{prefix}-{number}")));
        }
        Ok(Self { prefix, number })
    }

    /// Construct an ID from a crate-validated prefix.
    #[cfg(test)]
    pub(crate) fn new(prefix: impl Into<String>, number: u32) -> Self {
        let prefix = prefix.into();
        debug_assert!(is_safe_prefix(&prefix), "ItemId prefix must be validated");
        Self { prefix, number }
    }

    /// Prefix (e.g. `T`).
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Serial number.
    #[must_use]
    pub fn number(&self) -> u32 {
        self.number
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.prefix, self.number)
    }
}

impl FromStr for ItemId {
    type Err = Error;

    /// Parse `<PREFIX>-<NUMBER>`. Prefix is ASCII letters, number is decimal.
    fn from_str(s: &str) -> Result<Self> {
        let invalid = || Error::InvalidItemId(s.to_string());
        let (prefix, number) = s.rsplit_once('-').ok_or_else(invalid)?;
        if !is_safe_prefix(prefix)
            || number.is_empty()
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(invalid());
        }
        let number: u32 = number.parse().map_err(|_| invalid())?;
        ItemId::try_new(prefix, number).map_err(|_| invalid())
    }
}

/// Return whether `prefix` is a non-empty ASCII-letter project key.
fn is_safe_prefix(prefix: &str) -> bool {
    !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_display_and_parse_roundtrip() {
        let id = ItemId::new("T", 42);
        assert_eq!(id.to_string(), "T-42");
        assert_eq!("T-42".parse::<ItemId>().unwrap(), id);
    }

    #[test]
    fn item_id_parse_reads_prefix_and_number() {
        let id: ItemId = "PROJ-7".parse().unwrap();
        assert_eq!(id.prefix(), "PROJ");
        assert_eq!(id.number(), 7);
    }

    #[test]
    fn try_new_accepts_ascii_letters_only() {
        for prefix in [
            "",
            "123",
            "p9",
            "bug_fix",
            "PROJ-1",
            "../outside",
            "/tmp/outside",
            "T\\outside",
            "T.outside",
            "T space",
        ] {
            assert!(
                ItemId::try_new(prefix, 1).is_err(),
                "expected unsafe prefix {prefix:?} to be rejected"
            );
        }

        for prefix in ["T", "PROJ", "bugfix"] {
            assert!(
                ItemId::try_new(prefix, 1).is_ok(),
                "expected safe prefix {prefix:?} to be accepted"
            );
        }
        assert!(
            ItemId::try_new("日本", 1).is_err(),
            "non-ASCII prefix must be rejected"
        );
    }

    #[test]
    fn item_id_rejects_malformed() {
        for bad in [
            "",
            "T",
            "T-",
            "-1",
            "T-abc",
            "T-1-2extra ",
            "123-1",
            "p9-1",
            "bug_fix-1",
            "PROJ-1-1",
            "T- 1",
            "../outside-1",
            "/tmp/outside-1",
            "T\\\\outside-1",
            "T.outside-1",
        ] {
            assert!(
                bad.parse::<ItemId>().is_err(),
                "expected {bad:?} to be rejected"
            );
        }
    }
}
