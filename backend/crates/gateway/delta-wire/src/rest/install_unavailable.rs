//! A ready download the app does not install itself, and the way by hand.

use delta_usecase::InstallUnavailable;
use serde::Serialize;
use ts_rs::TS;

use super::WireManualInstall;

/// A ready download this app does not install itself although its platform
/// installs updates in the app, found when the app started (on macOS, it
/// does not run from a `Delta.app` this user may replace: not from an app
/// bundle, from a read-only copy macOS made since it was opened from its
/// disk image or the Downloads folder, or from a directory this user may not
/// write to). `error` says why, `manual` how the user installs the file by
/// hand ([`WireManualInstall`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(rename = "InstallUnavailable")]
pub struct WireInstallUnavailable {
    pub error: String,
    pub manual: WireManualInstall,
}

impl From<InstallUnavailable> for WireInstallUnavailable {
    fn from(unavailable: InstallUnavailable) -> Self {
        Self {
            error: unavailable.cause,
            manual: unavailable.manual.into(),
        }
    }
}
