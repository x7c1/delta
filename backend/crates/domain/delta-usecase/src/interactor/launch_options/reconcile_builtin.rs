//! Bringing the registry's Delta-shipped rows in line with the declared
//! catalog, at startup.
//!
//! Separate from the registry's CRUD because it is the only writer of a shipped
//! row's content, and the only caller is the composition root's boot sequence
//! — not the REST layer.

use delta_model::LaunchOptionPreset;

use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Bring the registry's Delta-shipped rows in line with the declared
    /// `catalog`: materialize every preset, then drop the shipped rows whose key
    /// the catalog no longer declares.
    ///
    /// `catalog` is the **whole** declared set — every provider's presets
    /// concatenated — because the sweep at the end is registry-wide: called
    /// once per provider with only that provider's presets, it would retire the
    /// other provider's rows as undeclared. Each preset carries its own
    /// provider, so one flat slice loses nothing.
    ///
    /// Run at startup, so a shipped option is already there the first time
    /// Settings is opened — and comes back by itself after a `make reset`.
    /// Idempotent: a preset already materialized is updated in place, so a
    /// second run leaves the same rows with the same ids.
    ///
    /// Only `label`, `name` and `value` are written from the catalog;
    /// `default_enabled` is preserved, because it is the one field on a shipped
    /// row the user owns (the REST layer cannot edit the other three at all,
    /// which is what makes overwriting them safe).
    ///
    /// A retired preset's row is deleted, and its id may still sit in a saved
    /// composer selection. That needs no extra handling here: a selected id
    /// that is no longer registered is skipped at launch rather than aborting
    /// it (see `resolve_launch_options`).
    pub async fn reconcile_builtin_launch_options(
        &self,
        catalog: &[LaunchOptionPreset],
    ) -> Result<()> {
        for preset in catalog {
            self.store
                .upsert_builtin_launch_option(
                    preset.key,
                    preset.label,
                    preset.name,
                    preset.value,
                    preset.provider,
                )
                .await?;
        }
        let keys: Vec<&str> = catalog.iter().map(|preset| preset.key).collect();
        let retired = self
            .store
            .delete_builtin_launch_options_except(&keys)
            .await?;
        if retired > 0 {
            tracing::info!(
                retired,
                "removed built-in launch options no longer declared in the catalog"
            );
        }
        Ok(())
    }
}
