//! [`CodexAppServerAdapter`]: the [`AgentAdapter`] for OpenAI Codex, driven over
//! a shared `codex app-server` connection.
//!
//! Where the Claude adapter reconstructs the neutral event stream from lossy
//! hooks + a JSONL transcript, Codex pushes structured `turn/*` / `item/*`
//! notifications and `*/requestApproval` server → client requests over the
//! JSON-RPC connection. This adapter maps the neutral operations onto the
//! app-server methods and translates the pushed frames back into
//! [`AgentEvent`]s (translation lives in [`crate::translate`]).
//!
//! ## Model
//!
//! One adapter owns one [`AppServerConnection`] (the shared server) and hosts
//! many sessions, each a Codex thread (session ↔ thread is 1:1). The connection
//! must already have completed its `initialize` handshake before the adapter is
//! constructed — the composition root does that once when it stands the shared
//! server up.
//!
//! - `launch` → `thread/start` (the server mints the thread id, which becomes
//!   both the provider session id and the handle key);
//! - `resume` → `thread/resume`;
//! - `send` → `turn/start`;
//! - `interrupt` → `turn/interrupt`;
//! - `close` → drop the session's local plumbing and emit `SessionEnded` (a
//!   real per-thread close RPC is not modelled in v1; the shared connection
//!   stays up for its other threads).
//!
//! ## Session death
//!
//! A session can also end without Delta asking. When the backing
//! `codex app-server` process exits (or its pipe breaks) the transport
//! announces [`ThreadEvent::ConnectionLost`] on every thread's channel, and the
//! translation task turns it into `SessionEnded { reason: ProcessExited }` — the
//! terminal event the core settles on (stuck turn, pending approvals, open
//! state). The reason is what tells the two apart: `Closed` is the orderly
//! teardown `close` emits, `ProcessExited` is a loss. Only one of them is ever
//! emitted for a session, because the orderly path removes the session (aborting
//! its translation task) while the death path returns from that task.
//!
//! ## Permission handling
//!
//! The two approval server-requests whose response is a binary decision —
//! `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`
//! (classified in [`crate::translate`]) — surface as
//! [`AgentEvent::PermissionRequested`]; the adapter remembers the verbatim wire
//! id so [`AgentAdapter::resolve_permission`] can answer it with the decision
//! mapping (allow → `accept`, session-scoped allow → `acceptForSession`, deny →
//! `decline`) and emit [`AgentEvent::PermissionResolved`]. Both response types
//! share the same `{ "decision": … }` shape
//! (`CommandExecutionRequestApprovalResponse` and
//! `FileChangeRequestApprovalResponse` in the vendored schema), so one reply path
//! serves both approval kinds and the session-scoped value needs no second path.
//! The remaining decision variants — execpolicy and network-policy amendments,
//! and `cancel` — are still unused: each would need a neutral projection Delta
//! does not have. That trait method — reachable through
//! `Arc<dyn AgentAdapter>` — is the Codex analogue of the Claude adapter's
//! ingestion seam: it is how a browser decision reaches the provider, routed by
//! the core from the Delta permission-row id back to the adapter-scoped token.
//!
//! `item/permissions/requestApproval` is deliberately **not** an approval here:
//! its response is a `GrantedPermissionProfile`, not a decision Delta can
//! synthesise, so it takes the never-hang path below (surfaced as
//! [`AgentEvent::UnsupportedInteraction`] and answered) rather than being
//! answered with a fabricated grant.
//!
//! ## The worktree git-directory grant
//!
//! Codex's `workspace-write` sandbox makes the thread's `cwd` writable but keeps
//! the `.git` directory at a writable root's top level read-only, so even in a
//! plain clone a `git add` escalates for approval. In a **linked worktree** —
//! which is what Delta launches a worktree session in — the situation is worse:
//! `<cwd>/.git` is only a pointer file, and git's real writes land in the source
//! repository's `.git`, outside `cwd` and outside any cwd-relative writable root
//! a user could configure. Every `git add` / `git commit` in such a session then
//! raises an approval prompt.
//!
//! Delta creates those worktrees, so it closes the gap itself: when the launch
//! (or resume) request names a [`LaunchRequest::worktree_repo_root`], the
//! adapter adds that repository's `.git` to the request's writable roots — to
//! the list the (merged) `config` already states, under whichever spelling it
//! states it, or on the [`worktree_git_grant::WRITABLE_ROOTS_KEY`] config key
//! when it states none (see [`apply_worktree_git_grant`]). Nothing is registered
//! or persisted — the grant is derived from the request on every start/resume.
//!
//! Two things about the real server are answered by the
//! `real_thread_start_honors_the_worktree_git_grant` canary rather than assumed:
//!
//! - the **dotted** config key is accepted on this path and applies at the leaf,
//!   exactly as the CLI's `-c` flag does (`codex-cli 0.153.4`). This one the
//!   canary *asserts*, so a version that stops honouring the spelling turns it
//!   red;
//! - a leaf override **replaces** the user's global `writable_roots` list for
//!   the thread rather than unioning with it — observed, not asserted: the
//!   canary prints the ungranted thread's roots next to the granted one's, so
//!   the trade-off is re-read on every run instead of being taken on trust, but
//!   a version that started unioning would not fail it. That is accepted, and it
//!   is exactly why the grant **appends** to a list the user's own `config`
//!   states rather than standing aside from it: whatever that list holds is what
//!   the thread ends up with, so the only way the worktree's git directory can
//!   be in it is for Delta to put it there. (A relative `".git"`, the workaround
//!   users write for a normal clone, does not name the worktree's real git
//!   directory — and on this path 0.153.4 resolves a relative root against the
//!   Codex home rather than the cwd, so it names nothing useful either way.) If
//!   replacement of the *global* list ever proves harmful, a
//!   `config/read`-then-union is the way out.
//!
//! ## Never hang
//!
//! Any server → client request the adapter does not model is answered
//! immediately with a JSON-RPC error and surfaced as
//! [`AgentEvent::UnsupportedInteraction`], so an app-server session — which has
//! no interactive TUI fallback — can never block forever on an unhandled
//! request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use delta_usecase::{
    AgentCapabilities, AgentEvent, AgentProvider, AgentSessionHandle, CommsDirection, CommsEntry,
    CommsFrameKind, CommsLogSink, ContextInjectionCapability, Error as UsecaseError,
    EventCapability, ForkCapability, InterruptCapability, LaunchCapability, PermissionCapability,
    Result as UsecaseResult, ResumeCapability, SendReceipt, SessionIdentityCapability,
    SessionScopedAllowCapability, SteerCapability, TerminalCapability, TranscriptCapability,
};

