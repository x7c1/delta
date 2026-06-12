use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W> SessionContext<'_, T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Close an open session: capture its final transcript line, kill its pane,
    /// and drop its binding.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. Closing a known session that is not open
    /// is a no-op (it has no live pane to tear down), but an *unknown* id is a
    /// clean `SessionNotFound` (404) — the same rejection [`Self::open_session`]
    /// gives — so the browser can tell "already closed" apart from "no such
    /// session" rather than having a stale id silently succeed.
    ///
    /// Once closed, a session loses its live pane and the background tail
    /// no longer polls it. But Claude Code may flush the turn's final
    /// assistant line to the JSONL just *after* its `Stop` hook fired, so
    /// without one last sync that line would never be ingested. So before the
    /// pane is dropped this runs [`Self::sync_transcript`] once — while the
    /// on-disk transcript still reflects this session's own run — to capture
    /// any straggler line.
    pub(in crate::interactor) async fn close_session(&mut self) -> Result<()> {
        let Some(session) = self.store.session(self.id).await? else {
            return Err(Error::SessionNotFound(self.id.as_str().to_owned()));
        };
        // Final sync to capture a last line flushed after `Stop`, before the
        // session loses its pane. A closed-but-known session that is being
        // re-closed has no live pane; the sync is still safe (it just finds no
        // new lines), so it runs unconditionally on the known path.
        self.sync_transcript(&session).await?;
        if let Some(handle) = self.state.remove_open() {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        // The pane is gone, so whatever turn was in flight can no longer
        // progress: feed `Close` into the turn machine (an unechoed outstanding
        // send is cancelled; an in-flight one is swept if it never matched).
        self.apply_turn_input(crate::turn::TurnInput::Close).await?;
        Ok(())
    }
}
