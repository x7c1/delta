//! In-memory [`ExternalOpener`] fake for the `open_cwd` use-case tests.

use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::{Error, Result};
use crate::ports::ExternalOpener;

/// One recorded `open()` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::interactor) struct OpenCall {
    pub(in crate::interactor) command: String,
    pub(in crate::interactor) args: Vec<String>,
}

/// Records every `open()` call and optionally scripts a failure so the
/// error-mapping paths can be exercised.
///
/// Tests build it directly and pass it into
/// [`crate::Interactor::with_external_opener`] via
/// `Arc<dyn ExternalOpener>`.
#[derive(Default)]
pub(crate) struct FakeExternalOpener {
    /// Every recorded call, in the order they happened.
    pub(crate) calls: Mutex<Vec<OpenCall>>,
    /// When set, `open()` returns this error instead of recording the call.
    /// Populated by [`Self::failing_with`] for the error-path tests.
    pub(crate) failure: Mutex<Option<Error>>,
}

impl FakeExternalOpener {
    /// Build a fake that fails every `open()` with the given error, mirroring
    /// the two production error variants the real driver reports (command
    /// missing / other spawn failure). The failure is single-shot: after one
    /// call the fake reverts to recording, so a follow-up call in the same
    /// test observes the reset state.
    pub(crate) fn failing_with(err: Error) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            failure: Mutex::new(Some(err)),
        }
    }

    pub(crate) fn calls(&self) -> Vec<OpenCall> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl ExternalOpener for FakeExternalOpener {
    async fn open(&self, command: &str, args: Vec<String>) -> Result<()> {
        if let Some(err) = self.failure.lock().unwrap().take() {
            return Err(err);
        }
        self.calls.lock().unwrap().push(OpenCall {
            command: command.to_owned(),
            args,
        });
        Ok(())
    }
}
