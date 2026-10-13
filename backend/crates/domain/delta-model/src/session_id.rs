//! The identifier of a session.

use crate::newtype::string_newtype;

string_newtype! {
    /// Identifier of the single Claude Code session (`session_id` from hooks).
    SessionId
}
