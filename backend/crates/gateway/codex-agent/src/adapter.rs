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
//! ## Permission handling
//!
//! The two approval server-requests whose response is a binary decision —
//! `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`
//! (classified in [`crate::translate`]) — surface as
//! [`AgentEvent::PermissionRequested`]; the adapter remembers the verbatim wire
//! id so [`AgentAdapter::resolve_permission`] can answer it with the frozen
//! decision mapping (allow → `accept`, deny → `decline`) and emit
//! [`AgentEvent::PermissionResolved`]. Both response types share the same
//! `{ "decision": … }` shape (`CommandExecutionRequestApprovalResponse` and
//! `FileChangeRequestApprovalResponse` in the vendored schema), so one reply path
//! serves both; v1 does not use the `acceptForSession` / execpolicy / network
//! amendment decision variants. That trait method — reachable through
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
//! ## Never hang
//!
//! Any server → client request the adapter does not model is answered
//! immediately with a JSON-RPC error and surfaced as
//! [`AgentEvent::UnsupportedInteraction`], so an app-server session — which has
//! no interactive TUI fallback — can never block forever on an unhandled
//! request.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Map, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use delta_usecase::{
    AgentAdapter, AgentCapabilities, AgentContentSource, AgentEvent, AgentEventStream,
    AgentProvider, AgentSessionHandle, ContentSourceRequest, ContextInjectionCapability,
    Error as UsecaseError, EventCapability, ForkCapability, InterruptCapability, LaunchCapability,
    LaunchOptionSpec, LaunchRequest, PermissionCapability, PermissionDecision, PtyHandle,
    Result as UsecaseResult, ResumeCapability, ResumeRequest, SendReceipt, SendRequest,
    SessionEndReason, SessionIdentityCapability, SteerCapability, TerminalCapability,
    TranscriptCapability,
};

use crate::translate::{
    classify_server_request, is_turn_completed, started_turn_id, translate_notification,
    ServerRequestKind,
};
use crate::{codex_content_source, AppServerConnection, StartedThread, ThreadEvent};

/// Codex's static capability profile — the single source of truth returned by
/// [`AgentAdapter::capabilities`] and read (without a live adapter) by the
/// composition root's per-provider capability accessor. Declaring it once here,
/// in the adapter that owns Codex's behaviour, keeps the two in lockstep: they
/// cannot drift because both read this const.
///
/// Codex reality: a JSON-RPC app-server, a provider-assigned thread id, resume
/// via `thread/resume`, structured pushed turn/item events, a transcript that is
/// only the pushed stream, permission decisions answered over the wire, hidden
/// per-turn context via `thread/inject_items`, interrupt via `turn/interrupt`,
/// and no terminal to attach. Fork/steer are unused in v1.
pub const CODEX_CAPABILITIES: AgentCapabilities = AgentCapabilities {
    launch: LaunchCapability::JsonRpcAppServer,
    session_identity: SessionIdentityCapability::ProviderReturnsId,
    resume: ResumeCapability::Supported,
    events: EventCapability::StructuredTurnEvents,
    transcript: TranscriptCapability::EventStreamOnly,
    permission: PermissionCapability::AdapterDecision,
    context_injection: ContextInjectionCapability::HiddenPerTurn,
    interrupt: InterruptCapability::Rpc,
    terminal: TerminalCapability::NoTerminal,
    fork: ForkCapability::None,
    steer: SteerCapability::None,
};

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

/// The Codex decision wire value for an allow.
const DECISION_ACCEPT: &str = "accept";
/// The Codex decision wire value for a deny.
const DECISION_DECLINE: &str = "decline";
/// JSON-RPC "method not found", reused to reject an unmodeled server request.
const METHOD_NOT_FOUND: i64 = -32601;

