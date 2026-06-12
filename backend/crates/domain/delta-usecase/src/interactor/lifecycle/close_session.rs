use delta_model::SessionId;

use crate::error::{Error, Result};
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};
use crate::interactor::InteractorCore;

impl<T, X, S, W> InteractorCore<T, X, S, W>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
{
    /// Close an open session: capture its final transcript line, kill its pane,
    /// and drop it from the registry.
    ///
    /// The conversational data remains in the store; only the live pane and the
    /// `claude` process are torn down. Closing a known session that is not open
    /// is a no-op (it has no live pane to tear down), but an *unknown* id is a
    /// clean `SessionNotFound` (404) — the same rejection [`Self::open_session`]
    /// gives — so the browser can tell "already closed" apart from "no such
    /// session" rather than having a stale id silently succeed.
    ///
    /// Once closed, a session leaves the open set and the background tail
    /// ([`Self::poll_transcript`]) no longer polls it. But Claude Code may flush
    /// the turn's final assistant line to the JSONL just *after* its `Stop` hook
    /// fired, so without one last sync that line would never be ingested. So
    /// before the session leaves the open set this runs [`Self::sync_transcript`]
    /// once — while the on-disk transcript still reflects this session's own run
    /// — to capture any straggler line. The sync happens before the pane is
    /// killed and before the registry entry is removed, so it observes exactly
    /// the state the live session produced.
    pub async fn close_session(&self, id: &SessionId) -> Result<()> {
        let Some(session) = self.store.session(id).await? else {
            return Err(Error::SessionNotFound(id.as_str().to_owned()));
        };
        // Final sync to capture a last line flushed after `Stop`, before the
        // session leaves the polled (open) set. A closed-but-known session that
        // is being re-closed has no live pane; the sync is still safe (it just
        // finds no new lines), so it runs unconditionally on the known path.
        self.sync_transcript(&session).await?;
        let handle = {
            let mut registry = self.open_sessions.lock().await;
            registry.remove(id)
        };
        if let Some(handle) = handle {
            self.tmux.kill_session(handle.token.as_str()).await?;
        }
        // The pane is gone, so whatever turn was in flight can no longer
        // progress: feed `Close` into the turn machine (an unechoed outstanding
        // send is cancelled; an in-flight one is swept if it never matched).
        self.apply_turn_input(id, crate::turn::TurnInput::Close)
            .await?;
        Ok(())
    }
}
