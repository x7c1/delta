use super::PromptPart;

/// A submitted prompt, split into the runs that were typed and the runs that
/// arrived as a bracketed paste, in input order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    pub parts: Vec<PromptPart>,
}

impl Submission {
    /// A prompt that was typed as a whole — how the launch's positional
    /// prompt, which never went through the pane, is submitted.
    pub fn typed(text: &str) -> Self {
        Self {
            parts: vec![PromptPart {
                text: text.to_owned(),
                pasted: false,
            }],
        }
    }

    /// The submitted text as the user sees it in the input box: every part,
    /// in order, with no paste boundary marked.
    pub fn text(&self) -> String {
        self.parts.iter().map(|part| part.text.as_str()).collect()
    }
}
