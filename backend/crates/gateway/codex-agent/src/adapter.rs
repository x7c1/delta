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
//! An `*/requestApproval` server request surfaces as
//! [`AgentEvent::PermissionRequested`]; the adapter remembers the verbatim wire
//! id so [`AgentAdapter::resolve_permission`] can answer it with the frozen
//! decision mapping (allow → `accept`, deny → `decline`) and emit
//! [`AgentEvent::PermissionResolved`]. That trait method — reachable through
//! `Arc<dyn AgentAdapter>` — is the Codex analogue of the Claude adapter's
//! ingestion seam: it is how a browser decision reaches the provider, routed by
//! the core from the Delta permission-row id back to the adapter-scoped token.
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
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use delta_usecase::{
    AgentAdapter, AgentCapabilities, AgentContentSource, AgentEvent, AgentEventStream,
    AgentProvider, AgentSessionHandle, ContextInjectionCapability, Error as UsecaseError,
    EventCapability, ForkCapability, InterruptCapability, LaunchCapability, LaunchRequest,
    PermissionCapability, PermissionDecision, PtyHandle, Result as UsecaseResult, ResumeCapability,
    ResumeRequest, SendReceipt, SendRequest, SessionEndReason, SessionId,
    SessionIdentityCapability, SteerCapability, TerminalCapability, ThreadId, TranscriptCapability,
};

use crate::translate::{classify_server_request, translate_notification, ServerRequestKind};
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

/// The Codex decision wire value for an allow.
const DECISION_ACCEPT: &str = "accept";
/// The Codex decision wire value for a deny.
const DECISION_DECLINE: &str = "decline";
/// JSON-RPC "method not found", reused to reject an unmodeled server request.
const METHOD_NOT_FOUND: i64 = -32601;

/// Per-session plumbing: the event sender the adapter and its translation task
/// emit through, the receiver handed out once by [`AgentAdapter::events`], and
/// the map of open approval requests awaiting a decision.
struct CodexSession {
    tx: UnboundedSender<AgentEvent>,
    rx: Option<UnboundedReceiver<AgentEvent>>,
    /// Open approval requests: neutral `request_id` → the verbatim wire id the
    /// response must echo. Shared with the translation task, which inserts an
    /// entry when it surfaces a `PermissionRequested`.
    approvals: Arc<Mutex<HashMap<String, Value>>>,
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
    /// channel, emit the opening [`AgentEvent::SessionStarted`], and spawn the
    /// translation task that drains the thread's frames onto the channel.
    fn register_session(&self, started: StartedThread) -> AgentSessionHandle {
        let StartedThread {
            thread_id, events, ..
        } = started;
        let (tx, rx) = mpsc::unbounded_channel();
        // Buffered before `events()` is called, so the opener is never dropped.
        let _ = tx.send(AgentEvent::SessionStarted {
            provider_session_id: thread_id.clone(),
        });
        let approvals: Arc<Mutex<HashMap<String, Value>>> = Arc::new(Mutex::new(HashMap::new()));
        let task = tokio::spawn(translate_loop(
            events,
            tx.clone(),
            // A weak reference: the task must not keep the connection alive, or
            // dropping the adapter would never close the thread channel and the
            // task would never exit.
            Arc::downgrade(&self.conn),
            Arc::clone(&approvals),
        ));
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(
                thread_id.clone(),
                CodexSession {
                    tx,
                    rx: Some(rx),
                    approvals,
                    task,
                },
            );
        AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: thread_id.clone(),
            key: thread_id,
        }
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

    /// Start a turn for `handle` with the visible prompt `text`.
    ///
    /// `UserPromptAccepted` is emitted before the `turn/start` request is even
    /// issued, so it always precedes the turn's pushed notifications on the
    /// stream. The `turn/start` response's `turnId` (when present) rides back as
    /// the send receipt's provider message id.
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
            },
        );
        let params = json!({ "threadId": handle.provider_session_id, "input": text });
        let result = self
            .conn
            .request("turn/start", Some(params))
            .await
            .map_err(to_usecase_err)?;
        let provider_message_id = result
            .get("turnId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(SendReceipt {
            provider_message_id,
        })
    }
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
        // rides `thread/start` as `cwd`.
        let started = self
            .conn
            .start_thread(Some(json!({ "cwd": req.workdir })))
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

    async fn interrupt(&self, handle: &AgentSessionHandle) -> UsecaseResult<()> {
        self.conn
            .request(
                "turn/interrupt",
                Some(json!({ "threadId": handle.provider_session_id })),
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
        session_id: SessionId,
        main_thread: ThreadId,
        seed_seq: i64,
    ) -> Box<dyn AgentContentSource> {
        // Codex pushes structured `item/*` / `turn/*` frames, so its event pump
        // folds them into canonical messages through this accumulator (the
        // `CodexConversationSource`), rather than reading a transcript.
        codex_content_source(session_id, main_thread, seed_seq)
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
) {
    while let Some(event) = events.recv().await {
        match event {
            ThreadEvent::Notification(notification) => {
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
