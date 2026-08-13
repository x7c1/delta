//! `codex-agent`: the transport that drives OpenAI Codex through a shared
//! `codex app-server` process.
//!
//! ## Scope
//!
//! This module is the **transport + demux plumbing**; the neutral
//! [`CodexAppServerAdapter`] built on top of it lives in the crate's `adapter`
//! module, and the wire → `AgentEvent` translation in its `translate` module.
//! What this file provides:
//!
//! - spawning `codex app-server` (the command is configurable, mirroring the
//!   core's `LaunchConfig::claude_bin`, so a test can point it at a fake),
//! - the newline-delimited JSON-RPC 2.0 framing (see [`wire`]),
//! - the `initialize` → `initialized` handshake,
//! - request/response correlation by id,
//! - the **`threadId` → session demux**: server notifications and
//!   server-originated requests are routed to a per-thread channel, so a single
//!   shared server hosting many threads fans out to one consumer per Delta
//!   session (session ↔ Codex thread is 1:1), and
//! - the terminal **connection-death** signal on each of those channels (see
//!   [`ThreadEvent::ConnectionLost`]).
//!
//! ## Model
//!
//! One [`AppServerConnection`] owns one `codex app-server` process. A background
//! reader task consumes the server's stdout line by line, parses each frame,
//! and dispatches it: a [`wire::Response`] wakes the pending request it
//! correlates to; a thread-scoped notification or server request is delivered to
//! that thread's [`ThreadEvent`] channel; anything not scoped to a known thread
//! goes to the connection-level "unrouted" channel.
//!
//! When that reader stops because the server is *gone* — EOF or a read error —
//! the death is announced rather than merely ending the traffic: every pending
//! request is woken with [`Error::ConnectionClosed`] and every subscribed thread
//! receives a terminal [`ThreadEvent::ConnectionLost`] on its own channel. That
//! is what lets the adapter surface a session-ended fact (and the core settle
//! the stuck turn and its pending approvals) instead of the session's event
//! stream simply going quiet forever.
//!
//! That channel is not a dead end: it is where genuinely **account-scoped**
//! frames arrive (`account/rateLimits/updated` names no thread, because the
//! account is shared by every thread on the connection), and the adapter drains
//! it — see [`AppServerConnection::take_unrouted`]. It is bounded
//! ([`UNROUTED_CAPACITY`]) so a connection nobody drains cannot grow without
//! limit; an overflowing frame is dropped with a log line rather than
//! accumulating silently.
//!
//! ## Observability
//!
//! Because this provider is headless — there is no terminal for a human to watch
//! — every frame is also mirrored into a
//! [`CommsLogSink`](delta_usecase::CommsLogSink) for the browser's comms-log
//! inspector: outgoing frames and their responses here (see
//! [`AppServerConnection::with_comms_log`]), server-pushed frames in the
//! adapter's own receive path (which is where a provider thread id can be
//! attributed to a Delta session id). Mirroring is observability only and never
//! blocks — see the port's docs.
//!
//! Attribution shapes what the inspector shows: a server frame carrying no
//! `threadId` belongs to no *one* session, so it goes to the unrouted channel
//! above — and the adapter's drain mirrors it into EVERY live session's log,
//! since it is a fact about the connection all of them share. Only a frame that
//! arrives while no session is open lands in no log at all: there is no
//! inspector open to show it in.

mod adapter;
mod content;
mod error;
mod factory;
pub mod schema;
mod translate;
pub mod wire;

pub use adapter::{CodexAppServerAdapter, CODEX_CAPABILITIES};
pub use content::{codex_content_source, CodexConversationSource};
pub use factory::CodexAdapterFactory;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

use delta_usecase::{CommsDirection, CommsEntry, CommsFrameKind, CommsLogSink, NullCommsLog};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub use error::{Error, Result};
use wire::{parse_incoming, Incoming, Notification, RequestId, ServerRequest};

