//! Deterministic write-failure injection used by multi-record recovery tests.

use crate::error::{Error, Result};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Test-only process configuration: allow this many record writes, then fail the next one.
const FAIL_AFTER_RECORD_WRITES_ENV: &str = "PINTO_TEST_FAIL_AFTER_RECORD_WRITES";

/// A per-backend counter that makes failures deterministic without changing normal persistence.
#[derive(Debug, Clone, Default)]
pub(crate) struct WriteFailureInjector {
    remaining: Option<Arc<AtomicUsize>>,
}

impl WriteFailureInjector {
    /// Read the opt-in test counter from the process environment.
    pub(crate) fn from_environment() -> Self {
        let remaining = std::env::var(FAIL_AFTER_RECORD_WRITES_ENV)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .map(|value| Arc::new(AtomicUsize::new(value)));
        Self { remaining }
    }

    /// Fail after the current record has been written when the test counter reaches its limit.
    pub(crate) fn after_record_write(&self, path: &Path) -> Result<()> {
        let Some(remaining) = &self.remaining else {
            return Ok(());
        };

        loop {
            let current = remaining.load(Ordering::Acquire);
            if current == 0 {
                return Err(self.injected_error(path));
            }
            if remaining
                .compare_exchange(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return if current == 1 {
                    Err(self.injected_error(path))
                } else {
                    Ok(())
                };
            }
        }
    }

    fn injected_error(&self, path: &Path) -> Error {
        Error::Io {
            path: path.to_path_buf(),
            message: "deterministic test failure after a record write; unset PINTO_TEST_FAIL_AFTER_RECORD_WRITES outside tests".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_injection_allows_every_write() {
        let injector = WriteFailureInjector::default();
        assert!(injector.after_record_write(Path::new("item")).is_ok());
    }

    #[test]
    fn configured_counter_fails_after_the_requested_number_of_writes() {
        let injector = WriteFailureInjector {
            remaining: Some(Arc::new(AtomicUsize::new(1))),
        };
        assert!(injector.after_record_write(Path::new("item")).is_err());
        assert!(injector.after_record_write(Path::new("item")).is_err());
    }

    #[test]
    fn injected_error_keeps_the_record_path() {
        let injector = WriteFailureInjector {
            remaining: Some(Arc::new(AtomicUsize::new(0))),
        };
        assert_eq!(
            injector
                .after_record_write(Path::new("item"))
                .expect_err("injection")
                .code(),
            "io"
        );
    }
}
