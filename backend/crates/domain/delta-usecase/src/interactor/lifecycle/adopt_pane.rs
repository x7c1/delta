//! Re-adopting a pane that outlived the Delta process that launched it.
//!
//! A Claude Code session runs in a tmux pane on Delta's own tmux server, and
//! that pane keeps running when Delta stops — the desktop app is quit or
//! upgraded, it crashes, a dev server is restarted. Everything that says the
//! session is open is runtime state the restart forgot, so without this the
//! session would read as closed while its agent is still at work, and the next
//! send would resume the conversation into a *second* `claude` process driving
//! the same files.
//!
//! The session row therefore remembers its pane while it is bound
//! ([`RememberedPane`], written by [`SessionContext::remember_bound_pane`] and
//! cleared by the bound teardown), and two paths read it back:
//!
//! - **Boot** ([`SessionContext::readopt_remembered_pane`]), driven for every
//!   remembered pane by [`Interactor::readopt_surviving_sessions`] before the
//!   server accepts requests.
//! - **The resume backstop** in [`SessionContext::open_session`], which adopts
//!   a still-live remembered pane instead of launching `claude --resume`, so no
//!   ordering between boot and a send can start a second process.
//!
//! Both bind the pane through [`SessionContext::adopt_live_pane`]. Adoption runs
//! nothing in tmux: the agent is already up and input-ready, so unlike a resume
//! there is no readiness window to hold the first send behind.
//!
//! [`Interactor::readopt_surviving_sessions`]: crate::Interactor::readopt_surviving_sessions

use delta_model::{Message, MessageUuid, Role, Send, Session};

use crate::error::Result;
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::OpenHandle;
use crate::pane_token::PaneToken;
use crate::ports::{GitWorktree, RememberedPane, SessionStore, TmuxDriver, Transcript, Workspace};

