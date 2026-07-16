//! [`AgentContentSource`]: the domain-side, push-based conversation-content
//! seam a provider's accumulator implements.
//!
//! Delta already has a *pull-based* content seam inside the interactor
//! (`ConversationSource::next_batch`, private to `delta-usecase`): Claude's
//! transcript reader is asked for the next batch and returns the newly-observed
//! `(messages, effects)`. That shape fits a provider whose content is a file
//! Delta reads on its own schedule.
//!
//! A push-based provider — Codex's app-server — is the mirror image: it *pushes*
//! structured `item/*` / `turn/*` frames as neutral [`AgentEvent`]s, one at a
//! time, and the content each frame completes is known only as it arrives. This
//! trait is the seam for that direction: the future event pump feeds it one
//! [`AgentEvent`] at a time and gets back the canonical conversation content
//! that event completed — the same `(Vec<Message>, Vec<Effect>)` batch the
//! provider-neutral persistence pipeline (`persist_conversation_batch`)
//! consumes, so a pushed Codex turn persists and renders through the exact path
//! Claude already runs through.
//!
//! ## Why it lives in `delta-usecase`
//!
//! The pump is a `delta-usecase` concern, and `delta-usecase` cannot depend on
//! the gateway crate (`codex-agent`) — that is the wrong dependency direction.
//! So the trait is declared here, in the domain, and `codex-agent`'s
//! `CodexConversationSource` implements it (`codex-agent` → `delta-usecase`).
//! Its output types ([`Message`], [`Effect`]) are re-exported from this crate,
//! so an implementor needs no direct dependency on `delta-model` /
//! `delta-attribution` beyond `delta-usecase`.
//!
//! [`Effect`]: crate::Effect

use delta_attribution::Effect;
use delta_model::Message;

use crate::agent::AgentEvent;

/// A per-session, push-based producer of canonical conversation content.
///
/// One instance per session. The pump feeds it every event from the session's
/// neutral event stream with [`Self::ingest`]; each call returns the content
/// that event *completed* — the messages plus the ordered [`Effect`]s the
/// neutral persistence pipeline must execute for this batch, in decision order.
/// Control-only and streaming events complete no content, so they yield an
/// empty batch (`(vec![], vec![])`).
///
/// A pure fold: the implementation owns whatever cross-event state it needs
/// (sequence counters, pending tool-call pairing), never performs I/O, and is
/// therefore cheap to call for every event. `Send + Sync` because the session
/// actor owns one inside its runtime state and holds a `&SessionContext` across
/// `await` points (which the actor future must be able to send across threads),
/// and `Debug` so the session runtime that owns one (via a boxed trait object)
/// keeps its derived `Debug`.
pub trait AgentContentSource: Send + Sync + std::fmt::Debug {
    /// Fold one neutral [`AgentEvent`] into the canonical conversation content
    /// it completed: the messages newly produced by this event and the ordered
    /// [`Effect`]s the persistence pipeline must execute for them. Empty when
    /// the event carried no content (control-only or streaming).
    fn ingest(&mut self, event: &AgentEvent) -> (Vec<Message>, Vec<Effect>);
}

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
