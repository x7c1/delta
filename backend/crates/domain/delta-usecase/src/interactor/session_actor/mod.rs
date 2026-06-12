//! Per-session actors: one tokio task per session owning all of that
//! session's runtime state.
//!
//! The interactor used to hold four separately-locked state families
//! (open-pane registry, turn machine, pending permission waiters, and a
//! global transcript-sync lock); per-session ordering was enforced by lock
//! discipline and *all* sessions' ingestion serialized behind the one sync
//! lock. Now each session's state lives in a [`runtime::SessionRuntime`]
//! owned by its actor ([`actor`]), fed by a single mailbox of
//! [`input::SessionInput`]s, and looked up through the
//! [`registry::SessionRegistry`]. Per-session ordering is structural
//! (mailbox order), cross-session work is naturally parallel, and the global
//! sync lock is gone — the per-session transcript cursor only ever needed
//! per-session serialization.
//!
//! The SQLite store stays a single shared connection behind the
//! `SessionStore` port, so actors still serialize at the DB boundary; that is
//! accepted for now (connection pooling is a separate decision).

pub(in crate::interactor) mod actor;
pub(in crate::interactor) mod input;
pub(in crate::interactor) mod registry;
pub(crate) mod runtime;

#[cfg(test)]
mod tests;
