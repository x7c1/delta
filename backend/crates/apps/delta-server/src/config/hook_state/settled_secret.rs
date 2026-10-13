//! The hook secret a start settled on.

/// The hook secret a start settled on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettledSecret {
    /// The secret this run renders into hook URLs and accepts on them.
    pub secret: String,
    /// Whether it differs from the secret the state file recorded — or there
    /// was none to compare with — so hook URLs a previous run rendered carry a
    /// secret this run refuses.
    pub changed: bool,
}
