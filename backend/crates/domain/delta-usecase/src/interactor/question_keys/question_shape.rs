//! The shape of one question, as the key generator needs it.

#[cfg(doc)]
use super::parse_question_shapes;

/// The shape of one question, as the generator needs it: its option count and
/// whether more than one option may be selected. Parsed from the stored
/// `{"questions":[…]}` tool input (see [`parse_question_shapes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestionShape {
    /// How many real options the question lists (the navigable range).
    pub option_count: usize,
    /// Whether the question is multi-select (`[ ]` checkboxes) vs single-select.
    pub multi_select: bool,
}
