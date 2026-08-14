//! An in-memory fake [`AgentAdapter`] + [`AgentAdapterFactory`] for the
//! terminal-less (Codex) session-creation use-case tests.
//!
//! The real Codex adapter lives in the `codex-agent` gateway crate, which the
//! domain must not depend on, so the interactor unit tests drive this stand-in
//! instead. It records what it was asked to do (launch/send) and returns a
//! scripted provider thread id + turn id, so a test can assert the persistence
//! and turn-FSM behaviour of a Codex spawn without a real `codex app-server`.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use delta_model::{ContentBlock, Message, MessageUuid, Role, SessionId, ThreadId};
use tokio::sync::mpsc;

use crate::agent::{
    AgentAdapter, AgentAdapterFactory, AgentCapabilities, AgentContentSource, AgentEvent,
    AgentEventStream, AgentProvider, AgentSessionHandle, ContentSourceRequest,
    ContextInjectionCapability, EventCapability, ForkCapability, InterruptCapability,
    LaunchCapability, LaunchRequest, PermissionCapability, PtyHandle, ResumeCapability,
    ResumeRequest, SendReceipt, SendRequest, SessionIdentityCapability, SteerCapability,
    TerminalCapability, TranscriptCapability, TurnStatus,
};
use crate::error::{Error, Result};
use crate::interactor::PermissionDecision;
use crate::Effect;

/// What the fake adapter observed, for a test to inspect after a spawn.
#[derive(Debug, Default, Clone)]
pub(crate) struct FakeAgentLog {
    /// The `LaunchRequest`s the adapter received, in order.
    pub launches: Vec<LaunchRequest>,
    /// The `ResumeRequest`s the adapter received, in order. Their
    /// `provider_session_id` proves a resume reattached to the persisted thread
    /// (not a fresh `launch`); the rest proves the reattach is configured from
    /// the persisted row exactly as the original launch was.
    pub resumes: Vec<ResumeRequest>,
    /// The [`ContentSourceRequest`] each `content_source` call was built from, in
    /// order. Its `seed_seq` proves a fresh spawn seeds `0` and a resume seeds
    /// the persisted count (so resumed history is not renumbered); its `cwd` and
    /// `git_branch` prove the launch site Delta resolved and observed reaches the
    /// accumulator that stamps them onto messages.
    pub content_requests: Vec<ContentSourceRequest>,
    /// The visible send texts the adapter received, in order.
    pub sends: Vec<String>,
    /// The hidden-context texts `inject_context` received, in order. Proves a
    /// branch send delivered the branched-from passage over the trait before its
    /// turn dispatched (the Codex `thread/inject_items` path).
    pub injects: Vec<String>,
    /// The number of `close` calls.
    pub closes: usize,
    /// The number of `interrupt` calls. Proves an interrupt reached the adapter
    /// over the trait.
    pub interrupts: usize,
    /// The `resolve_permission` calls the adapter received: the adapter-scoped
    /// provider token and the decision, in order. Proves a browser decision
    /// reached the adapter over the trait with the correct token/decision.
    pub resolves: Vec<(String, PermissionDecision)>,
}

/// A scripted, in-memory [`AgentAdapter`] standing in for the Codex adapter.
pub(crate) struct FakeAgentAdapter {
    /// The provider thread id the fake mints at launch (the provider session id
    /// + handle key). Session ↔ thread is 1:1, like Codex.
    thread_id: String,
    /// The turn id returned from each `send` as the receipt's provider message
    /// id. `None` reproduces a `turn/start` ack that carried no turn id.
    turn_id: Option<String>,
    log: Arc<Mutex<FakeAgentLog>>,
    /// The sender the test pushes live [`AgentEvent`]s on, drained by the event
    /// pump via [`AgentAdapter::events`]. `resolve_permission` also emits a
    /// `PermissionResolved` here, mirroring the real Codex adapter.
    tx: mpsc::UnboundedSender<AgentEvent>,
    /// The receiver handed out once by [`AgentAdapter::events`].
    rx: Mutex<Option<mpsc::UnboundedReceiver<AgentEvent>>>,
}

