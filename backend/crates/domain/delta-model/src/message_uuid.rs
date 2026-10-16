//! The internal handle for a message.

use crate::newtype::string_newtype;

string_newtype! {
    /// The internal handle for a message: a Claude transcript line's uuid, or
    /// one derived from the Codex item id (synthesized when Codex names
    /// none).
    MessageUuid
}
