//! Replaying a whole transcript through the attribution fold.

use delta_model::{SessionId, ThreadId};

use crate::attribute::{attribute_lines, Attributed, AttributionState, OutstandingSend};
use crate::transcript_message::TranscriptMessage;

/// Replay a session's full transcript from line 0 through the pure fold.
///
/// Given the same overlay inputs the live system saw — the session's sends
/// (with their thread and semantic parent) in dispatch order — this
/// reproduces every message's `(thread_id, semantic_parent_uuid)` assignment:
/// the seed is a fresh session's seed (`carry_thread = main`, no persisted
/// user message yet) with the whole send history queued, and each echo line
/// consumes its send in turn exactly as the live ingestion did batch by
/// batch.
///
/// This is the read-only half of a future repair tool: recomputing a
/// session's overlay from its transcript plus its send history, without
/// touching the store.
pub fn replay(
    session_id: &SessionId,
    main_thread: ThreadId,
    sends_in_dispatch_order: Vec<OutstandingSend>,
    lines: Vec<TranscriptMessage>,
) -> Attributed {
    let state = AttributionState {
        carry_thread: main_thread,
        outstanding: sends_in_dispatch_order.into(),
        // A whole-history replay starts with no outstanding background launch:
        // every launch and its completion fall within the single replayed
        // batch, so the map is built and drained as the lines fold.
        launched_threads: std::collections::BTreeMap::new(),
    };
    attribute_lines(session_id, main_thread, state, lines)
}