/// What became of one session's remembered pane at boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::interactor) enum Readoption {
    /// The pane is still running and the session is open on it again.
    /// `hooks_unreachable` says whether its agent can still reach this server.
    Adopted { hooks_unreachable: bool },
    /// The pane is gone. Its record was cleared and the session stays closed;
    /// the next send resumes it as usual.
    Gone,
    /// tmux could not be asked whether the pane exists. The record is kept, so
    /// the resume backstop asks again before any send can launch a second
    /// agent, and the session stays closed meanwhile.
    Unprobed,
    /// Nothing to do: the row remembers no pane (or no longer exists), or the
    /// session is already live in this process.
    NotRemembered,
}

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Record the currently bound pane on the session row, so a restarted Delta
    /// can find it again. A no-op when no pane is bound.
    ///
    /// Called right after every bind. A failed write does not undo the bind:
    /// the session is open and works in this process, and the only thing lost
    /// is surviving a restart, so the failure is logged rather than handed to a
    /// caller that has already committed to the binding.
    pub(in crate::interactor) async fn remember_bound_pane(&self) {
        let Some(handle) = self.state.handle() else {
            return;
        };
        if let Err(err) = self
            .store
            .remember_pane(self.id, &handle.remembered())
            .await
        {
            tracing::warn!(
                session_id = %self.id,
                token = %handle.token.as_str(),
                error = %err,
                "could not record the session's pane on its row; the session works, \
                 but a restart of Delta will not re-adopt it"
            );
        }
    }

    /// Erase the pane the session row remembers, for a path that dropped the
    /// binding while killing its pane outside the bound teardown (a resume the
    /// watchdog gave up on). A failed write is logged and left: the record
    /// then names a pane that no longer exists, which the next boot or resume
    /// finds gone and clears by itself.
    pub(in crate::interactor) async fn forget_pane_best_effort(&self) {
        if let Err(err) = self.store.forget_pane(self.id).await {
            tracing::warn!(
                session_id = %self.id,
                error = %err,
                "could not clear the pane a session remembered after killing it; \
                 the stale record is cleared when the pane is next looked for"
            );
        }
    }

    /// Re-adopt this session's remembered pane at boot, if it is still running.
    ///
    /// `hook_endpoint_changed` is the server's report that the hook URLs it
    /// serves differ from the previous run's
    /// (`delta_bootstrap::Config::hook_endpoint_changed`). That flag says
    /// surviving agents *would* be stranded, not that any exist — it is also
    /// true on a first run — so it is applied only here, to a pane actually
    /// found alive: that session is marked [`RememberedPane::hooks_unreachable`],
    /// persisted so a later restart whose endpoint happens to match does not
    /// forget it.
    ///
    /// See [`Readoption`] for the outcomes. Only a store error is an `Err`; a
    /// tmux probe that could not run is [`Readoption::Unprobed`].
    pub(in crate::interactor) async fn readopt_remembered_pane(
        &mut self,
        hook_endpoint_changed: bool,
    ) -> Result<Readoption> {
        if self.state.has_live_pane() {
            return Ok(Readoption::NotRemembered);
        }
        let Some(remembered) = self.store.remembered_pane(self.id).await? else {
            return Ok(Readoption::NotRemembered);
        };
        let Some(session) = self.store.session(self.id).await? else {
            return Ok(Readoption::NotRemembered);
        };
        let marked = RememberedPane {
            hooks_unreachable: remembered.hooks_unreachable || hook_endpoint_changed,
            ..remembered.clone()
        };
        match self.tmux.has_session(&remembered.tmux_session).await {
            Ok(true) => {
                self.adopt_live_pane(&session, marked.clone()).await;
                // The record only moves when this adoption marked the hooks
                // unreachable; otherwise it already says exactly this.
                if marked != remembered {
                    self.remember_bound_pane().await;
                }
                tracing::info!(
                    session_id = %self.id,
                    token = %marked.tmux_session,
                    hooks_unreachable = marked.hooks_unreachable,
                    "re-adopted a session whose pane survived the restart"
                );
                Ok(Readoption::Adopted {
                    hooks_unreachable: marked.hooks_unreachable,
                })
            }
            Ok(false) => {
                self.store.forget_pane(self.id).await?;
                tracing::info!(
                    session_id = %self.id,
                    token = %remembered.tmux_session,
                    "the pane a session remembered is gone; it stays closed"
                );
                Ok(Readoption::Gone)
            }
            Err(err) => {
                // The pane may still be running with the old hook URLs, so a
                // changed endpoint is recorded now: the backstop that adopts
                // it later cannot tell a changed endpoint from an unchanged one.
                if marked != remembered {
                    self.store.remember_pane(self.id, &marked).await?;
                }
                tracing::warn!(
                    session_id = %self.id,
                    token = %remembered.tmux_session,
                    error = %err,
                    "could not ask tmux whether a session's remembered pane survived \
                     the restart; leaving it closed, and a send to it checks again \
                     before resuming"
                );
                Ok(Readoption::Unprobed)
            }
        }
    }

    /// Adopt the remembered pane instead of resuming, when it is still alive —
    /// the backstop [`Self::open_session`] runs before it would launch
    /// `claude --resume`. Returns whether the session is now open on it.
    ///
    /// A pane that is gone has its record cleared and `false` comes back, so
    /// the resume goes ahead. A probe that could not run is an error: resuming
    /// past it could start a second agent next to one that is still running,
    /// which is the outcome this exists to rule out.
    pub(in crate::interactor) async fn adopt_remembered_pane_if_alive(
        &mut self,
        session: &Session,
    ) -> Result<bool> {
        let Some(remembered) = self.store.remembered_pane(self.id).await? else {
            return Ok(false);
        };
        if !self.tmux.has_session(&remembered.tmux_session).await? {
            self.store.forget_pane(self.id).await?;
            return Ok(false);
        }
        tracing::info!(
            session_id = %self.id,
            token = %remembered.tmux_session,
            hooks_unreachable = remembered.hooks_unreachable,
            "a send reached a closed session whose remembered pane is still running; \
             adopting that pane instead of resuming into a second one"
        );
        self.adopt_live_pane(session, remembered).await;
        Ok(true)
    }

    /// Bind a running pane to the session without launching anything, then
    /// catch the transcript up from the stored cursor.
    ///
    /// The turn is closed first, as a resume does: whatever turn the session's
    /// previous binding left behind cannot be continued by this one, and the
    /// agent in the pane is ready for input either way. The catch-up ingests
    /// whatever the agent wrote while Delta was not reading — a turn that
    /// finished while the app was closed shows up straight away — and from then
    /// on the background tail, the liveness probe and the echo watchdog treat
    /// the session like any other open one. A held send whose prompt the
    /// catch-up shows was delivered by the surviving pane, so it is settled
    /// rather than left waiting for a Send that would type it twice (see
    /// [`delivered_held_sends`]).
    ///
    /// Neither step can unbind the pane, so neither failure is returned: the
    /// pane is live and bound whatever happens, the next tick retries the sync,
    /// and an `Err` would tell a caller the session was not adopted when it
    /// was. Both are logged instead.
    async fn adopt_live_pane(&mut self, session: &Session, remembered: RememberedPane) {
        self.state.bind(OpenHandle {
            token: PaneToken::adopted(remembered.tmux_session.clone()),
            pane: remembered.pane.clone(),
            hooks_unreachable: remembered.hooks_unreachable,
        });
        if let Err(err) = self.apply_turn_input(crate::turn::TurnInput::Close).await {
            tracing::warn!(
                session_id = %self.id,
                error = %err,
                "closing the stale turn of a re-adopted session failed; its send row \
                 may be left unsettled"
            );
        }
        match self.sync_transcript(session).await {
            Ok((messages, events)) => {
                for event in events {
                    self.emit_async_event(event);
                }
                self.settle_delivered_held_sends(&messages).await;
            }
            Err(err) => tracing::warn!(
                session_id = %self.id,
                error = %err,
                "catching up the transcript of a re-adopted session failed; the \
                 background tail retries on its next tick"
            ),
        }
    }

    /// Settle the session's held sends whose prompt shows up in `caught_up`,
    /// the lines a re-adoption's catch-up just ingested.
    ///
    /// The boot restore holds every send the previous process left
    /// `dispatched`, because nothing then awaits its echo. But a pane that
    /// survived the restart may have received those keystrokes and submitted
    /// them, and the catch-up is where that becomes visible: pressing Send on
    /// such a row would type the prompt a second time. The match is
    /// deliberately narrow (see [`delivered_held_sends`]); a row it cannot vouch
    /// for stays held for the user to decide.
    ///
    /// Failures are logged and leave the row held, which is the state it was
    /// already in: the adoption itself has happened either way.
    async fn settle_delivered_held_sends(&self, caught_up: &[Message]) {
        let held: Vec<Send> = match self.store.open_sends(self.id).await {
            Ok(sends) => sends.into_iter().filter(|s| s.held_at.is_some()).collect(),
            Err(err) => {
                tracing::warn!(
                    session_id = %self.id,
                    error = %err,
                    "could not list the held sends of a re-adopted session; any the \
                     surviving pane already submitted stay held"
                );
                return;
            }
        };
        for (send_id, uuid) in delivered_held_sends(&held, caught_up) {
            match self.store.settle_held_send(send_id, &uuid).await {
                Ok(true) => tracing::info!(
                    session_id = %self.id,
                    send_id,
                    matched_uuid = %uuid.as_str(),
                    "a held send was already submitted by the pane that survived the \
                     restart; settled it as delivered"
                ),
                Ok(false) => {}
                Err(err) => tracing::warn!(
                    session_id = %self.id,
                    send_id,
                    error = %err,
                    "could not settle a held send the surviving pane already submitted; \
                     it stays held"
                ),
            }
        }
    }
}