/// Per-session plumbing: the event sender the adapter and its translation task
/// emit through, the receiver handed out once by [`AgentAdapter::events`], the
/// map of open approval requests awaiting a decision, and the provider facts
/// the opening response announced about the thread.
struct CodexSession {
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
    /// Open approval requests: neutral `request_id` → the verbatim wire id the
    /// response must echo. Shared with the translation task, which inserts an
    /// entry when it surfaces a `PermissionRequested`.
    approvals: Arc<Mutex<HashMap<String, Value>>>,
    /// The id of the turn currently in flight on this thread, if any. Real
    /// `turn/interrupt` params are `{threadId, turnId}`, so the adapter must know
    /// which turn to interrupt. Captured from the `turn/start` response (and
    /// re-affirmed by the `turn/started` notification), and cleared on
    /// `turn/completed`. Shared with the translation task, which maintains it as
    /// the pushed `turn/*` frames arrive.
    current_turn_id: Arc<Mutex<Option<String>>>,
    /// The translation task, aborted when the session is dropped so it never
    /// outlives the adapter.
    task: JoinHandle<()>,
}

impl Drop for CodexSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The [`AgentAdapter`] for Codex over a shared `codex app-server` connection.
pub struct CodexAppServerAdapter {
    conn: Arc<AppServerConnection>,
    /// Per-session channels, keyed by the provider thread id (which is also the
    /// handle key).
    sessions: Mutex<HashMap<String, CodexSession>>,
}

