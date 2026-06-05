//! JSONL transcript reader.
//!
//! Claude Code writes its session transcript as JSON Lines: one JSON object per
//! line, each a message with a `uuid`, `parentUuid`, `type`, an embedded
//! `message` carrying content blocks, and a `promptId`. [`JsonlTranscript`]
//! parses those lines into [`delta_usecase::TranscriptMessage`] values.
//!
//! Parsing is lenient: unknown top-level and block fields are ignored, and line
//! kinds Delta does not model still parse (their role becomes `Other`) so that
//! linear parent chains remain walkable.

mod error;
mod parse;
mod reader;

pub use error::{Error, Result};
pub use reader::JsonlTranscript;