use crate::file_change_items::FileChangeItems;
use crate::{AppServerConnection, StartedThread, ThreadEvent};

#[cfg(doc)]
use delta_usecase::{AgentAdapter, LaunchRequest};
#[cfg(doc)]
use worktree_git_grant::apply_worktree_git_grant;

/// Codex's static capability profile — the single source of truth returned by
/// [`AgentAdapter::capabilities`] and read (without a live adapter) by the
/// composition root's per-provider capability accessor. Declaring it once here,
/// in the adapter that owns Codex's behaviour, keeps the two in lockstep: they
/// cannot drift because both read this const.
///
/// Codex reality: a JSON-RPC app-server, a provider-assigned thread id, resume
/// via `thread/resume`, structured pushed turn/item events, a transcript that is
/// only the pushed stream, permission decisions answered over the wire —
/// including one scoped to the whole session — hidden
/// per-turn context via `thread/inject_items`, interrupt via `turn/interrupt`,
/// and no terminal to attach. Fork/steer are unused in v1.
pub const CODEX_CAPABILITIES: AgentCapabilities = AgentCapabilities {
    launch: LaunchCapability::JsonRpcAppServer,
    session_identity: SessionIdentityCapability::ProviderReturnsId,
    resume: ResumeCapability::Supported,
    events: EventCapability::StructuredTurnEvents,
    transcript: TranscriptCapability::EventStreamOnly,
    permission: PermissionCapability::AdapterDecision,
    // Both approval response schemas carry `acceptForSession` next to `accept`
    // and `decline`, so an allow can be widened to the whole thread — see
    // [`DECISION_ACCEPT_FOR_SESSION`].
    session_scoped_allow: SessionScopedAllowCapability::Supported,
    context_injection: ContextInjectionCapability::HiddenPerTurn,
    interrupt: InterruptCapability::Rpc,
    terminal: TerminalCapability::NoTerminal,
    fork: ForkCapability::None,
    steer: SteerCapability::None,
};