impl CodexAppServerAdapter {
    /// Build the adapter over an already-initialised connection to the shared
    /// `codex app-server`.
    pub fn new(conn: Arc<AppServerConnection>) -> Self {
        Self {
            conn,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Register a fresh session for a started/resumed thread: open its event
    /// channel, record the model the opening response announced (see
    /// [`resolved_model`]), emit the opening [`AgentEvent::SessionStarted`], and
    /// spawn the translation task that drains the thread's frames onto the
    /// channel.
    fn register_session(&self, started: StartedThread) -> AgentSessionHandle {
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
        let approvals: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
        let current_turn_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let task = tokio::spawn(translate_loop(
            events,
            tx.clone(),
            // A weak reference: the task must not keep the connection alive, or
            // dropping the adapter would never close the thread channel and the
            // task would never exit.
            Arc::downgrade(&self.conn),
            Arc::clone(&approvals),
            Arc::clone(&current_turn_id),
        ));
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(
                thread_id.clone(),
                CodexSession {
                    tx,
                    rx: Some(rx),
                    model,
                    approvals,
                    current_turn_id,
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
                .current_turn_id
                .lock()
                .expect("current turn id mutex poisoned") = turn_id;
        }
    }

    /// Start a turn for `handle` with the visible prompt `text`.
    ///
    /// `UserPromptAccepted` is emitted before the `turn/start` request is even
    /// issued, so it always precedes the turn's pushed notifications on the
    /// stream. The visible prompt rides `turn/start` as the reconciled `input`
    /// array (`[{ "type": "text", "text": … }]`, a single `TextUserInput`). The
    /// `turn/start` response carries the started `Turn` under `result.turn`, whose
    /// `id` becomes both the tracked turn id (which `turn/interrupt` references)
    /// and the send receipt's provider message id.
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
            .request("turn/start", Some(params))
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

/// Build the `thread/start` params for a launch: Delta's own fields plus the
/// user's selected launch options, mapped one-to-one onto `ThreadStartParams`
/// fields.
///
/// A launch option is passed through **unvalidated**: its `name` is the field
/// name and its `value` is that field's value. `ThreadStartParams` is a moving
/// target (`model`, `sandbox`, `approvalPolicy`, `personality`, `serviceTier`,
/// `config`, …), so an allowlist here would mean a Delta release for every new
/// upstream field. The cost of the pass-through is that a misspelled key or a
/// bad value is not caught here — it comes back as an error from the codex
/// server, which is where the authority over that schema actually lives.
///
/// Two things are still rejected, loudly, because silently accepting them would
/// corrupt state Delta is responsible for:
///
/// - a key Delta sets itself ([`DELTA_OWNED_THREAD_FIELDS`]). `cwd` is
///   load-bearing: with a worktree it is the resolved worktree path, and the
///   session's repo-root / display-name / branch-at-launch columns are recorded
///   against it, so a user-registered `cwd` overriding it would break the
///   worktree contract while the recorded columns went on describing a
///   directory the agent is not in;
/// - the same key twice. Unlike a repeatable CLI flag, a JSON field can only be
///   set once, so a second option carrying the same name would silently discard
///   the first.
pub(crate) fn thread_start_params(
    workdir: &str,
    options: &[LaunchOptionSpec],
) -> UsecaseResult<Value> {
    let mut params = Map::new();
    params.insert("cwd".to_owned(), json!(workdir));
    for option in options {
        if DELTA_OWNED_THREAD_FIELDS.contains(&option.name.as_str()) {
            return Err(UsecaseError::LaunchOptionRejected(format!(
                "`{}` cannot be used with Codex: Delta sets that thread/start \
                 field itself for every session",
                option.name
            )));
        }
        if params.contains_key(&option.name) {
            return Err(UsecaseError::LaunchOptionRejected(format!(
                "`{}` is selected more than once: a thread/start field can only \
                 be set once",
                option.name
            )));
        }
        params.insert(
            option.name.clone(),
            thread_start_value(option.value.as_deref()),
        );
    }
    Ok(Value::Object(params))
}

/// Map a launch option's registry value onto its `thread/start` field value.
///
/// The registry stores values as text, but `ThreadStartParams` fields are not
/// all strings — `ephemeral` is a boolean, `config` is an object, and
/// `approvalPolicy` is either a string or an object. So:
///
/// - **no value** → JSON `true`. A valueless option reads as a bare boolean
///   field being switched on (`ephemeral`), the same way a valueless CLI flag
///   reads on the Claude side;
/// - **a value that parses as JSON** → that JSON value. `true`, `42`,
///   `{"granular":{…}}` and `["a","b"]` all reach the server with their real
///   types;
/// - **anything else** → the value as a JSON string. This is the common case:
///   `gpt-5.6-sol`, `read-only` and `on-request` are not valid JSON documents,
///   so they arrive as the strings they are.
///
/// The consequence of that ordering is that a string value which *happens* to
/// be valid JSON (`5`, `null`, `true`) becomes the parsed type. A user who
/// needs the literal string writes it quoted (`"5"`), which parses as the JSON
/// string `5`.
fn thread_start_value(value: Option<&str>) -> Value {
    match value {
        None => Value::Bool(true),
        Some(raw) => serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned())),
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
/// response — verified against `codex-cli 0.144.4`, and pinned by the
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

#[async_trait]
impl AgentAdapter for CodexAppServerAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        CODEX_CAPABILITIES
    }

    async fn launch(&self, req: LaunchRequest) -> UsecaseResult<AgentSessionHandle> {
        // The server assigns the thread id; Delta's own session id is not pinned
        // onto it (that is the `ProviderReturnsId` identity model). The workdir
        // rides `thread/start` as `cwd`, and the user's selected launch options
        // ride it as further fields (see [`thread_start_params`]).
        let params = thread_start_params(&req.workdir, &req.launch_options)?;
        let started = self
            .conn
            .start_thread(Some(params))
            .await
            .map_err(to_usecase_err)?;
        let handle = self.register_session(started);
        // A first prompt from the composer's opening Send starts a turn straight
        // away, mirroring how the Claude adapter auto-submits its launch prompt.
        if let Some(prompt) = req.first_prompt {
            self.start_turn(&handle, prompt).await?;
        }
        Ok(handle)
    }

    /// Reattach to an existing thread with `thread/resume`.
    ///
    /// No launch options ride this call. `ThreadResumeParams`' config fields
    /// (`model`, `sandbox`, `approvalPolicy`, `config`, …) are documented as
    /// *overrides* for the resumed thread, so omitting them keeps whatever
    /// `thread/start` configured — and the core has no per-session record of
    /// which options were selected to replay anyway (see
    /// `resume_adapter_agent` in the core for the full reasoning).
    async fn resume(&self, req: ResumeRequest) -> UsecaseResult<AgentSessionHandle> {
        let started = self
            .conn
            .resume_thread(Some(
                json!({ "threadId": req.provider_session_id, "cwd": req.workdir }),
            ))
            .await
            .map_err(to_usecase_err)?;
        Ok(self.register_session(started))
    }

    async fn send(
        &self,
        handle: &AgentSessionHandle,
        req: SendRequest,
    ) -> UsecaseResult<SendReceipt> {
        self.start_turn(handle, req.text).await
    }

    /// Inject hidden per-turn context via `thread/inject_items`: append one
    /// Responses API user-message item to the thread's model-visible history, so
    /// the next `turn/start` runs with the branched-from passage in context
    /// without it ever appearing in the visible prompt.
    ///
    /// `ThreadInjectItemsParams` (vendored schema) is `{ threadId, items }`,
    /// where `items` are raw Responses API items; a user message is a
    /// `MessageResponseItem` — `{ type: "message", role: "user", content: [{
    /// type: "input_text", text }] }` (see `ResponseItem` / `ContentItem` in the
    /// vendored v2 schema). The response (`ThreadInjectItemsResponse`) is an
    /// empty object, so nothing is read off it.
    async fn inject_context(&self, handle: &AgentSessionHandle, text: &str) -> UsecaseResult<()> {
        let params = json!({
            "threadId": handle.provider_session_id,
            "items": [inject_message_item(text)],
        });
        self.conn
            .request("thread/inject_items", Some(params))
            .await
            .map_err(to_usecase_err)?;
        Ok(())
    }

    /// Interrupt the turn currently in flight on this thread.
    ///
    /// Real `turn/interrupt` params are `{threadId, turnId}` (both required), so
    /// the adapter sends the turn id it is tracking for this session. When no
    /// turn is in flight (nothing tracked) there is nothing to interrupt, so this
    /// is a no-op success rather than an RPC the server would reject for a missing
    /// turn id. The turn is ended by the resulting `turn/completed{interrupted}`
    /// flowing back through the same translation path as any other completion.
    async fn interrupt(&self, handle: &AgentSessionHandle) -> UsecaseResult<()> {
        let turn_id = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            sessions.get(&handle.key).and_then(|session| {
                session
                    .current_turn_id
                    .lock()
                    .expect("current turn id mutex poisoned")
                    .clone()
            })
        };
        let Some(turn_id) = turn_id else {
            // No turn in flight: nothing to interrupt.
            return Ok(());
        };
        self.conn
            .request(
                "turn/interrupt",
                Some(json!({
                    "threadId": handle.provider_session_id,
                    "turnId": turn_id,
                })),
            )
            .await
            .map_err(to_usecase_err)?;
        Ok(())
    }

    /// Answer an open approval request with a decision, mapping it to the Codex
    /// wire value (allow → `accept`, deny → `decline`), and emit
    /// [`AgentEvent::PermissionResolved`].
    ///
    /// This is the Codex counterpart to the Claude adapter's ingestion seam: the
    /// entry point — reachable through `Arc<dyn AgentAdapter>` — through which a
    /// browser decision reaches the provider. `request_id` is the adapter-scoped
    /// token surfaced on the matching `PermissionRequested`, which keys the open
    /// approval's verbatim wire id. Errors when the session or the request id is
    /// unknown (already answered, or never open).
    async fn resolve_permission(
        &self,
        handle: &AgentSessionHandle,
        request_id: &str,
        decision: PermissionDecision,
    ) -> UsecaseResult<()> {
        let wire_id = {
            let sessions = self.sessions.lock().expect("sessions mutex poisoned");
            let session = sessions
                .get(&handle.key)
                .ok_or_else(|| UsecaseError::Agent(format!("unknown session `{}`", handle.key)))?;
            let removed = session
                .approvals
                .lock()
                .expect("approvals mutex poisoned")
                .remove(request_id);
            removed
        };
        let wire_id = wire_id.ok_or_else(|| {
            UsecaseError::Agent(format!(
                "permission request `{request_id}` is not awaiting a decision"
            ))
        })?;
        let decision_value = match decision {
            PermissionDecision::Allow => DECISION_ACCEPT,
            PermissionDecision::Deny => DECISION_DECLINE,
        };
        // Both approval kinds that reach here — command execution and file change
        // — share the `{ "decision": … }` response shape, so a single reply
        // serves both without needing to remember which method opened the
        // request (the permissions approval, whose response differs, never
        // becomes an open approval — it takes the unsupported path).
        self.conn
            .respond(&wire_id, json!({ "decision": decision_value }))
            .await
            .map_err(to_usecase_err)?;
        self.emit(
            &handle.key,
            AgentEvent::PermissionResolved {
                request_id: request_id.to_owned(),
                decision,
            },
        );
        Ok(())
    }

    async fn close(&self, handle: &AgentSessionHandle) -> UsecaseResult<()> {
        // v1 models no per-thread close RPC: the shared connection stays up for
        // its other threads. Surface the end and drop the session's plumbing
        // (which aborts its translation task).
        self.emit(
            &handle.key,
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Closed,
            },
        );
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .remove(&handle.key);
        Ok(())
    }

