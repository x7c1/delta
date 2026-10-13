//! Why a key sequence could not be generated for the requested answer.

/// Why a key sequence could not be generated for the requested answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionKeyError {
    /// No questions were provided, so there is nothing to answer.
    NoQuestions,
    /// The number of selection groups did not match the number of questions.
    SelectionCountMismatch { questions: usize, selections: usize },
    /// A selected option index is outside the question's option range.
    OptionOutOfRange {
        question: usize,
        option: usize,
        option_count: usize,
    },
    /// A single-select question was given other than exactly one selection.
    SingleSelectNeedsOneOption { question: usize, selected: usize },
    /// A multi-select question was given no selections (nothing to submit).
    MultiSelectNeedsSelection { question: usize },
}