mod account_loop;
use account_loop::account_loop;
mod agent_adapter;
mod config_merge;
mod launch_option_catalog;
pub use launch_option_catalog::CODEX_LAUNCH_OPTION_CATALOG;
mod launch_option_cardinality;
pub use launch_option_cardinality::launch_option_cardinality;
mod launch_option_danger;
pub use launch_option_danger::is_dangerous_launch_option;
mod thread_params;
pub(crate) use thread_params::thread_resume_params;
pub use thread_params::thread_start_params;
use thread_params::thread_start_value;
mod translate_loop;
use translate_loop::translate_loop;
mod worktree_git_grant;

/// `thread/start` fields Delta fills in itself, which a user-registered launch
/// option must never overwrite.
///
/// `cwd` is the whole list today: Delta resolves the session's working
/// directory (the git worktree path, when the session was started with one) and
/// records the session's repo root, repository display name and
/// branch-at-launch against exactly that directory. Letting a launch option
/// override it would put the agent somewhere other than where those columns say
/// it is. A launch option naming one of these is rejected rather than dropped,
/// so the user sees why their option did not take effect.
const DELTA_OWNED_THREAD_FIELDS: &[&str] = &["cwd"];

/// The `thread/start` / `thread/resume` field carrying free-form Codex config
/// overrides — the one Delta merges its worktree git-directory grant into, and
/// the one every selected `config` launch option lands in (see
/// [`config_merge::merge_config`], which is why it is the one field a launch may
/// select more than once).
const CONFIG_FIELD: &str = "config";

/// The Codex decision wire value for an allow.
const DECISION_ACCEPT: &str = "accept";
/// The Codex decision wire value for an allow that stands for the rest of the
/// session: the provider stops asking for comparable requests on this thread.
///
/// The grant lives entirely in the provider's session — Delta sends this value
/// and holds nothing about it afterwards, so a restart or a resume simply asks
/// again. Carried by both approval response schemas
/// (`CommandExecutionRequestApprovalResponse` / `FileChangeRequestApprovalResponse`),
/// which is what lets the one reply path (`resolve_permission`) serve both
/// approval kinds.
const DECISION_ACCEPT_FOR_SESSION: &str = "acceptForSession";
/// The Codex decision wire value for a deny.
const DECISION_DECLINE: &str = "decline";
/// JSON-RPC "method not found", reused to reject an unmodeled server request.
const METHOD_NOT_FOUND: i64 = -32601;

/// Per-session plumbing: the event sender the adapter and its translation task
/// emit through, the receiver handed out once by [`AgentAdapter::events`], the
/// map of open approval requests awaiting a decision, and the provider facts
/// the opening response announced about the thread.
struct CodexSession {
    /// Delta's own conversation id for this session.
    ///
    /// The adapter addresses everything else by the provider thread id (which is
    /// the handle key), but the comms-log inspector is keyed by the id the
    /// browser knows — and this adapter is the only layer holding both, so it
    /// remembers Delta's id here to scope the frames it records.
    session_id: String,
    tx: UnboundedSender<AgentEvent>,
    rx: Option<UnboundedReceiver<AgentEvent>>,
    /// The model the server **resolved** for this thread, read off the
    /// `thread/start` / `thread/resume` response (both carry it as a required
    /// top-level `model`).
    ///
    /// This — not anything Delta asked for — is the truth about what is running:
    /// the model can come from a selected launch option, from the user's own
    /// `~/.codex/config.toml` default, or from the server's built-in default,
    /// and only the response says which won. `None` if a server omits it, so the
    /// fact degrades rather than being invented.
    ///
    /// It is the *only* session fact Codex reports here. The response's `cwd` is
    /// just an echo of what Delta sent, and its `thread.gitInfo` is null (see
    /// [`resolved_model`]), so the launch site is Delta's own to supply.
    model: Option<String>,
    /// The state the adapter's request path and the session's translation task
    /// both reach into, as the pushed frames move it.
    shared: SharedSessionState,
    /// The translation task, aborted when the session is dropped so it never
    /// outlives the adapter.
    task: JoinHandle<()>,
}

