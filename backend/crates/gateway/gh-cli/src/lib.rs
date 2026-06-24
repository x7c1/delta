//! `gh` CLI-backed [`delta_usecase::GhCli`] implementation.
//!
//! [`Gh`] spawns the `gh` binary to back the new-session PR tab: an
//! authenticated `gh search prs` per lens, plus a process-cached
//! availability check for the gateway's "is gh usable here at all?" gate.
//! All shell-outs are isolated to this crate so the use-case layer never
//! depends on a subprocess being present.

mod error;
mod gh;
mod parse;

pub use error::{Error, Result};
pub use gh::Gh;