/// How the connection launches the app-server.
#[derive(Debug, Clone)]
pub struct CodexLaunchConfig {
    /// The program launched to obtain an app-server (`codex` by default). Used
    /// verbatim as argv[0], so it may be a bare name resolved via `PATH` or an
    /// absolute path — matching the core's `LaunchConfig::claude_bin`. A test
    /// points this at the fake app-server binary.
    pub codex_bin: String,
    /// Arguments after argv[0] (`["app-server"]` by default, so `codex_bin`
    /// alone spawns the server). A test that points `codex_bin` at a fake
    /// clears this.
    pub args: Vec<String>,
    /// Extra environment variables set on the child, on top of the inherited
    /// environment. Empty by default; a test uses it to hand the fake its
    /// scenario without mutating the parent process's (shared) environment.
    pub env: Vec<(String, String)>,
}

impl Default for CodexLaunchConfig {
    fn default() -> Self {
        Self {
            codex_bin: "codex".to_owned(),
            args: vec!["app-server".to_owned()],
            env: Vec::new(),
        }
    }
}

/// A frame delivered to a per-thread channel by the demux — or the terminal
/// fact that no further frame can arrive.
///
/// The two frame variants are deliberately not translated into a neutral
/// `AgentEvent` here: the transport hands the adapter the raw, thread-scoped
/// server frames so it can do the translation.
///
/// [`ThreadEvent::ConnectionLost`] is not a frame at all. It rides the same
/// per-thread channel so it is ordered *behind* every frame that did arrive:
/// the adapter's translation task therefore sees a thread's last real frames
/// (an approval request the server managed to write before dying, say) before
/// it learns the connection is gone, and the core settles in that order.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadEvent {
    /// A server → client notification for this thread (`item/*`, `turn/*`, …).
    Notification(Notification),
    /// A server → client request for this thread (`*/requestApproval`), still
    /// awaiting a response the adapter will send.
    ServerRequest(ServerRequest),
    /// The connection carrying this thread died: the reader saw EOF (the
    /// `codex app-server` process exited) or a read error, so nothing further
    /// will ever arrive on this channel and no outgoing frame can be answered.
    ///
    /// Always the last event a thread receives, and emitted exactly once per
    /// thread — the connection's reader announces it as it exits. It
    /// exists so a dead connection is a *fact on the stream* rather than the
    /// stream merely going quiet: silence is indistinguishable from a slow
    /// model, and that is precisely how a stuck turn and an unanswerable
    /// approval dialog survived forever in the field.
    ConnectionLost,
}

/// How many unrouted frames the connection buffers before dropping them.
///
/// Bounded rather than unbounded so a connection nobody drains (a
/// transport-only unit test) costs a fixed, small amount of memory instead of
/// growing for the process's lifetime. The drain (the adapter's account loop)
/// empties it continuously, so in production the buffer only ever absorbs a
/// burst against traffic that is a handful of account notifications per session.
const UNROUTED_CAPACITY: usize = 256;

type PendingMap = Arc<Mutex<HashMap<RequestId, oneshot::Sender<wire::Response>>>>;
type ThreadMap = Arc<Mutex<HashMap<String, ThreadSlot>>>;

/// A thread's demux slot: either a live subscriber or a backlog of frames that
/// arrived before the thread was subscribed.
///
/// The backlog closes a race: `thread/start` learns the thread id from its
/// response, but the server may emit the thread's first notification on the very
/// next line — before the caller subscribes. Such frames are buffered here and
/// drained into the channel the moment [`AppServerConnection::subscribe_thread`]
/// registers it, so nothing scoped to a known thread is lost to timing.
enum ThreadSlot {
    /// Frames seen before anyone subscribed to this thread.
    Buffered(Vec<ThreadEvent>),
    /// The live subscriber's sender.
    Live(mpsc::UnboundedSender<ThreadEvent>),
}

