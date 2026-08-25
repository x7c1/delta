use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};
use crate::turn::TurnState;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Re-type the send this session is still waiting to hear about, leaving
    /// its status as `Dispatched`.
    ///
    /// Drives recovery after Claude Code's auto- or manual `/compact`: the
    /// compaction routine swallows any prompt the user keyed in at the same
    /// moment, so an `OutstandingSend` that reached `Dispatched` (PTY
    /// keystrokes flushed) is stuck behind a missing echo and the pending
    /// chip never clears. Re-typing the same text submits it as a fresh
    /// prompt, and the `UserPromptSubmit` that follows consumes the send by
    /// position — whatever text Claude Code ends up reporting for it.
    /// Statuses stay `Dispatched` so the turn machine's `AwaitingEcho` head
    /// is unchanged; only the keystrokes are re-sent. (Which transcript line
    /// the send is attributed to is still decided by text, separately, and
    /// does not gate any of this.)
    ///
    /// "Stuck" is read from the turn machine, not from the rows: only
    /// [`TurnState::AwaitingEcho`] means no prompt submission has been heard
    /// for the send yet. A row also sits at `Dispatched` while the turn is
    /// already `InFlight { send_id: Some(_) }` — there the send *was*
    /// delivered, its echo simply arrived rewritten, so no transcript line
    /// has claimed the row and it is settled at turn end instead. Re-typing
    /// that one would deliver the same message twice, which is precisely the
    /// duplicate this recovery exists to avoid. The single-outstanding rule
    /// leaves at most one awaited send, so at most one is ever re-typed.
    ///
    /// Both detection paths reach this helper through
    /// [`Self::try_redispatch_after_compact`], which owns the debounce so
    /// the live hook and the ingest signal do not double-submit.
    ///
    /// A no-op when the session has no live pane (closed) or no send is
    /// being awaited. A failed `send_line` short-circuits and is returned to
    /// the caller — there is no graceful recovery from a dead pane
    /// mid-recovery, and the caller can decide whether to surface it.
    pub(in crate::interactor) async fn redispatch_stuck_dispatched(&mut self) -> Result<usize> {
        let Some(pane) = self.state.handle().map(|h| h.pane.clone()) else {
            return Ok(0);
        };
        let TurnState::AwaitingEcho { send_id } = self.state.turn() else {
            return Ok(0);
        };
        let sends = self.store.dispatched_sends(self.id).await?;
        let Some(send) = sends.into_iter().find(|s| s.id == send_id) else {
            // The turn machine awaits a send the store no longer reports as
            // dispatched (cancelled underneath it, or a row/turn drift worth
            // knowing about). Nothing to re-type, but do not swallow it.
            tracing::warn!(
                session_id = %self.id.as_str(),
                send_id,
                "awaited send has no dispatched row; skipping compact re-dispatch"
            );
            return Ok(0);
        };
        self.tmux.send_line(&pane, &send.text).await?;
        // The wait for the echo starts over from these keystrokes, so the
        // echo-deadline watchdog must measure from here — otherwise a
        // compaction long enough to have consumed the deadline would leave
        // the freshly re-typed send looking overdue the instant it lands.
        // (This path costs no requeue budget: the status stays `Dispatched`
        // and the turn machine is untouched.)
        self.state.restamp_awaiting_echo();
        Ok(1)
    }

    /// Claim the debounce window for the supplied `source` and, on success,
    /// re-type the awaited send if one is stuck behind a compaction. The shared
    /// entry point for both detection paths — the live
    /// `SessionStart(source=compact)` hook and the ingestion-time
    /// `Effect::AutoCompactFinished` — so a session that fires both for the
    /// same compact event re-types exactly once.
    ///
    /// `source` is folded into the structured log only; behaviour is the
    /// same for both call sites.
    pub(in crate::interactor) async fn try_redispatch_after_compact(
        &mut self,
        source: &str,
    ) -> Result<()> {
        let claimed = self
            .state
            .try_claim_auto_compact_redispatch(std::time::Instant::now());
        if claimed {
            let count = self.redispatch_stuck_dispatched().await?;
            tracing::info!(
                session_id = %self.id.as_str(),
                source,
                count,
                "re-typed stuck dispatched sends after compact"
            );
        } else {
            tracing::debug!(
                session_id = %self.id.as_str(),
                source,
                "compact re-dispatch suppressed by debounce \
                 (already covered this compact event)"
            );
        }
        Ok(())
    }
}
