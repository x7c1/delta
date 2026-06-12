//! Pure transcript attribution: which thread does each transcript line
//! belong to?
//!
//! Claude Code writes a session's history as a flat JSONL transcript; Delta
//! overlays a thread graph on top of it. The mapping from flat lines to
//! threads is this crate's single concern, expressed as one pure fold:
//! [`attribute_lines`] consumes parsed transcript lines plus an
//! [`AttributionState`] seed (the carry thread and the FIFO of outstanding
//! dispatched sends) and returns the attributed messages, an ordered list of
//! [`Effect`]s for the caller to execute, and the updated state.
//!
//! Nothing here performs I/O. The session actor in `delta-usecase` is the
//! thin shell around the fold: it reads the cursor and the seed from the
//! store, runs the fold, executes the effects, persists the messages, and
//! advances the cursor. Because the fold is pure and state threads through
//! explicitly, attribution is *replayable*: folding a whole transcript in one
//! batch is exactly equivalent to folding it in arbitrary cursor-sized
//! batches ([`replay`] and the golden-corpus tests pin this).
//!
//! The textual conventions Claude Code uses on the wire (interrupt markers,
//! task-notification prompts) live in [`claude_format`], so that knowledge
//! has exactly one pure home.

mod attribute;
pub mod claude_format;
mod replay;
mod transcript_message;

pub use attribute::{attribute_lines, Attributed, AttributionState, Effect, OutstandingSend};
pub use replay::replay;
pub use transcript_message::TranscriptMessage;
