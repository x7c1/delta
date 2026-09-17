use crate::error::Result;
use crate::interactor::lifecycle::PaneTeardown;
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Close this session if it is open on a pane that no longer exists.
    ///
    /// One of the two checks the liveness tick runs; see
    /// [`Self::reap_tick`] for why they share a tick.
    ///
    /// A session is "open" because its runtime holds a bound pane handle, and
    /// nothing else ever re-checks that the pane behind it is still there. When
    /// `claude` goes away without delivering a `SessionEnd` hook — killed,
    /// crashed, its tmux session removed from outside — or ends normally (the
    /// `SessionEnd` normal-end path deliberately leaves the binding alone), the
    /// session keeps reading as open: the list shows it open, the embedded
    /// terminal tries to attach to a pane that is not there, every send is typed
    /// into the dead pane and cancelled with an error, and a background subagent
    /// stays lit because the only thing that clears it is a process-gone sweep
    /// nothing has called. Pressing Close fixes all of it — a *closed* session
    /// already behaves correctly, because a send resumes it — so noticing the
    /// pane is gone and closing the session is the whole fix.
    ///
    /// Three guards keep that from firing on anything healthy:
    ///
    /// - **Pane-backed and bound only.** An adapter-backed (Codex) session has
    ///   no pane, and its adapter already reports process exit
    ///   (`AgentEvent::SessionEnded`), which settles it on its own path.
    /// - **Not while resuming.** A resume binds its pane immediately but is not
    ///   ready until `SessionStart(source=resume)` arrives; that window belongs
    ///   to the resume reaper and its deadline, which reports a *failed resume*
    ///   rather than a close.
    /// - **A probe error is not "gone".** Only a definite "no such session"
    ///   closes; a probe that *errored* is logged and the session left open,
    ///   since closing a healthy session on a transient failure is far worse
    ///   than waiting for the next tick. [`TmuxDriver::has_session`] draws that
    ///   line: a non-zero `tmux has-session` is the absent answer — which a tmux
    ///   server that is no longer running gives for every token, and its panes
    ///   really are gone with it — while an `Err` means the probe could not be
    ///   run at all (the `tmux` command itself failed) and says nothing about
    ///   the pane.
    ///
    /// There is deliberately no age limit alongside them: a long-lived healthy
    /// session must never be closed by a timer.
    ///
    /// The close itself is the shared bound-session teardown
    /// ([`Self::tear_down_bound_session`]) with [`PaneTeardown::AlreadyGone`] —
    /// the same thing pressing Close does, minus killing a pane that is already
    /// gone — followed by [`SessionEvent::SessionClosed`] so the browser
    /// refetches the list and the session's open sends. The teardown's own
    /// events (a `PermissionResolved` per stranded dialog, then a swept
    /// background subagent's `SubagentFinished`) go out first, mirroring the
    /// explicit close's order: nothing about the session follows the close.
    ///
    /// That the dialogs settle here matters most on this path, because nobody
    /// pressed anything: the agent is killed while a dialog is on screen, and
    /// without the settle a tick later the browser would re-raise it over a
    /// session that is now closed, where Allow can only answer `409`.
    pub(in crate::interactor) async fn close_if_pane_vanished(
        &mut self,
    ) -> Result<Vec<SessionEvent>> {
        // Cloned rather than borrowed so the probe below can reach `self.tmux`.
        let Some(token) = self.state.handle().map(|handle| handle.token.clone()) else {
            return Ok(Vec::new());
        };
        if self.state.is_resuming() {
            return Ok(Vec::new());
        }
        match self.tmux.has_session(token.as_str()).await {
            Ok(true) => return Ok(Vec::new()),
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    token = %token.as_str(),
                    error = %err,
                    "could not probe an open session's pane; leaving the session \
                     open and retrying on the next tick"
                );
                return Ok(Vec::new());
            }
        }
        let Some(session) = self.store.session(self.id).await? else {
            // The row is gone while the runtime still holds a binding — nothing
            // to sync against and nothing to tell the browser about. Report it
            // and leave the state alone rather than tearing down blind.
            tracing::warn!(
                session_id = %self.id,
                token = %token.as_str(),
                "an open session's pane is gone and its row no longer exists; \
                 leaving the binding alone"
            );
            return Ok(Vec::new());
        };
        tracing::warn!(
            session_id = %self.id,
            token = %token.as_str(),
            "an open session's pane no longer exists — its agent exited, crashed or \
             was killed; closing the session"
        );
        let (_, mut events) = self
            .tear_down_bound_session(&session, PaneTeardown::AlreadyGone)
            .await?;
        events.push(SessionEvent::SessionClosed {
            session_id: self.id.clone(),
        });
        Ok(events)
    }
}
