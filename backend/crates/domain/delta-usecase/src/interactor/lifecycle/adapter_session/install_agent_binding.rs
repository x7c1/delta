//! Holding a connected adapter as the session's live agent: the runtime
//! mutations a fresh bind and a resume both end in.

use std::sync::Arc;

use delta_model::ThreadId;

use crate::agent::{AgentAdapter, AgentSessionHandle, ContentSourceRequest};
use crate::interactor::session_actor::actor::SessionContext;
use crate::interactor::session_actor::runtime::OpenAgentSession;
use crate::ports::{GitWorktree, SessionStore, TmuxDriver, Transcript, Workspace};

impl<T, X, S, W, G> SessionContext<'_, T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// Hold a freshly-connected adapter as this session's open agent: bind it,
    /// install the content accumulator, and start the event pump.
    ///
    /// The state-mutating half of standing an adapter-backed session up, split
    /// from the connecting half ([`InteractorCore::connect_adapter_agent`])
    /// because on a fresh spawn the two run in different places: the connect
    /// happens on the launch task (it is slow, and must not block the mailbox),
    /// while everything here touches [`SessionRuntime`] or needs the actor's own
    /// mailbox handle and so must run on the actor. A resume runs both back to
    /// back, on the actor, because it is already inside a send.
    ///
    /// `cwd` is the session's launch directory as Delta resolved and recorded
    /// it, and `git_branch` the branch observed in it, both handed to the
    /// content accumulator so every message reports where the agent is running;
    /// the model, which only the provider knows, is the adapter's to add.
    ///
    /// `seed_seq` is where the accumulator begins numbering — `0` for a fresh
    /// session, the session's persisted `MAX(seq) + 1` on a resume so
    /// replayed/continued frames extend the existing history instead of
    /// renumbering or duplicating it.
    ///
    /// Holding the adapter in the runtime is what keeps the connection up and
    /// makes the session read as open, with no `OpenHandle` for the PTY bridge
    /// to attach to. `events()` is taken after `bind_agent`, so the buffered
    /// opener (`SessionStarted`) and the first frames are all captured.
    ///
    /// [`InteractorCore::connect_adapter_agent`]: crate::interactor::InteractorCore::connect_adapter_agent
    /// [`SessionRuntime`]: crate::interactor::session_actor::runtime::SessionRuntime
    pub(super) fn install_agent_binding(
        &mut self,
        adapter: Arc<dyn AgentAdapter>,
        handle: AgentSessionHandle,
        cwd: String,
        git_branch: Option<String>,
        main_thread_id: ThreadId,
        seed_seq: i64,
    ) {
        // Represent the running session as open-without-pane: hold the live
        // adapter + handle so the connection stays up and the session reads as
        // open, with no `OpenHandle` (so the PTY bridge has nothing to attach).
        self.state.bind_agent(OpenAgentSession {
            adapter: adapter.clone(),
            handle: handle.clone(),
        });

        // Build the push-based content accumulator: seeded so minted ordering
        // continues past whatever is already persisted, and carrying the launch
        // site so every message it folds reports where the agent is running. The
        // adapter joins this with the fact only it knows (Codex: the model the
        // server resolved, read off the thread's opening response), which is why
        // it is handed the live handle too.
        self.state.set_agent_content_source(adapter.content_source(
            &handle,
            ContentSourceRequest {
                session_id: self.id.clone(),
                main_thread: main_thread_id,
                seed_seq,
                cwd,
                git_branch,
            },
        ));

        // Spawn the event pump. Adapter frames arrive after the send that
        // started the work has already returned to the browser — exactly why
        // they reach the browser through the async seam rather than a
        // synchronous return.
        crate::interactor::agent_event::spawn_agent_event_pump(
            self.self_sender.clone(),
            adapter.events(&handle),
        );
    }
}
