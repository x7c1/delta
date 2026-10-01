//! Helpers shared by the input decoder's tests.

use super::InputEvent;

/// An [`InputEvent`] with a prompt reduced to its text, for the tests
/// about what is submitted rather than how it was split.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Decoded {
    Prompt(String),
    Interrupt,
}

impl From<InputEvent> for Decoded {
    fn from(event: InputEvent) -> Self {
        match event {
            InputEvent::Prompt(submission) => Self::Prompt(submission.text()),
            InputEvent::Interrupt => Self::Interrupt,
        }
    }
}
