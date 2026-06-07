//! Filesystem-backed [`delta_usecase::Workspace`] implementation.
//!
//! [`FsWorkspace`] prepares the Claude Code session working directory on disk:
//! it creates `<workdir>/.claude/` and writes the server-rendered
//! `settings.json` there so Claude Code's HTTP hooks point back at the running
//! server.

mod error;
mod workspace;

pub use error::{Error, Result};
pub use workspace::FsWorkspace;
