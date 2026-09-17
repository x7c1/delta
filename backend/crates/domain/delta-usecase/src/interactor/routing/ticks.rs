//! Background ticks: the periodic fan-outs the server loop drives — the
//! transcript poll, the resume dispatch, the liveness reap (stale launches and
//! open sessions whose pane has gone) and the echo-deadline sweep. Each posts
//! one input to every live actor and collects the replies under a
//! caller-supplied bound; `collect_tick_replies` holds the shape they all
//! share.

use std::time::{Duration, Instant};

use delta_model::Message;
use tokio::sync::oneshot;

use crate::error::Result;
use crate::interactor::session_actor::input::{Reply, SessionInput};
use crate::interactor::Interactor;
use crate::ports::{GitWorktree, SessionEvent, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> Interactor<T, X, S, W, G>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
    G: GitWorktree + 'static,
{
    /// Post one tick input to every live actor and collect the replies,
    /// waiting no longer than `bound` for the fan-out as a whole.
    ///
    /// The shape all four background ticks share. Every input is posted
    /// *first* and the replies awaited after, so the per-session handlers run
    /// **concurrently** — only each session's own mailbox orders its work.
    /// Because they therefore all start at the same instant, a single
    /// deadline `bound` after the posts gives every actor the same window
    /// while capping what one tick can cost in total.
    ///
    /// That bound is the point: awaiting each reply indefinitely lets a
    /// single slow actor hold up every *other* session's result. An actor that
    /// misses the deadline is `warn!`ed about (named by session and stage)
    /// and skipped; the *work* is not lost — the actor keeps running on its own
    /// mailbox and its rows still land — only its contribution to *this* tick's
    /// return value is dropped. A dropped reply (the actor retired mid-tick,
    /// during tear-down) is skipped silently.
    ///
    /// What that dropped contribution costs differs by tick, and it is the
    /// caller's job to size `bound` accordingly. The transcript poll loses
    /// nothing a browser needs: its actor announces its own ingest on the async
    /// event seam. The other three ticks have no seam — the reply *is* how
    /// their [`SessionEvent`]s reach the broadcast, so a skipped one strands a
    /// `SendDispatched` or `SpawnFailed`. Their handlers also type into panes,
    /// which waits on purpose (the submit `Enter` is held back so the TUI
    /// cannot absorb it into the paste burst), so the poll interval is *not* a
    /// safe bound for them.
    ///
    /// `bound` comes from the caller rather than a constant here: the server
    /// loop passes its poll interval for the transcript poll and a longer
    /// backstop for the three sweeps, tests pass what they need.
    async fn collect_tick_replies<R>(
        &self,
        bound: Duration,
        stage: &'static str,
        mut make: impl FnMut(Reply<R>) -> SessionInput,
    ) -> Result<Vec<R>> {
        let mut pending = Vec::new();
        for id in self.sessions.ids() {
            let (tx, rx) = oneshot::channel();
            if self.sessions.post_existing(&id, make(tx)) {
                pending.push((id, rx));
            }
        }
        let deadline = tokio::time::Instant::now() + bound;
        let mut replies = Vec::with_capacity(pending.len());
        for (id, rx) in pending {
            match tokio::time::timeout_at(deadline, rx).await {
                Ok(Ok(result)) => replies.push(result?),
                // A dropped reply means the actor retired mid-tick
                // (tear-down); skip it rather than failing the whole tick.
                Ok(Err(_dropped)) => continue,
                Err(_elapsed) => tracing::warn!(
                    session_id = %id,
                    stage,
                    bound_ms = bound.as_millis() as u64,
                    "session actor missed the background tick deadline; \
                     skipping its result for this tick"
                ),
            }
        }
        Ok(replies)
    }

    /// Poll the transcript of every currently-open (live-pane) session for
    /// newly-written lines, by fanning a sync tick out to every live actor.
    ///
    /// Drives the continuous background tail: Claude Code often flushes the
    /// final assistant line to the JSONL *after* the `Stop` hook fires, so the
    /// hook's sync misses it. The ticks are posted to all actors first and the
    /// replies awaited after, so the per-session syncs run **concurrently** —
    /// the old global sync lock serialized them; now only each session's own
    /// mailbox orders its ingestion. A session with no live pane no-ops.
    ///
    /// Each session announces its *own* ingest as it happens, from inside its
    /// actor (see [`SessionContext::sync_tick`]), so a session whose reply
    /// misses `bound` does not delay any other session's
    /// [`SessionEvent::TranscriptUpdated`]. What is returned here is the
    /// batch itself, for callers that inspect it: each session that ingested
    /// new messages contributes one non-empty group, in arbitrary order —
    /// callers may index `group[0]` for the group's session id — alongside
    /// any [`SessionEvent`]s the ingest produced (e.g. permission resolutions
    /// from a tailed-in `tool_result`, or `TurnInterrupted` from an interrupt
    /// marker). Those events have *already been emitted* on the async event
    /// seam, so a caller that drains the seam must not broadcast them again.
    ///
    /// `bound` caps how long the fan-out waits on the actors; see
    /// [`Self::collect_tick_replies`].
    ///
    /// [`SessionContext::sync_tick`]: crate::interactor::session_actor::actor::SessionContext::sync_tick
    pub async fn poll_transcript(
        &self,
        bound: Duration,
    ) -> Result<(Vec<Vec<Message>>, Vec<SessionEvent>)> {
        let replies = self
            .collect_tick_replies(bound, "transcript poll", |reply| SessionInput::SyncTick {
                reply,
            })
            .await?;
        let mut groups = Vec::new();
        let mut events = Vec::new();
        for (messages, session_events) in replies {
            events.extend(session_events);
            if !messages.is_empty() {
                groups.push(messages);
            }
        }
        Ok((groups, events))
    }

    /// Dispatch the held first prompt of every resume that is ready *and* has
    /// settled, on the background tick (see the `ResumeTick` input docs). A
    /// settled resume with no held prompt instead flushes its session's
    /// oldest genuinely `queued` send — the resume window defers queued
    /// dispatch, and this settle is what flushes it. Held sends — restored at
    /// boot or parked by the echo deadline — are not flushed here; they wait
    /// for an explicit release ([`Self::release_send`]).
    ///
    /// Returns the [`SessionEvent::SendDispatched`]s those flushes produced,
    /// for the caller to broadcast so the browser sees each
    /// queued→dispatched transition.
    ///
    /// `now` is injected (rather than read here) so the dispatch is
    /// deterministic under test: the server loop passes `Instant::now()`,
    /// while tests advance a controlled instant. `bound` caps how long the
    /// fan-out waits on the actors; see [`Self::collect_tick_replies`].
    pub async fn dispatch_ready_resumes(
        &self,
        now: Instant,
        bound: Duration,
    ) -> Result<Vec<SessionEvent>> {
        let replies = self
            .collect_tick_replies(bound, "resume dispatch", |reply| SessionInput::ResumeTick {
                now,
                reply,
            })
            .await?;
        Ok(replies.into_iter().flatten().collect())
    }

    /// The liveness sweep: reap launches that never became ready before their
    /// deadline (the watchdog, covering both fresh spawns and resumed
    /// sessions), and close every open session whose tmux pane has gone.
    ///
    /// For each stale launch the owning actor kills the tmux pane
    /// (best-effort) and produces a [`SessionEvent::SpawnFailed`] so the
    /// browser can surface the failure and clear the optimistic pending chip.
    /// For each open, pane-backed session whose pane no longer exists — its
    /// agent exited, or went away without ever delivering a `SessionEnd` hook —
    /// the actor runs the same teardown pressing Close does and produces a
    /// [`SessionEvent::PermissionResolved`] for every request the close
    /// stranded, whatever its background-subagent sweep cleared, and a
    /// [`SessionEvent::SessionClosed`] after them, so the browser stops showing
    /// a session that cannot accept input; the next send then resumes it. Both
    /// checks ride this one
    /// tick rather than each owning a timer — see
    /// [`SessionContext::reap_tick`].
    ///
    /// `now` is injected so the watchdog is deterministic under test; the
    /// server owns the periodic tick that calls this and broadcasts the
    /// result. `bound` caps how long the fan-out waits on the actors; see
    /// [`Self::collect_tick_replies`].
    ///
    /// [`SessionContext::reap_tick`]: crate::interactor::session_actor::actor::SessionContext::reap_tick
    pub async fn reap_stale_spawns(
        &self,
        now: Instant,
        bound: Duration,
    ) -> Result<Vec<SessionEvent>> {
        let replies = self
            .collect_tick_replies(bound, "spawn reap", |reply| SessionInput::ReapTick {
                now,
                reply,
            })
            .await?;
        Ok(replies.into_iter().flatten().collect())
    }

    /// Give up on every dispatched send whose `UserPromptSubmit` echo — and
    /// every other signal — never arrived before its deadline (the echo
    /// watchdog sweep). See the
    /// [`echo_deadline`](crate::interactor::echo_deadline) module for why a
    /// time-driven input is the only thing that can recover this class.
    ///
    /// The owning actor retries such a send once (preceded by an `Escape` into
    /// the pane) and parks it on the second deadline, flushing the queue behind
    /// it either way; the returned [`SessionEvent::SendDispatched`]s are the
    /// promotions those flushes produced, for the caller to broadcast. `now` is
    /// injected so the sweep is deterministic under test; the server owns the
    /// periodic tick that calls this. `bound` caps how long the fan-out waits
    /// on the actors; see [`Self::collect_tick_replies`].
    pub async fn sweep_echo_deadlines(
        &self,
        now: Instant,
        bound: Duration,
    ) -> Result<Vec<SessionEvent>> {
        let replies = self
            .collect_tick_replies(bound, "echo deadline sweep", |reply| {
                SessionInput::EchoDeadlineTick { now, reply }
            })
            .await?;
        Ok(replies.into_iter().flatten().collect())
    }
}
