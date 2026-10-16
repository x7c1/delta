//! The identifier of a session.

use crate::newtype::string_newtype;

string_newtype! {
    /// Delta's identifier for a session, minted by Delta when it starts the
    /// launch, whatever the provider. For a Claude session it is also Claude
    /// Code's own session id (the `session_id` its hooks report), which is
    /// where the id of an externally launched `claude` comes from; a Codex
    /// session keeps the provider's ids separately on [`crate::Session`].
    SessionId
}