impl FakeAgentAdapter {
    fn new(
        thread_id: String,
        turn_id: Option<String>,
        log: Arc<Mutex<FakeAgentLog>>,
        tx: mpsc::UnboundedSender<AgentEvent>,
        rx: mpsc::UnboundedReceiver<AgentEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            thread_id,
            turn_id,
            log,
            tx,
            rx: Mutex::new(Some(rx)),
        })
    }
}

/// The Codex-shaped capability profile both the fake adapter and the fake
/// factory report — one declaration, mirroring how the real gateway's adapter
/// and factory read one `CODEX_CAPABILITIES` const.
const FAKE_AGENT_CAPABILITIES: AgentCapabilities = AgentCapabilities {
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

#[async_trait]
impl AgentAdapter for FakeAgentAdapter {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        FAKE_AGENT_CAPABILITIES
    }

    async fn launch(&self, req: LaunchRequest) -> Result<AgentSessionHandle> {
        self.log.lock().unwrap().launches.push(req);
        Ok(AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: self.thread_id.clone(),
            key: self.thread_id.clone(),
        })
    }

    async fn resume(&self, req: ResumeRequest) -> Result<AgentSessionHandle> {
        // Record the whole request, so a test can prove the reconnect reattached
        // to the persisted thread rather than launching a fresh one — and that
        // it was configured from the persisted row the way the launch was.
        let provider_session_id = req.provider_session_id.clone();
        self.log.lock().unwrap().resumes.push(req);
        Ok(AgentSessionHandle {
            provider: AgentProvider::Codex,
            provider_session_id: provider_session_id.clone(),
            key: provider_session_id,
        })
    }

    async fn send(&self, _handle: &AgentSessionHandle, req: SendRequest) -> Result<SendReceipt> {
        self.log.lock().unwrap().sends.push(req.text);
        Ok(SendReceipt {
            provider_message_id: self.turn_id.clone(),
        })
    }

    async fn inject_context(&self, _handle: &AgentSessionHandle, text: &str) -> Result<()> {
        // Record the injected passage so a branch test can prove the hidden
        // context reached the adapter over the trait (the real Codex adapter
        // sends it as `thread/inject_items`).
        self.log.lock().unwrap().injects.push(text.to_owned());
        Ok(())
    }

    async fn interrupt(&self, _handle: &AgentSessionHandle) -> Result<()> {
        self.log.lock().unwrap().interrupts += 1;
        // Mirror the real Codex path: `turn/interrupt` makes the provider end the
        // in-flight turn with an interrupted completion. The real adapter's
        // interrupt only sends the RPC; the provider (fake-codex) emits the
        // `turn/completed{interrupted}`. Here the adapter and server are
        // collapsed, so emit the terminal turn event on the stream — as
        // `resolve_permission` emits `PermissionResolved` — so the session's
        // event pump drives the turn machine to `TurnInterrupted`.
        let _ = self.tx.send(AgentEvent::TurnCompleted {
            status: TurnStatus::Interrupted,
        });
        Ok(())
    }

    async fn close(&self, _handle: &AgentSessionHandle) -> Result<()> {
        self.log.lock().unwrap().closes += 1;
        Ok(())
    }

    fn events(&self, _handle: &AgentSessionHandle) -> AgentEventStream {
        // Hand out the live receiver the test pushes events on (once); a second
        // request yields an already-closed stream, matching the real adapter.
        match self.rx.lock().unwrap().take() {
            Some(rx) => AgentEventStream::new(rx),
            None => {
                let (_tx, rx) = mpsc::unbounded_channel();
                AgentEventStream::new(rx)
            }
        }
    }

    async fn resolve_permission(
        &self,
        _handle: &AgentSessionHandle,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<()> {
        self.log
            .lock()
            .unwrap()
            .resolves
            .push((request_id.to_owned(), decision));
        // Mirror the real Codex adapter: emit the resolution on the stream so the
        // event pump settles the runtime mirror and browser notice.
        let _ = self.tx.send(AgentEvent::PermissionResolved {
            request_id: request_id.to_owned(),
            decision,
        });
        Ok(())
    }

    fn content_source(
        &self,
        _handle: &AgentSessionHandle,
        req: ContentSourceRequest,
    ) -> Box<dyn AgentContentSource> {
        // Record the request so a test can assert the seed (a fresh spawn seeds 0,
        // a resume the persisted count) and the launch site it carries. Return a
        // functional accumulator (unlike the trait's `NullContentSource` default)
        // so pushed user/assistant events actually persist as messages and
        // `message_count` advances — which is what makes the resume seed
        // observable end to end.
        self.log.lock().unwrap().content_requests.push(req.clone());
        Box::new(FakeContentSource {
            session_id: req.session_id,
            main_thread: req.main_thread,
            next_seq: req.seed_seq,
            cwd: req.cwd,
            git_branch: req.git_branch,
        })
    }

    async fn attach_terminal(&self, _handle: &AgentSessionHandle) -> Result<Option<PtyHandle>> {
        Ok(None)
    }
}

