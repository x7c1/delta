//! How a provider interprets a launch option's `name` and `value`.

use delta_usecase::LaunchCapability;
use serde::Serialize;
use ts_rs::TS;

/// How a provider interprets a launch option's `name` and `value`.
///
/// A registered launch option is a provider-neutral `(name, value?)` pair; what
/// the pair *means* depends on how the provider is launched, which is why this
/// is a capability rather than something the UI derives from the provider name.
/// Delta validates neither names nor values (the agent that receives them owns
/// that vocabulary), so telling the user which vocabulary to write in is the
/// only guard-rail there is — hence carrying it on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "LaunchOptionStyle")]
pub enum WireLaunchOptionStyle {
    /// `name` is a command-line flag and `value` its argument
    /// (`--permission-mode auto`). A valueless option is a bare flag.
    CliFlag,
    /// `name` is a field of the provider's session-start request and `value`
    /// that field's value (Codex's `thread/start`: `model` → `gpt-5.6-sol`). A
    /// valueless option sets a bare boolean field.
    RequestField,
}

impl From<LaunchCapability> for WireLaunchOptionStyle {
    /// Derived from *how the provider is launched*, the same profile field that
    /// decides which session path it takes: a provider Delta launches as a
    /// command line takes its options as argv flags, while one Delta drives over
    /// a structured request takes them as fields of that request. Deriving it
    /// keeps the adapter's capability profile the single source of truth, so a
    /// new provider cannot ship a launch style the UI has never heard of.
    fn from(launch: LaunchCapability) -> Self {
        match launch {
            LaunchCapability::PtyCommand => WireLaunchOptionStyle::CliFlag,
            LaunchCapability::JsonRpcAppServer => WireLaunchOptionStyle::RequestField,
        }
    }
}
