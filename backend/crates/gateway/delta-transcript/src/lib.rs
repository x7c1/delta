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
// The single-line parser is public so test harnesses (e.g. the attribution
// crate's golden corpus) can feed raw JSONL fixtures through the exact
// production parsing without going through the filesystem reader.
pub use parse::parse_line;
pub use reader::JsonlTranscript;