/// A minimal functional [`AgentContentSource`] for the Codex actor tests: folds
/// `UserPromptAccepted` / `AssistantMessage` into one canonical [`Message`] each,
/// minting `seq` from `next_seq` (seeded by the adapter's `content_source`) and
/// stamping the session's launch site the way the real accumulator does, so a
/// test can drive real conversation content through the event pump and assert
/// persistence + sequencing. Everything else the real accumulator handles (tool
/// pairing, turn grouping) is out of scope here.
#[derive(Debug)]
struct FakeContentSource {
    session_id: SessionId,
    main_thread: ThreadId,
    next_seq: i64,
    /// The session's launch directory, stamped on every message (as the real
    /// accumulator does) so the plumbing is observable on persisted rows.
    cwd: String,
    /// The branch observed in [`Self::cwd`] at bind time, stamped alongside it.
    git_branch: Option<String>,
}

impl AgentContentSource for FakeContentSource {
    fn ingest(&mut self, event: &AgentEvent) -> (Vec<Message>, Vec<Effect>) {
        // Key each uuid so successive turns never collide: the user prompt off
        // its text, the assistant message off the provider item id — mirroring
        // how the real accumulator derives stable, per-item uuids.
        let built = match event {
            AgentEvent::UserPromptAccepted { text, .. } => {
                Some((Role::User, text.clone(), format!("fake-user-{text}"), None))
            }
            AgentEvent::AssistantMessage {
                provider_item_id,
                text,
                ..
            } => Some((
                Role::Assistant,
                text.clone(),
                format!("codex-item-{provider_item_id}"),
                Some(provider_item_id.clone()),
            )),
            _ => return (Vec::new(), Vec::new()),
        };
        let (role, text, uuid, provider_item_id) = built.expect("matched arm builds a message");
        let seq = self.next_seq;
        self.next_seq += 1;
        let message = Message {
            uuid: MessageUuid::from(uuid),
            provider_item_id,
            session_id: self.session_id.clone(),
            thread_id: self.main_thread,
            role,
            linear_parent_uuid: None,
            semantic_parent_uuid: None,
            prompt_id: None,
            seq,
            content_text: Some(text.clone()),
            content: vec![ContentBlock::Text { text }],
            created_at: None,
            // A real adapter also stamps what its provider reported (the
            // model); the fake reports none, so only the launch site is here.
            model: None,
            git_branch: self.git_branch.clone(),
            cwd: Some(self.cwd.clone()),
            response_time_ms: None,
        };
        (vec![message], Vec::new())
    }
}

/// How the fake factory behaves when `connect` is called.
enum ConnectOutcome {
    /// Build the scripted adapter.
    Adapter {
        thread_id: String,
        turn_id: Option<String>,
    },
    /// Fail the connection (e.g. Codex not installed), for rollback tests.
    Fail,
}

