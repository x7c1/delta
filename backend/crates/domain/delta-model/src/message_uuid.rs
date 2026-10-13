//! The internal handle for a message.

use crate::newtype::string_newtype;

string_newtype! {
    /// A transcript line uuid; the internal handle for a message.
    MessageUuid
}
