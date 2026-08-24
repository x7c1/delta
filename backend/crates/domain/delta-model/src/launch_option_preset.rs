//! Delta-shipped launch options: the declared catalog.
//!
//! A launch option is a pass-through Delta does not validate (see
//! [`crate::LaunchOption`]): the provider owns the vocabulary, so a typo in a
//! `name` is only caught by the agent that receives it — and for Claude not
//! even there. The combinations people actually use, though, are a short known
//! list, so Delta declares them.
//!
//! A **preset** is one `(label, name, value?)` record Delta ships for a
//! provider. At startup every preset is materialized into the launch-option
//! registry as an ordinary row carrying the preset's [`key`] in
//! `launch_option.builtin_key`, so it is already there the first time the
//! Settings screen is opened. Nothing on the selection or launch path changes:
//! a shipped row's id is an ordinary id, so the picker, the composer's saved
//! selection and each adapter's launch rendering keep working unchanged.
//!
//! One user-visible consequence of "already there" is worth naming, since no
//! code changes to produce it: the composer's launch-option picker renders only
//! when the registry holds an option for the provider being started, so with
//! every provider's catalog non-empty it is now always present — including for
//! a user who never registered an option. Every shipped row starts unticked, so
//! what they see is an offer, not a changed launch.
//!
//! The catalog is the single source of truth for a shipped row's `label`,
//! `name` and `value`, which is safe precisely because the REST layer cannot
//! edit those three: `PATCH /api/launch-options/{id}` carries only
//! `default_enabled`. Startup reconciliation can therefore overwrite them
//! freely without ever destroying something the user typed, and it preserves
//! `default_enabled` — and only `default_enabled` — across a reconcile.
//!
//! Each provider's catalog is declared in that provider's gateway adapter,
//! next to the capability profile that says which vocabulary the `name` is
//! read in; the composition root exposes them behind one per-provider
//! accessor.
//!
//! [`key`]: LaunchOptionPreset::key

use crate::AgentProvider;

/// One launch option Delta ships for a provider.
///
/// Entirely `&'static str`: a catalog is a hand-written `const` in the gateway
/// adapter that owns the provider, not something loaded or generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchOptionPreset {
    /// Stable, provider-scoped identity for this preset (e.g.
    /// `claude:model-opus`), persisted in `launch_option.builtin_key`.
    ///
    /// It is the reconciliation key — the row it names is updated in place
    /// rather than recreated, so ids survive — and it is *internal*: never
    /// shown in the UI, which only learns that a row is Delta's own.
    /// Renaming a key retires the old row and ships a new one.
    pub key: &'static str,
    /// The human-friendly name shown in the Settings list and the
    /// session-start picker. Unlike a user row's label this is never absent: a
    /// shipped row the user did not write needs a name they can read.
    pub label: &'static str,
    /// What the option is called in the provider's own vocabulary — a CLI flag
    /// for Claude (`--model`), a `thread/start` field for Codex
    /// (`approvalsReviewer`).
    pub name: &'static str,
    /// The option's argument/value; `None` for a valueless option.
    pub value: Option<&'static str>,
    /// The provider this preset applies to. Must match the catalog it is
    /// declared in — pinned by a guard test in the composition root, where
    /// every catalog is visible at once.
    pub provider: AgentProvider,
}