/// A live connection to one `codex app-server` process.
pub struct AppServerConnection {
    /// The server's stdin, behind a mutex so concurrent callers serialise their
    /// frames (each frame is written whole).
    writer: tokio::sync::Mutex<Box<dyn AsyncWrite + Unpin + Send>>,
    /// Mints the monotonic ids Delta's outgoing requests are correlated by.
    next_id: AtomicI64,
    /// Outstanding requests awaiting their response, keyed by id.
    pending: PendingMap,
    /// Per-thread event channels, keyed by the provider thread id.
    threads: ThreadMap,
    /// Frames not scoped to any registered thread: the account-scoped
    /// notifications a shared connection carries (`account/rateLimits/updated`
    /// names no thread), plus any stray frame for an unknown thread. Handed out
    /// once, to the adapter that drains it.
    unrouted: Mutex<Option<mpsc::Receiver<ThreadEvent>>>,
    /// The child process, kept alive for the connection's lifetime and killed
    /// on drop (`kill_on_drop`). `None` when the connection was built over
    /// injected pipes in a test.
    _child: Option<Child>,
    /// The reader task, aborted on drop so it never outlives the connection.
    reader: JoinHandle<()>,
    /// Where every frame this connection writes — and every response it reads —
    /// is mirrored for the comms-log inspector. [`NullCommsLog`] unless the
    /// composition root attached one (see
    /// [`AppServerConnection::with_comms_log`]), so the emit sites need no
    /// `Option` handling.
    ///
    /// Server-originated frames (notifications and approval requests) are *not*
    /// mirrored here: the reader demuxes them by provider thread id, while the
    /// log is keyed by Delta session id, and only the adapter knows that
    /// mapping. It mirrors them from its own receive path (see
    /// `adapter::translate_loop`).
    comms: Arc<dyn CommsLogSink>,
}

impl Drop for AppServerConnection {
    fn drop(&mut self) {
        self.reader.abort();
    }
}

impl AppServerConnection {
    /// Spawn `codex app-server` per `config` and start the reader task.
    pub fn spawn(config: &CodexLaunchConfig) -> Result<Self> {
        let mut child = Command::new(&config.codex_bin)
            .args(&config.args)
            .envs(config.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // The server's stderr is its own diagnostic channel; leave it
            // attached to ours so a launch failure is visible, rather than
            // piping and silently dropping it.
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .map_err(Error::Spawn)?;

        let stdin = child.stdin.take().ok_or(Error::MissingPipe("stdin"))?;
        let stdout = child.stdout.take().ok_or(Error::MissingPipe("stdout"))?;
        Ok(Self::from_io(stdout, stdin, Some(child)))
    }

    /// Build a connection over arbitrary byte streams (used by unit tests to
    /// inject an in-process scripted server via a duplex pipe). `reader` is the
    /// server → client direction; `writer` is client → server.
    pub fn from_io<R, W>(reader: R, writer: W, child: Option<Child>) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let threads: ThreadMap = Arc::new(Mutex::new(HashMap::new()));
        let (unrouted_tx, unrouted_rx) = mpsc::channel(UNROUTED_CAPACITY);

        let reader_task = tokio::spawn(read_loop(
            reader,
            Arc::clone(&pending),
            Arc::clone(&threads),
            unrouted_tx,
        ));

        Self {
            writer: tokio::sync::Mutex::new(Box::new(writer)),
            next_id: AtomicI64::new(1),
            pending,
            threads,
            unrouted: Mutex::new(Some(unrouted_rx)),
            _child: child,
            reader: reader_task,
            comms: NullCommsLog::arc(),
        }
    }

    /// Mirror this connection's frames into `sink` for the comms-log inspector.
    ///
    /// A builder rather than a constructor argument so every existing call site
    /// (and every test that drives a connection over injected pipes) keeps its
    /// unobserved default.
    pub fn with_comms_log(mut self, sink: Arc<dyn CommsLogSink>) -> Self {
        self.comms = sink;
        self
    }