/// The per-session state the adapter and its translation task **share**: the
/// task maintains it as frames arrive, and the adapter's own methods read it
/// when a caller asks something of the session.
///
/// Grouped rather than passed field by field because they travel together — one
/// clone hands the whole of it to the spawned task — and because they answer
/// the same question in three parts: what this thread currently has open.
#[derive(Clone, Default)]
struct SharedSessionState {
    /// Open approval requests: neutral `request_id` → the verbatim wire id the
    /// response must echo. The translation task inserts an entry when it
    /// surfaces a `PermissionRequested`; `resolve_permission` removes it when it
    /// answers.
    approvals: Arc<Mutex<HashMap<String, Value>>>,
    /// The id of the turn currently in flight on this thread, if any. Real
    /// `turn/interrupt` params are `{threadId, turnId}`, so the adapter must know
    /// which turn to interrupt. Captured from the `turn/start` response (and
    /// re-affirmed by the `turn/started` notification), and cleared on
    /// `turn/completed`.
    current_turn_id: Arc<Mutex<Option<String>>>,
    /// The file-change items seen on this thread but not yet finished, keyed by
    /// the Codex item id an approval request names.
    ///
    /// This is what turns a file-change approval — whose params carry no path,
    /// kind or diff — into a card the user can actually answer: the details
    /// arrived earlier on the item's `item/started`, and this map is the join
    /// between the two (see [`FileChangeItems`]).
    file_change_items: Arc<Mutex<FileChangeItems>>,
}

