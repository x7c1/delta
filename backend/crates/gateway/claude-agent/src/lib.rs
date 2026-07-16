//! [`ClaudeCodePtyHookAdapter`]: the [`AgentAdapter`] for Claude Code.
//!
//! This adapter represents how Delta drives Claude Code today — a `claude`
//! process launched in a tmux pane, prompts typed in via `send-keys`, the
//! interrupt key injected the same way, and the pane killed on close. It is the
//! provider-specific *driving* side of the abstraction: the mechanical
//! operations (launch / send / interrupt / close / attach) expressed behind the
//! neutral [`AgentAdapter`] trait.
//!
//! ## Scope in this phase
//!
//! The adapter changes the *dependency direction* — the core will drive Claude
//! through this trait rather than reaching for a tmux driver directly — without
//! changing behaviour. It wraps the existing [`TmuxDriver`] port and reuses the
//! existing [`PaneTokenMinter`], so nothing about how Claude is launched or
//! driven differs.
//!
//! The event-source side of Claude — HTTP hooks and the JSONL transcript tail —
//! is fed in through the ingestion seam below rather than pushed by Claude. The
//! events the adapter produces from the operations it performs directly are
//! [`AgentEvent::SessionStarted`] at launch/resume,
//! [`AgentEvent::UserPromptAccepted`] on send, and [`AgentEvent::SessionEnded`]
//! on close; the hook/transcript-derived events arrive through the seam.
//!
//! The claude launch flags are mirrored here from the core's spawn path; the
//! two converge onto this adapter when the spawn path is rerouted through it.
//!
//! ## Ingestion seam
//!
//! [`ClaudeCodePtyHookAdapter::ingest_hook`] and
//! [`ClaudeCodePtyHookAdapter::ingest_transcript_lines`] are the entry points
//! that feed Claude's lossy wire input (hook payloads, transcript lines) into
//! the neutral [`AgentEvent`] projection on this session's [`events`] stream.
//! The parsing lives in the [`ingest`] module. This phase wires the
//! **permission** projection (the `PermissionRequest` hook and its correlated
//! `tool_result`) and the **turn-lifecycle** projection (the `UserPromptSubmit`
//! echo, the `Stop` hook, and the `[Request interrupted by user…]` transcript
//! marker); the tool projections join them as the seam grows. See that module
//! for the intended growth.
//!
//! [`events`]: AgentAdapter::events

mod ingest;

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

pub use ingest::ClaudeHook;
use ingest::{
    hook_needs_request_id, project_hook, project_interrupt_marker, project_transcript_line,
    OpenPermission,
};

use delta_usecase::{
    pane_for, AgentAdapter, AgentCapabilities, AgentEvent, AgentEventStream, AgentProvider,
    AgentSessionHandle, ContextInjectionCapability, EventCapability, ForkCapability,
    InterruptCapability, LaunchCapability, LaunchRequest, PaneTokenMinter, PermissionCapability,
    PtyHandle, Result, ResumeCapability, ResumeRequest, SendReceipt, SendRequest, SessionEndReason,
    SessionIdentityCapability, SteerCapability, TerminalCapability, TmuxDriver,
    TranscriptCapability,
};

/// The `claude` flag that loads Delta's session settings (hooks + theme) from a
/// Delta-owned file. Mirrors the core's spawn path.
const SETTINGS_FLAG: &str = "--settings";
/// The `claude` flag that pins a fresh conversation's `session_id` to the value
/// Delta mints up front.
const SESSION_ID_FLAG: &str = "--session-id";
/// The `claude` flag that reattaches to a stored conversation on resume.
const RESUME_FLAG: &str = "--resume";
/// The tmux key name Claude's TUI treats as "interrupt the in-flight turn".
const INTERRUPT_KEY: &str = "Escape";

/// How the adapter launches `claude`.
#[derive(Debug, Clone)]
pub struct ClaudeLaunchConfig {
    /// The program launched in each tmux session (`claude` by default). Used
    /// verbatim as argv[0], so it may be a bare name resolved via `PATH` or an
    /// absolute path — matching the core's `LaunchConfig::claude_bin`.
    pub claude_bin: String,
    /// Path to the Delta-owned session settings file passed via `--settings`.
    /// The file is written by the composition root (it is shared across
    /// sessions); the adapter only references its path.
    pub settings_path: String,
}

/// Per-session event plumbing: the sender the adapter emits through, the
/// receiver handed out (once) by [`AgentAdapter::events`], and the projection
/// state the ingestion seam keeps for this session.
struct SessionChannel {
    tx: UnboundedSender<AgentEvent>,
    rx: Option<UnboundedReceiver<AgentEvent>>,
    /// The permission dialog currently projected and not yet resolved, if any.
    /// Claude shows one at a time, so at most one exists.
    open_permission: Option<OpenPermission>,
    /// Monotonic per-session counter minting the ids the projected permission
    /// events are keyed by (Claude's `PermissionRequest` hook carries none).
    permission_seq: u64,
}

