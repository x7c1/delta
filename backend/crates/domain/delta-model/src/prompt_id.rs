//! The identifier shared by all lines of one turn.

use crate::newtype::string_newtype;

string_newtype! {
    /// The identifier shared by all messages of one turn: Claude Code's
    /// `promptId`, or the Codex turn id.
    PromptId
}