impl Drop for CodexSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Per-session channels, keyed by the provider thread id (which is also the
/// handle key). Shared with the account loop, which fans an account-scoped fact
/// out to every live session.
type SessionMap = Arc<Mutex<HashMap<String, CodexSession>>>;
/// The account loop's non-owning view of [`SessionMap`]: it must observe the
/// adapter's sessions without keeping them (and their translation tasks) alive.
type WeakSessionMap = Weak<Mutex<HashMap<String, CodexSession>>>;

/// The [`AgentAdapter`] for Codex over a shared `codex app-server` connection.
pub struct CodexAppServerAdapter {
    conn: Arc<AppServerConnection>,
    sessions: SessionMap,
    /// The account loop, aborted when the adapter is dropped so it never
    /// outlives the connection it drains.
    account_task: JoinHandle<()>,
}

impl Drop for CodexAppServerAdapter {
    fn drop(&mut self) {
        self.account_task.abort();
    }
}

impl CodexAppServerAdapter {
    /// Build the adapter over an already-initialised connection to the shared
    /// `codex app-server`, and start draining its account-scoped frames.
    ///
    /// Taking the unrouted channel here — at construction, once per connection —
    /// is what makes an account notification reach the browser at all: it names
    /// no thread, so the transport's demux cannot deliver it to a session, and
    /// nothing else reads that channel.
    pub fn new(conn: Arc<AppServerConnection>) -> Self {
        let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
        let account_task = tokio::spawn(account_loop(
            conn.take_unrouted(),
            Arc::clone(conn.comms_log()),
            // Weak, for the same reason the per-session translation task holds a
            // weak connection: the loop must not keep the adapter's sessions —
            // and through them the live translation tasks — alive after the
            // adapter is dropped.
            Arc::downgrade(&sessions),
        ));
        Self {
            conn,
            sessions,
            account_task,
        }
    }

    /// Register a fresh session for a started/resumed thread: open its event
    /// channel, record the model the opening response announced (see
    /// [`resolved_model`]), emit the opening [`AgentEvent::SessionStarted`], and
    /// spawn the translation task that drains the thread's frames onto the
    /// channel.
    fn register_session(&self, session_id: String, started: StartedThread) -> AgentSessionHandle {
        let StartedThread {
            thread_id,
            events,
            result,
        } = started;
        let model = resolved_model(&result);
        let (tx, rx) = mpsc::unbounded_channel();
        // Buffered before `events()` is called, so the opener is never dropped.
        let _ = tx.send(AgentEvent::SessionStarted {
            provider_session_id: thread_id.clone(),
        });
        let shared = SharedSessionState::default();
        let task = tokio::spawn(translate_loop(
            session_id.clone(),
            events,
            tx.clone(),
            // A weak reference: the task must not keep the connection alive, or
            // dropping the adapter would never close the thread channel and the
            // task would never exit.
            Arc::downgrade(&self.conn),
            Arc::clone(self.conn.comms_log()),
            shared.clone(),
        ));
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(
                thread_id.clone(),
                CodexSession {
                    session_id,
                    tx,
                    rx: Some(rx),
                    model,
                    shared,
                    task,
                },
            );
        AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: thread_id.clone(),
            key: thread_id,
        }
    }

    /// The model the server resolved for a still-known session's thread, as
    /// recorded by [`Self::register_session`]. `None` for an unknown or closed
    /// session, exactly as when the response carried no model.
    fn session_model(&self, key: &str) -> Option<String> {
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
            .and_then(|session| session.model.clone())
    }

    /// How many file-change items this session is currently correlating for its
    /// approval cards (see `FileChangeItems`). `0` for an unknown or closed
    /// session.
    ///
    /// The map is live display state with a lifecycle — entries go when the item
    /// completes, when the turn ends, and when the connection is lost — so this
    /// exists to make that lifecycle *observable* rather than taken on trust: a
    /// long session accumulating diffs is a leak, and the only way to assert it
    /// does not is to be able to count them.
    pub fn tracked_file_change_items(&self, key: &str) -> usize {
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
            .map(|session| {
                session
                    .shared
                    .file_change_items
                    .lock()
                    .expect("file change items mutex poisoned")
                    .len()
            })
            .unwrap_or(0)
    }

    /// The comms-log scope for a still-known session: Delta's conversation id,
    /// recorded when the session was registered. `None` for an unknown or closed
    /// session, in which case the frame is simply not recorded (there is no
    /// inspector left for it).
    fn scope_of(&self, key: &str) -> Option<String> {
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
            .map(|session| session.session_id.clone())
    }

    /// Emit an event on a session's channel, if the session is still known.
    fn emit(&self, key: &str, event: AgentEvent) {
        if let Some(session) = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
        {
            let _ = session.tx.send(event);
        }
    }

    /// Record (or clear) the id of the turn currently in flight for a session,
    /// if the session is still known. Called with the turn id when a turn starts
    /// and with `None` when it completes.
    fn set_current_turn_id(&self, key: &str, turn_id: Option<String>) {
        if let Some(session) = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
        {
            *session
                .shared
                .current_turn_id
                .lock()
                .expect("current turn id mutex poisoned") = turn_id;
        }
    }

    /// Start a turn for `handle` with the visible prompt `text`.
    ///
    /// `UserPromptAccepted` is emitted before the `turn/start` request is even
    /// issued, so it always precedes the turn's pushed notifications on the
    /// stream — including that turn's own `turn/started`, which is why the
    /// content fold cannot key the prompt off a turn id at acceptance time (see
    /// [`crate::content`]) and re-stamps its group once the turn names itself.
    /// The visible prompt rides `turn/start` as the reconciled `input` array
    /// (`[{ "type": "text", "text": … }]`, a single `TextUserInput`). The
    /// `turn/start` response carries the started `Turn` under `result.turn`, whose
    /// `id` becomes both the tracked turn id (which `turn/interrupt` references)
    /// and the send receipt's provider message id.
    ///
    /// Nothing here refuses a send while a turn is running: Codex accepts it and
    /// steers the input into the turn in flight, answering with a fresh turn id
    /// that never appears as a `turn/started`. One turn can therefore carry
    /// several accepted prompts.
    async fn start_turn(
        &self,
        handle: &AgentSessionHandle,
        text: String,
    ) -> UsecaseResult<SendReceipt> {
        self.emit(
            &handle.key,
            AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: text.clone(),
                // The prompt is accepted client-side here, before `turn/start`
                // is issued, so its message time is this send instant — the
                // Codex server exposes no separate accepted-at for it.
                at_ms: Some(Utc::now().timestamp_millis()),
            },
        );
        let params = json!({
            "threadId": handle.provider_session_id,
            "input": [{ "type": "text", "text": text }],
        });
        let result = self
            .conn
            .request(
                self.scope_of(&handle.key).as_deref(),
                "turn/start",
                Some(params),
            )
            .await
            .map_err(to_usecase_err)?;
        let provider_message_id = turn_id_of(&result);
        // Track the turn synchronously off its start response, so an interrupt
        // issued the instant `send` returns already knows which turn to end —
        // without racing the asynchronous `turn/started` notification.
        self.set_current_turn_id(&handle.key, provider_message_id.clone());
        Ok(SendReceipt {
            provider_message_id,
        })
    }
}

