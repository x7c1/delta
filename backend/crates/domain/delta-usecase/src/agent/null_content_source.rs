//! [`NullContentSource`]: the content source that produces nothing.

use delta_attribution::Effect;
use delta_model::Message;

use crate::agent::{AgentContentSource, AgentEvent};

/// A content source that produces nothing.
///
/// The default [`AgentContentSource`] a provider that does not push structured
/// content frames returns from [`crate::agent::AgentAdapter::content_source`].
/// Claude pulls its conversation content from a JSONL transcript
/// (`ConversationSource`), so its event pump — were one ever run — would fold no
/// content here; this is the harmless seam it gets. The event pump only runs for
/// a push-based provider (Codex), whose adapter overrides the default with a
/// real accumulator, so this is never actually fed in production.
#[derive(Debug, Default)]
pub struct NullContentSource;

impl AgentContentSource for NullContentSource {
    fn ingest(&mut self, _event: &AgentEvent) -> (Vec<Message>, Vec<Effect>) {
        (Vec::new(), Vec::new())
    }
}
