//! Holds the injected capabilities and implements Delta's use cases.

use std::collections::HashMap;
use std::sync::Arc;

use delta_model::{AgentProvider, SessionId};

use crate::agent::{AgentAdapterFactory, LaunchOptionVocabulary};
use crate::launch_config::LaunchConfig;
use crate::pane_token_minter::PaneTokenMinter;
use crate::ports::{
    AsyncEventSink, BinaryDetector, ExternalOpener, GhCli, GitWorktree, SessionEvent, SessionStore,
    TmuxDriver, Transcript, Workspace,
};
use crate::pull_request_lens::PullRequestLens;

use super::repository::CloneJobs;
use super::PrSearchCacheEntry;

#[cfg(doc)]
use super::{Interactor, PR_SEARCH_CACHE_TTL};
#[cfg(doc)]
use crate::agent::NullLaunchOptionVocabulary;

/// Holds the injected capabilities and implements Delta's use cases.
///
/// Generic over the five ports so callers can inject any implementation. This
/// is the shared core behind [`Interactor`]: the interactor (and any
/// background task it spawns) holds it through an [`Arc`], so the core itself
/// carries no registry of those tasks — only the capabilities and the
/// process-runtime state.
pub struct InteractorCore<T, X, S, W, G> {
    pub(in crate::interactor) tmux: T,
    pub(in crate::interactor) transcript: X,
    pub(in crate::interactor) store: S,
    pub(in crate::interactor) workspace: W,
    pub(in crate::interactor) git_worktree: G,
    /// Base directory for per-spawn working directories.
    ///
    /// Each fresh spawn runs in its own `<base>/<token>` subdirectory. The
    /// workdir is no longer the hook-binding key — correlation is by the
    /// Delta-minted session id pinned via `claude --session-id` — so this base
    /// is free to become a user-selected project path in a later change without
    /// breaking spawn↔session correlation.
    pub(in crate::interactor) session_workdir_base: String,
    /// Base directory for per-session git worktrees
    /// (`<base>/<org>-<repo>-<session-id>`, where `<org>-<repo>` is the
    /// repository-identity slug).
    ///
    /// Kept separate from [`Self::session_workdir_base`] and pointed at a
    /// neutral location *outside* any repository tree: Claude Code discovers
    /// `CLAUDE.md`/settings by walking up from its cwd, so a worktree nested
    /// under another repo would inherit that repo's config. Used only for
    /// worktree sessions; default per-token spawns still use
    /// [`Self::session_workdir_base`].
    pub(in crate::interactor) worktree_base: String,
    /// The directory a hook-reported `transcript_path` must resolve under to be
    /// registered (and therefore later read and surfaced to the browser).
    ///
    /// `Some(root)` turns on confinement: `register_session_row` refuses
    /// anything that is not a `.jsonl` file resolving under this root, so a
    /// forged (or first, unvalidated) hook cannot point Delta's transcript
    /// tailer at an arbitrary file. The path itself need not exist yet — Claude
    /// Code creates the per-project directory only on its first transcript
    /// write, after the `SessionStart` hook — so it is this root and the
    /// deepest *existing* ancestor of the reported path that get canonicalized
    /// (see `hooks::validate_transcript_path`). Production always wires it (from
    /// `Config::transcript_root`); `None` — the default constructor used by the
    /// domain tests, which register sessions with throwaway temp paths — skips
    /// the check, exactly as the other unwired capabilities default to a
    /// permissive stub. See [`Interactor::with_transcript_root`].
    pub(in crate::interactor) transcript_root: Option<String>,
    /// The Claude Code settings JSON whose hooks point back at this server (and
    /// which pins the session theme). Rendered by the caller (with the running
    /// port) and held verbatim; written to [`Self::session_settings_path`] and
    /// passed to `claude --settings`.
    pub(in crate::interactor) session_settings_json: String,
    /// Delta-owned path the settings JSON is written to before each launch, then
    /// passed to `claude --settings <path>`. Kept *outside* any session working
    /// directory so spawning/resuming in a real project never overwrites that
    /// project's own `.claude/settings.json`.
    pub(in crate::interactor) session_settings_path: String,
    /// How sessions are launched (which binary) and how long the watchdog
    /// waits on a launch. Production defaults via [`LaunchConfig::default`];
    /// overridden through [`Interactor::with_launch_config`].
    pub(in crate::interactor) launch: LaunchConfig,
    /// Mints unique [`PaneToken`]s for fresh spawns.
    ///
    /// [`PaneToken`]: crate::pane_token::PaneToken
    pub(in crate::interactor) minter: PaneTokenMinter,
    /// Per-process cache of `repo_root -> origin URL` (the `Option` is stored
    /// faithfully so a missing origin is memoised too). An origin URL is a
    /// property of the on-disk repo and effectively never changes for the
    /// server's lifetime, so the cost of shelling out to `git config` for it
    /// is paid once per root. Tokio mutex because the lookup happens inside
    /// async code and may briefly hold the lock across an await point on a
    /// fresh miss.
    pub(in crate::interactor) repository_origin_cache:
        tokio::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    /// The `gh` CLI driver. Held as a trait object (not generic) because the
    /// PR tab is the only consumer and it does not flow through the session
    /// actors — keeping it non-generic avoids threading a sixth type
    /// parameter through every interactor impl block.
    pub(in crate::interactor) gh_cli: Arc<dyn GhCli>,
    /// The external-tool opener used by `open cwd` (currently only VS Code
    /// via `code <path>`). Held as a trait object for the same reason as
    /// [`Self::gh_cli`]: it is not routed through the session actors, so a
    /// non-generic field keeps the interactor's five type parameters
    /// untouched.
    pub(in crate::interactor) external_opener: Arc<dyn ExternalOpener>,
    /// The registry of adapter factories, keyed by the provider each one
    /// drives ([`AgentAdapterFactory::provider`]). Held as trait objects for
    /// the same reason as [`Self::gh_cli`] — they are not routed through the
    /// session actors, so a non-generic field keeps the interactor's five type
    /// parameters untouched.
    ///
    /// Factories (rather than live adapters) are held because standing an
    /// adapter up may spawn a process — Codex spawns `codex app-server` and
    /// runs its `initialize` handshake; doing that at startup would break a
    /// machine without the provider installed. A factory carries only launch
    /// configuration, so construction has no side effects and the spawn is
    /// deferred to [`AgentAdapterFactory::connect`].
    ///
    /// A provider with a registered factory whose declared launch capability
    /// is adapter-backed dispatches its sessions over the adapter (see
    /// [`InteractorCore::adapter_backed_factory`]); every other provider takes
    /// the native PTY path. Empty when no factory has been wired (the default
    /// constructor, tests).
    pub(in crate::interactor) adapter_factories:
        HashMap<AgentProvider, Arc<dyn AgentAdapterFactory>>,
    /// How a launch option reads in its agent's vocabulary — whether it switches
    /// the agent's own safety mechanisms off, and whether its name takes one
    /// value or several (see [`LaunchOptionVocabulary`]).
    ///
    /// Held as one trait object rather than per-provider, and not on the adapter
    /// factories, because Claude registers no factory at all while being the
    /// provider with the loudest dangerous flag. Defaults to
    /// [`NullLaunchOptionVocabulary`]; production wiring installs the
    /// gateway-backed vocabulary through
    /// [`Interactor::with_launch_option_vocabulary`].
    pub(in crate::interactor) launch_option_vocabulary: Arc<dyn LaunchOptionVocabulary>,
    /// Resolves whether a provider's launch binary is present on this host, for
    /// the `/api/providers` availability endpoint. Held as a trait object for
    /// the same reason as [`Self::gh_cli`] — it is not routed through the
    /// session actors, so a non-generic field keeps the interactor's five type
    /// parameters untouched. The default constructor wires a stub reporting
    /// every binary as absent; production wiring installs the real PATH probe.
    pub(in crate::interactor) binary_detector: Arc<dyn BinaryDetector>,
    /// The Codex launch binary this server would spawn (`codex` by default,
    /// overridden by `DELTA_CODEX_BIN`). Stored so the availability endpoint
    /// probes the *same* binary a Codex spawn would use, rather than a divergent
    /// hardcoded path. Sourced from the same value handed to the Codex adapter
    /// factory at the composition root. The Claude launch binary is not
    /// duplicated here — it already lives on [`Self::launch`] as `claude_bin`.
    pub(in crate::interactor) codex_bin: String,
    /// The async event-emission seam: the sending half a producer that emits
    /// events *after* its driving call returned pushes on (see
    /// [`AsyncEventSink`]). The server owns the matching receiver and forwards
    /// each drained event to its broadcast.
    ///
    /// `None` by default — the synchronous return path (every hook handler and
    /// tick returning its `Vec<SessionEvent>`) is untouched, and a
    /// configuration that never wires the seam (the default constructor, the
    /// domain tests) simply drops any async emit. Production wiring installs the
    /// sink through [`Interactor::with_event_sink`]; the live emitters are
    /// listed on [`Self::emit_async_event`].
    pub(in crate::interactor) event_sink: Option<AsyncEventSink>,
    /// Per-lens memo of the latest PR search result, keeping a focus
    /// flip between the two lenses cheap. Bounded by
    /// [`PR_SEARCH_CACHE_TTL`] so the picker still picks up newly-opened
    /// PRs on the user's timescale.
    pub(in crate::interactor) pr_search_cache:
        tokio::sync::Mutex<std::collections::HashMap<PullRequestLens, PrSearchCacheEntry>>,
    /// request-row id → owning session, so a permission decision (which only
    /// carries the request id) can be routed to the right actor. Entries are
    /// claimed atomically by `decide_permission`/`abandon_permission_decision`,
    /// mirroring the waiter lifecycle inside the actor.
    ///
    /// Held on the core (not the outer [`Interactor`]) so both the routing layer
    /// and a session actor can reach it: the Claude path seeds it from the
    /// routing layer after the `PermissionRequest` hook returns, while the Codex
    /// event pump — which allocates the permission row inside the actor — seeds
    /// it there through the actor's [`Deref`](std::ops::Deref) to the core.
    pub(in crate::interactor) permission_index: std::sync::Mutex<HashMap<i64, SessionId>>,
    /// The repository clones in flight in this process, keyed by destination
    /// path, so a second request for a destination already being cloned joins
    /// the running job instead of starting a second `gh` process on the same
    /// directory.
    ///
    /// Deliberately in-memory, and deliberately unrelated to sessions: no
    /// session path reads it, so a clone in flight can never delay a spawn, and
    /// a restart simply forgets the job (see the `clone_repository` module for
    /// what that costs and how the leftovers are cleaned up).
    pub(in crate::interactor) clone_jobs: CloneJobs,
    /// Test seam: how many background launch preparations are still running.
    ///
    /// A new-session send is answered before its launch has done anything, and
    /// the launch's progress is not legible from session state alone — the
    /// pending spawn is recorded partway through (just before the pane is
    /// created), and the launch's first hook may consume it before the task even
    /// reports back. So a test that asserts on what the launch *did* waits on
    /// this counter instead: incremented as each task is spawned, decremented
    /// once it has posted its `LaunchFinished`. See `Interactor::await_launch`.
    #[cfg(test)]
    pub(in crate::interactor) launches_in_flight: std::sync::atomic::AtomicUsize,
}

