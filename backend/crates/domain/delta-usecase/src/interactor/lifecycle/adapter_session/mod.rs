//! Terminal-less sessions for an adapter-backed provider (currently Codex):
//! accepting one, standing its provider connection up, binding it, and running
//! its turns.
//!
//! The adapter-backed counterpart of [`spawn_fresh`](super::spawn_fresh): where
//! a Claude spawn mints a tmux pane, launches `claude`, and waits for the first
//! `UserPromptSubmit` hook to bind it, an adapter-backed session is created
//! entirely over the provider's adapter connection (Codex: `codex app-server`)
//! — there is no pane, no hook, and no transcript file. This is the
//! composition-layer half of provider dispatch; the actor's `SpawnFresh`
//! handler routes every non-Claude provider here and keeps the Claude path
//! byte-for-byte unchanged. Which adapter drives the session is resolved
//! through the factory registry
//! ([`InteractorCore::adapter_backed_factory`](crate::interactor::InteractorCore::adapter_backed_factory)),
//! so a new adapter-backed provider is a new registered factory, not a new
//! spawn path.
//!
//! Like the Claude spawn, an adapter-backed one splits into an *accept* phase
//! ([`spawn_adapter_session`]: the request validates, plans the launch
//! directory, writes the eager rows and replies) and a background *launch*
//! phase ([`adapter_launch`](super::adapter_launch): the worktree build,
//! `connect`, `thread/start`, and the bind that finishes the session off on the
//! actor). Both providers ride the same launch shell — one deadline, one
//! in-flight count, one `LaunchFinished` rollback (see
//! [`launch_prep`](super::launch_prep)) — so a Codex session started from a PR
//! answers as fast as a Claude one and its failures arrive as a
//! [`SessionEvent::SpawnFailed`](crate::ports::SessionEvent::SpawnFailed) the
//! browser can retry, not as a `5xx`.
//!
//! One file per element: the accept phase (`spawn_adapter_session`), the bind
//! that finishes it off on the actor (`activate_adapter_session`) and the
//! runtime binding it installs (`install_agent_binding`), the resume that
//! reattaches a closed session (`resume_adapter_agent`), the connect step both
//! of those share (`connect_adapter_agent`) with the branch observation that
//! accompanies it (`observe_launch_branch`), and the turn dispatch every send
//! goes through (`agent_turn`).

mod activate_adapter_session;

mod agent_turn;

mod connect_adapter_agent;
pub(in crate::interactor) use connect_adapter_agent::AdapterBind;

mod install_agent_binding;

mod observe_launch_branch;

mod resume_adapter_agent;

mod spawn_adapter_session;
