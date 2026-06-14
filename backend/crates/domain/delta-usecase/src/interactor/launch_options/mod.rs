//! Launch-option registry use cases: list, create, and delete the custom
//! `claude` CLI flags the user can later multi-select when starting a session.
//!
//! Each operation is a thin pass-through to the [`SessionStore`] port — the
//! registry has no cross-record invariants to enforce — kept together here so
//! the CRUD surface lives in one place.
//!
//! [`SessionStore`]: crate::ports::SessionStore

mod crud;

#[cfg(test)]
mod tests;
