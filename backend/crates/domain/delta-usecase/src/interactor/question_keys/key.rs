//! One keystroke to inject into the TUI.

#[cfg(doc)]
use super::cancel_keys;

/// One keystroke to inject into the TUI, named as the tmux `send-keys` key the
/// gateway forwards it as.
///
/// The vocabulary is exactly what the pinned `AskUserQuestion` sequences need:
/// `Down`/`Up` to move the highlight, `Space` to toggle a multi-select option,
/// `Right` to advance past a multi-select question (to the next tab or the
/// Submit tab), `Enter` to record a single-select choice / submit a review, and
/// `Escape` to cancel the whole call (see [`cancel_keys`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
    Space,
    Enter,
    Right,
    Escape,
}

impl Key {
    /// The tmux `send-keys` key name for this keystroke.
    pub fn tmux_name(self) -> &'static str {
        match self {
            Key::Down => "Down",
            Key::Up => "Up",
            Key::Space => "Space",
            Key::Enter => "Enter",
            Key::Right => "Right",
            Key::Escape => "Escape",
        }
    }
}