impl<T, X, S, W, G> InteractorCore<T, X, S, W, G>
where
    T: TmuxDriver,
    X: Transcript,
    S: SessionStore,
    W: Workspace,
    G: GitWorktree,
{
    /// How long the `PermissionRequest` hook response may block waiting for a
    /// browser decision before falling back to the TUI prompt.
    pub fn permission_decision_deadline(&self) -> std::time::Duration {
        self.launch.permission_decision_deadline
    }

    /// The registered adapter factory for `provider`, when that provider's
    /// sessions are adapter-backed — i.e. driven through an
    /// [`crate::agent::AgentAdapter`] binding (terminal-less) rather than the
    /// native PTY path.
    ///
    /// This is the single provider-dispatch predicate the session paths use:
    /// `Some` iff a factory is registered for the provider *and* its declared
    /// launch capability is adapter-backed (see
    /// [`crate::agent::LaunchCapability::is_adapter_backed`]). `None` sends
    /// the session down the native pane path (Claude, which never registers a
    /// factory) — so a new adapter-backed provider is dispatched correctly by
    /// registering its factory, with no new provider-identity `match` arm in
    /// the session paths.
    pub(in crate::interactor) fn adapter_backed_factory(
        &self,
        provider: AgentProvider,
    ) -> Option<Arc<dyn AgentAdapterFactory>> {
        self.adapter_factories
            .get(&provider)
            .filter(|factory| factory.capabilities().is_adapter_backed())
            .cloned()
    }

    /// Emit an event onto the async event seam, if one is wired.
    ///
    /// The complement of the synchronous return path: where a hook handler or
    /// tick hands its `Vec<SessionEvent>` back to a caller that broadcasts them,
    /// this pushes a single event onto the [`AsyncEventSink`] the server drains
    /// — for a producer that emits after its driving call has already returned.
    /// A no-op when no sink is wired (the default), so it is always safe to
    /// call. Live today: the Codex event pump, a deferred launch's outcome, the
    /// background tail's per-session ingest announcements, and the queue's
    /// out-of-request notices (a flush promoting a queued send, the
    /// echo-deadline sweep parking one). A background clone job is the one
    /// async producer that skips this helper: it holds its own clone of the
    /// sink.
    pub fn emit_async_event(&self, event: SessionEvent) {
        if let Some(sink) = &self.event_sink {
            sink.emit(event);
        }
    }
}