    /// The comms-log sink this connection mirrors into, so the adapter records
    /// its own receive path into the same log rather than being handed a second
    /// copy of the sink.
    pub fn comms_log(&self) -> &Arc<dyn CommsLogSink> {
        &self.comms
    }

    /// Perform the `initialize` → `initialized` handshake, returning the
    /// server's `initialize` result. `params` is passed through verbatim.
    pub async fn initialize(&self, params: Value) -> Result<Value> {
        // The handshake stands the shared server up before any thread exists, so
        // it belongs to no Delta session and is not recorded (scope `None`).
        let result = self.request(None, "initialize", Some(params)).await?;
        // The client signals it is ready to receive with an `initialized`
        // notification, mirroring the LSP-style handshake.
        self.notify(None, "initialized", None).await?;
        Ok(result)
    }

    /// Send a request and await its correlated response.
    ///
    /// `scope` is the Delta session id this exchange belongs to, for the
    /// comms-log inspector; `None` for a connection-level call that belongs to
    /// no session (the handshake). Both the outgoing request and the response it
    /// correlates to are recorded here — the awaiting caller is the one place
    /// that knows which request a response answers, so attributing it needs no
    /// side table.
    pub async fn request(
        &self,
        scope: Option<&str>,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending mutex poisoned")
            .insert(id, tx);

        let frame = wire::encode_request(id, method, params).map_err(Error::Encode)?;
        // Recorded BEFORE the write, and this is load-bearing: the server can
        // push a notification caused by this request while the write is still
        // completing, and the reader records that on another task — so recording
        // afterwards would let an effect appear ahead of its cause in the log,
        // destroying the one thing the log is for (the sequence). The cost is that
        // a frame whose write then fails is still shown, which is the better
        // failure: a write that fails has killed the connection, and the frame it
        // died on is exactly what an operator needs to see.
        self.record(
            scope,
            CommsDirection::ToAgent,
            CommsFrameKind::Request,
            Some(method),
            &frame,
        );
        if let Err(err) = self.write_frame(&frame).await {
            // Do not leak the pending slot if the write failed.
            self.pending
                .lock()
                .expect("pending mutex poisoned")
                .remove(&id);
            return Err(err);
        }

        // The reader task drops the sender (without sending) when the
        // connection closes, so a closed connection surfaces as `Err` rather
        // than hanging forever.
        let response = rx.await.map_err(|_| Error::ConnectionClosed)?;
        self.record(
            scope,
            CommsDirection::FromAgent,
            CommsFrameKind::Response,
            // A response names no method of its own; showing the method it
            // answers is what makes the pair readable at a glance.
            Some(method),
            &response.to_frame_json(),
        );
        response.outcome.map_err(|error| Error::Rpc {
            method: method.to_owned(),
            error,
        })
    }

    /// Send a notification (no response is expected). `scope` is as on
    /// [`Self::request`].
    pub async fn notify(
        &self,
        scope: Option<&str>,
        method: &str,
        params: Option<Value>,
    ) -> Result<()> {
        let frame = wire::encode_notification(method, params).map_err(Error::Encode)?;
        // Before the write, for the ordering reason given in [`Self::request`].
        self.record(
            scope,
            CommsDirection::ToAgent,
            CommsFrameKind::Notification,
            Some(method),
            &frame,
        );
        self.write_frame(&frame).await
    }

