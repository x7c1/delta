//! The composition root's host-requirement check: the startup probe for the
//! one command Delta cannot run a single session without.
//!
//! Its own module — named after the check it performs — so the probe, the
//! reason it exists and the tests that pin its message sit together instead of
//! thickening the crate root, which is already the crate's longest file and
//! grows with every host requirement a later slice adds.

use delta_usecase::BinaryDetector;
use tmux_driver::TMUX_BIN;

use crate::{Error, Result};

/// Refuse to start when `tmux` is not on `PATH`.
///
/// Delta deliberately bundles none of the host tools it drives. A missing
/// provider binary is a soft condition — that provider is simply reported
/// unavailable in the new-session selector and the other one still works — but
/// [`TMUX_BIN`] is not a provider: every launch, whichever provider it is for,
/// goes through the tmux driver, so a host without it can run no session at
/// all. Left unchecked, the first launch fails inside the driver with the raw
/// OS text ("No such file or directory"), which reaches the browser as a spawn
/// failure that never names tmux.
///
/// Startup is the earliest and plainest place to say so — the moment the binary
/// is first tried after an install — so the condition is reported as an error
/// here and the server never comes up. Probing goes through the same
/// [`BinaryDetector`] the availability endpoint uses, and names the binary
/// through the driver's own constant, so the check can never resolve a
/// different command than the spawn runs.
///
/// The probe is not repeated afterwards: tmux vanishing while the server runs
/// is a different failure, and the driver's own error covers it.
pub(super) async fn ensure_tmux_available(detector: &dyn BinaryDetector) -> Result<()> {
    if detector.is_available(TMUX_BIN).await {
        return Ok(());
    }
    Err(Error::MissingCommand {
        bin: TMUX_BIN.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scripted [`BinaryDetector`] answering from a fixed set of present
    /// binaries.
    ///
    /// Local to this crate: the use-case crate's equivalent fake is
    /// `pub(crate)` there, and the startup probe is not reason enough to widen
    /// another crate's test surface.
    struct ScriptedDetector {
        present: Vec<String>,
    }

    impl ScriptedDetector {
        fn with_present(bins: &[&str]) -> Self {
            Self {
                present: bins.iter().map(|bin| (*bin).to_owned()).collect(),
            }
        }

        fn none_present() -> Self {
            Self {
                present: Vec::new(),
            }
        }
    }

    #[async_trait::async_trait]
    impl BinaryDetector for ScriptedDetector {
        async fn is_available(&self, bin: &str) -> bool {
            self.present.iter().any(|each| each == bin)
        }
    }

    /// tmux absent → the startup probe refuses, naming the command and where it
    /// was looked for. This message is what a fresh install prints on a host
    /// that never had tmux.
    #[tokio::test]
    async fn the_tmux_probe_refuses_when_the_command_is_absent() {
        let detector = ScriptedDetector::none_present();

        let err = ensure_tmux_available(&detector)
            .await
            .expect_err("tmux is absent");

        assert!(
            matches!(&err, Error::MissingCommand { bin } if bin == "tmux"),
            "the missing command is carried as data: {err:?}"
        );
        assert_eq!(
            err.to_string(),
            "required command 'tmux' was not found on PATH"
        );
    }

    /// tmux present → the probe is silent and boot carries on. The probe reads
    /// the driver's own constant, so this also pins the two to one name.
    #[tokio::test]
    async fn the_tmux_probe_passes_when_the_command_is_present() {
        let detector = ScriptedDetector::with_present(&[TMUX_BIN]);

        assert!(ensure_tmux_available(&detector).await.is_ok());
    }
}
