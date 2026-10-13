//! One registered clone root.

/// One registered clone root: a directory where the user's git clones live,
/// whose direct children the Repository tab probes for clones on every list
/// call. Session-independent (no foreign key, never cascaded), so a clone root
/// outlives any individual session and is only ever rewritten through the
/// dedicated CRUD endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRoot {
    pub path: String,
    pub created_at: String,
}