    /// Register a per-thread event channel for `thread_id` and return its
    /// receiver. Frames the demux routes to this thread arrive here, including
    /// any that arrived before this call (they were buffered and are drained
    /// into the channel now, in arrival order). Registering the same id again
    /// replaces the channel (the old receiver then goes idle).
    pub fn subscribe_thread(&self, thread_id: &str) -> mpsc::UnboundedReceiver<ThreadEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut guard = self.threads.lock().expect("threads mutex poisoned");
        if let Some(ThreadSlot::Buffered(buffered)) = guard.remove(thread_id) {
            for event in buffered {
                let _ = tx.send(event);
            }
        }
        guard.insert(thread_id.to_owned(), ThreadSlot::Live(tx));
        rx
    }

    /// Start a thread via `thread/start`, register its event channel, and return
    /// the provider thread id together with the channel.
    ///
    /// The channel is registered before returning, and a well-behaved server
    /// emits a thread's notifications only after `turn/start`, so no early
    /// notification is lost between learning the id and subscribing.
    pub async fn start_thread(
        &self,
        scope: Option<&str>,
        params: Option<Value>,
    ) -> Result<StartedThread> {
        let result = self.request(scope, "thread/start", params).await?;
        let thread_id =
            thread_id_from_result(&result).ok_or_else(|| Error::UnexpectedResponse {
                method: "thread/start".to_owned(),
                detail: format!("result has no string `thread.id`: {result}"),
            })?;
        let events = self.subscribe_thread(&thread_id);
        Ok(StartedThread {
            thread_id,
            events,
            result,
        })
    }

    /// Resume a thread via `thread/resume`, register its event channel, and
    /// return the provider thread id together with the channel. Symmetric with
    /// [`AppServerConnection::start_thread`]; the server's history replay (if
    /// any) rides its own notifications on the returned channel.
    ///
    /// The server echoes the resumed thread back under `result.thread` (whose
    /// `id` is the thread id); when it omits it the requested id (carried in
    /// `params.threadId`) is used, so a lean resume result still yields a usable
    /// id.
    pub async fn resume_thread(
        &self,
        scope: Option<&str>,
        params: Option<Value>,
    ) -> Result<StartedThread> {
        let requested = params
            .as_ref()
            .and_then(|p| p.get("threadId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let result = self.request(scope, "thread/resume", params).await?;
        let thread_id = thread_id_from_result(&result)
            .or(requested)
            .ok_or_else(|| Error::UnexpectedResponse {
                method: "thread/resume".to_owned(),
                detail: format!(
                    "result has no string `thread.id` and none was requested: {result}"
                ),
            })?;
        let events = self.subscribe_thread(&thread_id);
        Ok(StartedThread {
            thread_id,
            events,
            result,
        })
    }

    /// Answer a server-originated request with a success result. `id` is the
    /// verbatim id the [`ServerRequest`] carried, echoed back with the same JSON
    /// type the server used.
    /// `scope` is as on [`Self::request`].
    pub async fn respond(&self, scope: Option<&str>, id: &Value, result: Value) -> Result<()> {
        let frame = wire::encode_success_response(id, result).map_err(Error::Encode)?;
        // Before the write, for the ordering reason given in [`Self::request`] —
        // which matters most here: answering an approval is what unblocks the
        // turn, so the frames it releases follow immediately.
        self.record(
            scope,
            CommsDirection::ToAgent,
            CommsFrameKind::Response,
            // Delta's answer names no method; the server request it answers is
            // the frame just above it in the log.
            None,
            &frame,
        );
        self.write_frame(&frame).await
    }

    /// Answer a server-originated request with a JSON-RPC error. Used to reject a
    /// request Delta does not model, so a well-behaved server unblocks its turn
    /// rather than waiting forever for a reply.
    /// `scope` is as on [`Self::request`].
    pub async fn respond_error(
        &self,
        scope: Option<&str>,
        id: &Value,
        code: i64,
        message: &str,
    ) -> Result<()> {
        let frame = wire::encode_error_response(id, code, message).map_err(Error::Encode)?;
        // Before the write, for the ordering reason given in [`Self::request`].
        self.record(
            scope,
            CommsDirection::ToAgent,
            CommsFrameKind::Response,
            None,
            &frame,
        );
        self.write_frame(&frame).await
    }

    /// Take the connection-level "unrouted" channel: frames that were not scoped
    /// to any registered thread — chiefly the account-scoped notifications that
    /// carry no `threadId`. Returns `None` if already taken.
    ///
    /// The adapter takes it when it is built and drains it for the connection's
    /// lifetime, which is what turns an account frame into a browser-visible
    /// fact. Nothing else may take it: a second caller would get `None` and
    /// silently see no frames.
    pub fn take_unrouted(&self) -> Option<mpsc::Receiver<ThreadEvent>> {
        self.unrouted
            .lock()
            .expect("unrouted mutex poisoned")
            .take()
    }

    /// Mirror one frame into the comms log, if it belongs to a session.
    ///
    /// A frame with no `scope` belongs to no Delta session (the shared server's
    /// handshake), so there is no inspector for it to appear in and it is
    /// dropped here rather than being fanned out to unrelated sessions. The
    /// trailing newline the wire framing adds is stripped — it is transport
    /// framing, not part of the JSON the inspector shows.
    ///
    /// Non-blocking by contract (see [`CommsLogSink`]), so this is safe to call
    /// on the turn's own code path.
    fn record(
        &self,
        scope: Option<&str>,
        direction: CommsDirection,
        kind: CommsFrameKind,
        method: Option<&str>,
        frame: &str,
    ) {
        if let Some(session_id) = scope {
            self.comms.record(
                session_id,
                CommsEntry::new(direction, kind, method, frame.trim_end()),
            );
        }
    }

    async fn write_frame(&self, frame: &str) -> Result<()> {
        let mut writer = self.writer.lock().await;
        writer
            .write_all(frame.as_bytes())
            .await
            .map_err(Error::Write)?;
        writer.flush().await.map_err(Error::Write)?;
        Ok(())
    }
}

/// The provider thread id a `thread/start` / `thread/resume` response carries,
/// read from the `Thread` object under `result.thread` (see the vendored
/// `ThreadStartResponse` / `ThreadResumeResponse` schemas: `{ thread: Thread, … }`,
/// `Thread.id`).
fn thread_id_from_result(result: &Value) -> Option<String> {
    result
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The result of [`AppServerConnection::start_thread`].
pub struct StartedThread {
    /// The provider's thread id (Codex's `thr_...`).
    pub thread_id: String,
    /// The thread's event channel — server notifications and server requests
    /// scoped to this thread arrive here.
    pub events: mpsc::UnboundedReceiver<ThreadEvent>,
    /// The raw `thread/start` result, for any provider-specific fields the
    /// caller wants beyond the thread id.
    pub result: Value,
}

/// The reader task: consume the server's stdout frame by frame and dispatch.
async fn read_loop<R>(
    reader: R,
    pending: PendingMap,
    threads: ThreadMap,
    unrouted: mpsc::Sender<ThreadEvent>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if line.trim().is_empty() {
                    continue;
                }
                match parse_incoming(&line) {
                    Ok(msg) => dispatch(msg, &pending, &threads, &unrouted),
                    Err(err) => {
                        // A frame we cannot parse is logged and skipped rather
                        // than tearing the connection down — a newer server
                        // might emit a shape this build does not model.
                        eprintln!("codex-agent: skipping unparseable frame: {err}");
                    }
                }
            }
            // EOF: the server closed its stdout (it exited). Stop reading; the
            // pending map is dropped below, dropping every waiting sender, so
            // outstanding requests resolve to `ConnectionClosed`.
            Ok(None) => break,
            Err(err) => {
                eprintln!("codex-agent: read error, closing connection: {err}");
                break;
            }
        }
    }
    // Explicitly clear the pending map so waiters wake immediately on close.
    pending.lock().expect("pending mutex poisoned").clear();
    // Reaching here means the server is gone — EOF or a read error, never an
    // orderly shutdown: this task has no other exit, and a connection dropped
    // deliberately aborts it (see `Drop for AppServerConnection`) at the read
    // above, before this line. So every subscribed thread is told, once, that
    // its connection died; waking the pending map alone would only settle
    // Delta's own in-flight requests and leave each session's event stream
    // silent.
    announce_connection_lost(&threads);
}

