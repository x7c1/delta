use serde::{Deserialize, Serialize};
use tauri::LogicalSize;

/// A remembered content size and the screen it was made on: the state file's
/// contents.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct SavedSize {
    /// Content width in logical pixels.
    pub width: f64,
    /// Content height in logical pixels.
    pub height: f64,
    /// The work area of the monitor the window had this size on, in that
    /// monitor's logical pixels (the whole monitor on GNOME on Wayland).
    /// `None` when no monitor could be read, and in a file written before
    /// Delta kept it; such a size is fitted to the screen like one from a
    /// smaller screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<LogicalSize<f64>>,
}

impl SavedSize {
    pub fn size(self) -> LogicalSize<f64> {
        LogicalSize::new(self.width, self.height)
    }
}