/// Pair each held send with the caught-up user line that shows it was
/// submitted, returning `(send id, line uuid)` for the pairs found.
///
/// Conservative on purpose — a held row that is wrongly settled is a message
/// silently lost, while one wrongly left held only asks the user once more. A
/// line counts for a send only when it is a user line whose trimmed text equals
/// the send's trimmed prompt (Delta typed the prompt itself, so the agent
/// records exactly that) and its timestamp is not earlier than the send row's
/// creation, compared at the one-second precision the send row keeps. A line
/// with no timestamp, or either timestamp in a shape that is not ISO-8601,
/// matches nothing. Sends are walked oldest first and each line is used once,
/// after the line the previous send matched, so two held sends with the same
/// text take two lines in order rather than one line twice.
fn delivered_held_sends(held: &[Send], caught_up: &[Message]) -> Vec<(i64, MessageUuid)> {
    let mut sends: Vec<&Send> = held.iter().collect();
    sends.sort_by_key(|s| s.id);
    let mut lines: Vec<&Message> = caught_up
        .iter()
        .filter(|m| matches!(m.role, Role::User))
        .collect();
    lines.sort_by_key(|m| m.seq);

    let mut settled = Vec::new();
    let mut next_line = 0;
    for send in sends {
        let Some(sent_at) = to_the_second(&send.created_at) else {
            continue;
        };
        let prompt = send.text.trim();
        let found = lines[next_line..].iter().position(|line| {
            line.content_text.as_deref().map(str::trim) == Some(prompt)
                && line
                    .created_at
                    .as_deref()
                    .and_then(to_the_second)
                    .is_some_and(|at| at >= sent_at)
        });
        if let Some(offset) = found {
            let index = next_line + offset;
            settled.push((send.id, lines[index].uuid.clone()));
            next_line = index + 1;
        }
    }
    settled
}

/// The `YYYY-MM-DDTHH:MM:SS` prefix of an ISO-8601 UTC timestamp, which orders
/// correctly as text; `None` for anything not shaped like one.
fn to_the_second(timestamp: &str) -> Option<&str> {
    let prefix = timestamp.get(..19)?;
    let bytes = prefix.as_bytes();
    let shaped = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 => *b == b'-',
        10 => *b == b'T',
        13 | 16 => *b == b':',
        _ => b.is_ascii_digit(),
    });
    shaped.then_some(prefix)
}