    fn events(&self, handle: &AgentSessionHandle) -> AgentEventStream {
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        match sessions.get_mut(&handle.key).and_then(|s| s.rx.take()) {
            Some(rx) => AgentEventStream::new(rx),
            None => {
                // Already handed out, or an unknown/closed session: an
                // already-closed stream rather than a panic.
                let (_tx, rx) = mpsc::unbounded_channel();
                AgentEventStream::new(rx)
            }
        }
    }

    async fn attach_terminal(
        &self,
        _handle: &AgentSessionHandle,
    ) -> UsecaseResult<Option<PtyHandle>> {
        // Codex is `TerminalCapability::NoTerminal`: there is nothing to attach.
        Ok(None)
    }

    fn content_source(
        &self,
        handle: &AgentSessionHandle,
        req: ContentSourceRequest,
    ) -> Box<dyn AgentContentSource> {
        // Codex pushes structured `item/*` / `turn/*` frames, so its event pump
        // folds them into canonical messages through this accumulator (the
        // `CodexConversationSource`), rather than reading a transcript. The
        // launch site rides the neutral request; the model is the one fact only
        // this adapter holds, recorded from the thread's opening response when
        // the session was registered.
        codex_content_source(req, self.session_model(&handle.key))
    }
}

/// The translation task for one session: drain the thread's frames, translate
/// each into neutral events, and push them onto the session's channel.
///
/// Server → client requests are handled here rather than in the pure translator
/// because they need I/O: a modeled approval is recorded (so a later decision
/// can answer it), and an unmodeled request is answered immediately with an
/// error so the session never hangs on it.
async fn translate_loop(
    mut events: UnboundedReceiver<ThreadEvent>,
    tx: UnboundedSender<AgentEvent>,
    conn: Weak<AppServerConnection>,
    approvals: Arc<Mutex<HashMap<String, Value>>>,
    current_turn_id: Arc<Mutex<Option<String>>>,
) {
    while let Some(event) = events.recv().await {
        match event {
            ThreadEvent::Notification(notification) => {
                // Maintain the tracked turn id off the pushed `turn/*` frames: a
                // `turn/started` re-affirms the id (the `turn/start` response
                // already set it), and a `turn/completed` clears it so a later
                // interrupt does not reference a finished turn.
                if let Some(turn_id) = started_turn_id(&notification) {
                    *current_turn_id
                        .lock()
                        .expect("current turn id mutex poisoned") = Some(turn_id);
                } else if is_turn_completed(&notification) {
                    *current_turn_id
                        .lock()
                        .expect("current turn id mutex poisoned") = None;
                }
                for event in translate_notification(&notification) {
                    if tx.send(event).is_err() {
                        return;
                    }
                }
            }
            ThreadEvent::ServerRequest(request) => match classify_server_request(&request) {
                ServerRequestKind::Approval(permission) => {
                    approvals
                        .lock()
                        .expect("approvals mutex poisoned")
                        .insert(permission.request_id.clone(), request.id.clone());
                    if tx
                        .send(AgentEvent::PermissionRequested {
                            request: permission,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
                ServerRequestKind::Unsupported {
                    method,
                    detail_json,
                } => {
                    // Answer first so the server unblocks, then surface it.
                    if let Some(conn) = conn.upgrade() {
                        let _ = conn
                            .respond_error(
                                &request.id,
                                METHOD_NOT_FOUND,
                                &format!("unsupported server request: {method}"),
                            )
                            .await;
                    }
                    if tx
                        .send(AgentEvent::UnsupportedInteraction {
                            method,
                            detail_json,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            },
        }
    }
}

/// Map a transport error into the use-case error type at the trait boundary.
fn to_usecase_err(err: crate::Error) -> UsecaseError {
    UsecaseError::Agent(err.to_string())
}
