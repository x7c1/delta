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
///
/// # Positional consumption applies here unchanged
///
/// The fold consumes the head outstanding send by POSITION: the next human
/// user line is that send's echo whatever its text. Live that is exact — at
/// most one send is outstanding, and it is outstanding only for the window
/// between its keystrokes and its turn. Replay seeds the WHOLE send history at
/// once, so the rule is weaker here: a human line typed straight into the pane
/// *before* a later send was ever dispatched would consume that send and be
/// filed on its thread, and the send's real echo would then land on `main`.
///
/// Replay is nonetheless kept byte-for-byte identical to the live fold, with no
/// dispatch-position guard, for two reasons:
///
/// - no case in the golden corpus can tell the two rules apart, so a guard
///   would be machinery nothing exercises. Of the sixteen cases only
///   `multi_send_session` mixes sends with pane typing at all, and its stray
///   line lands *after* the last send has been consumed; `external_input_only`
///   carries no sends whatsoever. Read that as "the corpus never contains the
///   dangerous ordering", not as "the corpus shows the ordering to be safe";
///   and
/// - the corpus replays through this function, so any divergence from the live
///   fold would stop the goldens (and the batch-split invariance suite) from
///   pinning what production actually does — a far more expensive loss than the
///   misattribution it would prevent in a tool that has no callers yet.
///
/// Revisit when the repair tool is built: the honest guard needs each send's
/// dispatch position in the transcript, which the `Send` row does not record
/// today.
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
        // Likewise empty: a whole-history replay sees each local-command group's
        // caveat before its trailing lines within the single replayed batch.
        local_command_prompts: std::collections::HashSet::new(),
    };
    attribute_lines(session_id, main_thread, state, lines)
}
