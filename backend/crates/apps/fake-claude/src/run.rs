//! The engine: wire the launch surfaces together and execute the scenario.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use delta_wire::hooks::{
    MessageDisplayPayload, PermissionRequestPayload, PostToolUsePayload, PreToolUsePayload,
    SessionStartPayload, StopPayload, UserPromptSubmitPayload,
};
use serde_json::{json, Value};

use crate::args::Args;
use crate::hooks::post_json;
use crate::input::{self, InputEvent};
use crate::scenario::{Scenario, SessionStartMode, Step};
use crate::settings::HookEndpoints;
use crate::transcript::TranscriptWriter;

/// Parse the launch, resolve the scenario, and run it to completion.
pub fn run() -> Result<(), String> {
    let args = Args::parse(std::env::args().skip(1));
    let session_id = args
        .effective_session_id()
        .ok_or("launched without --session-id or --resume")?
        .to_owned();
    let settings_path = args
        .settings
        .as_deref()
        .ok_or("launched without --settings")?;
    let settings: Value = serde_json::from_str(
        &std::fs::read_to_string(settings_path)
            .map_err(|e| format!("read settings {settings_path}: {e}"))?,
    )
    .map_err(|e| format!("parse settings {settings_path}: {e}"))?;
    let endpoints = HookEndpoints::from_settings(&settings)?;

    let cwd = std::env::current_dir()
        .map_err(|e| format!("read cwd: {e}"))?
        .to_string_lossy()
        .into_owned();

    let transcript_path = transcript_path_for(&session_id)?;
    let is_resume = args.resume.is_some();
    if is_resume && !transcript_path.exists() {
        // `claude --resume` replays the stored transcript; an unknown id is a
        // startup failure, which the fake mirrors by exiting non-zero.
        return Err(format!(
            "cannot resume {session_id}: no transcript at {}",
            transcript_path.display()
        ));
    }

    let scenario = Scenario::resolve(args.prompt.as_deref())?;

    // A minimal "TUI": the pane shows what this fake is and which session it
    // plays, so an attached human (or a captured pane in CI) can tell what is
    // running. The identifying line comes LAST: a terminal always keeps the
    // cursor row in view, so whatever is printed last stays visible no matter
    // how small the attached client's viewport is — the long transcript path
    // above it may wrap and scroll off. Tests watching for the attach key on
    // this ordering.
    println!("transcript: {}", transcript_path.display());
    println!("fake-claude session {session_id}");

    input::enable_raw_mode();
    let events = input::spawn_reader();

    let mut engine = Engine {
        session_id: session_id.clone(),
        cwd,
        transcript_path: transcript_path.to_string_lossy().into_owned(),
        transcript: TranscriptWriter::open(&transcript_path, &session_id)?,
        endpoints,
        events,
        pending_prompt: args.prompt,
        queued_prompts: VecDeque::new(),
        last_tool_use: None,
        tool_use_seq: 0,
        message_id_seq: 0,
        last_additional_context: String::new(),
    };

    let source = if is_resume { "resume" } else { "startup" };
    match &scenario.session_start {
        SessionStartMode::Named(mode) if mode == "immediate" => engine.fire_session_start(source),
        SessionStartMode::Named(mode) if mode == "skip" => {}
        SessionStartMode::Named(mode) => {
            return Err(format!("unknown session_start mode: {mode}"));
        }
        SessionStartMode::Delayed { delay_ms } => {
            std::thread::sleep(Duration::from_millis(*delay_ms));
            engine.fire_session_start(source);
        }
    }

    loop {
        for step in &scenario.steps {
            engine.execute(step)?;
        }
        if !scenario.looped {
            break;
        }
    }

    // The script is done but the session is not: a real `claude` sits at its
    // prompt until the pane is killed. Park so tmux does not see an exit (which
    // would end the pane and look like a crashed session).
    eprintln!("fake-claude: scenario complete; idling");
    loop {
        std::thread::park();
    }
}

