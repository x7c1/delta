/// One run of a [`Submission`](super::Submission): consecutive bytes that
/// were all typed, or that all arrived inside the same paste. Two pastes back
/// to back stay two parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptPart {
    pub text: String,
    pub pasted: bool,
}