/// Tell every thread on this connection that it is gone, as the reader exits.
///
/// A live subscriber receives [`ThreadEvent::ConnectionLost`] on its channel;
/// a thread nobody has subscribed to yet gets it appended to its backlog, so a
/// subscriber that registers after the death is told immediately rather than
/// waiting for a frame that can never come. The slot is left in place either
/// way — no further frame can be routed to it, so nothing accumulates.
fn announce_connection_lost(threads: &ThreadMap) {
    let mut guard = threads.lock().expect("threads mutex poisoned");
    for (thread_id, slot) in guard.iter_mut() {
        match slot {
            ThreadSlot::Live(tx) => {
                if tx.send(ThreadEvent::ConnectionLost).is_err() {
                    // The subscriber is already gone (its pump exited), so
                    // there is nobody left to tell. Not an error: the session
                    // this thread belonged to has no live consumer.
                    eprintln!(
                        "codex-agent: connection lost, but thread `{thread_id}` has no subscriber"
                    );
                }
            }
            ThreadSlot::Buffered(buffer) => buffer.push(ThreadEvent::ConnectionLost),
        }
    }
}

/// Route one parsed frame to its destination.
fn dispatch(
    msg: Incoming,
    pending: &PendingMap,
    threads: &ThreadMap,
    unrouted: &mpsc::Sender<ThreadEvent>,
) {
    // The thread id (if any) is read before `msg` is moved into the event.
    let thread_id = msg.thread_id().map(str::to_owned);
    match msg {
        Incoming::Response(response) => {
            if let Some(tx) = pending
                .lock()
                .expect("pending mutex poisoned")
                .remove(&response.id)
            {
                // The receiver may have gone away (request future dropped); a
                // failed send is then harmless.
                let _ = tx.send(response);
            } else {
                eprintln!(
                    "codex-agent: response for unknown request id {}",
                    response.id
                );
            }
        }
        Incoming::Notification(notification) => route_thread_event(
            thread_id,
            ThreadEvent::Notification(notification),
            threads,
            unrouted,
        ),
        Incoming::ServerRequest(request) => route_thread_event(
            thread_id,
            ThreadEvent::ServerRequest(request),
            threads,
            unrouted,
        ),
    }
}