/// Build a Responses API user-message item carrying `text`, the shape
/// `thread/inject_items` appends to the thread's model-visible history (a
/// `MessageResponseItem` with a single `input_text` content item — see the
/// vendored `ResponseItem`/`ContentItem` schema).
fn inject_message_item(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "user",
        "content": [{ "type": "input_text", "text": text }],
    })
}

/// The model a `thread/start` / `thread/resume` response reports for the thread
/// it opened, read from the top-level `model` both responses carry (see the
/// vendored `ThreadStartResponse` / `ThreadResumeResponse` schemas, where it is
/// a required string alongside `cwd`, `sandbox` and `approvalPolicy`).
///
/// This is deliberately read from the **response** rather than echoed from the
/// request: `model` is only one of several inputs the server reconciles (a
/// selected launch option, the user's `config.toml` default, the server's own
/// default), so the request says what Delta asked for while the response says
/// what is actually running. A missing, null, non-string or empty value degrades
/// to `None` rather than being invented.
///
/// It is the only session fact worth reading here. The response's `thread` also
/// declares a `gitInfo` (`GitInfo | null`, documented as "captured when the
/// thread was created"), but the real server returns it as **null** on this
/// response — verified against `codex-cli 0.153.4`, and pinned by the
/// `real_thread_start_reports_the_metadata_delta_stamps_on_messages` canary. The
/// field's presence in the schema is not evidence that this response populates
/// it, so Delta observes the branch of its own launch directory instead of
/// waiting for one here.
fn resolved_model(result: &Value) -> Option<String> {
    result
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .map(str::to_owned)
}

/// The id of the turn a `turn/start` response announces, read from the `Turn`
/// object it carries under `result.turn` (see the vendored `TurnStartResponse`
/// schema: `{ turn: Turn }`, `Turn.id`).
fn turn_id_of(result: &Value) -> Option<String> {
    result
        .get("turn")
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Mirror one server → client frame into the comms log.
///
/// A notification and a server-originated request differ only in whether the
/// frame carries an id, which is exactly the [`CommsFrameKind`] distinction —
/// both arrive here on the same channel, so one function covers both.
///
/// [`ThreadEvent::ConnectionLost`] is deliberately not mirrored: the inspector
/// shows the JSON frames that crossed the wire, and a lost connection is the
/// absence of one. It reaches the browser as the session's terminal event (and
/// the settle that follows), not as a fabricated frame in the frame log.
fn record_inbound(comms: &Arc<dyn CommsLogSink>, session_id: &str, event: &ThreadEvent) {
    let entry = match event {
        ThreadEvent::ConnectionLost => return,
        ThreadEvent::Notification(notification) => CommsEntry::new(
            CommsDirection::FromAgent,
            CommsFrameKind::Notification,
            Some(&notification.method),
            notification.to_frame_json(),
        ),
        ThreadEvent::ServerRequest(request) => CommsEntry::new(
            CommsDirection::FromAgent,
            CommsFrameKind::Request,
            Some(&request.method),
            request.to_frame_json(),
        ),
    };
    comms.record(session_id, entry);
}

/// Map a transport error into the use-case error type at the trait boundary.
fn to_usecase_err(err: crate::Error) -> UsecaseError {
    UsecaseError::Agent(err.to_string())
}