/// Where this session's transcript lives: `<dir>/<session-id>.jsonl` under
/// `FAKE_CLAUDE_TRANSCRIPT_DIR` (or a fixed temp-dir fallback). Deterministic
/// per session id so a resume finds the transcript the fresh run wrote.
fn transcript_path_for(session_id: &str) -> Result<PathBuf, String> {
    let dir = match std::env::var("FAKE_CLAUDE_TRANSCRIPT_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => std::env::temp_dir().join("fake-claude-transcripts"),
    };
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create transcript dir {}: {e}", dir.display()))?;
    Ok(dir.join(format!("{session_id}.jsonl")))
}

/// The most recent `tool_use`, kept so `permission_request` and `tool_result`
/// steps can refer to it without restating the call.
struct ToolUse {
    id: String,
    name: String,
    input: Value,
}

struct Engine {
    session_id: String,
    cwd: String,
    transcript_path: String,
    transcript: TranscriptWriter,
    endpoints: HookEndpoints,
    events: Receiver<InputEvent>,
    /// The launch's positional prompt, consumed by the first `await_prompt` —
    /// mirroring how `claude` auto-submits a positional prompt at startup.
    pending_prompt: Option<String>,
    /// Prompts the scenario enqueued mid-turn (`enqueue_prompt`), awaiting
    /// their `dequeue_prompt` replay — mirroring claude's prompt queue.
    queued_prompts: VecDeque<String>,
    last_tool_use: Option<ToolUse>,
    tool_use_seq: usize,
    /// A fresh display-message id per `stream_text` step, mirroring how the real
    /// `claude` stamps one `message_id` across a streamed message's chunks.
    message_id_seq: usize,
    /// The `additionalContext` the most recent `UserPromptSubmit` hook response
    /// injected (empty when none): the real `claude` folds it into the model
    /// prompt, so the fake records it and exposes it to `reply` steps via the
    /// `{additional_context}` placeholder — letting a test observe, end to end,
    /// exactly what context the server delivered.
    last_additional_context: String,
}