/// Deliver a thread-scoped event to its thread channel. A frame for a thread
/// not yet subscribed is buffered against that thread (see [`ThreadSlot`]); a
/// frame with no thread id goes to the connection's unrouted channel.
fn route_thread_event(
    thread_id: Option<String>,
    event: ThreadEvent,
    threads: &ThreadMap,
    unrouted: &mpsc::Sender<ThreadEvent>,
) {
    let Some(thread_id) = thread_id else {
        // `try_send` rather than an await: the reader must never block on a slow
        // (or absent) drain, since that would stall every thread's frames behind
        // one account notification. A full or closed channel drops the frame
        // loudly — the buffer is sized so this means the drain is gone, not that
        // it is briefly behind.
        if let Err(err) = unrouted.try_send(event) {
            eprintln!("codex-agent: dropping an unrouted frame ({err})");
        }
        return;
    };
    let mut guard = threads.lock().expect("threads mutex poisoned");
    match guard.get_mut(&thread_id) {
        Some(ThreadSlot::Live(tx)) => {
            if let Err(mpsc::error::SendError(returned)) = tx.send(event) {
                // The subscriber dropped its receiver: buffer against the thread
                // again so a later re-subscribe still sees it, rather than losing
                // it to the unrouted channel.
                guard.insert(thread_id, ThreadSlot::Buffered(vec![returned]));
            }
        }
        Some(ThreadSlot::Buffered(buffer)) => buffer.push(event),
        None => {
            guard.insert(thread_id, ThreadSlot::Buffered(vec![event]));
        }
    }
}

#[cfg(test)]
mod tests;