#[cfg(test)]
mod interactor_tests {
    use delta_model::AgentProvider;

    use crate::interactor::testing::{interactor, FakeAgentFactory};

    /// The provider-dispatch predicate resolves through the factory registry:
    /// a provider with a registered, adapter-backed factory gets it back; a
    /// provider with no factory (Claude — the native pane path) gets `None`.
    #[test]
    fn adapter_backed_factory_resolves_only_registered_providers() {
        let ix = interactor().with_adapter_factory(FakeAgentFactory::new("thr_1", None));

        let codex = ix.adapter_backed_factory(AgentProvider::Codex);
        assert!(
            codex.is_some_and(|f| f.provider() == AgentProvider::Codex),
            "the registered Codex factory is resolved by its provider key"
        );
        assert!(
            ix.adapter_backed_factory(AgentProvider::Claude).is_none(),
            "Claude registers no factory, so it stays on the native pane path"
        );
    }

    /// An unwired configuration (the default constructor) reports every
    /// provider as not adapter-backed, so no session is sent down an adapter
    /// path that cannot connect.
    #[test]
    fn adapter_backed_factory_is_none_when_nothing_is_wired() {
        let ix = interactor();
        assert!(ix.adapter_backed_factory(AgentProvider::Codex).is_none());
        assert!(ix.adapter_backed_factory(AgentProvider::Claude).is_none());
    }
}
