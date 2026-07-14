//! User-facing timezone selection for human-readable timestamps.

use chrono::{DateTime, FixedOffset, Local, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Timezone used only when rendering human-readable output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DisplayTimezone {
    /// Use the operating system's local timezone.
    #[default]
    Local,
    /// Render in UTC.
    Utc,
    /// Render using an explicit fixed offset from UTC.
    Fixed(FixedOffset),
}

impl DisplayTimezone {
    /// Format a UTC instant in this display timezone.
    #[must_use]
    pub fn format_datetime(&self, value: DateTime<Utc>, pattern: &str) -> String {
        match self {
            Self::Local => value.with_timezone(&Local).format(pattern).to_string(),
            Self::Utc => value.format(pattern).to_string(),
            Self::Fixed(offset) => value.with_timezone(offset).format(pattern).to_string(),
        }
    }
}

impl fmt::Display for DisplayTimezone {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Local => f.write_str("local"),
            Self::Utc => f.write_str("UTC"),
            Self::Fixed(offset) => {
                let seconds = offset.local_minus_utc();
                let sign = if seconds < 0 { '-' } else { '+' };
                let seconds = seconds.unsigned_abs();
                write!(
                    f,
                    "{sign}{:02}:{:02}",
                    seconds / 3_600,
                    seconds % 3_600 / 60
                )
            }
        }
    }
}

/// Invalid configured display timezone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimezoneParseError {
    value: String,
}

impl fmt::Display for TimezoneParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid display timezone {:?}; use `local`, `UTC`, or a fixed offset `+HH:MM` / `-HH:MM`",
            self.value
        )
    }
}

impl std::error::Error for TimezoneParseError {}

impl FromStr for DisplayTimezone {
    type Err = TimezoneParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("local") {
            return Ok(Self::Local);
        }
        if value.eq_ignore_ascii_case("utc") || value.eq_ignore_ascii_case("z") {
            return Ok(Self::Utc);
        }

        let offset = value
            .get(..3)
            .filter(|prefix| prefix.eq_ignore_ascii_case("utc"))
            .map_or(value, |_| &value[3..]);
        let valid_shape = offset.len() == 6
            && matches!(offset.as_bytes().first(), Some(b'+' | b'-'))
            && offset.as_bytes().get(3) == Some(&b':')
            && offset[1..3].bytes().all(|byte| byte.is_ascii_digit())
            && offset[4..].bytes().all(|byte| byte.is_ascii_digit());
        if !valid_shape {
            return Err(TimezoneParseError {
                value: value.to_string(),
            });
        }
        let hours = offset[1..3]
            .parse::<i32>()
            .map_err(|_| TimezoneParseError {
                value: value.to_string(),
            })?;
        let minutes = offset[4..].parse::<i32>().map_err(|_| TimezoneParseError {
            value: value.to_string(),
        })?;
        if hours > 23 || minutes > 59 {
            return Err(TimezoneParseError {
                value: value.to_string(),
            });
        }
        let seconds =
            (hours * 3_600 + minutes * 60) * if offset.as_bytes()[0] == b'-' { -1 } else { 1 };
        let fixed = FixedOffset::east_opt(seconds).ok_or_else(|| TimezoneParseError {
            value: value.to_string(),
        })?;
        Ok(Self::Fixed(fixed))
    }
}

impl Serialize for DisplayTimezone {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DisplayTimezone {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Utc};

    fn instant(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn parses_local_utc_and_fixed_offsets_and_formats_date_boundaries() {
        assert_eq!(
            "local".parse::<DisplayTimezone>().expect("local"),
            DisplayTimezone::Local
        );
        assert_eq!(
            "UTC".parse::<DisplayTimezone>().expect("UTC"),
            DisplayTimezone::Utc
        );
        let plus = "+09:00"
            .parse::<DisplayTimezone>()
            .expect("positive offset");
        let minus = "UTC-01:00"
            .parse::<DisplayTimezone>()
            .expect("negative offset");

        assert_eq!(
            DisplayTimezone::Utc.format_datetime(instant(0), "%Y-%m-%d %H:%M"),
            "1970-01-01 00:00"
        );
        assert_eq!(
            plus.format_datetime(instant(0), "%Y-%m-%d %H:%M"),
            "1970-01-01 09:00"
        );
        assert_eq!(
            minus.format_datetime(instant(30 * 60), "%Y-%m-%d %H:%M"),
            "1969-12-31 23:30"
        );
    }

    #[test]
    fn rejects_invalid_or_unavailable_timezone_values_with_guidance() {
        for value in ["", "Mars/Base", "+24:00", "+09:60", "UTC+9"] {
            let error = value
                .parse::<DisplayTimezone>()
                .expect_err("invalid timezone");
            assert!(
                error.to_string().contains("local")
                    && error.to_string().contains("UTC")
                    && error.to_string().contains("HH:MM"),
                "guidance for {value:?}: {error}"
            );
        }
    }
}
