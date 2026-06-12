//! The session-actor registry: `session_id` → mailbox.
//!
//! The registry is the routing layer's only way to reach a session's runtime
//! state. Posting locks the map, spawning the actor on first contact when the
//! caller asked for that, and sends while still holding the lock — sends are
//! non-blocking (unbounded channel), and posting under the lock is what makes
//! actor retirement race-free (see the `actor` module docs).
//!
//! ## Unknown/dead sessions
//!
//! - **Commands and hooks** spawn an actor on first contact ([`Self::post`]):
//!   a hook may legitimately name a stored-but-quiet session (or register an
//!   external one), and a command for a truly unknown id fails inside the
//!   handler with the same store-backed error as before. An actor left with
//!   no runtime state retires on its own.
//! - **Queries and ticks** never spawn ([`Self::post_existing`]): a session
//!   with no actor is closed/idle by definition, so the caller substitutes
//!   that default instead of materializing an actor just to read it.
//! - A message posted while the interactor is being torn down (the core is
//!   gone) is dropped; its reply channel closes and the caller surfaces
//!   [`Error::Internal`](crate::error::Error::Internal).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use delta_model::SessionId;
use tokio::sync::mpsc;

use crate::interactor::InteractorCore;
use crate::ports::{SessionStore, TmuxDriver, Transcript, Workspace};

use super::actor;
use super::input::SessionInput;

/// The shared map behind the registry. Actors hold a [`Weak`] to it so they
/// can remove themselves on retirement without keeping the registry alive.
pub(in crate::interactor) type ActorMap = HashMap<SessionId, mpsc::UnboundedSender<SessionInput>>;

/// Routes per-session inputs to the owning actor, spawning actors lazily.
pub(in crate::interactor) struct SessionRegistry<T, X, S, W> {
    /// The shared core handed to each spawned actor. Weak so the actors (which
    /// each hold a strong `Arc`) are what keep the core alive, not the
    /// registry — dropping the interactor closes every mailbox and lets the
    /// actors run down.
    core: Weak<InteractorCore<T, X, S, W>>,
    actors: Arc<Mutex<ActorMap>>,
}

impl<T, X, S, W> SessionRegistry<T, X, S, W>
where
    T: TmuxDriver + 'static,
    X: Transcript + 'static,
    S: SessionStore + 'static,
    W: Workspace + 'static,
{
    pub(in crate::interactor) fn new(core: &Arc<InteractorCore<T, X, S, W>>) -> Self {
        Self {
            core: Arc::downgrade(core),
            actors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Post an input to the session's actor, spawning it on first contact.
    pub(in crate::interactor) fn post(&self, id: &SessionId, input: SessionInput) {
        let mut map = self.actors.lock().expect("actor registry poisoned");
        let input = match map.get(id) {
            Some(sender) => match sender.send(input) {
                Ok(()) => return,
                // The actor is gone without having removed itself (it
                // panicked). Replace it: a fresh actor's default state is the
                // safe baseline.
                Err(mpsc::error::SendError(input)) => {
                    tracing::error!(
                        session_id = %id,
                        "session actor died unexpectedly; respawning"
                    );
                    map.remove(id);
                    input
                }
            },
            None => input,
        };
        self.spawn_and_send_locked(&mut map, id, input);
    }

    /// Post an input only if the session already has an actor, returning
    /// whether it was delivered. Never spawns: used by queries and ticks,
    /// where "no actor" simply means closed/idle.
    pub(in crate::interactor) fn post_existing(&self, id: &SessionId, input: SessionInput) -> bool {
        let map = self.actors.lock().expect("actor registry poisoned");
        match map.get(id) {
            Some(sender) => sender.send(input).is_ok(),
            None => false,
        }
    }

    /// The ids of every session that currently has an actor, for tick fan-out.
    pub(in crate::interactor) fn ids(&self) -> Vec<SessionId> {
        self.actors
            .lock()
            .expect("actor registry poisoned")
            .keys()
            .cloned()
            .collect()
    }

    fn spawn_and_send_locked(&self, map: &mut ActorMap, id: &SessionId, input: SessionInput) {
        let Some(core) = self.core.upgrade() else {
            // Tear-down: the interactor is gone, so there is nothing to run
            // the actor against. Dropping the input closes its reply channel
            // and the caller reports the internal error.
            tracing::warn!(session_id = %id, "session input dropped: interactor is shutting down");
            return;
        };
        let (sender, receiver) = mpsc::unbounded_channel();
        let _ = sender.send(input);
        map.insert(id.clone(), sender);
        tokio::spawn(actor::run(
            core,
            id.clone(),
            receiver,
            Arc::downgrade(&self.actors),
        ));
    }
}
