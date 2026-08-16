use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Re-type each `Dispatched` send for this session to the TUI, in FIFO
    /// order, leaving each send's status as `Dispatched`.
    ///
    /// Drives recovery after Claude Code's auto- or manual `/compact`: the
    /// compaction routine swallows any prompt the user keyed in at the same
    /// moment, so an `OutstandingSend` that reached `Dispatched` (PTY
    /// keystrokes flushed) is stuck behind a missing echo and the pending
    /// chip never clears. Re-typing the same text submits it as a
    /// fresh prompt — the next `UserPromptSubmit` echo matches the send's
    /// existing row via the normal text-based attribution and clears it via
    /// the usual `SendMatched` flow. Statuses stay `Dispatched` so the turn
    /// machine's `AwaitingEcho` head is unchanged; only the keystrokes are
    /// re-sent.
    ///
    /// Both detection paths reach this helper through
    /// [`Self::try_redispatch_after_compact`], which owns the debounce so
    /// the live hook and the ingest signal do not double-submit.
    ///
    /// A no-op when the session has no live pane (closed) or no
    /// `Dispatched` sends. A failed `send_line` short-circuits and is
    /// returned to the caller — there is no graceful recovery from a dead
    /// pane mid-recovery, and the caller can decide whether to surface it.
    pub(in crate::interactor) async fn redispatch_stuck_dispatched(&mut self) -> Result<usize> {
        let Some(pane) = self.state.handle().map(|h| h.pane.clone()) else {
            return Ok(0);
        };
        let sends = self.store.dispatched_sends(self.id).await?;
        let mut count = 0;
        for send in &sends {
            self.tmux.send_line(&pane, &send.text).await?;
            count += 1;
        }
        if count > 0 {
            // The wait for the echo starts over from these keystrokes, so the
            // echo-deadline watchdog must measure from here — otherwise a
            // compaction long enough to have consumed the deadline would leave
            // the freshly re-typed send looking overdue the instant it lands.
            // (This path costs no requeue budget: the statuses stay
            // `Dispatched` and the turn machine is untouched.)
            self.state.restamp_awaiting_echo();
        }
        Ok(count)
    }

    /// Claim the debounce window for the supplied `source` and, on success,
    /// re-type any `Dispatched` send stuck behind a compaction. The shared
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