/// The [`AgentAdapter`] for Claude Code (tmux PTY + hooks + transcript).
///
/// Generic over the [`TmuxDriver`] so production wires the real tmux driver and
/// tests inject a recording fake.
pub struct ClaudeCodePtyHookAdapter<T: TmuxDriver> {
    tmux: T,
    config: ClaudeLaunchConfig,
    minter: PaneTokenMinter,
    /// Per-session event channels, keyed by the session's tmux pane token
    /// (which is [`AgentSessionHandle::key`]).
    sessions: Mutex<HashMap<String, SessionChannel>>,
}

impl<T: TmuxDriver> ClaudeCodePtyHookAdapter<T> {
    /// Construct the adapter over a tmux driver and launch configuration.
    pub fn new(tmux: T, config: ClaudeLaunchConfig) -> Self {
        Self {
            tmux,
            config,
            minter: PaneTokenMinter::new(),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Register a fresh event channel for `key` and emit the opening
    /// [`AgentEvent::SessionStarted`]. Returns the session handle.
    ///
    /// Called after the tmux session is live, so a launch that failed to spawn
    /// never leaves a dangling channel behind.
    fn open_session(&self, key: String, provider_session_id: String) -> AgentSessionHandle {
        let (tx, rx) = mpsc::unbounded_channel();
        // The receiver is buffered, so emitting before the consumer calls
        // `events()` never drops the opening event.
        let _ = tx.send(AgentEvent::SessionStarted {
            provider_session_id: provider_session_id.clone(),
        });
        self.sessions
            .lock()
            .expect("sessions mutex poisoned")
            .insert(
                key.clone(),
                SessionChannel {
                    tx,
                    rx: Some(rx),
                    open_permission: None,
                    permission_seq: 0,
                },
            );
        AgentSessionHandle {
            provider: AgentProvider::Claude,
            provider_session_id,
            key,
        }
    }

    /// Emit an event on `key`'s channel, if the session is still known. A send
    /// on a dropped receiver is ignored — the event stream is best-effort once
    /// the consumer has gone away.
    fn emit(&self, key: &str, event: AgentEvent) {
        if let Some(channel) = self
            .sessions
            .lock()
            .expect("sessions mutex poisoned")
            .get(key)
        {
            let _ = channel.tx.send(event);
        }
    }

    /// Ingest a Claude hook payload for `handle`'s session, projecting the
    /// [`AgentEvent`] it implies onto the session's [`events`] stream.
    ///
    /// This is one half of the ingestion seam. Parsing lives in [`ingest`]; the
    /// adapter only mints the correlation id and tracks the open dialog. For an
    /// unknown session (the stream was never opened) this is a no-op.
    ///
    /// Errors only on a malformed payload; a well-formed hook is always
    /// projected. This phase recognises the `PermissionRequest`,
    /// `UserPromptSubmit`, and `Stop` hooks.
    ///
    /// [`events`]: AgentAdapter::events
    pub fn ingest_hook(
        &self,
        handle: &AgentSessionHandle,
        payload_json: &str,
    ) -> serde_json::Result<()> {
        let hook: ClaudeHook = serde_json::from_str(payload_json)?;
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        if let Some(channel) = sessions.get_mut(&handle.key) {
            // Mint a correlation id only for the permission request — the one
            // hook whose event is keyed by an id Claude's payload omits — so a
            // turn hook never burns a sequence number.
            let request_id = if hook_needs_request_id(&hook) {
                channel.permission_seq += 1;
                format!("{}-perm-{}", handle.key, channel.permission_seq)
            } else {
                String::new()
            };
            let (event, open) = project_hook(hook, request_id);
            // A permission request opens a dialog to track; the turn hooks open
            // none, and must not clear one already open.
            if open.is_some() {
                channel.open_permission = open;
            }
            let _ = channel.tx.send(event);
        }
        Ok(())
    }

    /// Ingest transcript lines for `handle`'s session, projecting any
    /// [`AgentEvent`] they imply onto the session's [`events`] stream.
    ///
    /// The other half of the ingestion seam. Lines that carry no projected
    /// meaning (blank, non-JSON, or records this phase does not model) are
    /// skipped, so a whole transcript tail can be fed safely. This phase
    /// resolves the open permission dialog when its correlated `tool_result`
    /// arrives, and projects the `[Request interrupted by user…]` marker as a
    /// turn-ending [`AgentEvent::TurnCompleted`]. For an unknown session this is
    /// a no-op.
    ///
    /// [`events`]: AgentAdapter::events
    pub fn ingest_transcript_lines(&self, handle: &AgentSessionHandle, lines: &[String]) {
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        let Some(channel) = sessions.get_mut(&handle.key) else {
            return;
        };
        for line in lines {
            // A `tool_result` resolving the open permission dialog takes the
            // line first: a resolution consumes the dialog and moves on.
            if let Some(open) = channel.open_permission.as_ref() {
                if let Some(event) = project_transcript_line(line, open) {
                    channel.open_permission = None;
                    let _ = channel.tx.send(event);
                    continue;
                }
            }
            // Otherwise the line may be the turn-ending interrupt marker.
            if let Some(event) = project_interrupt_marker(line) {
                let _ = channel.tx.send(event);
            }
        }
    }
}

#[async_trait]
impl<T: TmuxDriver> AgentAdapter for ClaudeCodePtyHookAdapter<T> {
    fn provider(&self) -> AgentProvider {
        AgentProvider::Claude
    }

    fn capabilities(&self) -> AgentCapabilities {
        // Today's Claude reality: a tmux-hosted PTY, a Delta-pinned session id,
        // resume via `--resume`, events reconstructed from hooks + the JSONL
        // transcript, hook-carried permission decisions, hidden per-turn
        // context via the `UserPromptSubmit` hook, interrupt by injecting the
        // `Escape` keystroke, and an attachable pane. Fork/steer are unused in
        // v1.
        AgentCapabilities {
            launch: LaunchCapability::PtyCommand,
            session_identity: SessionIdentityCapability::DeltaCanSetId,
            resume: ResumeCapability::Supported,
            events: EventCapability::HookAndTranscript,
            transcript: TranscriptCapability::JsonlFile,
            permission: PermissionCapability::HookDecision,
            context_injection: ContextInjectionCapability::HiddenPerTurn,
            interrupt: InterruptCapability::PaneKeystroke,
            terminal: TerminalCapability::AttachablePty,
            fork: ForkCapability::None,
            steer: SteerCapability::None,
        }
    }

    async fn launch(&self, req: LaunchRequest) -> Result<AgentSessionHandle> {
        let token = self.minter.mint();
        // Claude pins the Delta-minted id via `--session-id`, so the provider
        // session id IS that id.
        let mut command = vec![
            self.config.claude_bin.clone(),
            SETTINGS_FLAG.to_owned(),
            self.config.settings_path.clone(),
            SESSION_ID_FLAG.to_owned(),
            req.session_id.clone(),
        ];
        command.extend(req.extra_args);
        // A first prompt rides the launch command line as a trailing positional
        // argument, which `claude` auto-submits at startup (matching the core's
        // spawn path).
        if let Some(prompt) = req.first_prompt {
            command.push(prompt);
        }
        self.tmux
            .create_session(token.as_str(), &req.workdir, &command)
            .await?;
        Ok(self.open_session(token.as_str().to_owned(), req.session_id))
    }

    async fn resume(&self, req: ResumeRequest) -> Result<AgentSessionHandle> {
        let token = self.minter.mint();
        let command = vec![
            self.config.claude_bin.clone(),
            SETTINGS_FLAG.to_owned(),
            self.config.settings_path.clone(),
            RESUME_FLAG.to_owned(),
            req.provider_session_id.clone(),
        ];
        self.tmux
            .create_session(token.as_str(), &req.workdir, &command)
            .await?;
        Ok(self.open_session(token.as_str().to_owned(), req.provider_session_id))
    }

    async fn send(&self, handle: &AgentSessionHandle, req: SendRequest) -> Result<SendReceipt> {
        let pane = pane_for(&handle.key);
        // Only the visible prompt text is typed into the pane; Claude's hidden
        // per-turn context arrives via the `UserPromptSubmit` hook, never here.
        self.tmux.send_line(&pane, &req.text).await?;
        self.emit(
            &handle.key,
            AgentEvent::UserPromptAccepted {
                provider_message_id: None,
                text: req.text,
            },
        );
        Ok(SendReceipt {
            provider_message_id: None,
        })
    }

    async fn interrupt(&self, handle: &AgentSessionHandle) -> Result<()> {
        let pane = pane_for(&handle.key);
        // Claude's TUI interrupts the in-flight turn on a single `Escape`.
        self.tmux.send_keys(&pane, &[INTERRUPT_KEY]).await
    }

    async fn close(&self, handle: &AgentSessionHandle) -> Result<()> {
        self.tmux.kill_session(&handle.key).await?;
        self.emit(
            &handle.key,
            AgentEvent::SessionEnded {
                reason: SessionEndReason::Closed,
            },
        );
        Ok(())
    }

    fn events(&self, handle: &AgentSessionHandle) -> AgentEventStream {
        let mut sessions = self.sessions.lock().expect("sessions mutex poisoned");
        match sessions.get_mut(&handle.key).and_then(|c| c.rx.take()) {
            Some(rx) => AgentEventStream::new(rx),
            None => {
                // Already handed out (or unknown session): return an
                // already-closed stream rather than panicking.
                let (_tx, rx) = mpsc::unbounded_channel();
                AgentEventStream::new(rx)
            }
        }
    }

    async fn attach_terminal(&self, handle: &AgentSessionHandle) -> Result<Option<PtyHandle>> {
        Ok(Some(PtyHandle {
            target: pane_for(&handle.key),
        }))
    }
}

#[cfg(test)]
mod contract;