/// A [`AgentAdapterFactory`] that hands out a [`FakeAgentAdapter`] (or fails).
///
/// It is **reconnectable**: each `connect` builds a fresh adapter, so a test can
/// simulate a server restart (drop the in-process binding, then send again → the
/// resume path calls `connect` a second time). The event channel driving the
/// live adapter is regenerated per reconnect, and [`Self::event_sender`] always
/// returns the current adapter's sender (usable before the first connect too, so
/// existing single-connect tests keep working regardless of call order).
pub(crate) struct FakeAgentFactory {
    outcome: ConnectOutcome,
    log: Arc<Mutex<FakeAgentLog>>,
    /// The sender the currently-built adapter drains through `events()`; a test
    /// pushes live [`AgentEvent`]s here to drive the session's event pump. Swapped
    /// to a fresh channel on each reconnect.
    event_tx: Mutex<mpsc::UnboundedSender<AgentEvent>>,
    /// The receiver for the NEXT `connect`. `Some` before the first connect (so
    /// the pre-made channel is reused, matching the original single-connect
    /// behaviour); `None` afterwards, which makes each reconnect mint a fresh
    /// channel.
    pending_rx: Mutex<Option<mpsc::UnboundedReceiver<AgentEvent>>>,
}

impl FakeAgentFactory {
    /// A factory whose `connect` yields an adapter minting `thread_id` and
    /// returning `turn_id` from each send.
    pub(crate) fn new(thread_id: impl Into<String>, turn_id: Option<&str>) -> Arc<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            outcome: ConnectOutcome::Adapter {
                thread_id: thread_id.into(),
                turn_id: turn_id.map(str::to_owned),
            },
            log: Arc::new(Mutex::new(FakeAgentLog::default())),
            event_tx: Mutex::new(event_tx),
            pending_rx: Mutex::new(Some(event_rx)),
        })
    }

    /// A factory whose `connect` fails, so a caller must roll back cleanly.
    pub(crate) fn failing() -> Arc<Self> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Arc::new(Self {
            outcome: ConnectOutcome::Fail,
            log: Arc::new(Mutex::new(FakeAgentLog::default())),
            event_tx: Mutex::new(event_tx),
            pending_rx: Mutex::new(Some(event_rx)),
        })
    }

    /// The observation log the built adapter writes to. For the failing factory
    /// this stays empty (no adapter is built).
    pub(crate) fn log(&self) -> Arc<Mutex<FakeAgentLog>> {
        Arc::clone(&self.log)
    }

    /// A sender that pushes live [`AgentEvent`]s onto the currently-built
    /// adapter's event stream, so a test can drive the session's event pump (e.g.
    /// surface a `PermissionRequested`, or feed a turn's content). After a
    /// reconnect this returns the reconnected adapter's sender.
    pub(crate) fn event_sender(&self) -> mpsc::UnboundedSender<AgentEvent> {
        self.event_tx.lock().unwrap().clone()
    }
}

#[async_trait]
impl AgentAdapterFactory for FakeAgentFactory {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Codex
    }

    fn capabilities(&self) -> AgentCapabilities {
        FAKE_AGENT_CAPABILITIES
    }

    async fn connect(&self) -> Result<Arc<dyn AgentAdapter>> {
        match &self.outcome {
            ConnectOutcome::Adapter { thread_id, turn_id } => {
                // First connect: reuse the pre-made channel (so `event_sender()`
                // called before connect drives this adapter). Reconnect: mint a
                // fresh channel and publish its sender so `event_sender()` now
                // targets the reconnected adapter.
                let rx = match self.pending_rx.lock().unwrap().take() {
                    Some(rx) => rx,
                    None => {
                        let (tx, rx) = mpsc::unbounded_channel();
                        *self.event_tx.lock().unwrap() = tx;
                        rx
                    }
                };
                // Share the factory's log with the adapter, so `factory.log()`
                // reflects the built adapter's live observations.
                let adapter = FakeAgentAdapter::new(
                    thread_id.clone(),
                    turn_id.clone(),
                    Arc::clone(&self.log),
                    self.event_tx.lock().unwrap().clone(),
                    rx,
                );
                Ok(adapter as Arc<dyn AgentAdapter>)
            }
            ConnectOutcome::Fail => Err(Error::Agent(
                "fake codex connect failed (provider unavailable)".to_owned(),
            )),
        }
    }
}
