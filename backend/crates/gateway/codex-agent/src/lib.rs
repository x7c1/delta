//! `codex-agent`: the transport that drives OpenAI Codex through a shared
//! `codex app-server` process.
//!
//! ## Scope in this phase (C1)
//!
//! This crate is the **transport + demux plumbing** only — it does not yet
//! translate Codex's wire events into Delta's neutral `AgentEvent`s, and it
//! does not implement the `AgentAdapter` trait (that is the next phase). What
//! it provides:
//!
//! - spawning `codex app-server` (the command is configurable, mirroring the
//!   core's `LaunchConfig::claude_bin`, so a test can point it at a fake),
//! - the newline-delimited JSON-RPC 2.0 framing (see [`wire`]),
//! - the `initialize` → `initialized` handshake,
//! - request/response correlation by id, and
//! - the **`threadId` → session demux skeleton**: server notifications and
//!   server-originated requests are routed to a per-thread channel, so a single
//!   shared server hosting many threads fans out to one consumer per Delta
//!   session (session ↔ Codex thread is 1:1).
//!
//! ## Model
//!
//! One [`AppServerConnection`] owns one `codex app-server` process. A background
//! reader task consumes the server's stdout line by line, parses each frame,
//! and dispatches it: a [`wire::Response`] wakes the pending request it
//! correlates to; a thread-scoped notification or server request is delivered to
//! that thread's [`ThreadEvent`] channel; anything not scoped to a known thread
//! goes to the connection-level "unrouted" channel.

mod adapter;
mod content;
mod error;
mod factory;
mod translate;
pub mod wire;

pub use adapter::CodexAppServerAdapter;
pub use content::{codex_content_source, CodexConversationSource};
pub use factory::CodexAdapterFactory;

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

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

/// A frame delivered to a per-thread channel by the demux.
///
/// Deliberately not yet translated into a neutral `AgentEvent` — that is the
/// next phase. The C1 transport hands the C2 adapter the raw, thread-scoped
/// server frames so it can do the translation.
#[derive(Debug, Clone, PartialEq)]
pub enum ThreadEvent {
    /// A server → client notification for this thread (`item/*`, `turn/*`, …).
    Notification(Notification),
    /// A server → client request for this thread (`*/requestApproval`), still
    /// awaiting a response the adapter will send.
    ServerRequest(ServerRequest),
}

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
    /// Frames not scoped to any registered thread (connection-level
    /// notifications, or a stray frame for an unknown thread). Handed out once.
    unrouted: Mutex<Option<mpsc::UnboundedReceiver<ThreadEvent>>>,
    /// The child process, kept alive for the connection's lifetime and killed
    /// on drop (`kill_on_drop`). `None` when the connection was built over
    /// injected pipes in a test.
    _child: Option<Child>,
    /// The reader task, aborted on drop so it never outlives the connection.
    reader: JoinHandle<()>,
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
        let (unrouted_tx, unrouted_rx) = mpsc::unbounded_channel();

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
        }
    }

    /// Perform the `initialize` → `initialized` handshake, returning the
    /// server's `initialize` result. `params` is passed through verbatim.
    pub async fn initialize(&self, params: Value) -> Result<Value> {
        let result = self.request("initialize", Some(params)).await?;
        // The client signals it is ready to receive with an `initialized`
        // notification, mirroring the LSP-style handshake.
        self.notify("initialized", None).await?;
        Ok(result)
    }

    /// Send a request and await its correlated response.
    pub async fn request(&self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .expect("pending mutex poisoned")
            .insert(id, tx);

        let frame = wire::encode_request(id, method, params).map_err(Error::Encode)?;
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
        response.outcome.map_err(|error| Error::Rpc {
            method: method.to_owned(),
            error,
        })
    }

    /// Send a notification (no response is expected).
    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<()> {
        let frame = wire::encode_notification(method, params).map_err(Error::Encode)?;
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
    pub async fn start_thread(&self, params: Option<Value>) -> Result<StartedThread> {
        let result = self.request("thread/start", params).await?;
        let thread_id = result
            .get("threadId")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::UnexpectedResponse {
                method: "thread/start".to_owned(),
                detail: format!("result has no string `threadId`: {result}"),
            })?
            .to_owned();
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
    /// The server may echo the requested thread id back under `threadId`; when
    /// it omits it the requested id (carried in `params.threadId`) is used, so a
    /// lean resume result still yields a usable id.
    pub async fn resume_thread(&self, params: Option<Value>) -> Result<StartedThread> {
        let requested = params
            .as_ref()
            .and_then(|p| p.get("threadId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let result = self.request("thread/resume", params).await?;
        let thread_id = result
            .get("threadId")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or(requested)
            .ok_or_else(|| Error::UnexpectedResponse {
                method: "thread/resume".to_owned(),
                detail: format!("result has no string `threadId` and none was requested: {result}"),
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
    pub async fn respond(&self, id: &Value, result: Value) -> Result<()> {
        let frame = wire::encode_success_response(id, result).map_err(Error::Encode)?;
        self.write_frame(&frame).await
    }

    /// Answer a server-originated request with a JSON-RPC error. Used to reject a
    /// request Delta does not model, so a well-behaved server unblocks its turn
    /// rather than waiting forever for a reply.
    pub async fn respond_error(&self, id: &Value, code: i64, message: &str) -> Result<()> {
        let frame = wire::encode_error_response(id, code, message).map_err(Error::Encode)?;
        self.write_frame(&frame).await
    }

    /// Take the connection-level "unrouted" channel: frames that were not scoped
    /// to any registered thread. Returns `None` if already taken.
    pub fn take_unrouted(&self) -> Option<mpsc::UnboundedReceiver<ThreadEvent>> {
        self.unrouted
            .lock()
            .expect("unrouted mutex poisoned")
            .take()
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
    unrouted: mpsc::UnboundedSender<ThreadEvent>,
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
}

/// Route one parsed frame to its destination.
fn dispatch(
    msg: Incoming,
    pending: &PendingMap,
    threads: &ThreadMap,
    unrouted: &mpsc::UnboundedSender<ThreadEvent>,
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
    unrouted: &mpsc::UnboundedSender<ThreadEvent>,
) {
    let Some(thread_id) = thread_id else {
        let _ = unrouted.send(event);
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
