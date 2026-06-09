//! Filesystem-backed [`delta_usecase::Workspace`] implementation.
//!
//! [`FsWorkspace`] writes the server-rendered `settings.json` to a Delta-owned
//! path (so Claude Code's HTTP hooks point back at the running server) and
//! serves the read-only filesystem queries a directory picker needs: validating
//! a user-selected session working directory and browsing local subdirectories.

mod error;
mod workspace;

pub use error::{Error, Result};
pub use workspace::FsWorkspace;