impl Engine {
    fn execute(&mut self, step: &Step) -> Result<(), String> {
        match step {
            Step::AwaitPrompt => {
                let prompt = self.next_prompt()?;
                self.submit_prompt(&prompt, false)
            }
            Step::Reply { text, thinking } => {
                // `{additional_context}` substitutes the context the most
                // recent `UserPromptSubmit` response injected, so a scenario
                // can surface it in the visible conversation for assertions.
                let text = text.replace("{additional_context}", &self.last_additional_context);
                let mut blocks = Vec::new();
                if let Some(thinking) = thinking {
                    blocks.push(json!({ "type": "thinking", "thinking": thinking }));
                }
                blocks.push(json!({ "type": "text", "text": text }));
                self.transcript.assistant_blocks(blocks)
            }
            Step::StreamText { deltas } => {
                // Stream the visible assistant text live via `MessageDisplay`,
                // before any transcript line lands — exactly the order the real
                // `claude` delivers it. One `message_id` spans the message; the
                // chunks carry increasing `index` and only the last is `final`.
                let message_id = format!("msg_fake_{:04}", self.message_id_seq);
                self.message_id_seq += 1;
                let last = deltas.len().saturating_sub(1);
                for (index, delta) in deltas.iter().enumerate() {
                    self.fire(
                        "MessageDisplay",
                        &self.endpoints.message_display,
                        &MessageDisplayPayload {
                            session_id: self.session_id.clone(),
                            message_id: message_id.clone(),
                            index: index as u32,
                            r#final: index == last,
                            delta: delta.clone(),
                            turn_id: Some(message_id.clone()),
                        },
                    );
                }
                Ok(())
            }
            Step::ToolUse { name, input } => {
                let id = format!("toolu_fake_{:04}", self.tool_use_seq);
                self.tool_use_seq += 1;
                self.transcript.assistant_blocks(vec![json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": input,
                })])?;
                self.fire(
                    "PreToolUse",
                    &self.endpoints.pre_tool_use,
                    &PreToolUsePayload {
                        session_id: self.session_id.clone(),
                        tool_name: name.clone(),
                        tool_input: input.clone(),
                        tool_use_id: id.clone(),
                    },
                );
                self.last_tool_use = Some(ToolUse {
                    id,
                    name: name.clone(),
                    input: input.clone(),
                });
                Ok(())
            }
            Step::PostToolUse => {
                // Signal that the most recent tool call completed, mirroring the
                // real `claude` PostToolUse hook. Used to close a subagent's
                // running window without writing a `tool_result`.
                let tool_use = self
                    .last_tool_use
                    .as_ref()
                    .ok_or("post_tool_use step without a preceding tool_use")?;
                self.fire(
                    "PostToolUse",
                    &self.endpoints.post_tool_use,
                    &PostToolUsePayload {
                        session_id: self.session_id.clone(),
                        tool_name: tool_use.name.clone(),
                        tool_use_id: tool_use.id.clone(),
                    },
                );
                Ok(())
            }
            Step::PermissionRequest { on_allow, on_deny } => {
                let tool_use = self
                    .last_tool_use
                    .as_ref()
                    .ok_or("permission_request step without a preceding tool_use")?;
                let payload = PermissionRequestPayload {
                    session_id: self.session_id.clone(),
                    tool_name: tool_use.name.clone(),
                    tool_input: tool_use.input.clone(),
                };
                // `fire` reads the whole response before returning, so this
                // BLOCKS until the server's permission hook responds — exactly
                // like the real `claude` awaiting its permission hook. The
                // server holds the response until a browser decision or its
                // decision deadline (env-shrunk under e2e, well inside the
                // socket read timeout in hooks.rs); the body then either
                // carries `hookSpecificOutput.decision.behavior` or is the
                // empty passthrough.
                let body = self.fire(
                    "PermissionRequest",
                    &self.endpoints.permission_request,
                    &payload,
                );
                let behavior = body
                    .as_deref()
                    .and_then(|b| serde_json::from_str::<Value>(b).ok())
                    .and_then(|v| {
                        v.pointer("/hookSpecificOutput/decision/behavior")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    });
                let branch = match behavior.as_deref() {
                    Some("allow") => on_allow,
                    Some("deny") => on_deny,
                    Some(other) => {
                        return Err(format!("unknown permission decision behavior: {other}"))
                    }
                    // Empty passthrough: no decision was made in the browser,
                    // so the dialog stays with the TUI — the scenario's
                    // following steps script that path.
                    None => return Ok(()),
                };
                for step in branch {
                    self.execute(step)?;
                }
                Ok(())
            }
            Step::ToolResult { is_error } => {
                let id = self
                    .last_tool_use
                    .as_ref()
                    .map(|t| t.id.clone())
                    .ok_or("tool_result step without a preceding tool_use")?;
                self.transcript.tool_result(&id, *is_error)
            }
            Step::TaskNotification => {
                // The harness-injected completion line for a background tool
                // call: a `<task-notification>` user line correlating back to the
                // launching `tool_use_id`. The server folds it and finishes the
                // background subagent's running window.
                let id = self
                    .last_tool_use
                    .as_ref()
                    .map(|t| t.id.clone())
                    .ok_or("task_notification step without a preceding tool_use")?;
                self.transcript.task_notification(&id)
            }
            Step::Stop { stop_reason } => {
                self.fire(
                    "Stop",
                    &self.endpoints.stop,
                    &StopPayload {
                        session_id: self.session_id.clone(),
                        stop_reason: stop_reason.clone(),
                    },
                );
                Ok(())
            }
            Step::AwaitInterrupt => {
                self.await_escape()?;
                // The marker line — and deliberately NO `Stop` hook, exactly
                // like a real interrupt: the transcript tail is what tells the
                // server the turn was aborted.
                self.transcript.interrupt_marker()
            }
            Step::AwaitEscape => {
                // Block until Escape, writing nothing: the cancel's effect (an
                // `is_error` tool_result) is the scenario's next step. This
                // models cancelling an AskUserQuestion, where a single Escape
                // cancels the call and the TUI then writes the error result.
                self.await_escape()
            }
            Step::EnqueuePrompt { text } => {
                // A prompt submitted while the turn is busy: claude records
                // only the uuid-less `queue-operation` enqueue line now (no
                // hook fires) and replays the prompt at dequeue.
                self.queued_prompts.push_back(text.clone());
                self.transcript.queue_operation_enqueue(text)
            }
            Step::DequeuePrompt => {
                let prompt = self
                    .queued_prompts
                    .pop_front()
                    .ok_or("dequeue_prompt step without a pending enqueue_prompt")?;
                // The dequeued prompt flows the same path as a TUI-typed one:
                // its own `UserPromptSubmit`, then a plain user line (stamped
                // `promptSource: "queued"`).
                self.submit_prompt(&prompt, true)
            }
            Step::Delay { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
            Step::Hang => loop {
                std::thread::park();
            },
        }
    }

    /// The submit sequence a fresh prompt and a dequeued replay share: fire
    /// `UserPromptSubmit` first, then write the user transcript line — the
    /// real `claude` fires the hook before the line lands in the JSONL (the
    /// server is built to tolerate — and expects — that order). Records any
    /// injected `additionalContext` from the hook response, like the real
    /// `claude` consuming it; exposed to `reply` steps via the
    /// `{additional_context}` placeholder.
    fn submit_prompt(&mut self, prompt: &str, dequeued: bool) -> Result<(), String> {
        let body = self.fire(
            "UserPromptSubmit",
            &self.endpoints.user_prompt_submit,
            &UserPromptSubmitPayload {
                prompt: prompt.to_owned(),
                session_id: self.session_id.clone(),
                transcript_path: self.transcript_path.clone(),
                cwd: self.cwd.clone(),
            },
        );
        self.last_additional_context = body
            .as_deref()
            .and_then(|b| serde_json::from_str::<Value>(b).ok())
            .and_then(|v| {
                v.pointer("/hookSpecificOutput/additionalContext")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        if dequeued {
            self.transcript.dequeued_user_text(prompt)
        } else {
            self.transcript.user_text(prompt)
        }
    }

    /// The next submitted prompt: the launch's positional prompt first, then
    /// whatever the pane input submits. Escapes pressed while idle are ignored
    /// (there is no turn to interrupt), like a TUI at its prompt.
    fn next_prompt(&mut self) -> Result<String, String> {
        if let Some(prompt) = self.pending_prompt.take() {
            return Ok(prompt);
        }
        loop {
            match self.events.recv() {
                Ok(InputEvent::Prompt(text)) => return Ok(text),
                Ok(InputEvent::Interrupt) => continue,
                Err(_) => return Err("stdin closed while awaiting a prompt".to_owned()),
            }
        }
    }

    /// Block until Escape arrives. Prompts submitted while a turn is in
    /// flight are dropped: modelling claude's prompt queue is the explicit
    /// `enqueue_prompt`/`dequeue_prompt` steps' job, not an implicit side
    /// effect of waiting.
    ///
    /// Shared by the `await_interrupt` (turn interrupt) and `await_escape`
    /// (AskUserQuestion cancel) steps — both wait for the same Escape byte and
    /// differ only in what they write afterwards.
    fn await_escape(&mut self) -> Result<(), String> {
        loop {
            match self.events.recv() {
                Ok(InputEvent::Interrupt) => return Ok(()),
                Ok(InputEvent::Prompt(dropped)) => {
                    eprintln!("fake-claude: dropping prompt submitted mid-turn: {dropped}");
                }
                Err(_) => return Err("stdin closed while awaiting an escape".to_owned()),
            }
        }
    }

    fn fire_session_start(&self, source: &str) {
        self.fire(
            "SessionStart",
            &self.endpoints.session_start,
            &SessionStartPayload {
                session_id: self.session_id.clone(),
                source: source.to_owned(),
                cwd: self.cwd.clone(),
                transcript_path: self.transcript_path.clone(),
            },
        );
    }

    /// Fire one hook, logging (but tolerating) delivery failures — a real
    /// `claude` keeps running when a hook endpoint misbehaves, and the
    /// scenario's later assertions will surface the breakage. Returns the
    /// response body on a 2xx (the hooks whose response `claude` consumes need
    /// it), `None` otherwise.
    fn fire<P: serde::Serialize>(&self, event: &str, url: &str, payload: &P) -> Option<String> {
        match post_json(url, payload) {
            Ok((status, body)) if (200..300).contains(&status) => Some(body),
            Ok((status, _)) => {
                eprintln!("fake-claude: {event} hook returned HTTP {status}");
                None
            }
            Err(err) => {
                eprintln!("fake-claude: {event} hook failed: {err}");
                None
            }
        }
    }
}
