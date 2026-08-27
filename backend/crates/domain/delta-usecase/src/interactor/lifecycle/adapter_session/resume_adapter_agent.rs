//! Reattaching a closed adapter-backed session to its persisted provider
//! thread, inline on the actor.

use std::sync::Arc;

use delta_model::Session;

use crate::agent::AgentAdapterFactory;
use crate::error::{Error, Result};
use crate::interactor::session_actor::actor::SessionContext;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

use super::AdapterBind;

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Reconnect a **closed** adapter-backed session by resuming its provider
    /// thread, so a send that arrives after the in-process binding was lost
    /// (e.g. across a server restart) can dispatch over the adapter instead of
    /// falling into Claude's `claude --resume` path (which a terminal-less
    /// session cannot take — it has no pane and no transcript).
    ///
    /// This is the adapter-backed mirror of a fresh spawn's launch step: it runs
    /// the same [`InteractorCore::connect_adapter_agent`] →
    /// [`Self::install_agent_binding`] sequence, but with `adapter.resume`
    /// against the session's **persisted** provider id (reattaching to the same
    /// thread) and the content source **seeded at the session's persisted
    /// message count** — which, for a single-thread adapter-backed session whose
    /// seqs are minted densely from 0, is exactly `MAX(seq) + 1`. Seeding at 0
    /// (as a fresh spawn does) would renumber/duplicate the existing history.
    ///
    /// Unlike a fresh spawn it stays **synchronous**, inside the send that
    /// triggered it: the session already exists and is being written to, so
    /// there is no eager row to answer with and nothing to show the user
    /// meanwhile — deferring it would only turn a failure the caller can see
    /// into an event it cannot correlate.
    ///
    /// **Launch options are deliberately not re-applied here.** They configure
    /// the provider *thread*, which the resume reattaches to rather than mints:
    /// Codex's `thread/resume` takes its config fields as optional *overrides*
    /// of what the resumed thread already carries, so sending none keeps the
    /// thread exactly as `thread/start` configured it. Delta also has no
    /// per-session record of which options were selected (the registry is
    /// session-independent and the `session` row stores no selection), so there
    /// is nothing to replay. This matches the Claude path, where a resume is
    /// `claude --settings … --resume <id>` with none of the launch flags the
    /// original spawn carried.
    ///
    /// The worktree's source repository **is** re-supplied, because it is not a
    /// selection being replayed: it is a fact about the session's own working
    /// directory, persisted on the row and read back below, and a provider that
    /// sandboxes writes needs it as much on the second attach as on the first.
    ///
    /// The session's **metadata is still reported after a resume**, and is
    /// re-established rather than remembered: the launch directory comes from the
    /// persisted row (which outlives the restart by definition), its branch is
    /// re-observed below, and the model comes from the `thread/resume` response
    /// — which carries the same required top-level `model` as `thread/start`, so
    /// the reattached thread re-announces what it is running. Nothing about a
    /// resumed session's metadata degrades relative to a fresh one.
    ///
    /// The caller resolves `factory` through the registry
    /// ([`InteractorCore::adapter_backed_factory`](crate::interactor::InteractorCore::adapter_backed_factory))
    /// — the same predicate that decided the session is adapter-backed at all.
    /// The persisted provider ids and session row are the source of truth that
    /// survives the restart; on failure the row is left as-is (unlike a fresh
    /// spawn there is nothing eager to roll back).
    ///
    /// [`InteractorCore::connect_adapter_agent`]: crate::interactor::InteractorCore::connect_adapter_agent
    pub(in crate::interactor) async fn resume_adapter_agent(
        &mut self,
        factory: &Arc<dyn AgentAdapterFactory>,
        session: &Session,
    ) -> Result<()> {
        let provider_session_id = session.provider_session_id.clone().ok_or_else(|| {
            Error::Agent(format!(
                "{:?} session `{}` has no persisted provider id to resume",
                session.provider, session.id
            ))
        })?;
        let main_thread_id = self.store.main_thread_id(self.id).await?;
        // The store's current message count is the next `seq` to mint: an
        // adapter-backed session lands every message on its main thread with
        // seqs minted densely from 0, so `message_count == MAX(seq) + 1`.
        // Continuing from here is what keeps resumed history from being
        // renumbered or duplicated.
        let seed_seq = self.store.message_count(self.id).await? as i64;
        let (adapter, handle) = self
            .connect_adapter_agent(
                factory,
                self.id,
                AdapterBind::Resume {
                    provider_session_id,
                },
                &session.cwd,
                // `repo_root` is written by [`Self::spawn_adapter_session`] only
                // on the worktree arm, so for an adapter-backed session a
                // non-NULL value *is* "this cwd is a worktree Delta cut from
                // that repository".
                session.repo_root.clone(),
            )
            .await?;
        let git_branch = self.observe_launch_branch(self.id, &session.cwd).await;
        self.install_agent_binding(
            adapter,
            handle,
            session.cwd.clone(),
            git_branch,
            main_thread_id,
            seed_seq,
        );
        Ok(())
    }
}
