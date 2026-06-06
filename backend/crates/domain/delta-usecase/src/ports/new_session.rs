//! The fields needed to register the session on first contact.

use delta_model::SessionId;

/// The fields needed to register the session on first contact.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: SessionId,
    pub cwd: String,
    pub transcript_path: String,
}
