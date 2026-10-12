//! Why the app does not install a ready download itself where its platform
//! does, and the way by hand.

use super::ManualInstall;

/// A ready download this app does not install itself, although its platform
/// installs updates in the app: the installer found at startup that it
/// cannot install any update here (on macOS, the app does not run from a
/// `Delta.app` this user may replace). The browser shows `cause` and
/// `manual` in place of an Install that could only end
/// [`UpdateInstall::Unavailable`](super::UpdateInstall::Unavailable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallUnavailable {
    /// Why Delta cannot install the update itself, one line.
    pub cause: String,
    /// How the user installs the ready download by hand.
    pub manual: ManualInstall,
}
