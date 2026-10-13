//! The identifier shared by all lines of one turn.

use crate::newtype::string_newtype;

string_newtype! {
    /// Claude Code's `promptId`, shared by all lines of one turn.
    PromptId
}
