//! In-memory [`BinaryDetector`] fake for the provider-availability use-case
//! tests.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::ports::BinaryDetector;

/// Scripts per-binary presence without touching the real filesystem.
///
/// `default_present` is the answer for any binary not listed in `overrides`, so
/// a test can start from "everything present" ([`Self::all_present`]) or
/// "nothing present" ([`Self::default`]) and then flip specific binaries with
/// [`Self::with_present`] / [`Self::with_absent`].
#[derive(Default)]
pub(crate) struct FakeBinaryDetector {
    default_present: bool,
    overrides: Mutex<HashMap<String, bool>>,
}

impl FakeBinaryDetector {
    /// A detector that reports every binary as present unless explicitly
    /// overridden absent.
    pub(crate) fn all_present() -> Self {
        Self {
            default_present: true,
            overrides: Mutex::new(HashMap::new()),
        }
    }

    /// Mark a specific binary present.
    pub(crate) fn with_present(self, bin: &str) -> Self {
        self.overrides.lock().unwrap().insert(bin.to_owned(), true);
        self
    }

    /// Mark a specific binary absent.
    pub(crate) fn with_absent(self, bin: &str) -> Self {
        self.overrides.lock().unwrap().insert(bin.to_owned(), false);
        self
    }
}

#[async_trait]
impl BinaryDetector for FakeBinaryDetector {
    async fn is_available(&self, bin: &str) -> bool {
        self.overrides
            .lock()
            .unwrap()
            .get(bin)
            .copied()
            .unwrap_or(self.default_present)
    }
}
