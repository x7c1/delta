//! The connect → (`launch`|`resume`) step: standing the provider's adapter up
//! and obtaining the session's provider handle, without touching runtime state.

use std::sync::Arc;

use delta_model::SessionId;

use crate::agent::{
    AgentAdapter, AgentAdapterFactory, AgentSessionHandle, LaunchOptionSpec, LaunchRequest,
    ResumeRequest,
};
use crate::error::Result;
use crate::interactor::InteractorCore;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

/// How to obtain the provider handle when standing up an agent binding: a
/// fresh `thread/start` (launch) or a `thread/resume` reattach to an existing
/// provider thread. The single difference the shared
/// [`InteractorCore::connect_adapter_agent`] branches on.
pub(in crate::interactor) enum AdapterBind {
    /// A fresh spawn: start a new provider thread (`adapter.launch`), carrying
    /// the launch options the user selected for it. Only a fresh thread takes
    /// them — see [`SessionContext::resume_adapter_agent`] for why a resume
    /// does not.
    ///
    /// [`SessionContext::resume_adapter_agent`]: crate::interactor::session_actor::actor::SessionContext::resume_adapter_agent
    Launch {
        launch_options: Vec<LaunchOptionSpec>,
    },
    /// A resume: reattach to the persisted provider thread (`adapter.resume`),
    /// so no new thread is minted.
    Resume { provider_session_id: String },
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Connect the provider's adapter and obtain the session's provider handle
    /// — the shared connect → (`launch`|`resume`) step used by BOTH a fresh
    /// spawn (on its launch task) and a resume (inline, on the actor).
    ///
    /// It deliberately touches **no runtime state**: standing the connection up
    /// is the slow part (Codex: spawning `codex app-server`, its handshake, then
    /// `thread/start`), so it must be callable off the actor without holding the
    /// mailbox. What to do with the result — bind it, accumulate content, pump
    /// events — is [`SessionContext::install_agent_binding`]'s job.
    ///
    /// The only two things that differ between the callers are passed in:
    /// `bind` selects the provider handle ([`AdapterBind::Launch`] starts a new
    /// thread, [`AdapterBind::Resume`] reattaches to the persisted one), and
    /// `worktree_repo_root` says whether `cwd` is a worktree Delta created and
    /// out of which repository — the one thing about the launch directory a
    /// provider cannot work out for itself (see
    /// [`LaunchRequest::worktree_repo_root`]).
    ///
    /// It performs no rollback: a fresh spawn deletes its eager row on failure,
    /// while a resume leaves the already-persisted row untouched — so the caller
    /// owns that decision. On any error here the adapter is dropped before this
    /// returns, which tears the provider connection (and its process) down with
    /// it.
    ///
    /// [`SessionContext::install_agent_binding`]: super::install_agent_binding
    pub(in crate::interactor) async fn connect_adapter_agent(
        &self,
        factory: &Arc<dyn AgentAdapterFactory>,
        session_id: &SessionId,
        bind: AdapterBind,
        cwd: &str,
        worktree_repo_root: Option<String>,
    ) -> Result<(Arc<dyn AgentAdapter>, AgentSessionHandle)> {
        let adapter = factory.connect().await?;
        let handle = match bind {
            AdapterBind::Launch { launch_options } => {
                adapter
                    .launch(LaunchRequest {
                        session_id: session_id.as_str().to_owned(),
                        workdir: cwd.to_owned(),
                        // The adapter renders these for its provider. A first
                        // prompt is delivered as its own turn (not on launch) so
                        // the send row completes at the `turn/start`
                        // acknowledgement.
                        launch_options,
                        first_prompt: None,
                        worktree_repo_root,
                    })
                    .await?
            }
            AdapterBind::Resume {
                provider_session_id,
            } => {
                adapter
                    .resume(ResumeRequest {
                        session_id: session_id.as_str().to_owned(),
                        provider_session_id,
                        workdir: cwd.to_owned(),
                        worktree_repo_root,
                    })
                    .await?
            }
        };
        Ok((adapter, handle))
    }
}
